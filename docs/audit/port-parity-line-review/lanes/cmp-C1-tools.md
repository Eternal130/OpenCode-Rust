# C1 结论: 内建工具域整体高度对齐（含多条字节级 pinned 文案与多层已修复的 gate），未发现 P0；存在少量有意的输出校验豁免（G-230）、进度上报时机、question 布尔回答映射与注册失败通道等 🟡 级差异。

## 对比表

| 功能 | ref 实现 | rust 实现 | 实现方式差异 | 状态 |
|---|---|---|---|---|
| 注册表/命名 | tool.ts:26-57,169-205,274-279（Editor、effectiveName、RegistrationError 收集+logError） | registry.rs:26-70 + location.rs:529-643（BTreeMap 按 effectiveName 键） | ref 将无效注册记录 errors 并跳过继续；rust `register` 直接 `Err(String)` 上抛（见 D-6） | 🟡 |
| snapshot whollyDisabled | tool.ts:225-241,292-295 | location.rs:517-520,608-625 + codemode.rs:108-122 | 语义一致（最后匹配规则 resource=* deny）；direct/codemode 拆分与 `execute` 整体禁用判断在 codemode.rs | ✅ |
| 执行管线 (decode→execute→after hook) | tool/runtime.ts:28-60 + tool.ts:103-156 | registry.rs:243-503（PipelineExecutor） | decode 失败也触发 after-error（G-321）；decline 隧道、permission.rejected 分类对应 | ✅ |
| encodeOutput | runtime.ts:45-56,109-132 | registry.rs:353-398（G-230） | ref 内建声明 Output schema 并校验；rust 内建全部走未声明豁免（见 D-1） | 🟡 |
| normalizeContent/normalizeImages | runtime.ts:259-272 + tool.ts:74-101 | registry.rs:107-123；normalizeImages 在 image.rs:238 + session/runner/step.rs:704+ 执行 | 位置从工具服务挪到 runner seam，语义保留 | ✅ |
| tool-input-repair | plugin/tool-input-repair.ts:21-170 | tool_input_repair.rs（959L 全量规则+maxDepth 6+`execute` 豁免） | 忠实移植 | ✅ |
| read 工具 | tool/plugin/read.ts + read-filesystem.ts | read.rs（1013L） | 逐段对齐：常量(2000/50KiB/20MiB/256KiB/行宽2000)、目录排序、分页 rope、OffsetOutOfRange、Binary 判定均一致 | ✅ |
| read AGENTS.md 注入 | read.ts:87-107（fs.resolve+fs.up+contains 过滤） | read.rs:925-967（lexical 向上走到 root 排除） | ref 用符号链接规范化的 fs.resolve/up；rust 词法路径（D-2） | 🟡 |
| read alternatePath/missing 建议 | read.ts:138-171 | read.rs:773-850 | NFC+NBSP/弯引号规范、唯一 sibling、3 条建议一致 | ✅ |
| edit | tool/plugin/edit.ts | edit.rs | 三段匹配（exact→unicode→line-trimEnd）、CRLF 主导换行、BOM 保持、assert 先于 0/多匹配失败、锁 — 全对齐；unicode pass 用字节偏移回映（更稳） | ✅ |
| write | tool/plugin/write.ts | write.rs | added/modified 预览、BOM 保持写、formatter resync、输出结构一致 | ✅ |
| file-diff | plugin/file-diff.ts + jsdiff | file_diff.rs（LCS+4 上下文+hunk 合并+`\ No newline`） | 自实现 jsdiff 等价物，测试字节级 pin | ✅ |
| patch | tool/plugin/patch.ts + util/patch.ts + patch.txt | patch.rs + patch_util.rs + patch.txt(嵌入) | heredoc 解包、Move to、End of File、累积 update 链、单一 edit assert（Set 去重资源）、部分失败 `Completed before failure`、trimDiff、gpt-* codemod hook 全对齐 | ✅ |
| glob | tool/plugin/glob.ts | glob.rs | `"undefined"/"null"` 降级、rg 参数、limit+1 截断、超时 30s 文案一致 | ✅ |
| grep | tool/plugin/grep.ts | grep.rs | UTF-16 钳制（G-282⑤）、InvalidPattern→`Invalid regex pattern:`、exit-2 容忍、位置相对化输出一致 | ✅ |
| ripgrep 获取 | ripgrep/binary.ts | ripgrep_runner.rs（15.1.0、env→bundled→PATH→cache→download） | rust 加 `OPENCODE_DISABLE_RG_DOWNLOAD` 开关（➕rust独有，无害） | ✅ |
| shell | tool/plugin/shell.ts | shell.rs | 名称两侧均为 `shell`（提示所称 ref 用 `bash` 不成立——shell.ts:22）；DEFAULT_TIMEOUT_MS/BACKGROUND_INSTRUCTION/描述/元数据/超时附言字节级一致；progress `{shellID}`；Drop guard = onInterrupt | ✅ |
| shell 风险扫描/prepare | shell.ts:116-152 | shell.rs:167-267（parse::scan→目录授权→assert→cwd 探测） | portable_scanner 开关、目录资源、cwd follow 语义一致（OpaqueReason 实现在 shell/parse，另 lane） | ✅ |
| question | tool/plugin/question.ts | question.rs | multiselect/string 字段、custom、取消隧道一致；布尔回答映射差异（D-3）；空数组错误文案手写（D-4） | 🟡 |
| skill | tool/plugin/skill.ts | skill.rs | `<skill_content>` 信封、10 文件采样、SKILL.md 排除、permission action `skill` 一致 | ✅ |
| subagent | tool/plugin/subagent.ts | subagent.rs | 名称两侧均为 `subagent`（提示所称 `task` 不成立）；深度限制/agent-mode 拒绝/child 校验/switch 语义/background 文案/`<subagent>` 信封/metadata{sessionID,status}/空串参数剥离全对齐 | ✅ |
| webfetch | tool/plugin/webfetch.ts + http-body.ts | webfetch.rs | 常量 30/120/5MiB 一致；Accept 三态头、UA、cf-mitigated 重试、Content-Length 预检+流式上限、整流程超时预算（G-89）、timeout 先校验后 assert（G-98）。**两侧均无 llms.txt/prompt/extract_main（提示说法不成立，v2.0.24 源无此参数）** | ✅ |
| html→markdown | html-markdown.ts (htmlparser2) | html_markdown.rs（自带 tokenizer 子集） | 转换器状态机逐分支对齐（含 5MiB/64KiB 预算、表格/引用/代码栅栏/marketer 规则）；命名实体表为子集（D-5）；非流式喂数 | 🟡 |
| websearch | tool/plugin/websearch.ts + websearch.ts | websearch.rs + websearch_providers.rs | 单信号量（tokio::Mutex）、1 分钟总预算覆盖双表单（G-310）、429 轮换+Retry-After 冷却、KV 选中清洗、locale 排序、结果格式一致；progress 时机差异（D-7） | 🟡 |
| mcp 资源工具 | tool/plugin/mcp-resource.ts | mcp_resource.rs | namespace `opencode`+codemode:true、action 名、`{server}:{uri}` 资源、`{server}:*` 保存、blob 图/PDF file part、目录 JSON 文本化（G-60/G-191）一致 | ✅ |
| opencode.session_rename | opencode.ts:90-108 | opencode_tools.rs:41-116 | title trim/空拒绝/sessionID 缺省当前会话/输出文案一致（直接发 session.renamed 事件替代 ctx.session.update） | ✅ |
| opencode.session_move | opencode.ts:110-133（pinned:true, steer） | opencode_tools.rs:121-263 | steer 投递+唤醒一致；pinned 在 codemode.rs:191 以路径特判补齐 | ✅ |
| opencode.models | opencode.ts:135-197 | opencode_tools.rs:275-509 | own-provider 会话时解析（G-86）、family 去重、分页 next、limit 钳 1..100、locale 排序近似一致 | ✅ |
| 工作区指引 system part | opencode.ts:71-81 | opencode_tools.rs:15-38 | 三 hook 全挂（G-266③） | ✅ |
| 内建插件注册顺序 | internal.ts:213-247（pre 列表） | mod.rs:288-328（register_all/builtin_plugins，repair 最先、patch hook 最后） | 顺序一致 | ✅ |
| MCP 工具桥 | tool/mcp.ts（本次顺带核对） | mcp/{client,registry}（另 lane） | 不在本 lane 深审 | — |

## 行为差异清单

D-1: 内建工具输出不经 Output-schema 校验。
ref 每个内建工具声明 `output` schema 且 runtime.ts:53-54 强制“声明了 schema 必须返回 output 并通过 encodeOutput 校验”；rust registry.rs:164/220-237 将全部内建置于 `output_schema: None` 的 MCP 式豁免（注释 G-230 自认）。read/edit/glob/grep/patch/write/shell/webfetch/websearch/subagent/mcp_resource 均返回 `output: Some(...)` 但不校验——畸形 output 不会被拒。用户可见影响：几乎无（内建自产 output），但插件改写 output 后无再校验，且与 ref 的运行时契约不同。ref plugin/edit.ts:36-40 vs rust edit.rs:431-437。

D-2: read 的 AGENTS.md 发现用词法路径。ref read.ts:89-100 经 `fs.resolve`/`fs.up`（符号链接规范化、经 Files 层）；rust read.rs:937-961 纯词法向上遍历+`type_following` 探测。符号链接目录下 AGENTS.md 的去重/发现路径可能不同（重复注入由 ledger 去重兜底）。

D-3: question 布尔回案映射。ref question.ts:99-104 `typeof value === "object" → Array.from` 否则 `[String(value)]`，即 `true` → `["true"]`；rust question.rs:160-167 `Bool(true)→[]`、`Bool(false)→[]`。正常 string/multiselect 字段不会产生布尔，但 custom 或异常客户端回复下答案不同（ref 回 `"true"`，rust 回 Unanswered）。

D-4: question 空数组校验错误文案为手写近似。rust question.rs:110-114 硬编码 `"Expected a value with a length of at least 1"` + `JSON.stringify(…,2)` 格式；ref 由 effect schema 生成（formatInputIssues 顶部工具名/参数路径格式一致，但底层 issue 措辞与 pretty-print 由库产生）。低风险文案漂移。

D-5: HTML 命名实体表为子集。ref htmlparser2 完整实体表；rust html_markdown.rs:371-437 仅约 60 个常用实体+全部数字形式。生僻实体（如 `&hellip;` 有、`&rsquor;` 无）保持原样进入 markdown 文本。webfetch markdown 输出的稀有实体页面有轻微文本差异。

D-6: 注册失败通道。ref tool.ts:177-200 将 RegistrationError 记入 errors 数组、notify 时 logError 并继续（其余工具照常注册）；rust mod.rs `register_all`/`register_builtin` 在首个无效注册时整体 `Err` 上抛使位置 boot 失败。对静态内建无影响；对 MCP/插件注册（复用同一 `registry::register`）时，一条坏工具会拖垮整批注册而非跳过。ref tool.ts:177-218 vs rust registry.rs:141-144。

D-7: websearch provider 进度时机。ref websearch.ts:58-64 `onProvider` 在 provider 接管时（请求进行中）发 `context.progress({provider})`；rust websearch.rs:765-771 在结果边界一次性补发。运行中搜索卡片的 provider 元数据到达更晚。

D-8: 命名事实澄清（对任务前提的更正）：ref v2.0.24 的工具名就是 `shell`（shell.ts:22）与 `subagent`（subagent.ts:17），不存在 `bash`/`task` 命名；rust 同名。同理 ref webfetch（webfetch.ts:23-31）无 `MAX_TIMEOUT` 之外的 llms.txt 探测、`prompt`、`extract_main`、`save_binary`、`prefer_llms_txt` 参数——两侧一致地没有这些功能；该前提疑来自其他版本。

D-9: shell create-before 派发顺序。ref 在 `shell.create(…, before)` 内联闭包里跑 hook+prepare（shell.ts:200-216）；rust 先 `trigger_create_before` 再 `prepare` 再 `create_invocation`（shell.rs:329-355）。等效（都在 spawn 前），但 ref 的闭包在 shell 服务内部上下文执行，可改写 `invocation.timeout` 的时点略不同——rust 显式回写 `invocation.timeout = final_timeout`，语义等价。

D-10: skill 文件采样走宿主 std::fs（skill.rs:78-92）而非 Files seam；ref `fs.scan` 经文件系统服务。远程 workspace 位置下 rust 仍读宿主路径（与 G-1 目标不一致），本地行为一致。

## 正确性风险

R-1 (P2): D-6 注册失败通道——恶意/损坏的 MCP 工具名（如超 128 字符或含非法字符）在 rust 会使该位置全部工具注册失败，ref 仅跳过该工具并记日志。可用性差异，非数据风险。证据：ref tool.ts:177-218 vs rust tool/mod.rs:288-305、registry.rs:141-144。

R-2 (P2): D-3 question 布尔回案。自定义表单回复路径上模型看到的答案与 ref 不同（Unanswered vs "true"）。证据：ref question.ts:99-104 vs rust question.rs:160-167。

R-3 (P2): D-1 输出 schema 豁免为已知有意偏离（注释声明“compat survey found 14 builtin tools returning output”），但意味着 ref 会在 encodeOutput 拒绝的畸形输出在 rust 静默通过；若未来插件在 `execute.after` 改写 output，rust 不再校验。建议后续把内建 Output 声明补齐。证据：registry.rs:220-237 注释 vs ref runtime.ts:45-56。

R-4 (P2): D-5 实体子集——webfetch markdown 对含生僻实体的页面产出与 ref 不同的文本（不影响安全与结构）。

R-5 (P2): read.rs:969-977 非媒体 base64 分支在 AGENTS.md 注入之后才失败（ref read.ts:108-113 同序——注入先于二进制拒绝），两侧一致，无风险（列此存档）。

未发现 P0/P1：所有权限断言次序（edit/write/patch 的 assert 先于匹配失败）、外部目录授权、decline 隧道、超时预算、后台通知携带全量输出（G-267①）等关键安全/语义点均逐一对齐。

## 覆盖声明

已完整逐行读完（ref，/opt/opencode-ref/packages/core/src/）：tool.ts、tool/runtime.ts、tool/read-filesystem.ts、tool/http-body.ts、tool/html-markdown.ts、tool/patch.txt、tool/plugin/{read,edit,write,patch,file-diff,glob,grep,shell,question,skill,subagent,webfetch,websearch,mcp-resource,opencode}.ts、plugin/internal.ts、plugin/tool-input-repair.ts；顺带读了 tool/mcp.ts（MCP 桥，标注另 lane）。

已读完（rust，/opt/opencode-rust/crates/oc-core/src/tool/）：mod.rs、registry.rs、read.rs、edit.rs、write.rs、glob.rs、grep.rs、shell.rs（含内联测试）、question.rs、skill.rs、subagent.rs、webfetch.rs、websearch.rs、mcp_resource.rs、opencode_tools.rs、file_diff.rs、patch.rs、patch_util.rs、ripgrep_runner.rs、tool_input_repair.rs（前 200 行全读+结构核对）、html_markdown.rs（1-1702 行全读，1703-2047 为 escape_title 尾部+extract_text_from_html+内联测试，已 grep 定位核对关键符号）、codemode.rs（按指示仅读注册面：1-260 行 + pinned/catalog grep，解释器语义归 C8）。另读 location.rs:500-679（snapshot/wholly_denied）并 grep 验证 normalizeImages 于 image.rs/step.rs 存在。

部分读（含理由）：websearch.rs 1217-1538 行为内联测试（截断处）；websearch_providers.rs 仅读 1-120 行+结构 grep（对应 ref plugin/websearch/*，明确不在本 lane 的 ref 范围内，故按 lane 边界浅核）；tool/tests.rs、tests_subagent.rs 按任务说明仅在需要行为证据时查阅（本报告引用了其中的 harness 用例，未全读）。
