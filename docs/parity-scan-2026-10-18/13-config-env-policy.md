# Parity 扫描 13 — 配置键 / 环境变量 / 托管策略 / 会话保留 / 新会话位置 / 平台差异（补漏通道）

- 日期：2026-10-18（第二轮「补漏」通道）
- 参考：`/opt/opencode-ref`（opencode v2.0.24, TS）
- 目标：`/opt/opencode-rust`
- 审计员：Oracle（只读子代理，config/env/policy lane）
- 任务边界：系统排查「解析了但无人消费」「消费了但未解析」「v2 有而 Rust 无」。已读 `00-MASTER.md`、`07-cli-config.md`、`09-database.md`，本文不重复已登记项的论证，只交叉引用（`07:N-x`、`06:#x`、`02:D2` 等）。
- 方法：ref 侧全量枚举 `OPENCODE_*`（仅 .ts/.tsx，区分运行时读取 / 构建期 define / 写出型标记 / 测试与桌面侧），Rust 侧全树 grep 认领点；`opencode.json` 28 根键与 `cli.json` 20 节逐 leaf 找消费者；`managed-policy.ts`、`session-retention.ts`、`new-session-location.ts` 三专题逐行对照。

## 0. 总览

| 主题 | 结论 | NEW | KNOWN |
|---|---|---:|---:|
| `OPENCODE_*` env | 去重 118 个名字；in-scope 运行时约 55 个：**Rust 缺 18 个认领**（其中 10 个已由 `07:N-1/N-4` 登记，本轮新增 8 个散项）；另有 7 个「Rust 有测试/局部认领但生产缺失」 | 8 | 10 |
| `opencode.json` 28 根键 | schema 28/28（07 已核对）；**leaf 级 dead config 5 簇**：`compaction.*`、`warming.*`、`watcher.ignore`、`experimental.policies`、（`media.image.*` 根因=11#5）；另有 5 键 ref 也是死键（双方 parity-dead） | 4 | 1 |
| `cli.json` 20 节 | 类型/校验/写回 20/20（07 已核对）；**读侧（`TuiConfig::from_info` + mini）只消费 14/约 70 个 leaf**，其余全部 dead-on-read；`plugins[]`（07:N-3）之外新增 **11 个 dead 簇** | 11 | 3 |
| managed-policy | **运行时整体缺失**：无 ManagedPolicy 服务、无 `provider.use` 目录裁剪、无 `permission` 策略拒绝、Console 下发语句不落地（console.rs:79-80 自述 deferred）；normalize 还把 action 写成 kebab `provider-use` ≠ wire 的 `provider.use` | 1 大项 | 0（代码内自述，登记面 NEW） |
| session-retention | **完全缺失**：ref 的 keep(tabs)+limit(3)+current 保留集与 `session.evict(root)` 在 Rust 无对应；`MessageState.by_session` 只增不减（`store.rs:6501` 的 LRU 是 SSE replay 去重窗，非会话保留） | 1 | 0 |
| new-session-location | **子系统缺失**：Rust `Route::Home` 无 location 参数（单 location 架构）；`session.new_location` 解析+校验后零消费者 | 1 | 0（06:#5 下游） |
| 平台 | Windows/macOS 专属项全部被 R-11 暂缓 gating；当前 linux-x64 下有实际影响的仅 `OPENCODE_GIT_BASH_PATH`（并入 N-4）与 Zed/SSE 编辑器上下文（06:#15） | 0 | 全部 |

新一轮计数：**NEW 26 项**（S1×6，S2×20），无 S0。最重三项：managed-policy 运行时（S1）、cli.json `session.permissions` 等读侧死键簇（S1）、session retention（S1）。

---

## 1. `OPENCODE_*` 环境变量全量对照

统计口径：ref 全树（.ts/.tsx，去重 118 个名字）分五类：**A 运行时读取**（in-scope 包：core/ai/server/protocol/sdk/client/cli/tui/schema/plugin/theme/util/codemode/session-ui/ui）、**B 构建期 define**（vite/bun 注入，`declare const`）、**C 写出型**（opencode 设置给子进程/自身的标记）、**D native 模块加载**（bun/node require 解析覆盖）、**E 测试与 out-of-scope 包**（desktop/web/gui-extensions/enterprise/storage/app e2e/scripts）。Rust 判定以 crates/ 生产代码 grep 为准（`/tests/`、`tests_*.rs`、`#\[cfg(test)\]` 除外）。

### 1.1 A 类：运行时读取（in-scope）

| env | ref 读取点 | Rust 认领点 | 判定 |
|---|---|---|---|
| `OPENCODE_CONFIG` | server-process.ts:117 | config.rs:386,478-516；boot.rs:316；plugin_boot.rs:24 | PORTED |
| `OPENCODE_CONFIG_CONTENT` | config.ts:210-214；server-process.ts:118 | config.rs:393-403 | PORTED |
| `OPENCODE_CONFIG_DIR` | index.ts:120；server-process.ts:36；theme.ts:313 | global_roots.rs:98-99；boot.rs:383 | PORTED |
| `OPENCODE_CLI_CONFIG_CONTENT` | cli/config/config.ts:32-34 | cli_config/mod.rs:466-475；oc-tui config.rs:190 | PORTED |
| `OPENCODE_TEST_HOME` | global.ts:17 | global_roots.rs:74-79 | PORTED |
| `OPENCODE_DB` | database-path.ts:6 | database/path.rs:58-103（`debug paths db` 显示面除外 `07:N-5`） | PORTED |
| `OPENCODE_DISABLE_CHANNEL_DB` | database-path.ts:8-9 | database/path.rs:60-97 | PORTED |
| `OPENCODE_CHANNEL` | version.ts:6（define）→ db/UA/service | oc-util/channel.rs:19-29（运行时 env） | PORTED（注册分叉 `09:D24`/`07:K-10`） |
| `OPENCODE_TUI_CHANNEL` | default.ts:100 | oc-tui/storage.rs:13-24,407-408 | PORTED |
| `OPENCODE_PASSWORD` / `OPENCODE_SERVER_PASSWORD` | env.ts:10-21（含 `Env.session()` 过滤）；server-process.ts:79-80,86-88 | util.rs:19-32；standalone.rs:95-109；server_connection.rs:95；boot.rs:45 | PORTED |
| `OPENCODE_SSH_ASKPASS_PORT` / `OPENCODE_SSH_ASKPASS_TOKEN` | index.ts:20-23；ssh-askpass.ts:10,24 | main.rs:30-32；ssh_askpass.rs:2,17,22 | PORTED |
| `OPENCODE_PTY_HANDOFF` | server-process.ts:45-56（读后删除+JSON 解码失败告警） | boot.rs:234,275,525；persistent_pty.rs:25；lifecycle.rs:1290-1333 | PORTED |
| `OPENCODE_PRINT_LOGS` | runtime.ts:98（flag→env）；logging.ts:160；standalone.ts:29 | errors.rs:562；bunman.rs:383（env 本体生效） | **PARTIAL**（flag 桥缺失 `07:N-1` KNOWN） |
| `OPENCODE_DISABLE_AUTOUPDATE` | updater.ts:456 | autoupdate.rs:28,89 | PORTED |
| `OPENCODE_API_KEY` | opencode.ts:319,331（zen hasKey→public key 降级）；opencode-zen.ts:20（`AuthOptions.bearer` 请求侧） | provider_catalog/catalog.rs:538-591（仅 `/api/model` 可用性折叠） | **PARTIAL**：请求侧/env-bearer 无认领（归 `04:B-1/B-3` S0 族） |
| `OPENCODE_CLIENT` | acp.ts:16（写）；server-process.ts:103（读，app.name）；index.ts:129-133（OTEL client 名） | acp/mod.rs:114（仅写） | **PARTIAL / NEW S2**：服务端 `app.name` 无读者 → acp 场景 UA/观测名错；OTEL 侧随 `07:N-6` |
| `OPENCODE_CLI_NAME` | commands.ts:13,38（自命令二进制名） | cli.rs:21 硬编码 `opencode-rust` | MISSING（已登记 `07`§1.1 注册分叉） |
| `OPENCODE_CONFIG_PROJECT_DISABLE` / `OPENCODE_DISABLE_PROJECT_CONFIG` | server-process.ts:115-117 | 无 | MISSING（`07:N-4` KNOWN） |
| `OPENCODE_FILEWATCHER_DISABLE` / `OPENCODE_DISABLE_FILEWATCHER` | server-process.ts:121-122 | 无 | MISSING（`07:N-4`） |
| `OPENCODE_DISABLE_FFF` | server-process.ts:124-128（win 默认关） | 无 | MISSING（`07:N-4`） |
| `OPENCODE_MODELS_URL` / `OPENCODE_MODELS_PATH` | server-process.ts:108-110 | 无（仅 EXTRA `OPENCODE_MODELS_DEV_URL`≈URL 覆盖，boot.rs:179-180） | MISSING（`07:N-4`；EXTRA 只盖一半且名字不同） |
| `OPENCODE_DISABLE_MODELS_FETCH` | server-process.ts:113-114 | **仅** session/tests_parity.rs:225（测试） | MISSING-生产（`07:N-4`；注意存在测试假认领点） |
| `OPENCODE_SIMULATE` | server-process.ts:94 | 无 | MISSING（`07:N-4`；simulation 属 P2 dev 工具，可降 S2） |
| `OPENCODE_GIT_BASH_PATH` | server-process.ts:128-130；updater.ts:186（win 升级必经） | 无 | MISSING（`07:N-4` + §7 平台） |
| `OPENCODE_LOG_LEVEL` | logging.ts:164-170（DEBUG/INFO/WARN/ERROR 最低级别） | 无（连 `RUST_LOG` 都未接线） | **MISSING / NEW S2** |
| `OPENCODE_DIRECT_TRACE` | mini-host.ts:50（mini 诊断 JSONL trace） | 无 | **MISSING / NEW S2**（mini 诊断面） |
| `OPENCODE_SHOW_TTFD` | app.tsx:330-333（startup.showTiming） | 无 | **MISSING / NEW S2**（与 cli.json `debug.timing` 同族，见 §2.2 T-9） |
| `OPENCODE_STORY` | app.tsx:344-348；builtins.ts:29（storybook 注册） | 无 | MISSING（`06:#22` KNOWN） |
| `OPENCODE_ROUTE` | app.tsx:349（JSON 深路由注入） | 无 | MISSING（`06:#22` KNOWN） |
| `OPENCODE_FAST_BOOT` | app.tsx:351（skipInitialLoading） | 无 | **MISSING / NEW S2**（TUI 启动跳过首帧 loading） |
| `OPENCODE_EDITOR_SSE_PORT`（+`CLAUDE_CODE_SSE_PORT`） | editor.ts:117-121（编辑器 SSE 集成端口） | 无（editor.rs 仅 `$VISUAL/$EDITOR`） | MISSING（`06:#15` 编辑器上下文族） |
| `OPENCODE_ZED_DB` | editor-zed.ts:189-194（含 macOS 默认路径） | 无 | MISSING（`06:#15`） |
| `OPENCODE_NODE_ASSETS_DIR` | attention-sounds.node.ts:7-8（音频资产目录覆盖） | 无（Rust 声音为内建 sound.rs；外部资产目录覆盖面缺） | MISSING S2（低：cli.json `attention.sounds{}` 路径覆盖已 PORTED） |
| `OPENCODE_REPO_CLONE_GITHUB_BASE_URL` | repository.ts:166 | 无 | MISSING（随 `09:D11` RepositoryCache 整体缺席） |
| `OPENCODE_GITLAB_AUTH_CLIENT_ID` | gitlab.ts:26（**仅注释提及**，无读取） | — | N/A（ref 自身未消费） |

### 1.2 B 类：构建期 define（vite/bun 注入，非 env 读取）

| 名字 | ref | Rust | 判定 |
|---|---|---|---|
| `OPENCODE_VERSION` / `OPENCODE_ARTIFACT` / `OPENCODE_LOCAL` | version.ts:1-9 | `CARGO_PKG_VERSION` / 常量 / 「dev 构建无 release 戳即 local」(autoupdate.rs:29-30) | PORTED（等价机制；`OPENCODE_LOCAL` 用构建戳替代 define，语义等价） |
| `OPENCODE_LIBC` | watcher-binding.ts:7,12（parcel-watcher linux 变体选择） | —（notify 原生） | N/A（架构性） |
| `OPENCODE_CLI_DIST` / `OPENCODE_CLI_TARGET` / `OPENCODE_BUMP` / `OPENCODE_RELEASE` / `OPENCODE_DEV` / `OPENCODE_DEPLOYMENT_TARGET` / `OPENCODE_FFF_FFI(_PATH)` 等 | vite.node.config.ts / script/*.ts | — | N/A（构建脚本面，单二进制无对应物） |

### 1.3 C 类：写出型标记（opencode set，子进程读）

| 名字 | ref 写点 | Rust 写点 | 判定 |
|---|---|---|---|
| `OPENCODE_TERMINAL=1` | pty.ts:175；shell.ts:271 | pty.rs:412；shell.rs TERMINAL_ENV（tool/shell.rs:274,572） | PORTED |
| `OPENCODE_SESSION_ID` | tool/plugin/shell.ts:213 | tool/shell.rs:280,576 | PORTED |
| `OPENCODE_WORKTREE_BASE` / `OPENCODE_WORKTREE_PATH` | worktree.ts commands.start | handlers/project.rs:478-492 | PORTED |
| `OPENCODE_CLIENT=acp` | acp.ts:16 | acp/mod.rs:114 | PORTED（写侧；读侧缺，见 §1.1） |

### 1.4 D 类：native 模块加载覆盖（bun/node `require.resolve` 面）

| 名字 | ref | Rust | 判定 |
|---|---|---|---|
| `OPENCODE_NODE_PTY_PATH` | pty.node.ts:8 | —（portable-pty 内建） | N/A（架构性；R-11 前无 Windows SEA 场景） |
| `OPENCODE_PARCEL_WATCHER_PATH` | watcher-binding.ts:11 | —（notify） | N/A |
| `OPENCODE_TREE_SITTER_{,BASH_,POWERSHELL_}WASM_PATH`（3 个） | parser-wasm.node.ts:6-10 | —（oc-syntax 原生 tree-sitter crate） | N/A（等价能力内建；env 覆盖点无意义） |
| `OPENCODE_PHOTON_WASM_PATH` | photon-wasm.node.ts:3 | 无 | MISSING-随功能（image 管线整体缺席 `11:#5` KNOWN） |
| `OPENCODE_PTY_BIN` | persistent-pty/binary.{bun,node}.ts（daemon 二进制覆盖） | —（persistent PTY 为 in-process 状态机，`02:C9` KNOWN） | N/A（架构性，Phase-9 再议） |

### 1.5 E 类：测试 / out-of-scope 包（列出备查，不计差异）

- **desktop**：`OPENCODE_DESKTOP_{WSL_CLI_OUTPUT,WSL_CLI_BUILD,ISOLATED_SERVER,CLI_DEV,SERVER_CHANNEL,REMOTE_DEBUGGING_PORT,DIST,DISABLE_PROTOCOL_REGISTRATION,TEST_ROOT}`。
- **storage/enterprise（P3）**：`OPENCODE_STORAGE_{ADAPTER,ACCOUNT_ID,ACCESS_KEY_ID,SECRET_ACCESS_KEY,BUCKET,REGION}`。
- **gui-extensions（书面 out-of-scope）**：`OPENCODE_SSH_{STATUS,REGISTRATION_BEGIN,REGISTRATION_END}`。
- **app/web/perf/storybook**：`OPENCODE_{BASE_URL,FAVICON,APPLE_TOUCH_ICON,REMOTE_TARGET,PERSISTENCE_*,PERFORMANCE_*,STABILITY_CAPTURE,SHOW_*}`、`OPENCODE_THEME_{STATES,OBSERVER}__`、`OPENCODE_LOADED_{SKILL,PATH}__`、`OPENCODE__`（内部标记）。
- **测试专用**：`OPENCODE_TEST_{ONBOARDING,MISSING,MCP_TOKEN,SHELL,FIXTURES}`、`OPENCODE_{SESSION,SERVICE}_ENV_TEST`、`OPENCODE_INSTALLER_TEST_ROOT`。
- ref 的 `OTEL_EXPORTER_OTLP_*`（index.ts:125-133）→ `07:N-6` KNOWN S2。

### 1.6 Rust EXTRA env（v2 无此开关；登记在案）

| env | Rust 点 | 备注 |
|---|---|---|
| `OPENCODE_RELEASE_URL` | updater.rs:16,96-272 | 自有更新通道（`07:K-2` 注册分叉） |
| `OPENCODE_LOG_FILE` | errors.rs:614-621 | 日志落点覆盖 |
| `OPENCODE_APP_ASSETS` / `OPENCODE_APP_ARCHIVE` | app_archive.rs:9-51,322-331；build.rs | web 资产目录覆盖（R-04 配套） |
| `OPENCODE_ACP_DEBUG` | acp/transport.rs:65,154；service.rs:328 | ACP 帧调试 |
| `OPENCODE_NO_TTY_PROMPT` | run/noninteractive.rs:378-386 | oracle gate-7 决策，文档自述非 v2 分歧 |
| `OPENCODE_RG_PATH` | tool/grep.rs:4 | ripgrep 路径覆盖（ref 无；rg 自动下载另属 `02:A5`） |
| `OPENCODE_DEBUG_ERRORS` | errors.rs:564,759 | 错误面板调试 |
| `OPENCODE_DISABLE_PLUGINS` | plugins.rs:192-195 | 服务端插件总开关 |
| `OPENCODE_DISABLE_BUN_DOWNLOAD` | bunman.rs:17 | bun 运行时下载开关 |
| `OPENCODE_MODELS_DEV_URL` | boot.rs:179-180 | 模型目录 URL 覆盖（与 ref 的 `OPENCODE_MODELS_URL` 名字不同、只盖 URL） |
| `OPENCODE_CATALOG_FETCH` | boot.rs:168-176 | 测试基建（自述非 v2 分歧） |
| `OPENCODE_TRACE_BOOT` | boot.rs:112 | boot 追踪 |
| `OPENCODE_USER_AGENT` | tool/webfetch.rs:16,83 | webfetch UA 覆盖 |
| `OPENCODE_SMOKE_BIN` | release 冒烟脚本 | 测试 |

（`OPENCODE_CLIENT_ID`/`OPENCODE_DEFAULT_SERVER`/`OPENCODE_MD`/`OPENCODE_DESCRIPTION`/`OPENCODE_VERSION` 在 Rust 里是**常量名**而非 env 读取，不计入。）

---

## 2. `opencode.json` 28 根键 leaf 级审计

schema 层 28/28 对齐是 `07`§3.1 的结论；本节查**运行时消费者**。Rust 的消费模式是 `engine/config_websearch_selection(documents)` 这类显式读取函数（engine/mod.rs:1819-1843），因此逐键可 grep 验证。

### 2.1 消费状态总表

| 根键（leaf） | ref 消费点 | Rust 消费点 | 判定 |
|---|---|---|---|
| `$schema` | — | — | 双侧装饰性 |
| `shell` | config/plugin/shell.ts → shell 选择 | engine 硬编码 `None`（tool/shell.rs:278 `resolve(None,true)`） | **DEAD**（`02:C3` KNOWN S1） |
| `model`（provider/model/variant） | model resolver | resolver.rs | PORTED |
| `default_agent` | config/plugin/agent.ts:88 | config/agent_md.rs:324 | PORTED |
| `update` | updater 策略 | autoupdate.rs `policy_from_text` | PORTED |
| `share` | 无运行时消费者（v2 share 命令自身是 "not implemented" toast，session/index.tsx:892-897） | 同样无（command_palette.rs:1825-1834 镜像了同一 toast） | **DEAD-双侧**（parity） |
| `enterprise.url` | 无 core 消费者（enterprise 包 P3 out-of-scope；仅 wire 类型） | 无 | **DEAD-双侧**（P3 消费者缺席） |
| `username` | 无运行时消费者（schema 描述"显示与遥测"，实际无读者） | 无（normalize.rs:521 保形） | **DEAD-双侧**（parity；信息项） |
| `permissions`（Ruleset） | permission.ts | engine/mod.rs:2108 读取 Ruleset | PORTED |
| `agents`（12 字段） | config/plugin/agent.ts | config/agent_md.rs + AgentRegistry | PORTED |
| `snapshots` | config/plugin/snapshot.ts | snapshot.rs（B7：enabled 门缺 vcs 判定） | PARTIAL（`02:B7` KNOWN） |
| `watcher.ignore` | location-watcher.ts:17-24 → LocationWatcherPolicy | **无**（config/watch.rs 的 ignore 是 config 目录发现硬编码，非本键） | **DEAD / NEW S2**（见 2.2 C-3） |
| `formatter` | config/plugin/formatter.ts | formatter.rs no-op | PARTIAL（`02:B8` KNOWN，默认等价） |
| `lsp`（bool\|entries） | **无运行时消费者**（v2 core 未落 LSP 子系统） | 无（normalize.rs:1207-1218 仅保形） | **DEAD-双侧**（parity） |
| `media.image.{auto_resize,max_width,max_height,max_base64_bytes}` | config/plugin/image.ts → Image 服务 | 无（image normalize/resize 全缺） | **DEAD**（根因 `11:#5` KNOWN；`max_base64_bytes` 在 oc-schema 为 Rust 自述 EXTRA，同样无消费者） |
| `tool_output.{max_lines,max_bytes}` | config/plugin/tool-output.ts:16-24 | tool_output.rs:187 `configure` 存在但无配置接线（engine/mod.rs:444 固定默认） | **DEAD**（`02:D2` KNOWN） |
| `mcp`（servers） | config/plugin/mcp.ts | engine/mod.rs:2043（latest-doc-wins） | PORTED |
| `compaction.{auto,keep.tokens,buffer}` | config/plugin/compaction.ts:15-22 → `Settings{auto,buffer,keep}`（compaction.ts:197 默认 `{auto:true,keep:15000}`） | runner/compaction.rs:40-51 有同名 `Settings`+Default，**但无任何配置接线**（全树无第二处构造） | **DEAD / NEW S1**（C-1） |
| `skills`（paths/URLs） | config/plugin/skill.ts | engine/skills.rs（远程 URL 不下载 `01:C-03` KNOWN） | PARTIAL（KNOWN） |
| `commands` | config/plugin/command.ts | engine/commands.rs:58 + command_md | PORTED |
| `instructions`（paths/URLs） | **无运行时消费者**（instruction 插件只做 AGENTS.md 发现，config/plugin/instruction.ts 全文无此键） | 无（config/instruction.rs 同为 AGENTS.md 发现） | **DEAD-双侧**（parity；信息项） |
| `references` | config/plugin/reference.ts | 11:#6 PARTIAL（git 源不物化等） | PARTIAL（KNOWN） |
| `websearch` | config/plugin/websearch.ts | engine/mod.rs:1819-1843 | PORTED（`02:A9` env 键门控 KNOWN） |
| `plugins` | config/plugin/source.ts | plugin_boot.rs:105-147（opencode.json 通道） | PORTED（cli.json 通道 `07:N-3`） |
| `worktree.directory` | worktree.ts:204-255 | handlers/project.rs:275-296 | PORTED |
| `warming`（bool\|{prompt,interval,duration}） | plugin/warming.ts:16-100（`session.generate` 保活循环） | **无**（normalize.rs:583-587 保形后零消费者） | **DEAD / NEW S2**（C-2） |
| `providers` | config/plugin/provider.ts | overlay.rs/facade.rs（`04:A-4` 等已知差异） | PORTED（KNOWN 差异族） |
| `experimental.portable_shell_scanner` | 可切换 | engine 硬编码 false | `02:C4` KNOWN |
| `experimental.subagent_depth` | 可配置 | engine 硬编码 1 | `02:C5` KNOWN |
| `experimental.policies` | config/plugin/policy.ts（provider 目录裁剪 + permission 拒绝）+ Console 语句合并 | **无运行时消费者**；normalize.rs:1074-1103 从 v1 provider 名单生成的 policies 同样死码 | **DEAD / NEW S1**（§4） |

### 2.2 NEW 项展开

**C-1 `compaction.*` 全键 dead — NEW S1**
- EXPECTED：`config/plugin/compaction.ts:15-22` 把各文档的 `compaction.{auto,buffer,keep.tokens}` 叠进 `SessionCompaction` 的 `Settings`（session/compaction.ts:196-207，默认 `{auto:true, keep:15_000}`）。
- ACTUAL：Rust `runner/compaction.rs:40-51` 有逐字段对齐的 `Settings`（Default = 同样默认值），但除 `Default` 外全树无构造点；engine 装配（engine/mod.rs:444 一带）不读 config。
- 影响：`"compaction": {"auto": false}` 被静默忽略（自动压缩照跑）；`keep`/`buffer` 调参无效。修复成本低（一处接线）。

**C-2 `warming` dead + warming 插件缺失 — NEW S2**
- EXPECTED：`plugin/warming.ts:16-100`——`warming: true|{prompt,interval,duration}`（默认 prompt 保活话术、4min 间隔、30min 时长）对根会话启动 `session.generate` 保活循环。
- ACTUAL：Rust 无 `opencode.warming` 对应物（grep `warming` 仅 normalize/schema/client 类型）。`08`§3.3 曾以「内建插件 native 化」一笔带过并称「属模型 lane」，但 01/03 均未展开——本条补齐登记。
- 影响：配置死键；「保持会话热」行为缺失（模型侧无 KV 保活，长会话首 token 变慢）。

**C-3 `watcher.ignore` dead + LocationWatcher/`filesystem.changed` 缺席 — NEW S2**
- EXPECTED：`config/plugin/location-watcher.ts:17-24` 汇总各文档 `watcher.ignore` 进 `LocationWatcherPolicy`；`filesystem/location-watcher.ts:27-107` 监视 location 目录与 VCS `HEAD`/`branch` 文件，越过滤后发 **`filesystem.changed`** 事件（schema/filesystem.ts:8-13，ephemeral）。
- ACTUAL：Rust 的 `notify` 仅用于 config 文件监视（config/watch.rs；`FsUpdate` 只在 config.rs:1231-1252 出现）；全树无 `filesystem.changed`/`fs.changed` 生产者；`watcher.ignore` 解析（normalize.rs:1147-1168）后零消费者。
- 下游：ref `vcs.ts:159` 订阅该事件刷新分支状态——Rust 的 branch 显示只能靠轮询/重进（与 `11:#9` 的 vcs info 细节族相关，但事件通道本身是独立缺口）。
- 注：ref 侧 `OPENCODE_FILEWATCHER_DISABLE` 开关（N-4）控制的正是这层 watcher 的开关面。

**C-4 双侧死键清单（parity，无需动作，登记防误报）**：`share`、`username`、`lsp`、`instructions`、`enterprise.url`——v2.0.24 参考实现自身无运行时消费者；Rust 同样只做 schema/normalize 保形。若未来 ref 落地消费者（如 LSP 子系统、share 服务），Rust 需同步。

---

## 3. `cli.json` 20 节 leaf 级审计（读侧）

07 已核对类型/校验/写回/flock/env overlay（`07`§3.3）；06:#23 已登记 **Settings 对话框只写 4 行**（theme/mode/sidebar/grouping，command_palette.rs:734-745）。本节补上**读侧**：`oc-tui/src/config.rs::TuiConfig::from_info`（config.rs:202-296）是 TUI 唯一的 cli.json → 运行态映射，加上 `oc-cli/src/main.rs:22-25` 的 mini 读取。映射后的字段集 = 实际消费集。

**from_info 实际消费**：`theme.{name,mode}`、`leader.timeout`、`attention.{notifications,sound,volume,sound_pack,sounds}`（+EXTRA `bell`）、`session.image_preview`、`tabs.{mode,enabled→mode,scope}`、`session.sidebar`、`mouse`、`cursor.{style,blinking}`、`keybinds`（经 apply_config 进 keymap，231/231 对齐）。
**mini 实际消费**：`mini.{replay,replay_limit}`（main.rs:134-163）。
**其余全部 leaf 在 Rust 无读点**（grep 验证：oc-tui 内除 validate/迁移/写回外无第二处出现）。

### 3.1 20 节 × leaf 消费矩阵

| 节.leaf | ref 消费点 | Rust 读点 | 判定 |
|---|---|---|---|
| theme.name / theme.mode | resolve + ThemeProvider | config.rs:204-208 → app.rs apply_config | PORTED |
| keybinds（~231 键） | BindingLookup | config.rs:286-290 → keymap | PORTED |
| plugins[] | TUI 装载源（tui config index.tsx:79-81） | 无 | **DEAD**（`07:N-3` KNOWN S1） |
| leader.timeout | resolve:307（默认 2000） | config.rs:212 → app.rs:241 | PORTED |
| scroll.speed / scroll.acceleration | 滚动驱动 | oc-tui-core scroll.rs 无 speed/accel 概念 | **DEAD / NEW S2**（T-6） |
| attention.{notifications,sound,volume,sound_pack,sounds} | resolve:302-308 | config.rs:215-234 → attention.rs | PORTED |
| diffs.source | app 初始 diff 源（resolve 透传） | `DiffSource::parse`（store.rs:1496-1505）仅被 dialog.rs:725 运行时切换用，boot 不读 | **DEAD / NEW S2**（T-5） |
| diffs.{wrap,tree,single,view} | diff 视图选项 | diff_viewer.rs 无配置入口 | **DEAD**（同 T-5） |
| terminal.title | OSC 0/2 标题 | 无 | DEAD（`06:#18` TM-5 KNOWN） |
| terminal.copy（copy_on_select） | 选区即复制 | 无 | DEAD（`06:#16` KNOWN） |
| prompt.editor | 编辑器文件/选区注入上下文 | editor.rs 无 SSE 集成 | DEAD（`06:#15` KNOWN） |
| prompt.paste（compact\|full） | app.tsx:573 `pasteSummaryEnabled` | 无 | **DEAD / NEW S2**（T-7） |
| prompt.image_preview | prompt/index.tsx:1676 | 无（只读了 `session.image_preview`） | **DEAD**（T-7） |
| session.sidebar | resolve 透传 | config.rs:273-277 | PORTED |
| session.scrollbar | session/index.tsx:230（默认 **false**） | 无；Rust transcript 常显滚动条 | **DEAD / NEW**（T-3；默认值亦分歧：ref 藏、Rust 显） |
| session.thinking | session/index.tsx:229（默认 **hide**） | 无；Rust 思路折叠为一行 header（常显） | **DEAD**（T-3；默认呈现分歧） |
| session.grouping | index.tsx:233（`!== "none"`） | **只写不读**：Signal 硬编码 true（store.rs:2528），toggle 写回（app.rs:4281-4294） | **DEAD-on-read / NEW S2**（T-3；设 `"none"` 的用户 boot 后仍分组） |
| session.verbosity | transcript 细节档位 | 无（verbosity cycle 运行态存在，footer/panels.rs:93-140，默认档不来自 config） | **DEAD**（T-3） |
| session.image_preview | 附件/工具图预览 | config.rs:236-237 | PORTED |
| session.tps | session/index.tsx:1892（默认 true，footer tokens/sec 行） | 常显（message/list.rs:492-510 恒渲染 tok/s；默认与 ref 一致，仅无法关闭） | **DEAD**（T-3） |
| session.markdown（source\|rendered） | transcript markdown 呈现 | 无 | **DEAD**（T-3） |
| session.new_location | new-session-location.ts（§6） | 无（config.rs `from_info` 不映射；oc-tui 全树 0 命中） | **DEAD / NEW S2**（T-2） |
| session.permissions（prompt\|autoaccept） | permission.tsx:14（`args.auto ? "autoaccept" : config...`） | 无；`AppArgs.autoaccept` 仅来自 `--auto` flag（app.rs:94-97,153） | **DEAD / NEW S1**（T-1） |
| tabs.mode / tabs.enabled / tabs.scope | resolve:277-289 | config.rs:246-266 | PORTED |
| tabs.layout（horizontal\|vertical） | tab 布局 | 无 | DEAD（`06:#3` KNOWN——垂直布局整面缺） |
| tabs.indicators（status\|numbers） | tab 序号/状态图标 | 无 | **DEAD / NEW S2**（T-12） |
| mini.thinking / tools / shell_output / turn_summary / footer / splash / work_spinner / mono（8 键） | mini/runtime.boot.ts:87-98 `resolveMiniSettings` | 无（MiniInput 无对应字段，runtime.rs:59-71） | **DEAD / NEW S1 簇**（T-4） |
| mini.replay / replay_limit | mini.ts:37-38 | main.rs:134-163 | PORTED |
| debug.devtools | DevToolsBar | 无 | DEAD（`06:#20` KNOWN） |
| debug.timing | showTiming | 无 | **DEAD / NEW S2**（T-9；与 `OPENCODE_SHOW_TTFD` 同族） |
| debug.turn_tokens（bool\|"verbose"） | 每 turn token 用量诊断 | 无 | **DEAD / NEW S2**（T-9） |
| experimental（record<string,bool>） | dialog-experiments.tsx:25-45（实验开关对话框读写） | 无（cli_config/mod.rs:325 解析后无读者；实验对话框面缺失） | **DEAD / NEW S2**（T-11） |
| animations | 全局动画门 | 无（config.rs 不映射；Rust 有内建动画 seam） | **DEAD / NEW S2**（T-10） |
| mouse | resolve:309（默认 true） | config.rs:271-272 | PORTED |
| cursor.{style,blinking} | resolve:311-318 | config.rs:275-284 | PORTED |

### 3.2 NEW 项展开

**T-1 `session.permissions` 读侧缺失 — NEW S1**
- EXPECTED：`context/permission.tsx:14` —— `args.auto ? "autoaccept" : config.data.session.permissions`（默认 prompt）；autoaccept 下权限请求不弹（session/index.tsx:200,255,2594 三处消费）。
- ACTUAL：Rust 只有 `--auto`/`--yolo` flag 路径（app.rs:94-97 → `autoanswer_permissions` app.rs:1524-1526）；cli.json 配置零读取。
- 影响：用户在 cli.json 设 `"permissions": "autoaccept"` 后 Rust TUI 仍逐项弹权限——配置承诺的安全边界/便利开关失效（方向上偏「多弹窗」，不引入越权，故 S1 而非 S0）。

**T-4 mini 显示配置 8 键全 dead — NEW S1（簇）**
- EXPECTED：`resolveMiniSettings`（mini/runtime.boot.ts:87-98）给 mini 渲染器供 thinking/tools/shell_output/turn_summary/footer/splash/work_spinner/mono 八个档位（注意 ref 默认 `tools:"hide"`、`shell_output:"hide"`）。
- ACTUAL：`MiniInput`（runtime.rs:59-71）无任何显示偏好字段；mono 仅作为函数参数在内部传递（scrollback.rs:290），不来自 config。
- 影响：mini 用户的一切显示调优失效；且 Rust 默认行为对齐的是 ref 的哪一档无从切换（tools/shell_output 相当于恒 show）。

**T-3 `session.*` 六键读侧缺失（thinking/verbosity/tps/markdown/scrollbar/grouping-boot）— NEW S1（簇，单键 S2）**
- 证据见矩阵；共性是「运行时 toggle/默认行为存在、boot 默认不读 config」。最可见的三处：`scrollbar`（ref 默认藏、Rust 恒显）、`tps`（ref 可关、Rust 恒显——默认值两侧一致）、`grouping:"none"` 被 boot 忽略。
- `grouping` 的特殊性：写回链路完整（app.rs:4281-4294），读链路缺失（store.rs:2528 硬编码 true）——形成「改了能存、重启失效」的半坏状态。

**T-5 `diffs.*` 5 键 — NEW S2**；**T-6 `scroll.*` 2 键 — NEW S2**；**T-7 `prompt.{paste,image_preview}` — NEW S2**（editor 见 06:#15）；**T-9 `debug.{timing,turn_tokens}` — NEW S2**；**T-10 `animations` — NEW S2**；**T-11 `experimental` — NEW S2**（含 DialogExperiments 面缺失，06 缺失对话框清单未列）；**T-12 `tabs.indicators` — NEW S2**（layout 归 06:#3）。

**Rust 私有键**：`attention.bell`（cli_config mod.rs:108-110，v2 无；07 已登记 EXTRA）。除此之外本轮未发现新的 Rust 私有 cli.json 键。

---

## 4. 专题：managed-policy（ConfigPolicy → provider/permission/catalog）

### 4.1 ref 侧结构

1. **状态服务** `managed-policy.ts:9-32`：全局 `State{statements, organization?}`，`current()` 同步读（供 catalog 变换内联调用）、`set()` 整体替换（换连接即换策略，绝不合并）。
2. **Console 注入** `plugin/provider/opencode.ts:167-260`：`OpencodePlugin` 在 active connection 解析后 `fetchConfig`，`publish(snapshot)` → `managed.set({ statements: config.experimental?.policies ?? [], organization })`（:204-207）；加载失败但连接未变时**保留旧策略**（:214-224——防止凭据仍有效时组织策略被瞬时网络错误 lifted）。
3. ** authored 配置策略** `config/plugin/policy.ts:20-35`：`policies()` = 各文档 `experimental.policies` **倒序**（用户全局盖过仓库）+ 每条附 "Blocked by configuration policy"，再拼 Console 语句（附 "Blocked by {org}'s policy"）。
4. **两个执行面**（policy.ts:36-52）：
   - `provider.use` + `Wildcard.match(providerID, resource)` 命中 deny → `providers.remove(id)`（目录裁剪，`/models` 立即消失）；
   - `permission.evaluate` hook：`Wildcard.match("{action}:{resource}", policy.resource)` 命中 deny → `event.effect="deny"` + message（权限请求直接拒绝并可解释原因）。
5. `Wildcard.match`（util/wildcard.ts:3-15）：`\`→`/` 归一、`*`→`.*`、`?`→`.`、尾部 `" .*"` 可选段、**win32 大小写不敏感**。
6. opencode.ts 还有 session hook「explainers」用于在会话里解释为何 provider 不可用（:180 一带）。

### 4.2 Rust 现状

| 环节 | Rust | 判定 |
|---|---|---|
| schema | oc-schema/src/config/policy.rs（`07`§4.1 判 PORTED） | 有 |
| v1 名单→policies 生成 | config/normalize.rs:1074-1103（provider allow/deny 列表迁移生成） | 有（但下游死码） |
| action 字面量 | normalize.rs:58-62 `POLICY_ACTION_PROVIDER_USE="provider-use"`（kebab），自述 TODO：wire 是 `provider.use`（oc-client types.gen.rs:556,565 佐证） | **WRONG**：即便接上消费者，生成的 v1 迁移策略也匹配不上 v2 语义 |
| authored `experimental.policies` 消费者 | **无**（grep 全树仅 schema/normalize/client 类型） | **MISSING** |
| ManagedPolicy 等价运行时（statements/organization/换连接替换/失败保留） | **无** | **MISSING** |
| Console 侧 | integration/console.rs:79-84 `RemoteResponse` 只解 `providers`——注释自述 "websearch / mcp / policies remain on the deferred list"（:80）；fetch_config（:113-157）丢弃 policies | **MISSING（代码内自认 deferred，登记面 NEW）** |
| `provider.use` 目录裁剪 | 无（catalog/facade 无 policy 过滤） | **MISSING** |
| `permission` 策略拒绝 | 无（`permission.evaluate` hook 机制在 hooks.rs 存在，但无 config-policy 注册者——`08:D-24` KNOWN 的子集；且 `is_reserved_plugin_id("opencode.config.policy")`（hooks.rs:2035）保留了插件 id 却无实现） | **MISSING** |
| Wildcard 工具 | 无对应实现 | MISSING（随本专题） |

**C-7 结论（NEW S1）**：托管策略是「解析了但无人消费」的最大单簇——`experimental.policies`（authored）、v1 provider 名单迁移产物、Console 下发语句三条来源全部死码；执行面（目录裁剪 + 权限拒绝 + 拒绝文案/组织名）双侧皆缺。对个人用户无感，对企业/托管场景是整面功能缺失。修复建议：以 `ManagedPolicy` 等价的 `RwLock<State>` + engine 装配期一次 authored 合并 + console.rs 扩展三字段 + `Wildcard` 移植为一个改动簇；同时修 `provider-use`→`provider.use` 字面量（normalize.rs:62）。

---

## 5. 专题：session-retention（TUI 会话保留）

### 5.1 ref 语义（session-retention.ts:4-41 + session-tabs.tsx:139-147）

- 保留集 = `keep`（开启 tabs 时的全部 tab session，root 归并）∪ `recent`（**导航**推进的最近会话 root，去重后截 `limit=3`）∪ 当前会话；`recent` 内部再按保留集裁剪。
- 对 `session.list()` 里所有不在保留集的会话计算 `excluded: Map<sessionID→root>`；与上一轮 `evicted` 求差得到**新过期 root 集合**（注释：新后代在 root 已被逐后仍需再逐），对每个 root 调 `session.evict(root)`。
- `evict`（client/solid/data.ts:1363-1369）→ 对 root+自身+family 全部 `evictSession`（data.ts:540-560）：清 message/messageCursor/messageLoading/pending 缓存，**保留未确认 outbox 项**，使 heavy 数据可被重新 `session.message` 拉取。
- 效果：TUI 常驻内存 ≈ O(打开 tabs + 3 个最近会话族 + 当前)。

### 5.2 Rust 现状

- 消息态：`MessageState { by_session: HashMap<String, Vec<Message>> }`（store.rs:501-503）；`set_messages` 只 insert（:3530-3533），SSE reducer `update_messages` 按事件 session insert（:6842 起）；**无任何删除路径**——`session.deleted` 只关 tab（:6689-6694），不清理 `by_session`。
- 任务书怀疑的 `store.rs:6501` 确认只是 SSE replay 去重窗（`SEEN_EVENT_WINDOW` 的 LRU，is_replayed，:6483-6506），与会话保留无关。
- 无 keep/limit 概念；无 root/family evict API；无 outbox 保留语义（Rust pending 结构更简单，无对应物需保留）。

**C-8 结论（NEW S1）**：`createSessionRetention` 无 Rust 对应。长时间运行的 TUI 中，每个被访问过的会话的完整消息列表、以及该 location 总线上所有会话的 SSE 增量，都永久驻留 `by_session`——与 ref 的有界保留（tabs+3+当前）相反。大型多会话工作流（后台 subagent 多、历史会话多）下内存无界增长且切回旧会话用的是可能过期的缓存（ref 逐出后重拉）。建议按 ref 形状在 `Store` 加保留集计算 + `evict_root`（含 family 展开），挂在 tab 变更与路由导航两个触发点上。

---

## 6. 专题：new-session-location（新会话位置决策）

### 6.1 ref 语义（new-session-location.ts:3-14）

- `mode ∈ {launch, inherit}`（cli.json `session.new_location`，默认 launch，config/index.tsx:173,321；Settings 对话框可改，dialog-config.tsx:118-120）。
- `inherit` 且当前会话 location 存在、且它与 `unavailable`（`location.error?.location`——当前 boot 解析失败的位置）不同（directory 或 workspaceID 任一不同）→ 新会话继承当前会话 location；否则回落 `{directory: launchDirectory}`。
- 消费点：`session.new` / `session.clear` 命令（app.tsx:724-734, 751-761——current 取 `session.get(id).location ?? location.ref`，unavailable 传 `location.error?.location`）；mini 的 onNewSession（mini/runtime.ts:936-946，不传 unavailable）。
- 关联面：`SessionLocationMissing` 屏（location-missing.tsx:70，`06:#30` UNVERIFIED）。

### 6.2 Rust 现状

- `Route` 只有 `Home` / `Session{session_id}`（app.rs:735-736 等），**Home 无 location 字段**——TUI 为单 location（boot 目录）架构，项目/worktree 切换面本就半缺（`06:#5`）。
- `session.new_location` 在 oc-core 解析+校验（cli_config/mod.rs:192-193,405-408）后，oc-tui **零消费者**（全树 grep 无命中；`from_info` 不映射）。
- mini：`QueueAction::NewSession{unavailable}`（mini/queue.rs:80）只有「能否新建」档位，无位置决策。

**C-9 结论（NEW S2，结构上从属于 `06:#5` 单 location TUI）**：new-session-location 决策子系统（launch/inherit/unavailable 回退）在 Rust 无对应；`session.new_location` 为 dead config。在多 location 能力（#5）落地前，本项无可独立修复面；建议随 #5 一并移植 `newSessionLocation` 纯函数（14 行）+ boot 读 `new_location`。

---

## 7. 平台差异（Windows / macOS / ConPTY / 信号 / service / git-bash / libc）

**总判定**：Rust 当前仅产 linux-x64（`R-11` P3 暂缓：aarch64/musl/macOS/Windows）。所有 Windows/macOS 专属运行时分叉都被 R-11 gating——在 R-11 解锁前它们不构成「当前功能不一致」，但以下代码级事实应随 R-11 一并处理：

| 项 | ref | Rust | 处置 |
|---|---|---|---|
| ConPTY DLL | pty.node.ts:13（win32 + SEA 时 `useConptyDll:true`）；pty.bun.ts:13 注释（detached server Ctrl+C 忽略被 ConPTY shell 继承） | portable-pty（win 上自带 ConPTY 路径），SEA/`OPENCODE_NODE_PTY_PATH` 无对应 | R-11 随办；linux 无影响 |
| git-bash | `OPENCODE_GIT_BASH_PATH`：server `windows.gitbash`（server-process.ts:128-130）+ win 升级强制 Git Bash（updater.ts:180-192，git --exec-path 推导 bash.exe，缺失报 "Git Bash is required"） | 无认领（全树 0 命中） | **并入 `07:N-4`**；win 支持落地前为潜在项 |
| win service 保留镜像 | server-process.ts:74-76（win32 + RetainedImage.installed → `retain(cache,"service")`，让包管理器/curl 安装可在服务运行时被替换） | 无（boot/service 路径无 windows 分支） | R-11 随办 |
| 文件 watcher native 绑定 | parcel-walker + `OPENCODE_LIBC` define + `OPENCODE_PARCEL_WATCHER_PATH`（watcher-binding.ts:11-14） | notify（inotify） | 架构性 N/A；`watcher.ignore`/`filesystem.changed` 缺口见 C-3（**linux 上即有影响**） |
| 路径归一 | LocationRef 仅 win32 反斜杠归一 | 全平台归一（`09:D18` KNOWN S2） | 已登记 |
| 大小写敏感性 | Wildcard win32 `si` 标志（wildcard.ts:14） | 无 Wildcard（随 C-7 移植时需带） | 随 C-7 |
| macOS Zed DB | `~/Library/Application Support/Zed/db/0-stable/db.sqlite`（editor-zed.ts:189-194，+ `OPENCODE_ZED_DB` 覆盖） | Zed 上下文整体缺失（`06:#15`） | 随 #15；macOS 二进制本身在 R-11 |
| 信号 | AppProcess 进程组安全信号 + cross-spawn（win .cmd/.bat 解析） | procutil.rs（进程组）PORTED；win spawn 语义未建 | R-11 随办；TERM 宽限 `02:C8` KNOWN |
| channel/libc | channel 构建期 define vs 运行时 env（`09:D24`/`07:K-10` KNOWN） | — | 已登记 |
| `HERDR_ENV`（tabs auto 档） | resolve:289 | config.rs:246-252 同判 | PORTED（良性一致） |

当前 linux-x64 下**实际影响功能一致性**的平台项只有两个，均已归档：`watcher.ignore`/`filesystem.changed`（C-3，NEW）与 `OPENCODE_GIT_BASH_PATH`（N-4）。

---

## 8. 差异清单汇总

| ID | 判定 | 级别 | K/N | 摘要 | ref | rust |
|---|---|---|---|---|---|---|
| C-1 | MISSING | **S1** | NEW | `compaction.{auto,keep.tokens,buffer}` 无配置接线（Settings 结构就绪） | config/plugin/compaction.ts:15-22 | runner/compaction.rs:40-51 |
| C-2 | MISSING | S2 | NEW（08 提及未展开） | `warming` dead + warming 保活插件缺失 | plugin/warming.ts:16-100 | 无 |
| C-3 | MISSING | S2 | NEW | `watcher.ignore` dead；LocationWatcherPolicy/`filesystem.changed` 事件通道缺失（vcs 分支刷新依赖） | location-watcher.ts:17-24；filesystem/location-watcher.ts:27-44 | normalize.rs:1147-1168；无 |
| C-4 | — | — | NEW(信息) | 双侧死键：share/username/lsp/instructions/enterprise.url | §2.1 | §2.1 |
| C-7 | MISSING+WRONG | **S1** | NEW | managed-policy 运行时整体缺失（authored/Console/v1 迁移三源死码；目录裁剪+权限拒绝两执行面缺；`provider-use` 字面量错） | managed-policy.ts:9-32；config/plugin/policy.ts:20-52；opencode.ts:204-207 | normalize.rs:58-62,1074-1103；console.rs:79-84；hooks.rs:2035 |
| C-8 | MISSING | **S1** | NEW | TUI 会话保留/逐出缺失；`by_session` 无界增长 | session-retention.ts:4-41；session-tabs.tsx:139-147；data.ts:1363-1369 | store.rs:501-503,3530-3533,6689-6694 |
| C-9 | MISSING | S2 | NEW | new-session-location 子系统缺失；`session.new_location` dead | new-session-location.ts:3-14；app.tsx:724-734 | cli_config/mod.rs:192-193；Route 无 location（app.rs:735） |
| T-1 | MISSING | **S1** | NEW | cli.json `session.permissions` 读侧缺失（仅 `--auto` flag 生效） | permission.tsx:14 | app.rs:94-97,1524 |
| T-3 | PARTIAL | **S1**(簇) | NEW | `session.{thinking,verbosity,tps,markdown,scrollbar,grouping}` 读侧缺失；grouping 只写不读；scrollbar/tps 默认呈现分歧 | index.tsx:229,230,1892；config/index.tsx:233 | store.rs:2527-2528；config.rs:202-296 |
| T-4 | MISSING | **S1**(簇) | NEW | mini 显示 8 键 dead（thinking/tools/shell_output/turn_summary/footer/splash/work_spinner/mono） | runtime.boot.ts:87-98 | runtime.rs:59-71 |
| T-2 | MISSING | S2 | NEW | `session.new_location` dead（并入 C-9） | config/index.tsx:173 | 无读者 |
| T-5 | MISSING | S2 | NEW | `diffs.{source,wrap,tree,single,view}` 5 键 dead（parse 仅运行时切换用） | resolve 透传 | store.rs:1496；dialog.rs:725 |
| T-6 | MISSING | S2 | NEW | `scroll.{speed,acceleration}` dead | resolve 透传 | oc-tui-core/scroll.rs 无概念 |
| T-7 | MISSING | S2 | NEW | `prompt.{paste,image_preview}` dead（editor→06:#15） | app.tsx:573；prompt/index.tsx:1676 | config.rs:236（只读 session.image_preview） |
| T-9 | MISSING | S2 | NEW | `debug.{timing,turn_tokens}` dead（devtools→06:#20） | schema config/index.tsx:229-235 | 无 |
| T-10 | MISSING | S2 | NEW | `animations` dead | resolve 透传 | config.rs 不映射 |
| T-11 | MISSING | S2 | NEW | `experimental`（cli.json）dead + DialogExperiments 缺 | dialog-experiments.tsx:25-45 | cli_config/mod.rs:325 |
| T-12 | MISSING | S2 | NEW | `tabs.indicators` dead（layout→06:#3） | resolve:292 | 无 |
| E-1 | MISSING | S2 | NEW | `OPENCODE_LOG_LEVEL` 无对应（日志级别不可调） | logging.ts:164-170 | 无 |
| E-2 | PARTIAL | S2 | NEW | `OPENCODE_CLIENT` 服务端读者缺失（app.name/UA；OTEL→N-6） | server-process.ts:103 | acp/mod.rs:114 仅写 |
| E-3 | MISSING | S2 | NEW | `OPENCODE_FAST_BOOT` 缺 | app.tsx:351 | 无 |
| E-4 | MISSING | S2 | NEW | `OPENCODE_SHOW_TTFD` + `OPENCODE_DIRECT_TRACE` 缺（mini/startup 诊断） | app.tsx:330-333；mini-host.ts:50 | 无 |
| E-5 | PARTIAL | S2 | NEW | `OPENCODE_DISABLE_MODELS_FETCH` 仅测试认领；`OPENCODE_MODELS_URL/PATH` 缺而 EXTRA `OPENCODE_MODELS_DEV_URL` 只盖 URL | server-process.ts:108-114 | tests_parity.rs:225；boot.rs:179 |
| E-6 | MISSING | S2 | NEW | `OPENCODE_NODE_ASSETS_DIR` 外部音频资产目录覆盖缺（sounds{} 路径覆盖已 PORTED） | attention-sounds.node.ts:7-8 | 无 |
| — | MISSING | S1/S2 | KNOWN | `OPENCODE_{PRINT_LOGS(flag 桥),CONFIG_PROJECT_DISABLE,DISABLE_PROJECT_CONFIG,FILEWATCHER_DISABLE,DISABLE_FILEWATCHER,DISABLE_FFF,MODELS_URL,MODELS_PATH,DISABLE_MODELS_FETCH,SIMULATE,GIT_BASH_PATH}` | server-process.ts:94-133 | `07:N-1/N-4` |
| — | MISSING | S2 | KNOWN | `OPENCODE_{STORY,ROUTE}`（06:#22）、`OPENCODE_{EDITOR_SSE_PORT,ZED_DB}`（06:#15）、`OPENCODE_REPO_CLONE_GITHUB_BASE_URL`（09:D11）、`OPENCODE_PHOTON_WASM_PATH`（11:#5） | §1.1 | §1.1 |
| — | DEAD | S1 | KNOWN | `shell`（02:C3）、`tool_output`（02:D2）、`media.image.*`（11:#5）、`experimental.{portable_shell_scanner,subagent_depth}`（02:C4/C5）、cli.json `plugins[]`（07:N-3） | §2.1/§3.1 | §2.1/§3.1 |

**Rust EXTRA（登记备查，非缺陷）**：cli.json `attention.bell`；env 见 §1.6（14 个，其中 `NO_TTY_PROMPT`/`CATALOG_FETCH` 自述为测试基建）。

---

## 9. 修复优先级建议（只读建议）

1. **P1 簇：cli.json 读侧接线**（T-1/T-3/T-4）：`TuiConfig::from_info` 扩展字段 + boot 注入即可覆盖 16 个 leaf；`session.permissions` 与 mini 8 键是用户可感知面。
2. **P1：C-8 会话保留**：`evict_root` + 保留集计算，两个触发点（tab 变更/路由导航），防长跑内存膨胀。
3. **P1/P2：C-7 托管策略**：企业面整缺；若目标用户含组织托管则提 P1，否则 P2。连带修 `provider-use` 字面量与 Wildcard 移植。
4. **P2：C-1 compaction 接线**（一处改动，S1 级影响/S2 级成本比最高）、C-3 watcher 事件通道、C-2 warming、E-1 LOG_LEVEL、E-5 models 源开关补齐（与 N-4 同簇）。
5. **P3**：C-9 随 06:#5；T-5..T-12 按面值；§1.6 EXTRA env 保持登记。

---

## 10. 覆盖清单（本轮实际核对）

- [x] ref：`OPENCODE_*` 全树枚举（118 名字，.ts/.tsx，非 docs）+ 上述全部引用点的上下文精读（server-process.ts:30-140、env.ts、global.ts、version.ts、ssh-askpass.ts、database-path.ts、updater.ts:180-192,450-460、mini-host.ts:44-56,160-170、app.tsx:248-260,330-356,690-770、editor.ts:110-125、editor-zed.ts:183-195、attention-sounds.node.ts、logging.ts:150-172、pty.node.ts、watcher-binding.ts、photon-wasm.node.ts、parser-wasm.node.ts、repository.ts:160-172、persistent-pty/binary.*.ts、acp.ts:10-22、default.ts:92-104、standalone.ts:24-34、theme.ts:308-318、builtins.ts:24-34、config/config.ts:26-114）
- [x] ref 配置面：schema/config.ts（28 键）、config/*.ts 19 子 schema、config/plugin/{policy,compaction,tool-output,image,location-watcher,agent,command,instruction}.ts 精读、plugin/{warming,identity,internal}.ts、instructions/index.ts、instruction-discovery.ts、tui/config/index.tsx:69-330（20 节+resolve）、mini/{types.ts:396-425,runtime.boot.ts:75-98}、managed-policy.ts、session-retention.ts、new-session-location.ts、context/{session-tabs.tsx:120-160,permission.tsx}、util/wildcard.ts、global-roots.ts、process.ts
- [x] rust：oc-core config.rs + config/{normalize,watch,instruction,agent_md,command_md}.rs 关键段、cli_config/mod.rs（SessionInfo/TabsInfo/MiniInfo/validate/from 消费 grep）、oc-tui config.rs:150-330（from_info 全文）、app.rs:195-260,4235-4340（apply_config/HttpConfigSink）、store.rs:501-503,1680-1724,2527-2528,3518-3533,6483-6520,6685-6700、engine/{mod.rs:1819-1843,2043,2108,skills.rs}、runner/compaction.rs:38-60、tool_output.rs、permission{.rs,/saved.rs,sql.rs}、integration/{providers.rs:60-130,console.rs:1-160}、provider_catalog/catalog.rs:115-145,575-620、oc-server/{boot.rs:104-190,232-285,plugins.rs:185-200,errors.rs:560-625}、oc-cli/{main.rs:20-30,120-170,util.rs:14-40,acp/mod.rs:108-118,autoupdate.rs:24-40,noninteractive.rs:375-390}、oc-util/{global_roots,channel,bunman}.rs、persistent_pty.rs 头部、editor.rs、mini/{runtime.rs:59-90,queue.rs:75-85,scrollback.rs}
- [x] 既有登记去重：00-MASTER（S0×5、S1 全表、06:#3/#5/#15/#16/#18/#20/#22/#23/#30、02:C3/C4/C5/D2、07:N-1..N-6/K-*、09:D11/D24、11:#5/#9）、07 全文、09 全文、06 的 #22/#23 行、08 §3.3 warming 行
- [ ] 未做：desktop/web/gui-extensions/enterprise/storage 包内 env 的语义审计（out-of-scope）；`OPENCODE_*` 在 docs/markdown 中的出现（任务明确排除）

— 审计员：Oracle（只读）；除本报告文件外未改动任何文件，未跑 build/test。
