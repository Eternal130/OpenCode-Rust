# S3b 结论: ACP 域 20 文件 Rust 移植与 ref v2.0.24 结构逐文件对齐、wire 协议版本一致（双侧 protocolVersion 1），但 cancel/drain 语义、ask 并发模型、MCP 重注册去重、child `_meta` 投影等 10+ 处行为偏差，其中 5 处为正确性风险。

## 对比表

| ref (packages/cli/src/acp) | rust (oc-cli/src/acp) | 对齐度 |
|---|---|---|
| agent.ts (134L) | agent.rs (358L) + mod.rs (167L) + transport.rs | 高；13 请求 + cancel 全注册，boot 等待 server.connected |
| service.ts (251L) | service.rs (538L) | 高；initialize 能力集/authMethods/_meta 逐字段一致 |
| sessions.ts (211L) | sessions.rs (422L) | 中；attach/select/watch 对齐，MCP 去重与 release 身份判定缺失 |
| turn.ts (412L) | turn.rs (629L) | 中；consume/submit/abandon/usage_update 对齐，drain/asks/handoff 语义偏差 |
| translate.ts (471L) | translate.rs (1200L) | 高；fold 事件分支 1:1（text/reasoning/tool 5 事件/compaction/retry/child） |
| tool.ts (210L) | tool.rs (563L) | 高；kind 表/V1 别名/locations/rawInput/diff 一致；readDisplayText 缺 |
| content.ts (171L) | content.rs (445L) | 高；audience/synthetic/ignored、resource/file/zed:// 转换一致 |
| prompt.ts (61L) | prompt.rs (170L) | 高；slash/compaction start/文件可读性降级一致 |
| permission.ts (161L) | permission.rs (419L) | 中；options/title/locations/patch 预览一致；added 文件预览丢失 |
| elicitation.ts (217L) | elicitation.rs (686L) | 高；credential 门控/representable/properties/fieldAnswer 逐条对齐 |
| compaction.ts (144L) | compaction.rs (320L) | 高；marker/standard 双模式、replay、abandon 一致 |
| replay.ts (127L) | replay.rs (301L) | 高；分页 200/asc、reasoning 序号、tool 四态一致 |
| catalog.ts (170L) | catalog.rs (268L) | 高；NotReady 25ms/5s 重试、reloadOn 三事件一致（读改为串行） |
| config-option.ts (145L) | config_option.rs (419L) | 高；model/effort/mode 三选项、parse/require/selectVariant 一致 |
| directories.ts (71L) | directories.rs (222L) | 高；parse/grant/activate/rules 一致（resolve 缺 realpath 半） |
| error.ts (139L) | error.rs (258L) | 高；14 错误→JSON-RPC code/data/message 逐条一致 |
| capabilities.ts (17L) | capabilities.rs (82L) | 全同 |
| child.ts (68L) | child.rs (169L) | 中；update/meta/toolCallID/prefixTitle 一致，project 缺 `_meta` 注入 |
| client.ts (32L) | error.rs classify/decode_session_id | 高 |
| connection.ts (78L) | vendored crate ConnectionTo + transport.rs | 高 |

**vendored 版本对照**：ref = `@agentclientprotocol/sdk` 1.6.0（protocolVersion: 1，service.ts:119）；Rust = third_party/agent-client-protocol 3.3.0 + schema 1.11.0，`ProtocolVersion::V1`——同一 spec 面，**wire 版本一致**。`PromptResponse.usage` 在 vendored schema 是 `unstable_end_turn_token_usage` feature 门控（未启用）→ Rust 不回 turn usage（D-3）。
**tool_call 流式（partial）**：两侧均为 started→pending→called→update 序列，无部分内容流；行为一致。

## 行为差异清单 D-n

- **D-1 cancel 后 drain 不再折叠事件**。ref turn.ts:269 在 drain 窗口内继续折叠事件直到 terminal（5s 超时才 abandon）；Rust turn.rs:314 `if turn.is_cancelled() { return Cancelled }` 在折叠前短路——drain 期间到达的第一个事件即令 consume 返回，后续 `tool_call_update`/compaction 结束/`session.execution.interrupted` 全部被丢弃，也不走 abandon 分支（turn.rs:254-255 直接取 Interrupted）。
- **D-2 权限/表单 ask 阻塞事件折叠循环**。ref queueAsk 异步入队，fold 持续推进；Rust turn.rs:368-421 在 interpret 内 await 整个 request_permission/elicitation 往返。一个未决 ask 期间，后续事件积压在容量 64 的 channel，turn 无法到达 terminal，`$/cancel_request` 也要等 ask 返回才生效（cancel 信号只在事件间检查）。ref 中取消立即生效。
- **D-3 PromptResponse 不含 usage**。ref 返回 inputTokens/outputTokens/thoughtTokens/cachedRead/cachedWrite；Rust 仅 stopReason+_meta（自认依赖未启用的 unstable feature，用 usage_update 通知替代）。
- **D-4 子代理投影更新缺 `_meta`**。ref `project()` 对所有投影更新注入 `_meta["opencode/child-session"]`；Rust child.rs:98-112 只命名空间化 toolCallId/title，不注入 meta。
- **D-5 MCP 注册无跨 attach 去重**。ref `registeredMcpBySession` 超越 entry 生命周期，重复 attach 不再调 `mcp.add`，失败回滚；Rust sessions.rs:201-263 每次 attach 重新 `mcp.add`，失败不清理且 entry 留在表中（ref onError remove）。
- **D-6 handoff 仅在 interrupted 时跟随子会话**。ref 对任何非失败 exit（含 succeeded）且 openChildren 非空时 fork 后台跟随；Rust turn.rs:531 `if terminal != Terminal::Interrupted { return }`——成功回合结束时仍开放的子代理流被丢弃。
- **D-7 command prompt 提交不等结果**。ref await `session.command` 后才返回 succeeded；Rust 命令后台 spawn 后立即返回 Succeeded，HTTP 失败被静默吞掉。
- **D-8 available_commands 时机**。ref 等响应写出（Responded）后才推送；Rust spawn_watch 与响应并发，通知可能先于 `session/new` 响应到达。
- **D-9 `read` 工具完成更新不做 readDisplayText 整形**。Rust tool.rs:354-356 注释自认跳过，保留全部 text part。
- **D-10 非 patch 权限预览的失败语义**。ref 任一文件读/apply 失败→整体 previews=[]；Rust 均只丢该文件——部分失败的降级粒度不同（另见 R-1）。
- **D-11 zed:// 链接仅识别 `?path=` 为首参**。ref 用 URL.searchParams 任意位置取 `path`；Rust `split_once("?path=")`。
- **D-12 replay file URL 不含目录**。ref `pathToFileURL(filename)` 按 cwd 解析绝对路径；Rust 恒放根目录。
- **D-13 catalog 载入串行**（仅延迟差异）；且 Rust reload 无 generation 合并，风暴时重复拉取。
- **D-14** 模型排序 localeCompare vs 字节序、catalog key canonicalize vs resolve、`classify` 未知名错误映射——次要可观察差异。
- **D-15** multiselect 自定义答案 value 非数组时 ref 得 `[custom]`、Rust 得 `[value, custom]`；data-URI 带参数时 mime 串不同；空 base64 data 串 ref 保留、Rust 丢弃。

## 正确性风险 R-n

- **R-1（高）added 文件的权限 diff 预览全丢**。permission.rs:194-197：`status=="added"` 时 `old_text=None`，随即 `let Some(old_text) = old_text else { continue }` 直接跳过——新增文件永远不生成预览（ref added→oldText null→对 "" 应用 patch 生成新文件预览）。edit/write 新建文件场景客户端拿不到 diff。
- **R-2（高）`session.retry.scheduled` 的 `nextRetryAt` 恒为空串**。wire 上 `at` 是 NonNegativeInt（epoch 毫秒，session_event.rs:591）；ref `new Date(at).toISOString()`（translate.ts:263）；Rust `string(&event.data,"at")` 对数字取不出字符串→""（translate.rs:529）。retry 投影的 nextRetryAt 均坏。
- **R-3（中）`Sessions::release` 身份判定恒真**。sessions.rs:325-330 比较 `current.id == attached.id`——同 key 必然相等，等价于无条件删除；ref 用 attachment 身份保证旧 attachment 的 release 不误删新 attach 的条目。loadSession 失败路径可能把并发的重新 attach 一并拆掉。
- **R-4（中）turn 取消后若事件流静默，prompt 永不返回**。Rust 取消依赖事件到达触发 `is_cancelled` 检查；若 server interrupt 后不再产出事件，`consume` 无限等待，`session/prompt` 悬挂（ref 用 fiber interrupt 强制终结）。
- **R-5（中）`$/cancel_request` 预取消路径不中断 server**。agent.rs:180-187 cancellation 已取消时直接回 `Cancelled`，不调 `session.interrupt` 也不 abandon 已跟踪工具。
- **R-6（低）directories 解析缺 realpath**。ref 全走 `FSUtil.resolve`（lexical+realpath）；Rust 仅 lexical 折叠——经 symlink 的 cwd/附加目录在去重与授权规则上可能与 server 实际判定不一致。
- **R-7（低）`session/load` replay 期间 ask 事件不可达**（双侧等价，风险仅在 Rust 的 load 失败释放路径配合 R-3 放大）。

## 覆盖声明

- 逐行读完 ref 20 文件（共 3290L）与 Rust 20 文件（共 7862L），含两侧内联测试；tests/acp_integration.rs 与 ref 测试仅按需抽查。
- vendored 面核对 Cargo.toml 版本、ProtocolVersion、v1 schema 中 PromptResponse.usage feature 门控与 set_config_option/session_list/elicitation form 等方法存在性；ref 侧核对 @agentclientprotocol/sdk 1.6.0。未逐行读 vendored crate 全部生成代码。
- `session/tool.input.delta` 事件两侧 fold 均未处理（与 ref 一致）。
- 特别核对项均有结论：wire 版本一致；tool_call 流式行为一致；`opencode/child-session-updates` 能力协商双侧一致（capabilities 全同），投影 `_meta` 缺失见 D-4。
