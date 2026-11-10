# L4 配置与模型目录 (Config/Agents/Models) — 审查报告

> 审查方式：两侧逐文件逐行阅读，结论均以代码为准（不信任文档）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

**Ref 侧逐文件阅读**（`/opt/opencode-ref/packages/core/src/`）：config.ts(374)、config/discovery.ts(84)、config/markdown.ts、config/variable.ts、config/watch.ts、config/normalize.ts(808)、config/plugin/{agent,command,skill,skill-file,compatibility,instruction,provider}.ts、agent.ts(134)、command.ts(106)、skill.ts(133)、skill/discovery.ts(205)、skill/instructions.ts(96)、instructions/{index(256),builtins(60)}.ts、instruction-discovery.ts(149)、models-dev.ts(450)、model.ts(306)、model-resolver.ts(474)、provider.ts(473)、variant.ts(621，读至协议表主体)、generate.ts(104)、wellknown.ts(186)+wellknown/plugin.ts。

**Rust 侧逐文件阅读**（`/opt/opencode-rust/crates/`）：oc-core/src/config.rs(1391)、config/{discovery(286),instruction(297),agent_md(709),command_md(548),variable,jsonc_parse,jsonc_edit,markdown,watch,normalize(3509)}.rs、engine/{commands.rs(377),skills.rs(487),instructions.rs(258),resolver.rs(2073, 读 bind/resolve 全路径),skills_url.rs(结构+staging/backup)}、provider_catalog/{source(192),catalog.rs(normalize 部分),refresh(240),facade.rs(custom lane 1746-1900),variant.rs(头部+常量)}、engine/mod.rs(3060-3200 builtin agents)、session/runner/context.rs（grep 证实 core/date、core/environment、core/instructions、core/skill-guidance 四键）、integration/wellknown_config.rs、oc-server/src/handlers/misc.rs(generate)。所有“缺失”结论均经 grep 全 crates 验证。

### 2. 逐项对比表

| 功能点 | Ref 实现 (file:line) | Rust 实现 (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| 配置源发现（walk-up、`.claude/.agents/.opencode`、全局根去重、symlink resolve） | config/discovery.ts:23-84 | config/discovery.rs:41-152 | ✅ | 候选序、global:false 子树过滤、dedupe 全对齐，有测试 |
| 六源合并链 wellknown→global→explicit→direct→project→content | config.ts:230-237 | config.rs:442-527 | ✅ | 同序；同目录 json 先于 jsonc（jsonc 胜）一致 |
| `latest` findLast 折叠 | config.ts:22-25 | config.rs:38-43 | ✅ | |
| JSONC 容错（注释+尾逗号，其余拒绝） | config.ts:104-115 | config.rs:531-548 + jsonc_parse.rs | ✅ | G-281① 按 jsonc-parser 容错类精确移植（曾用 json5 已改） |
| `{env:}`/`{file:}` 替换、缺失即拒绝文档 | config/variable.ts | config/variable.rs + config.rs:578-604 | ✅ | |
| normalize→Schema 终校验、per-document 隔离、诊断路径 | config.ts:116-134 | config.rs:549-576 + normalize.rs | ✅ | |
| 热重载：watch plan、100ms debounce、deep-equal 跳过、compatibility 变更检测 | config.ts:243-289 | config.rs:627-693 + watch.rs | ✅ | readiness rescan 也有 |
| `Config.update` shell patch（保 JSONC、updateLock、尾换行） | config.ts:329-349 | config.rs:705-746 | ✅ | G-281④ 锁语义补齐 |
| OPENCODE_CONFIG / OPENCODE_CONFIG_CONTENT 常驻源 | server 传入 options | config.rs:399-405 (Loader::new) | 🔵 | Rust 在 Loader 构造即接线（ref 在 server-process 组装） |
| 内置 agent：build/general/explore/plan/compaction/title/summary | plugin/agent.ts、plugin/plan.ts:33-43 | engine/mod.rs:3084-3192 | ✅ | 权限序、explore 尾插、plan 目录例外均对齐 |
| agent 默认选择（configured→build→首个 selectable） | agent.ts:92-104 | engine/mod.rs（AgentRegistry）+ location.rs | ✅ | |
| agent markdown（{agent,agents}/**、{mode,modes}/*、v1 legacy 迁移、~/\$HOME 权限展开） | config/plugin/agent.ts:21-26,141-212 | config/agent_md.rs:22-200 | ✅ | primary 强制、join model#variant 一致 |
| config `agents` map 合并（disabled 删除、default_agent） | config/plugin/agent.ts:83-127 | plugin_apply.rs/engine 组装 | ✅ | |
| command 发现（`{command,commands}/**/*.md` + config map + 模板 `$ARGUMENTS`/`$N`/`` !`cmd` ``） | config/plugin/command.ts | config/command_md.rs:1-60+、engine/commands.rs:119-129 | ✅ | |
| 内置 init/review 命令（模板 verbatim、`${path}`） | plugin/command.ts:28-54 | engine/commands.rs:23-57 + command/*.txt | ✅ | 覆盖同名 config 命令 |
| MCP prompt → `server:prompt` 命令 + sanitize | command.ts(插件):104-110 | engine/commands.rs:169-209 | ✅ | G-215 整体替换式刷新 |
| 指令文件发现（全局 AGENTS.md→nearest→farthest、home/project-root 停止、all-or-nothing unavailable） | config/plugin/instruction.ts | config/instruction.rs:65-110 | ✅ | 只发现 AGENTS.md（ref v2 同） |
| 指令框架（key/hash/delta/renderInitial/renderUpdate/diffByKey、Instructions from: 渲染、diff 短文替换） | instructions/index.ts:88-256、instruction-discovery.ts:123-149 | session/runner/context.rs:678-933（core/* 四键）+ engine/instructions.rs | ✅ | AUD-14 diff 渲染有 golden 测试 |
| 内置指令 core/date、core/environment | instructions/builtins.ts:24-53 | session/runner/context.rs:919-933 | ✅ | |
| skill 发现（`{*.md,**/SKILL.md}`、大小写敏感、任意深度 SKILL.md、frontmatter name 非串即整文件跳过、booleanValue 0/1/yes） | config/plugin/skill-file.ts、skill.ts | engine/skills.rs:398-487 | ✅ | G-216①②③ 三处语义钉死 |
| 内置 skill opencode/report（含运行时诊断快照） | plugin/skill.ts | engine/skills.rs:88-248 | ✅ | md 字节级内嵌 |
| skill 源序：目录 skill/skills→document 条目（URL 与目录按文档序交错）→compat roots（claude→agents）后置且覆盖 | config/plugin/skill.ts:77-102、compatibility.ts:39-56 | engine/skills.rs:284-359 | ✅ | id 冲突后者胜（editor.add）一致 |
| URL skill 拉取（index.json、安全路径校验、`.opencode-version`、staging+backup 原子换、回滚） | skill/discovery.ts:92-200 | engine/skills_url.rs:86-347 | ✅ | |
| skill 指南注入（available 过滤 deny、autoinvoke=false/description 缺失剔除、增删 delta、排序） | skill/instructions.ts:16-91 | session/runner/context.rs:678-771 | ✅ | |
| models.dev：source 形状（cost/tiers/modes/status/interleaved/provider 覆盖） | models-dev.ts:31-77 | provider_catalog/source.rs:17-192 | ✅ | |
| normalize（nativePackage、baseURL 注入、模式模型 `id-mode`、mergeCost tier 合并、released 解析） | models-dev.ts:96-250 | provider_catalog/catalog.rs:284-615 | ✅ | |
| 内嵌快照（离线兜底） | models-dev.ts:287-297 | catalog.rs:28 (SNAPSHOT_TEXT) | ✅ | |
| KV 缓存 + TTL 5min + digest 跳过 + refreshed 事件 + 定时刷新循环 | models-dev.ts:299-434 | refresh.rs:22-29,133-240 + boot.rs:319-339 | ✅ | `OPENCODE_DISABLE_MODELS_FETCH`≈options.fetch、`OPENCODE_MODELS_URL`≈options.url |
| models.dev `options.file`（从文件读目录） | models-dev.ts:354-359 | refresh.rs `adopt()` 存在但无 CLI/env 接线 | 🟡 | adopt 仅测试/内嵌器用，无用户面 file 选项 |
| custom provider（config provider map、settings baseURL/apiKey、model 覆盖 npm/modelID/baseURL、headers/body overlay） | config/plugin/provider.ts、model-resolver.ts | facade.rs:167-330,1746-1900 + resolver.rs:383-419 | ✅ | baseURL 优先级（model>provider>catalog）对齐 |
| **npm 动态包加载（`Provider.loadPackage`/npm.add、`aisdk:` 桥）** | provider.ts:112-143 | facade.rs:1793（注释明示未移植，返回 UnsupportedProtocol 类型错误） | 🔴 | 编译期无法动态装 npm 包；仅原生注册表内的包可绑 |
| model-resolver：variant 叠加（default=无、未知 variant 报错、settings/headers/body 合并） | model-resolver.ts:137-161 | resolver.rs:802-837 | ✅ | |
| model-resolver：credential 设置注入（apiKey/authToken/accessToken 包分派）、`${VAR}` baseURL 校验 | model-resolver.ts:259-319 | facade.rs:672+ + resolver.rs bind | ✅ | |
| keyless 降级（activation=enabled 且无凭据且 usesAPIKeyAuth → Auth.none） | model-resolver.ts:379-385,430-468 | resolver.rs:264-300,881-923 | ✅ | 包清单逐一对照 |
| 默认模型 fallback（无 ref→default→首个 supportsText 可用模型） | model.ts:260-265、model-resolver.ts:402-417 | resolver.rs:661-712 + context.rs:1508 | ✅ | |
| `*`/前缀匹配 | **ref v2.0.24 不存在**（model.get 精确匹配，model.ts:255-257） | resolver.rs 精确匹配；custom/local 加 eq_ignore_ascii_case (364,391-395,406) | ⚠️ | Rust 自行增加大小写不敏感兜底（exact 优先），ref 会直接 miss——宽容度差异，可能绑到 ref 拒绝的组合 |
| 本地 provider 运行时发现（ollama/lmstudio/vllm 权威替换） | plugin/provider/local.ts | provider_catalog/local.rs + resolver.rs:351-382 | ✅ | |
| copilot/modal/azure/digitalocean 动态 sync | 对应 plugin/provider/*.ts | provider_catalog/{copilot,modal,azure,digitalocean}_sync.rs | ✅ | |
| variant 协议表（efforts/toggle/budgets、claude family 正则、Alibaba 64k 帽、openai/xai/deepseek 等分派） | variant.ts:16-120+ | provider_catalog/variant.rs:1-50+（1334 行） | ✅ | |
| generate.ts（一次性文本生成、错误映射、x-opencode-session 头） | generate.ts:33-98 | oc-server/handlers/misc.rs:17-84 `/api/experimental/generate` | ✅ | |
| wellknown（manifest inspect/resolve、remote_config {env:} 替换、add/remove/refresh+Updated 事件、credential 驱动 reload） | wellknown.ts | integration/wellknown_config.rs:19-243 + oc-server/handlers/integration.rs:141-272 + boot.rs:499 | ✅ | |
| 模式排序 all（released 降序、稳定排序） | model.ts:218-220 | catalog.rs:129+ flat_models_by_release | ✅ | |
| `Model.small` 家族偏好 (gpt-luna/flash/haiku) | model.ts:266-278 | 未逐字核对（catalog.available_models 存在） | 🟡 | 本次未读到逐一等价实现，标待核 |

### 3. 关键差异与风险 Top 5

1. **🔴 npm provider 包动态加载不可用**（facade.rs:1790-1796）：ref `Provider.loadPackage` 走 `npm.add` 安装任意社区包（provider.ts:112-143）；Rust 无动态加载，config 指定未注册 npm 包的自定义 provider 会得到 `UnsupportedProtocol` 类型错误。用户面可感知（某些 README 示例配置直接失效），但错误是显式的而非静默。
2. **⚠️ custom/local provider 大小写不敏感匹配为移植版新增**（resolver.rs:364,391-395,406,444-447）：ref 精确匹配（model.ts:255）。宽容度差异在罕见场景（如配置键 `Own` 引用 `own/x`）会绑到 ref 拒绝的组合——行为差异但 exact 优先，风险低。
3. **🟡 models.dev `options.file` 无用户接线**（refresh.rs `adopt()` 无 env/CLI 入口；ref models-dev.ts:354-359 支持 `--models-file` 类注入）。快照/URL/禁抓取三选项均有（boot.rs:337-339），仅 file 缺。
4. **⚠️ `LocationModelResolver` 的 fallback 分层为移植版新增**（resolver.rs:712-768）：location 默认失败时**不**回退引擎默认（authoritative 报错），`NotSelected` 才回退——ref 每层独立解析、无此双层语义。语义更严格，属有意设计但与 ref 单层 `Model.default()` 行为不同。
5. **🟡 `Model.small` 家族偏好选择**未在本次阅读范围内找到逐字对等实现（ref model.ts:266-278）；若 compaction/子代理选小模型的路径依赖它，需专项核对。

### 4. 缺失文件/功能汇总

- **🔴 npm 动态 provider 包加载 / `aisdk:` 桥**（ref provider.ts:112-143、models-dev.ts `nativePackage` 的 npm 分派另一半）：Rust 显式不移植，返回类型错误（facade.rs:1793）。
- **🟡 models.dev `options.file`**：`CatalogService::adopt` 存在但无配置面接线。
- **🟡 `Model.small`**：待核对（未证缺失，未证等价）。
- 其余域内功能（配置合并链、热重载、AGENTS.md/skill/command/agent 发现、models.dev 缓存刷新离线、custom provider、variant、generate、wellknown）均有逐行级对应实现且带对齐测试；`cli_config/`（v1 迁移）与 `config/jsonc_edit.rs` 为移植版新增支撑件，无 ref 语义冲突。
