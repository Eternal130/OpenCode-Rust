# opencode v2.0.24 → Rust 移植：独立逐项对比审查总表

> 审查日期：2026-11-08。锚定版本：双侧 2.0.24（ref `/opt/opencode-ref` ↔ rust `/opt/opencode-rust`）。
> 方法：19 条并行审查通道，双侧代码逐文件逐行阅读（不抽查、不信任任何已有文档/注释结论），每项判定附双侧 文件:行号 证据。明细见 `/tmp/opencode/review-lanes/L01–L19`（19 份通道报告，含完整覆盖清单与逐文件表格）。本文为合并总表。
> 判定：✅ 对等 · 🟡 部分（有分歧/缺分支） · ❌ 缺失 · ➕ rust 独有 · 🔄 形态重构（净效果需看差异列）。

---

## 一、总体结论

| 维度 | 结论 |
|---|---|
| 总体完成度 | **非常高**。核心执行链路（session 执行循环、AI 协议、内置工具、MCP、权限/凭证、PTY/shell、server API、TUI 状态机、CLI）达到逐行级保真，大量冷门分支被忠实移植并有 parity/golden 测试钉住 |
| 最大架构差异 | ① 插件系统：ref 进程内 Effect 插件 ↔ rust bun 外置 plugin-host（按值 JSON 桥）；② 内置插件群 → engine/provider_catalog 编译期内建；③ Effect Layer/State 生态 → 直接组合（state.ts 通用框架未移植）；④ 全局存储根 `opencode-rust` + 默认通道 `latest`（与 ref 同机互不可见，刻意共存设计） |
| 功能性破坏级缺口 | 9 项（见二、P0）：MCP 工具执行期无权限门、zen/自托管 Console 元数据链路、非持久 PTY 多字节 panic、warming 零实现、WS transport 默认丢失、credential HTTP create 激活反转、媒体凭证持久化泄漏面、CodeMode 沙箱逃逸面、mini 文本流疑似重复输出 |
| 整包缺失 | enterprise(share)、桌面/Web GUI 族(app/desktop/web/gui-extensions/session-ui/ui/storybook)、console 云站点、stats 云站点、plugin-browser、sdks/vscode、云服务/infra（多数属 TS/云生态专属，见五） |
| 明确不可用通道 | `aisdk:` 任意 npm AI SDK 包动态装载（类型化 UnsupportedProtocol 报错，非静默）；Effect 风格插件（`@opencode/plugin/effect` 显式拒绝）；perplexity/sap-ai-core（依赖前者） |

---

## 二、高优先级缺陷清单（合并去重，证据见通道文件）

### P0（行为破坏 / 安全）

| # | 域 | 缺陷 | 证据 |
|---|---|---|---|
| 1 | MCP/工具 | **MCP server 工具执行期无权限断言且绕过统一管线**：`McpToolExecutor.permission` 标 `#[allow(dead_code)]`，`PipelineExecutor` 不做 permission.assert，桥接直接注册裸执行器——agent 规则的 ask/deny 只影响目录可见性，ask 不会拦截询问 | L4-D21（mcp/registry.rs:1616-1705、registry.rs:239-445） |
| 2 | 集成/Console | **zen device flow 不写 `metadata.server`**，也不调 `/api/user`、`/api/orgs`：自托管 Console 的 token 刷新与配置拉取回退到 opencode.ai 官方服务器——自托管场景整体失效；orgName 恒显示 org_id | L17-分歧#1（providers.rs:175-194 vs opencode.ts:592-623） |
| 3 | PTY | **非持久 PTY 两处多字节 panic**：①缓冲按字节 drain 无 UTF-8 边界回退（超 2MiB CJK/emoji 输出直接 panic）；②attach 重放用 UTF-16 偏移切字节 String（错位/panic）。persistent_pty 已做边界回退，pty.rs 漏做 | L9-分歧#1/#2（pty.rs:258-266,798-803） |
| 4 | 配置 | **warming 会话保活完全无运行时**：config schema+normalize 解析了 ref 同形配置，但 boot/lifecycle/hooks 全库无任何 Warming 引用——配置被静默接受、零效果 | L18-分歧#1（oc-schema/config/warming.rs、normalize.rs:583-587） |
| 5 | AI 传输 | **WebSocket transport 默认丢失（系统性）**：ref openai/xai 注入 `transport ?? "websocket"`、azure 条件注入；rust 快照 normalize 不产出 transport、resolver 只读 settings——openai/xai/azure 默认走 HTTP SSE，长连接/prompt-cache 连续性默认不可用（WS 机器本身完整且对齐） | L18-分歧#2（resolver.rs:597-606） |
| 6 | 凭证 | **credential HTTP create 激活语义反转**：rust handler `activate` 缺省 false 且无 current 检查——经 HTTP 创建某集成**首个**凭证默认 inactive，登录后模型不可用（oc-core CredentialStore 实现正确，handler 手写 SQL 旁路） | L7-D1（handlers/credential.rs:124） |
| 7 | 媒体 | **Asset 下载凭证 headers 写入可序列化 providerMetadata**（ref 明确运行时保密不入 toJSON）——凭证随资产 JSON 持久化泄漏；另 BFL 结果 URL 10 分钟过期不物化且无 bytes() 下载路径，排队类媒体结果到期即废 | L19-分歧#15/#11（protocol.rs:280-294） |
| 8 | CodeMode | **沙箱逃逸面与描述不符**：guest 内动态 `import()`、`setTimeout` 仍可达（可间接访问 fs），工具描述向模型宣称不可用 | L5-风险#1（host.js:2110-2112、codemode.rs:76-81） |
| 9 | mini TUI | **文本增量疑似重复输出**：rust 每个 text.delta 的 Commit 携带累计全文，mono writer 逐条追加无同组替换——与 ref 增量模型相反，推断每个 delta 重复打印前文（需实机验证） | L14-R1（transport.rs:269-277、scrollback.rs:350-391） |

### P1（明显行为偏差 / 数据正确性）

| # | 域 | 缺陷 | 证据 |
|---|---|---|---|
| 10 | session | steer 收件箱后不 wake（空闲会话 queue→steer 滞留，ref "steer 即插即跑"） | L1-分歧#1 |
| 11 | session | GET context 缺 native-checkpoint 前置 system 消息过滤（原生压缩后输出陈旧 instruction 消息） | L1-分歧#2 |
| 12 | session | 子会话 interrupt 默认 reason=Shutdown：claim 保留（重启"复活"）、job 不取消——ref 为 user 语义（释放+取消） | L2-分歧#2 |
| 13 | session | import 非原子（两事务间崩溃留半导入 session）；手动压缩失败双重发布 Compaction.Failed | L1-分歧#3、L2-分歧#4 |
| 14 | session | session_message.time_updated 不推进（增量拉取消费者观察不到更新） | L1-分歧#10 |
| 15 | 权限 | permission.saved.list 无 projectID 时返回全部项目（跨项目泄露+UI 错误） | L7-D3 |
| 16 | 权限 | agent 全局 4 条 external_directory allow 未接线（函数存在零调用）——build/plan 等代理对 shell 输出/tool-output/tmp/config 触发多余询问 | L7-D4 |
| 17 | 工具 | websearch `random` 退化为恒第一个 provider；read `limit:0` 返回空页+翻页提示；worktree 指南常量零引用（Code Mode 引导丢失） | L4-D2/D1/D9 |
| 18 | 工具 | builtin 工具全部无 Output schema 声明与校验（G-230 自认），字段漂移不可捕获 | L4-D8 |
| 19 | VCS | vcs.status(git) 增删恒 0、rename 记录误解析为伪记录、缺统一 git cfg 前缀（宿主 gitconfig 干扰）；缺 branch.default | L8-分歧#1-#3 |
| 20 | server | persistent-pty WS framed 输入不校验 type/cols/rows（畸形帧按输入写入、0 尺寸 resize） | L10-分歧#7 |
| 21 | server | agent.list 过滤 hidden 且不置顶默认（ref 全量+默认置首）；reference.list git 源无物化（"看得见用不了"）；fs.find 返回绝对路径（ref 相对） | L10-分歧#1/#4/#5 |
| 22 | 基础设施 | state.ts 通用框架大面积缩减（batch/shutdown/inherit/reconcile/notify 合并/Scope/revision/invalidate/reload 无对应；仅 agent 域 transform 链）——以 State.Transformable 为接缝的插件面无通用机制 | L11-分歧#1 |
| 23 | 集成 | GitLab 刷新 single-flight 无 60s 保留（并发二次消费已轮换 token→误报 revoked）；ChatGPT 重授权不复用 client/无 host-id；表单校验子集（options 闭合集/未知字段/neq/format 缺） | L17-D2/D3/D4 |
| 24 | 插件 | 热重载失败丢旧代注册（ref 保留旧版运行）；重载无内置 fs watcher（依赖 embedder 轮询）；插件事件经 TUI store tap（headless 送达不确定，rpc.* 无对应） | L5-分歧#4/#35、L5-分歧#7 |
| 25 | 模型目录 | models-dev 快照清洗不完整：2 个弃用别名 provider 仍可绑定、bedrock 13 个 profile-only 模型未滤除（选中即错） | L18-分歧#3 |
| 26 | provider | azure/gitlab/digitalocean 凭据驱动的模型清单缺失（部署发现/duo 工作流/routers——登录可用但动态模型面不出现，azure 反而全量展示） | L18-分歧#7 |
| 27 | TUI | `@` 文件实时补全缺失（"no live file source yet"）；崩溃屏三行文本（ref 有 Copy report/issue 链接/stack/按钮）；shell 输出查看器无轮询 | L13-风险#2/#1/#3 |
| 28 | mini | 运行时功能缺口：catalog、agent/model/variant 面板、`!` shell 直通、附件 parts、inbox 操作 UI、background、重连不恢复权限/表单、SIGINT、verbosity 三档、样式化滚动回写（高亮/主题色/图片预览整层） | L14-分歧#4/#5、L14-表#32 |
| 29 | CLI | 全局 flag 超集静默忽略（`mcp list --server X` 不报错无效）；run --standalone 会取消 global 表单（语义反转）；service set cors 接受尾斜杠 | L15-分歧#3/#1/#4 |
| 30 | 事件流 | 事件载荷差异：session.step.started 缺 snapshot 字段；Compaction reason "overflow" vs "auto"；Form publish 顺序反转不回滚；usage 扇出失败仅 eprintln；transport pump 丢弃 title/foreground 事件 | L2-分歧#9/#3、L11-分歧#6、L9-分歧#4 |

### P2（边缘/低风险，代表性条目）

- replay 一致性比较数值语义（serde 1≠1.0 vs JS 深相等）；per-aggregate 锁无淘汰（无界增长）；canonical 键 POSIX 上恒归一反斜杠；DbError::Pragma 字符串隧道碰撞面（L11）
- 中断时托管工具 RESULT_MISSING 分类偏移；标题冲突 20 次重试（ref 放弃）；token 估算按字节；attachmentLocation 不解码百分号（L2）
- MCP 瞬态重试第二跳 500ms vs 1000ms；elicitation number/format 收窄；McpRegistry overrides 无持久化 seam（L5）
- JSONC→json5 宽松解析；mcp_timeout 接受 0；根外 skill 目录无热重载；watch 通道有界丢弃（L6）
- MIME 缺 BMP/AVIF、isText 启发式缺失；oc-syntax 默认构建缺 17 语言+markdown 无注入；snapshot 无 per-repo 锁；find 索引排除 target（L8）
- 全部小项见各通道报告"语义分歧明细"

---

## 三、逐域对比总表

### 3.1 核心引擎（L1/L2）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| session create/remove/fork | ✅/🟡 | create/fork 全对等；remove 有界 10s 等待（ref 无界） |
| store/sql/schema（5 表 DDL） | ✅ | DDL 逐字转写（含部分索引） |
| message-updater（事件→part 状态机） | ✅ | 逐 arm 对等 |
| projector（43 投影+usage 扇出） | ✅ | 1:1；usage 扇出改 rust 串行链 |
| inbox（promote/steer/queue/reconcile） | ✅ | 完整对等；**但 steer 不 wake（P1#10）** |
| 分页 cursor/消息分页 | ✅ | 三 scope/anchor 对齐 |
| revert/diff/fork/transfer-sanitize | ✅ | 语义对齐；import 非原子（P1#13） |
| title/usage/stats | ✅/🟡 | 口径一致；title 重试 20 次、中断 usage 丢失 |
| move | 🟡 | 无 trim/~ 展开/相对解析/contextEffect 探测（弱实现） |
| 执行循环 step/drain/retry/max-steps | ✅ | 终止顺序 1-13、退避参数逐项对齐；单写者循环替代并发 fiber |
| compaction | ✅ | due/ceiling/shrink 阶梯/native 双机制/模板 verbatim；reason 载荷分歧 |
| model-request/prompt 组装 | ✅ | hook/headers/affinity/预算/组装顺序全对齐；GENERATION_KEYS 硬编码 |
| system-prompt/instructions 注入 | ✅ | 文案逐字；双层去重对齐 |
| subagent 生命周期 | 🟡 | 深度守卫/错误文案对齐；中断 reason 语义反转（P1#12） |
| 子代理完成通知/job | ✅ | 包络字符串/去重对齐 |
| to-llm-message | 🟡 | 类型转换对齐；百分号附件不解码 |
| 事件发布 publisher | 🟡 | 事件集对齐；snapshot 字段缺失/state 合并语义/首 text 顺序 |

### 3.2 AI provider 层（L3/L3b/L19）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| SSE framing/lifecycle/tool-stream | ✅ | 逐条对等；rust 更严格（UTF-8/10MB） |
| Anthropic Messages | ✅ | 逐行级（cache_control 4 断点/thinking/redacted/server_tool/beta 头/usage iterations） |
| OpenAI Chat | ✅ | 多方言 reasoning/无 index 工具/finish 语义全对齐 |
| OpenAI/Open Responses | ✅ | summary-part 状态机/encrypted_content/compaction 对齐 |
| Gemini | ✅ | thought signature/去重/回填全对齐 |
| GitHub Copilot | ✅ | device OAuth/模型派生/头族/双通道路由对齐 |
| Bedrock | 🟡 | 主体对等；system-update 包裹/cache/document 命名/流分类分歧（P2） |
| 媒体 image 9/speech 4 | ✅/🟡 | 结构对齐；usage 包裹/3.8-tts 识别/n>1 消息缺陷 |
| transcription 5/video 4 | ✅(结构级) | 种类/常量对齐；未逐行 |
| WebSocket 通道/续传 | ✅ | 55min/15s/30min/5 次/checkpoint 全对齐；无 Interceptor/指标 |
| models-dev/variant/native 包表 | ✅ | snapshot md5 一致；38 包覆盖表逐项对等；**快照滤除缺失（P1#25）** |
| 错误分类/重试/鉴权链 | ✅ | 逐条对等（10+ 家鉴权方式） |
| `aisdk:` 动态 npm 通道 | ❌ | 类型化 UnsupportedProtocol（已知架构决策） |
| experimental（evaluation/system-one） | ❌ | 零对应 |
| verbosity | 🟡 | 谓词逐字；hook→路由默认（近似等价） |

### 3.3 内置工具（L4）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| edit/write/file-diff/patch（含解析应用） | ✅ | 三级匹配/jsdiff 自实现/seek 梯子逐字对齐 |
| read | 🟡 | 分页/行号/媒体/missing 建议对齐；limit:0 空页（P1#17） |
| glob/grep/ripgrep 引擎 | 🟡 | 参数逐字一致；消息前缀/截断单位小差；二进制发现链增强 |
| shell 工具 | 🟡 | prepare/三分支/超时对齐；后台通知缺 output 元数据 |
| question/skill/subagent/webfetch | ✅ | 信封/文案逐字；subagent 全链对齐 |
| websearch+5 provider | 🟡 | 选择流/冷却/持久对齐；random 恒第一（P1#17）；日期解析弱化 |
| opencode 三工具/mcp-resource | 🟡/✅ | 对齐；worktree 指南死常量；pinned 特判补偿 |
| MCP 工具桥 | 🟡 | 注册/去抖/解析对齐；**执行期无权限门（P0#1）** |
| 工具注册/启停/codemod hooks | ✅ | wholly-denied 过滤/hook 三挂点一致 |
| CodeMode execute/catalog | ✅ | 描述/预算/签名字节级对齐；**沙箱可达面与描述不符（P0#8）** |
| 工具 Output schema 声明 | ❌ | 全部 builtin 无声明无校验（G-230） |

### 3.4 MCP / 插件 / codemode（L5）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| MCP 生命周期/reconcile/重连 | ✅ | 五态/latch/锁/差量替换对齐；恢复路径多余重启（无害） |
| MCP OAuth（发现/DCR/PKCE/回环/刷新/回调页） | ✅ | RFC 语义+页面逐字节对齐；登录入口仅 CLI（ref Integration 服务可触发） |
| elicitation | ✅/🟡 | 哨兵会话/停靠表对齐；表单类型收窄 |
| stdio 传输/工具桥注册 | ✅ | env seam/16MiB/防抖 flush 对齐 |
| 插件运行时（25 ctx 域） | 🟡 | 全域有对应；按值 JSON 桥（LanguageModel 句柄不可替换）、单 guest vs 每位置实例 |
| Effect 风格插件 | ❌ | `@opencode/plugin/effect` 显式拒绝（生态断裂，文档化） |
| 热重载/激活回退 | 🟡 | sha256 导入图对齐；失败丢旧代、无内置 watcher（P1#24） |
| 插件安装/更新/GC/KV | ✅ | 24h/keep-2/命名空间对齐 |
| codemode 限制/console/search/web | ✅ | 逐条一致（以真实 JS 引擎替代受限解释器） |

### 3.5 配置 / 技能 / 指令 / 工作区（L6）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 六源加载/discovery/variable/watch/normalize | ✅ | 逐键移植；jsonc_edit 自研端口 golden 对齐 |
| agent/command/skill 的 Markdown 发现 | ✅ | 排序/legacy 判定/transform 全字段一致 |
| skills（发现/URL 源/兼容根） | ✅/🟡 | 全对齐；根外目录无热重载、description 类型容错 |
| instruction 发现（AGENTS.md 层级） | ✅/🟡 | 顺序/stop 规则一致；无 per-file watcher |
| wellknown/project/workspace/environment 驱动 | ✅/🟡 | 对齐；workspace janitor/共享 provision deferred |
| instance Layer 编排 | 🔄 | Engine::assemble 一次性组装 |
| warming | ❌ | **零运行时（P0#4）** |

### 3.6 权限 / 凭证 / 账户 / 集成（L7/L17）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| permission 状态机/saved grants/wildcard | ✅ | 逐字对齐；saved.list 缺省过滤分歧（P1#15） |
| credential store（oc-core） | ✅ | 排序/激活/legacy pin/事件全对齐 |
| credential HTTP handler | ❌(行为) | create 激活反转/list active 派生分歧（P0#6，handler 旁路正确实现） |
| OAuth 10+ 家 provider | ✅/🟡 | 端点/scope/PKCE/device 逐项一致；zen 元数据链路破坏（P0#2）、GitLab/ChatGPT/Azure 细节缺口（P1#23） |
| server 认证（Basic/HMAC cookie/Origin） | ✅ | 全对齐 |
| managed-policy/enterprise 策略 | ✅ | 评估顺序/修剪/文案一致 |
| IntegrationHost（方法机/attempt/scrub/命令流） | ✅ | 高度对等+回归矩阵；editor.remove 缺 |
| enterprise share 整包 / identity | ❌ | 无对应 |

### 3.7 文件系统 / VCS / formatter / 语法（L8）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| file-access/file-mutation | ✅ | 全对齐 |
| filesystem read/list/write/protected/watcher | ✅/🟡 | 对齐；notify 替代 @parcel（entries 类型缺） |
| fff 模糊查找引擎 | ❌ | 由索引进似替代（VCS+非 home 场景 ref 默认后端） |
| ripgrep 引擎/二进制管理 | ✅ | 逐参数一致；发现链增强 |
| git discover/clone/worktree/tree/snapshot | ✅ | 语义等价；gitconfig include 简化+stat-cache 代偿；无 per-repo 锁 |
| vcs 服务层 | 🟡 | info/base/diff 对齐；status 增删恒 0/rename 误解析/default 缺/cfg 前缀缺（P1#19）；无 provider 注册抽象 |
| hg | ✅ | 全对齐（HGPLAIN/status -0/合成补丁） |
| worktree 管理/formatter 26 项/image 压缩 | ✅ | 逐字/逐项对齐 |
| oc-syntax | 🟡 | scope 体系对齐；默认构建缺 17 语言+markdown 注入 |

### 3.8 PTY / shell（L9）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 非持久 PTY | 🟡 | 语义对齐但两处多字节 panic（P0#3）；无 finalizer |
| persistent-pty 服务面/传输/认领 | ✅ | 常量/错误分类/重试梯子对齐；chunked replay 扩展 |
| 守护进程（oc-pty 二进制） | ➕ | rust 自带实现（ref 为外部工件）；事件黑洞：pump 丢 title/foreground |
| shell 服务 | ✅ | env/进程组/TERM→3s→KILL/分页对齐；cwd 缺省分歧 |
| shell 解析/扫描（bash/zsh/posix/PS，5366 行） | 🟡 | 忠实移植；3 处小偏差+legacy PS 类型丢失 |

### 3.9 server HTTP API / schema（L10/L16）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 路由面（141 操作） | ✅ | 全覆盖+1 rust 独有（POST /api/plugin/install）；错误注册表 27 类全对齐 |
| SSE/事件流/分页/auth/CORS/就绪闸门 | ✅ | 逐项对齐（首帧/心跳/溢出断流/HMAC/allowlist） |
| 各 handler | 🟡 | 主体对齐；agent.list/reference.list/fs.find/pty location 回退/worktree 归属/vcs.init 等载荷与语义分歧（P1#21） |
| workerd/fetch 嵌入/simulation | ❌ | 平台性缺失（TS 生态专属为主） |
| V1 schema 模块 | ❌ | oc-schema 无 v1（仅 client legacy 类型清单） |

### 3.10 基础设施（L11/L16）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 事件总线（publish/replay/订阅/log） | ✅ | 逐语义对等；数值比较语义差异 |
| 数据库（18 表/48 迁移/连接/事务/V1 迁移） | ✅ | 逐表逐条转写+bun 验收；workerd 回退缺 |
| KV/Form/Job/ToolOutput | ✅ | 对齐（Form 顺序反转、job 内嵌 SessionRestart） |
| state 框架 | 🟡 | 大面积缩减（P1#22） |
| location 图/生命周期/活动 TTL | ✅/🟡 | TTL 对齐；project/vcs 字段缺、invalidate 不等借用者 |
| 全局根/通道 | 🟡 | `opencode-rust`+`latest`（刻意共存，互操作需 legacy_v2_roots） |
| oc-util（wildcard/retry/path/encode/hash/flock/glob…） | ✅/🟡 | 对应模块对等；glob 无 brace/extglob、flock 缩减为原语 |

### 3.11 TUI（L12/L13/L14）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 启动/路由/Store/合并重绘 | ✅ | 130+ 字段级对应；整帧重绘+100ms tick（vs 60fps，动画粗化） |
| tabs 模型/持久化/inbox 镜像/模型解析链 | ✅ | 1:1 移植+测试钉住 |
| 键位默认表/v1 迁移 | ✅/🟡 | 逐条对齐；非法条目容错替代 throw |
| 渲染管线/diff/panic 恢复/resize/输入 | ✅ | 对齐+增强；transcript O(all-rows)+40ms 临时预算 |
| 鼠标面/滚动/锚点/导航/历史分页 | ✅ | 对齐；tab 拖拽/长按关闭缺 |
| 组件 55 项（dialog 族/palette/tabs/spinner…） | ✅/🟡 | 大部对齐（文案逐字级）；@file 补全/崩溃屏/devtools 面板/shell 轮询/超链接/panel-host 缺（P1#27） |
| mini 模式 | 🟡 | 传输/fragment/队列/mono 对齐；文本重复输出疑似缺陷（P0#9）、catalog/面板族/`!`/附件/verbosity 缺（P1#28） |
| 编辑器（zed/IDE/外部 $EDITOR） | ✅/🟡 | SQL/评分/锁文件对齐；at_mentioned 缺、重连不重解析 |
| 剪贴板/attention/声音/通知/suspend | ✅/🟡 | 读对齐、写仅 OSC52；非 Linux 外部播放器 |
| mermaid(merman)/latex 渲染 | ✅(等价) | oc-tui/render 内建（timeline 为 rust 增补；未逐行） |

### 3.12 CLI（L15）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 命令树 17 命令/run/mini/api/debug | ✅/🟡 | 对齐；flag 全局化+静默忽略（P1#29） |
| noninteractive（事件机/退出码/reconcile） | ✅/🟡 | 全对等；新增 TTY 权限交互、v1 桥接缺 |
| auth/mcp/plugin/session/service 命令族 | ✅/🟡 | 文案逐字级对齐；cors 尾斜杠/import 相对路径等小分歧 |
| serve/standalone/server-connection/stats | ✅ | 对等+加固；升级通道换 GitHub feed（注册偏差） |
| heap/cpu-profile 诊断钩子 | ❌ | 无对应 |

### 3.13 client SDK / theme / util（L16）

| 功能 | 判定 | 关键差异 |
|---|---|---|
| 生成客户端（139 方法） | ✅ | 全覆盖+3 rust 扩展（plugin.install/pty connect×2）；SSE/错误/RPC 语义对齐 |
| SharedEvents/RPC/服务生命周期 | ✅/🟡 | 对齐；生命周期移至 oc-server/oc-cli |
| theme（schema/OKLCH/expand/resolve/v1 迁移） | ✅ | 全对齐；**34+1 主题资产逐字节一致** |
| util 25 模块 | ✅/🟡 | 对应模块对等；binary.ts/Glob.scan/artifactKind 等以标准库/分散实现 |

### 3.14 内置插件群（L18，51 项逐插件）

| 插件 | 判定 | 插件 | 判定 |
|---|---|---|---|
| agent/plan/skill/optimize/identity/command | ✅/🟡 | websearch 6 provider | ✅ |
| verbosity | 🟡 | vcs git/hg | ✅ |
| **warming** | ❌ | local/ollama/lmstudio/vllm | ✅ |
| tool-input-repair/models-dev | ✅/🟡 | cerebras/kilo/nvidia/llmgateway 头注入 | ✅ |
| chatgpt/copilot/vertex/modal/poe/cohere | ✅ | **zenmux** | ❌ |
| openai/xai | 🟡(ws 默认) | **perplexity/sap-ai-core** | ❌ |
| azure/gitlab/digitalocean | 🟡 | **prompt-cache-key** | ❌ |
| opencode(console) | 🟡 | openrouter | 🟡 |
| snowflake | 🟡 | npm SDK 桥(factory/dynamic) | ❌ |
| 10 个资产文件 | ✅ 逐字节 | — | — |

### 3.15 ref 独有包盘点（L16-B）

| 包 | 判定 | 包 | 判定 |
|---|---|---|---|
| app/desktop/web/gui-extensions/session-ui/ui/storybook | 不适用（TS/GUI 生态） | merman/latex | 🟡（oc-tui/render 内建等价） |
| console 云站点/stats 云站点 | 不适用（云服务） | stats 本地 API/TUI | ✅ |
| simulation/http-recorder | 不适用（测试基建，rust 用 golden 替代） | httpapi-codegen | ✅ 等价（openapi-gen） |
| function/script/containers/posts/infra/.github | 不适用 | plugin-browser | ❌（依赖桌面 GUI） |
| sdks/vscode | 不适用（VS Code 扩展） | services/files/update/www | 🟡（消费侧本地实现；云端 Worker 不适用） |

---

## 四、ref 有而 rust 无（完整合并清单）

**整包/平台**：enterprise（share 全套）、identity 资产、桌面/Web GUI 族、console/stats 云站点、plugin-browser、sdks/vscode、services 云 Worker、infra、workerd/fetch-handler/simulation 嵌入轮廓、V1 schema 模块与 CLI v1 桥接、heap/cpu-profile 诊断。
**AI 层**：`aisdk:` 动态 npm 通道、experimental 评测、Media Asset 下载/materialize 路径、AWS 凭证链高级项（credential_process/assume-role/web-identity/ECS）、WebSocket Interceptor、bedrock system-update 包裹/流分类、openrouter 头与隐藏别名、zenmux/prompt-cache-key/perplexity/sap-ai-core、azure 部署发现、gitlab 工作流模型、DO routers、snowflake 假流改写、Console websearch/MCP 下发、models-dev 别名与 bedrock 滤除。
**核心**：state.ts 通用框架族、Session.Generate 顶层服务形态（server 路由承担）、instruction-entry list/put/remove、`!` 后台命令服务形态（runner 承担+文本格式不同）、fff 引擎、vcs provider 注册面、git history.rootCommits、Mime BMP/AVIF+isText、oc-syntax 17 语言（默认构建）、agent 全局 external allow 接线、plugins 的 rpc 入口探测。
**集成**：editor.remove、zen /api/user+/api/orgs、ChatGPT host-id/client 复用、browser→新流凭据迁移、Copilot 双文案、Azure configured 短路/per-scope 缓存、wellknown Auth.command/嵌套 config 键、Console baseURL 覆盖丢弃面。
**TUI/CLI**：panel-host、retry-provider、file#range mention、devtools 面板、模型收藏/Recent、allProjects 切换、tab 拖拽/长按/spring、超链接、样式化 mini surface/图片预览/TOOL_RULES 文案体系/usage 显示、SIGINT/终端标题/SIGUSR2、at_mentioned、剪贴板 host 写臂、autoClose 选项、@file live 补全、effect/solid 同址 SDK。
**其它**：Bus beforeAggregateRead（仅测试）、keyed-mutex 条目清理、fs-util windowsPath、process.ts 统一封装、artifactKind、V1 严格 schema 校验（auth import）、opencode.ai 更新服务、clack UI 细节、ref 按 Location 的 Form/插件实例隔离。

## 五、rust 独有（要点）

plugin-host 进程模型（监督/退避/死锁协议）、oc-pty 守护二进制（认证/超迁/孤儿宽限/VT100 仿真）、chunked replay、golden/parity/wire_fidelity 测试体系、POST /api/plugin/install、x-opencode-build+探针协议、oc-http、mcp.prompts.changed（私有）、panic 500 envelope+5xx 落盘、PreparedRing/prompt backstop、RUNTIME-SWEEP-RACE 守卫、usage 扇出串行链、G-226 合成失败行、UNWIND/TOOL_JOIN 有界放弃、stat-cache sidecar、25ms watcher 合并、ripgrep 发现链、`opencode-rust` 根+legacy_v2_roots、cli_config v1 迁移、TTY 权限交互、OPENCODE_NO_TTY_PROMPT、update-notice/installation.* 事件、suspend 孤儿保护、os_notify/image_codec/image_view 重实现、stderr 重定向、BOOT_DISCARD_WINDOW、?2026 包络+RGB256 降级、fuzzysort 自研移植、100 主题金测、tree_capture、oc-syntax 别名族、v2→rust 数据迁移、warming config schema（陷阱：有解析无运行时）。

## 六、残留未验证项（诚实清单）

1. **结构级/抽验级未逐行**：transcription 5 对与 video 4 对（常量级）；mistral/cohere/google-interactions/meta-responses/alibaba-responses（函数面 1:1+签名抽验）；providers/ 41 文件中 39 个（经 facade 绑定表+grep）；facade.rs 后 2/3 部分精读；ref plugin/vcs/hg.ts 的 rust 侧注释引用（hg 行为未与原文逐行）。
2. **测试文件**：双侧全部测试（ref ~5.8 万行、rust ~7.7 万行含内联）仅结构/标题级扫描，作为佐证引用。
3. **需运行验证的推断**：mini 文本重复输出（P0#9）、空页 cursor null vs 省略（L10）、CORS 逐字节头、portable-pty 环境继承、win32 分支编译。
4. **跨道未闭环**：Console http.response 解释器在 rust 引擎是否存在（L17）；openai omitOutputLimit 定位（L18）；boot.rs config.updated→重组装链路完整性（L6）；oc-server plugins.rs rpc.call guest 校验细节（L10）。
5. 模板资产 verbatim 声明（optimize/command/skill/system-prompt）：L18 已对 10 个资产做 diff 逐字节一致；L2 的 runner 内嵌 txt 与 ref 的比对覆盖同族资产。

## 七、通道明细索引

| 通道 | 文件 | 通道 | 文件 |
|---|---|---|---|
| L1 session 数据 | `L01-session-data.md` | L10 server API | `L10-server-api.md` |
| L2 执行循环 | `L02-session-exec.md` | L11 基础设施 | `L11-infra.md` |
| L3 AI provider | `L03-ai-providers.md` | L12 TUI 框架 | `L12-tui-app.md` |
| L4 内置工具 | `L04-tools.md` | L13 TUI 组件 | `L13-tui-components.md` |
| L5 MCP/插件 | `L05-mcp-plugin-codemode.md` | L14 TUI 杂项/mini | `L14-tui-misc.md` |
| L6 配置/技能 | `L06-config-skills.md` | L15 CLI | `L15-cli.md` |
| L7 权限/凭证 | `L07-auth-permission.md` | L16 client/theme/盘点 | `L16-client-theme-pkg-inventory.md` |
| L8 fs/VCS | `L08-fs-vcs.md` | L17 integration 补审 | `L17-integration-supplement.md` |
| L9 PTY/shell | `L09-pty-shell.md` | L18 内置插件补审 | `L18-builtin-plugins-supplement.md` |
| （目录：`/tmp/opencode/review-lanes/`） | | L19 AI 媒体补审 | `L19-ai-media-ws-bedrock-supplement.md` |
