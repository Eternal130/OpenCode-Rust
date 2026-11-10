# C4 结论: 配置/环境/项目/workspace/wellknown/v1 域整体为高保真逐行移植（发现/替换/normalize/watch/v1 迁移语义对齐到诊断消息级），但有 3 个配置键在 rust 侧被接受却从不生效（`mcp.timeout` 文档级默认、`snapshots`、`lsp`）、1 个 rust 独有键（`experimental.file_retention`）、以及 AGENTS.md 全局文件读失败被过度降级为整体 unavailable 的行为差异。

## 对比表

| ref (v2.0.24 TS) | rust | 对齐度 | 要点（双侧 file:line） |
|---|---|---|---|
| config.ts (374L) | oc-core/config.rs (1629L, 含测试) | 高 | 六源优先级 wellknown→global→explicit→direct(根最近最后)→project→content 一致（ref config.ts:230-237 vs rust config.rs:493-571）；`latest`=findLast 一致（config.ts:22-25 vs config.rs:38-43）；`Config.update` shell 补丁+尾换行+updateLock 一致（config.ts:329-349 vs config.rs:787-828，G-281④ 进程级锁） |
| config/discovery.ts | config/discovery.rs | 高 | 候选序 `.claude/.agents/.opencode/jsonc/json` 一致（discovery.ts:41 vs discovery.rs:13-14）；global roots/global files 过滤、symlink 经父目录 resolve、claude/agents 去重序一致（discovery.ts:52-83 vs discovery.rs:74-151） |
| config/variable.ts | config/variable.rs | 高 | `{env:}`缺失→空串、`{file:}` `//`注释行保留、trim+JSON 内联转义、NotFound 消息格式一致（variable.ts:27-83 vs variable.rs:70-131） |
| config/normalize.ts (808L) + v1/config/* | config/normalize.rs (3506L) | 高 | 逐键迁移表一致：unsupported 清单（normalize.ts:45-54 vs :65-75）、autoupdate/autoshare/snapshot/attachment、small_model→title agent（ref :137-151 vs rust :359-386）、mode→primary、providers 迁移（azure/vertex-anthropic 特例、options 三分、Math.trunc limit、cost context_over_200k→tier）、mcp v1（enabled→disabled、timeout→catalog+execution、oauth camelCase）、skills paths+urls、compaction reserved/buffer prefer、experimental enabled/disabled_providers→provider.use 规则、PositiveInt 拒 0（G-281②）。诊断三类消息文本逐字相同 |
| config/watch.ts + config.ts reload 环 | config/watch.rs | 高 | plan：global+present .opencode 目录递归（ignore node_modules/.git）、散文件按父目录 entries、去重键一致（watch.ts:8-39 vs watch.rs:85-154）；100ms debounce+串行 reload+深等价跳过+config.updated 一致（config.ts:265-289 vs watch.rs:240-284, config.rs:748-775）；rust 额外把 skill 源目录并入 plan（G-320，见 D-3） |
| config/markdown.ts | config/markdown.rs (212L) | 高 | gray-matter 失败→sanitize（未引号冒号→块标量）重试语义移植 |
| 21 个 config/plugin/* | 分布式对应（无 1:1 文件） | 中高 | instruction→config/instruction.rs；agent/command→agent_md.rs/command_md.rs；compatibility(skill 兼容根)→engine/skills.rs:429-438；mcp→engine/mod.rs:3330；policy→config/managed_policy.rs+ManagedPolicyModelResolver(engine/mod.rs:3016)；formatter→formatter::BuiltinsFormatter::from_config_documents(engine/mod.rs:1534)；shell→config_shell_of(engine/mod.rs:2892)；tool_output→G-33/G-48 热重应用；websearch→config_websearch_selection(engine/mod.rs:2941)；watcher.ignore→filesystem/location_watcher.rs:30；reference→reference.rs；plugins→plugin_apply.rs:243；provider→provider_catalog/overlay.rs:53；media/image→image.rs:71；**snapshot/lsp 无消费者（R-2/R-3）**；entry-observer→oc-server boot ConfigReloadBridge(boot.rs:92-115)→bus `config.updated`(bus/registry.rs:302) |
| project.ts + project/{schema,sql}.ts | project.rs (2124L) | 高 | resolve 链 remote-URL(https/scp 规范化小写 host)→cached(commonDir/opencode 文件)→rootCommit；hg roots(all()) 兜底；无 VCS→`directory:<path>` Hash.fast(SHA-1, DB-D1)；upsert 只刷 vcs 标签保留 canonical；markerless 收养→Worktree.Resolved(commit-with-insert)；activate 60s 限流且不动 time_updated（project.ts:241-255 vs project.rs:393-418） |
| workspace.ts + workspace/ | environment/workspace.rs | 中 | create/provision/connect/destroy + workspace 表 + Binding 不透明存储一致；**空闲 20min 挂起 janitor、共享 provision attempt、连接池/destroy-vs-provision 竞态为已注册的延期项**（workspace.rs:1-11 头注） |
| environment/（driver/files/exec-defaults/local/memory/unavailable） | environment/（+pty/stdio/workspace 扩展） | 高 | Files 契约（read 跟随 symlink/stat 不跟随/64MiB 上限/44-45-46 退出码/LC_ALL=C 脚本逐字移植）一致（exec-defaults.ts:20-90 vs exec_defaults.rs:21-120）；per-location workspace 绑定 orDie 语义一致（environment.ts vs mod.rs:65-76）；rust 增 pty/stdio spawner 面（G-1，rust 新增能力） |
| wellknown.ts + wellknown/plugin.ts | integration/wellknown_config.rs + config.rs WellknownSource | 高 | manifest schema、`{origin}/.well-known/opencode`、remote_config `{env:}` 替换（credential 变量→进程 env→空）、`wellknown:sources` KV 键、refresh 深等价门、文档为最低优先级层一致（wellknown.ts:60-96 vs wellknown_config.rs:95-126） |
| state.ts (252L) | state_transform.rs (433L) | 中 | 有界移植：仅 agent 域、急切重建（无懒读/revision）、组隔离仅限 state transform（不含 tool/mcp 行）——头注 :22-33 明示三条分歧 |
| v1/（config 兼容, 602L） | normalize.rs 内联 + cli_config/v1.rs | 高 | v1 config 迁移并入 normalize（见上）；CLI cli.json 的 v1→v2 键位手术在 cli_config（值级等价、注释丢失为已注册分歧， cli_config/mod.rs:33-35） |
| ref v1→v2 数据迁移（database/v1-migration*, 原地） | v2_migration.rs + database/v1_migration（隔离复制） | 设计性分歧 | rust 数据根 `opencode-rust` 隔离，一次性单向 COPY（db backup API、auth.json、config 文件、npm/bin 缓存、state 根文档），源永不修改；marker `v2-migration.json`+flock+30s 等待/10min 陈旧锁（v2_migration.rs:1-52） |

## 配置键差异表（顶层键全集 ref schema/config.ts:25-110 vs oc-schema/config/mod.rs:54-137）

| 键 | ref 默认 | rust 默认 | 备注 |
|---|---|---|---|
| $schema/shell/model/default_agent/update/share/enterprise/username/permissions/agents/snapshots/watcher/formatter/lsp/media/tool_output/mcp/compaction/skills/commands/instructions/references/websearch/plugins/worktree/warming/providers/experimental | optional，无 schema 级默认 | 同（Option + skip_serializing_if） | 顶层键面 1:1；Document.path optional 一致 |
| experimental.portable_shell_scanner | false（描述性） | 同 | |
| experimental.subagent_depth | 1（描述性） | 同 | |
| experimental.policies | optional | 同 | ref normalize.ts:407-445 vs rust :1062-1136 语义一致（含 LenientPolicy 宽松解码） |
| **experimental.file_retention.cleanup_interval** | **无此键**（ref schema/config/experimental.ts 仅 3 字段；normalize 丢弃） | 有（experimental.rs:12-16，默认 3_600_000 描述） | **rust 独有键**（D-1） |
| media.image.max_base64_bytes | optional | 同（media.rs:18） | 键在 |
| mcp.timeout.{startup,catalog,execution} | optional；经 ConfigMcpPlugin 与每 server timeout 浅合并（mcp.ts:50-55） | schema/normalize 保留（normalize.rs:920-942 排序 startup<catalog<execution） | **engine 消费缺失（R-1）** |
| snapshots | optional → Snapshot 服务开关（snapshot.ts 插件） | schema/normalize 保留 | **无消费者（R-2）** |
| lsp | optional → 内建 LSP 开关/覆盖（lsp 插件） | schema/normalize 保留 | **无 LSP 服务，键惰性（R-3）** |
| warming.interval/duration | "4 minutes"/"30 minutes"（运行时默认） | 同（warming.rs 注释） | 值级一致 |
| websearch | false \| {provider}（true 为 schema 错误） | 同（G-160① 拒 true，websearch.rs:53-83） | 精确对齐 |
| command.subtask | subagent 的废弃别名 | 同（serde alias 面） | |
| provider.* 嵌套 | 一致（含 cost 一或多、tier.context.size、settings.rest） | 一致；rust 另有内部 `rest`/`tier_type` 展开字段 | 键面等价 |

子 schema 逐文件比对（agent/command/compaction/formatter/lsp/mcp/media/plugin/policy/reference/shell/tool-output/warming/watcher/websearch/worktree/model/provider）：除上述三项外字段集一致（grep 键面 + 抽读确认；ref 的 `description` 命中均为 annotate 注释非字段）。

## 行为差异清单 D-n

- **D-1 rust 独有配置键 `experimental.file_retention.cleanup_interval`**：normalize.rs:1092-1111 将其保留进 encoded，oc-schema experimental.rs:30-33 定义该字段；ref v2.0.24 schema 无此键且 normalizeExperimental（ref normalize.ts:381-448）会丢弃。为 rust 侧 file-retention 清扫的私有扩展（forward-compat 分歧，已注释声明用途）。
- **D-2 Loader 构造即注入 env 源**：rust `Loader::new`（config.rs:437-452）把 `OPENCODE_CONFIG`、`OPENCODE_CONFIG_CONTENT`、`OPENCODE_CONFIG_PROJECT_DISABLE`/`OPENCODE_DISABLE_PROJECT_CONFIG` 烘进每个 loader 实例（含 engine 内部的 provider overlay / URL-skill 加载点）；ref 只在 server-process 组装 Config layer 时传入一次（options 传递）。效果等价（每 location 同一文档），但 rust 的覆盖面更宽；项目禁用 env 的双名 OR 语义与 ref `??` 的差异已注释（config.rs:380-390）。
- **D-3 reload 触发面**：ref 的 config.reload 仅由 watch 事件（debounce 100ms）、credential.switched（命中 wellknown integration 时）、wellknown.updated、10min wellknown refresh 驱动（config.ts:282-326）；rust WatchedConfig.do_reload 额外比较 skill 源签名（config.rs:681,753-766，G-320）并把 wellknown 刷新/凭证事件交给宿主桥（wellknown_config.rs:48-50 注释声明）。机制不同、净行为对齐。
- **D-4 AGENTS.md 全局文件读失败降级**：ref 中全局 `Global.config/AGENTS.md` 读失败只返回空（instruction.ts globalSource `file ? [file] : []`），仅 project 走查文件读失败才 `Instructions.unavailable`；rust config/instruction.rs:104-107 对**任何**候选（含全局文件）读失败即整体 `Unavailable`。过度降级（R-4）。
- **D-5 v1→v2 vs v2(TS)→rust 数据迁移**：ref 的 v1→v2 数据迁移在原目录原地执行；rust 因 XDG 根隔离（`opencode-rust`）改为一次性单向 COPY（db 经 rusqlite backup、state 根文档、插件缓存），源只读、marker+latch（v2_migration.rs:1-52）。为已注册的架构性分歧，非缺陷。
- **D-6 state.ts 注册表语义有界化**：rust state_transform.rs 只移植 agent 域的 transform/reload/组失败隔离，急切重建替代懒读+revision；组隔离不跨域（ref 一个 plugin 失败掉其全部域注册）。头注 :22-33 明示三条分歧。
- **D-7 workspace 生命周期缩减**：空闲挂起 janitor（ref 20min 阈值 + 轮询）、共享 provision attempt/Deferred 取消、连接池 spawner 包装、destroy 与竞争 provision 的 NotFound 结算均延期（workspace.rs:1-11）；当前仅 local driver，无行为面损失。
- **D-8 jsonc 容错面**：rust 用自写 jsonc_parse 精确复刻 ref jsonc-parser 容错类（注释+尾逗号，其余拒绝，G-281①，config.rs:583-595）；normalize 测试内部用 json5 仅测试便利，非生产行为。
- **D-9 CLI 配置 v1 手术**：ref 对 cli.json 原文做重复键 v1 keybind 原地手术；rust 值级等价后整文重排版（一次性迁移丢注释），已注册（cli_config/mod.rs:33-35）。

## 正确性风险 R-n

- **R-1（中）`mcp.timeout` 文档级默认被丢弃**：ref ConfigMcpPlugin 把各文档 `mcp.timeout` 浅合并进每个 server（`{...timeout, ...server.timeout}`，ref mcp.ts:47-55）；rust `mcp_servers_from_config`（engine/mod.rs:3330-3350）只取 `mcp.servers`，文档级 `mcp.timeout.{startup,catalog,execution}` 与 normalize 产出的 `experimental.mcp_timeout`→timeout 迁移结果（normalize.rs:824-836）在 engine 侧无消费者（client.rs:1222-1225 只读 per-server timeout）。用户配置的全局 MCP 超时静默失效。
- **R-2（中）`snapshots: false` 不生效**：ref ConfigSnapshotPlugin 用该键配置 Snapshot 服务开关；rust 全仓 `info.snapshots` 无读取点（grep 仅 schema/normalize），snapshot 恒开启——显式关闭快照的用户仍会写影子 git 快照（磁盘增长 + 违背用户意图）。
- **R-3（低）`lsp` 键惰性**：rust 无 LSP 服务，`lsp: false`/覆盖均被接受但忽略。键面兼容、行为缺失（与 ref 功能差已属 lane 级缺口，此处记 config 域事实）。
- **R-4（低）全局 AGENTS.md 读失败过度降级**（D-4）：全局文件权限异常时 rust 把整个 instruction 集标记 Unavailable，ref 仅丢全局份、项目文件继续生效。可用性回归而非安全风险。
- **R-5（低）`update`/`share`/`enterprise`/`username` 等原子键的引擎侧消费未在本 lane 逐一验证**（update 属 CLI updater、share 属 share 服务，分别在其它 lane）；本 lane 确认 schema/normalize/default_agent/shell/model/websearch/warming/media/tool_output/compaction/formatter/watcher/references/plugins/providers/mcp.servers 的消费链存在。
- **R-6（信息）watch hub 单点**：rust `WatchHub::run` 线程 `rx.recv()` 循环里 reload 串行执行且 debounce 窗口随事件延展（watch.rs:251-281）——与 ref sliding(1)+debounce 语义一致；但 `request_reload` 在 reload 执行期间到达会再开一个窗口（ref 同样），无风险，仅记录。
- **R-7（信息）wellknown 凭证联动**：ref 侧 reload 仅当切换的 credential 命中 wellknown integration（config.ts:290-301）；rust 依赖宿主桥（server）按同条件调 `request_reload`——本 lane 未逐行验证 server 桥的过滤条件，标注为跨 lane 待核。

## 覆盖声明

**逐行读完（双侧）**：ref config.ts、config/{discovery,variable,watch,markdown}.ts、normalize.ts 全 808 行、config/plugin/{instruction,compatibility,entry-observer,shell,snapshot,tool-output,websearch,worktree,compaction,image,location-watcher,mcp,policy,formatter,reference}.ts 全文、project.ts 全 390 行、wellknown.ts、state.ts、environment/{index,driver,environment,files}.ts + exec-defaults.ts 前 90 行、schema/config.ts + experimental.ts + 子 schema 键面（grep 全量+抽读 8 个）；rust config.rs 全、discovery.rs/variable.rs/watch.rs/instruction.rs 全、normalize.rs 1-2800（2801-3506 为测试，扫读）、project.rs 1-700（701-2124 的 resolve/persist/sql 段经头注+grep 定向核验）、environment/{mod,exec_defaults(1-120),workspace(1-120)}、wellknown_config.rs 1-130、state_transform.rs 1-120、v2_migration.rs 头注 1-52、cli_config/mod.rs 头注、oc-schema/config/{mod,media,experimental,plugin,warming,websearch,worktree}.rs 全文。
**部分读/经注释与 grep 锚定（未逐行）**：ref config/plugin/{agent(1-60),command(1-55),source(1-60),skill,skill-file,provider}.ts、v1/config/migrate.ts(1-80)；rust agent_md.rs/command_md.rs/engine/skills.rs/plugin_apply.rs/provider_catalog/overlay.rs/mcp/registry.rs/environment/{local,memory,unavailable,files,driver,pty,stdio}.rs、cli_config/{v1,jsonc}.rs 全文、v2_migration.rs 实现体、project.rs 701-2124。这些以“消费链存在性 grep + 文件头声明”核验，未做逐行比对——如需零静默跳过标准，此为本报告的残留盲区（无发现级问题输出，仅覆盖度声明）。
**未读**：ref workspace/driver.ts(88L) 细节、ref local.ts/memory.ts 全文（rust 对应 local.rs/memory.rs 亦仅键面核验）——Files 契约与 exec-defaults 的逐字比对已覆盖其公共语义面。
