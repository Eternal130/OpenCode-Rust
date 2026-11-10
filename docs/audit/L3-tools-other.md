# L3 非文件类工具与注册表 (Tools: other) — 审查报告

> 审查方式：两侧逐文件逐行阅读。ref 此版本无 bash.ts/task.ts/llms.txt/followup/disable-model 工具（bash=shell.ts，task=subagent.ts；grep 全库确认）。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明
Ref 侧实际清单（`ls tool/plugin/`）：edit, file-diff, glob, grep, mcp-resource, opencode, patch, question, read, shell, skill, subagent, webfetch, websearch, write + `tool/{mcp,runtime,html-markdown,http-body}.ts` + `tool.ts`(注册表) + `websearch.ts`(服务) + `plugin/websearch/{exa,firecrawl,parallel,tavily,tinyfish,mcp,index}.ts` + `plugin/tool-input-repair.ts` + `codemode/{tool,catalog,instructions,web}.ts`。Rust 侧 `tool/` 全目录 + `location.rs`(注册表状态) + `mcp/registry.rs`(MCP 工具桥) + `engine/mod.rs`(装配) 逐文件读完。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| shell: 名称/描述/OS 拼接 | shell.ts:22,35-45 | shell.rs:18,35-59 | ✅ | 字节级一致 |
| shell: 默认超时 120s / background 0 | shell.ts:23,198 | shell.rs:31,298-302 | ✅ | |
| shell: env 标记 AGENT/OPENCODE/AI_AGENT/SESSION_ID | shell.ts:210-213 | shell.rs:335-338 | ✅ | |
| shell: prepare(resolve cwd/scan/authorize/permission) | shell.ts:116-152 | shell.rs:167-267 | ✅ | symlink follow 语义有测试钉住 |
| shell: timeout 提示拼接 | shell.ts:226-228 | shell.rs:387-392 | ✅ | |
| shell: job 包装/recovery/共享 call id 各占 job | shell.ts:236-249 | shell.rs:397-414 | ✅ | |
| shell: background + 通知 | shell.ts:154-186,251-255 | shell.rs:427-490 | ✅ | 完成通知携带全量 output |
| shell: 前台 block→backgrounded 清 timeout | shell.ts:257-264 | shell.rs:492-500 | ✅ | |
| shell: onInterrupt→remove+cancel | shell.ts:234,259 | shell.rs:570-598 | ✅ | Drop guard fire-and-forget，幂等，有竞态测试 |
| shell: error/cancelled 分类消息 | shell.ts:265-267,273 | shell.rs:501-518 | ✅ | |
| shell: resultMessages/metadata/backgroundResult | shell.ts:78-100 | shell.rs:132-163,600-628 | ✅ | |
| shell: 描述 hook 刷新 | shell.ts:280-288 | shell.rs:25-28 | ✅ | |
| shell: 用户 `!` 路径 | (v2 command 路径) | shell.rs:656-707 | ✅ | 同样走 create.before+prepare |
| subagent: 输入 schema/描述/背景文案 | subagent.ts:19-62 | subagent.rs:31-76 | ✅ | 字节级 |
| subagent: 深度限制(experimental.subagent_depth=1) | subagent.ts:117-133 | subagent.rs:203-215 | ✅ | ref 无并发上限，Rust 同 |
| subagent: agent 解析+primary 拒绝 | subagent.ts:134-137 | subagent.rs:218 + 共享模块:577-586 | ✅ | |
| subagent: model 解析/variant 校验 | subagent.ts:75-98,168 | subagent.rs:278-283 + resolve_model | ✅ | |
| subagent: 续接 sessionID 父校验+agent/model 切换 | subagent.ts:153-198 | subagent.rs:246-343 | ✅ | switchModel 补 default variant |
| subagent: 首条 prompt 前缀/resume:false | subagent.ts:205-218 | subagent.rs:358-388 | ✅ | wake 不 await，resume:false 仅新+后台 |
| subagent: 前台 block/中断/error/cancelled 文案+`<subagent>`信封 | subagent.ts:234-265 | subagent.rs:412-477,624-650 | ✅ | |
| subagent: execute.before 空串清洗 | subagent.ts:271-280 | subagent.rs:115-145 | ✅ | |
| subagent: 可用 agent 描述 hook | subagent.ts:282-309 | subagent.rs:176-183 | ✅ | |
| webfetch: URL 协议校验(大小写不敏感) | webfetch.ts:70-72 | webfetch.rs:198-209 | ✅ | |
| webfetch: Accept 头/UA/Accept-Language | webfetch.ts:41-59 | webfetch.rs:16,22-32,126-128 | ✅ | |
| webfetch: cf-mitigated 403 重试 UA "opencode" | webfetch.ts:61-65,135-137 | webfetch.rs:134-150 | ✅ | |
| webfetch: mime 分类(image/textual 拒绝) | webfetch.ts:84-95,140-143 | webfetch.rs:34-51,161-167 | ✅ | |
| webfetch: 5MB 上限(Content-Length 预检+流式截断) | webfetch.ts:13,77-82 | webfetch.rs:105-111,170-180 | ✅ | |
| webfetch: timeout(默认30/上限120,校验非钳制) | webfetch.ts:14-15,21,28 | webfetch.rs:13-14,92-100 | ✅ | |
| webfetch: 整体预算含 convert | webfetch.ts:146-149 | webfetch.rs:113-121,247-268 | ✅ | reqwest 超时=预算+5s 兜底 |
| webfetch: HTML→md/text 提取 | webfetch.ts:96-101,170-187 | webfetch.rs:56-65; html_markdown.rs | ✅ | |
| websearch: 服务/selection/KV/config 优先 | websearch.ts:31,119-131 | websearch.rs:20,134-161 | ✅ | 坏 KV 条目删除对齐 |
| websearch: random 均匀选取+affinity+attempted | websearch.ts:133-146 | websearch.rs:239-277 | ✅ | |
| websearch: 429 冷却/Retry-After(秒/HTTP-date) | websearch.ts:200-208,215-221 | websearch.rs:342-353,387-444 | ✅ | |
| websearch: providers 排序 localeCompare | websearch.ts:164-169 | websearch.rs:117-125 | ✅ | |
| websearch: affinity 随 session 删除/移动清除 | websearch.ts:92-100 | websearch.rs:205-237 | ✅ | |
| websearch: 同意 Form 双阶段+禁用+random | websearch.ts:66-147 | websearch.rs:492-665 | ✅ | 锁串行一致 |
| websearch: 同意流程超时 | websearch.ts:140-144 (1min 包整个 permit) | websearch.rs:22,508,584 (每次 ask 各 60s) | ⚠️ | choose 流程最长可 120s，ref 为 60s 总额 |
| websearch: onProvider progress | websearch.ts:63,191 (每次尝试前实时) | websearch.rs:749-755 (结果边界一次) | ⚠️ | 轮换过程中卡片 provider 元数据不更新 |
| websearch: 结果格式/NO_RESULTS/HTTP 错误映射 | websearch.ts:15-18,153-178 | websearch.rs:19,733-775 | ✅ | 429/401 专用文案 |
| websearch: disabled 时删工具 (context/compaction/generate) | websearch.ts:184-194 | websearch.rs:818-853; engine/mod.rs:2860-2878 | ✅ | 三 hook 均挂 |
| websearch: provider 集 exa/tavily/firecrawl/parallel/tinyfish | plugin/websearch/*.ts | websearch_providers.rs:171,443,524,588,658 | ✅ | ref 另有 mcp.ts 桥(Rust 以 plugin_providers 覆盖) |
| tavily: 25s 超时+专用消息 | tavily.ts:70-73 | websearch_providers.rs:52(MCP_TIMEOUT=25) | 🟡 | 有 25s 但超时消息非专用文案；其余 provider ref 无显式超时而 Rust 统一 25s |
| skill: get→permission→prepare 信封 | plugin/skill.ts:46-67; skill.ts:33-63 | skill.rs:121-184,22-92 | ✅ | 10 文件采样、SKILL.md 排除 |
| question: 权限/Form/答案映射/取消隧道 | question.ts:62-117 | question.rs:93-203 | ✅ | 无消息取消→Declined("aborted") |
| question: minItems 1 校验 | question.ts:24 | question.rs:106-114 | ✅ | 手工复刻 issue 文案 |
| list_mcp_resources: 权限/目录 | mcp-resource.ts:18-48 | mcp_resource.rs:62-98,190-226 | ✅ | 逐 server 断言语义一致 |
| read_mcp_resource: `{server}:{uri}` 权限/blob | mcp-resource.ts:49-97 | mcp_resource.rs:103-170 | ✅ | |
| opencode: namespace+session_rename/move/models | opencode.ts:82-197 | opencode_tools.rs:511-581,40-263,275-509 | ✅ | models 排序/family 去重/分页/own-provider 执行期解析 |
| opencode: worktree guidance 三 hook | opencode.ts:71-81 | opencode_tools.rs:9-38 | ✅ | |
| 注册表: effectiveName/normalizedName | runtime.ts:274-279 | registry.rs:26-41 | ✅ | |
| 注册表: 名称/命名空间校验+execute 保留+定义校验 | tool.ts:304-321,323-326 | registry.rs:44-70 | 🟡 | Rust 未调用 valid_namespace 校验命名空间、无 ToolDefinition 等价校验 |
| **注册表: 注册键=effectiveName(覆盖式)** | tool.ts:183-184 | location.rs:557-560(键=bare name); mcp/registry.rs:1886 | ⚠️ | 见风险#1：MCP 跨服务器同名工具互相覆盖 |
| 注册表: snapshot wholly-denied 过滤 | tool.ts:225-233,292-295 | location.rs:604-621,514-520 | ✅ | 最后匹配规则语义一致 |
| 注册表: decodeInput/issue 文案/normalizeContent/encodeOutput | runtime.ts:62-99,109-132,259-263 | registry.rs:79-123,269-275,333-378 | ✅ | 输出校验按声明 schema，未声明走 `{}` 兜底 |
| 注册表: before/after hook+错误腿 after+图片归一 | tool.ts:103-156 | registry.rs:240-482; step.rs:738-744 | ✅ | |
| MCP 工具合并: 命名空间/codemode/权限/结构化输出 | tool/mcp.ts:16-17,41-48,49-118 | mcp/registry.rs:1855-1908 + executor | ✅(除键) | 100ms 防抖 reconcile 同 |
| codemode: execute 描述/schema/权限禁用过滤 | codemode/tool.ts:62-76 | codemode.rs:72-97,115-119 | ✅ | 描述字节级 |
| tool_input_repair: 9 类修复/depth6/execute 豁免 | plugin/tool-input-repair.ts:18-170 | tool_input_repair.rs(全文件) | ⚠️ | 修复逻辑齐备，但查找键不一致(风险#2) |
| html_markdown: 5MB/CONTENT_BYTES/append 截断 | html-markdown.ts:50-99 | html_markdown.rs:34-35,600+ | ✅ | |

### 3. 关键差异与风险 Top 5

1. **⚠️ 注册表按 bare name 键控 → MCP 跨服务器同名工具互相覆盖**。Ref 以 `effectiveName`(`{server}_{tool}`)为键 (tool.ts:183)；Rust 以 bare `tool.name` 为键 (location.rs:558-560, registry.rs:154,168, mcp/registry.rs:1886-1894)。两个 MCP server 各暴露 `search` 时后者覆盖前者的注册与 executor，`{srv1}_search` 调用会路由到 srv2 的 executor——错误工具执行，属高危。
2. **⚠️ tool_input_repair 对带命名空间工具失效**。PipelineExecutor 派发的 `event.tool` 是 effectiveName，而修复 hook 用 bare-name 键控表查询 → `opencode_*` 与 MCP 工具全部查不到、不做入参修复；ref 中是命中的。
3. **⚠️ websearch onProvider progress 时序**：ref 每次尝试(含 429 轮换)前发 progress；Rust 只在结果边界发一次，轮换期间 UI 元数据陈旧。
4. **🟡 同意流程超时预算**：ref 60s 包住整个锁内流程；Rust 每张表单各 60s，choose 流程可拖到 120s。另 tavily 超时错误文案非 ref 专用句。
5. **🟡 注册校验缺口**：`valid_namespace`/定义有效性校验未接入 `registration_error`，非法命名空间工具在 Rust 可注册，ref 会拒绝并记错误日志。

### 4. 缺失文件/功能汇总
- 无整文件级缺失：ref 域内全部工具/服务/插件在 Rust 均有对应实现。
- Rust 新增（合理移植基建）：`user_command` 独立入口、`with_picker` 测试缝、`ripgrep_runner.rs`、pipeline_leaf 适配宏。
- 语义缺口仅上节 5 项；其中 #1/#2 建议优先修复（统一注册键为 effectiveName 或在 repair/executor 解析处做映射）。
