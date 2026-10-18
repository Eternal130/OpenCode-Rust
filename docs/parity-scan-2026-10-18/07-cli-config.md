# Parity 扫描 07 — CLI / 配置 / Schema / Util / 安装打包与迁移（2026-10-18）

> 只读审计。参考实现：`/opt/opencode-ref`（opencode v2.0.24，TS）；目标实现：`/opt/opencode-rust`。
> 范围：`packages/cli/**`（commands/run/acp/services/config/util 等）、`packages/core/src/config*`、`packages/schema/**`、`packages/util/**`、根 `install` 与 bin/scripts ↔ `crates/oc-cli/**`、`crates/oc-core/src/{config.rs,config/,cli_config/,v2_migration.rs,env.rs}`、`crates/oc-schema/**`、`crates/oc-util/**`、`install/`、`.github/workflows/`、`tools/release-smoke.sh`。
> 方法：双侧全树结构走查 + 逐命令/flag/exit-code/输出格式比对 + 关键 handler 精读；每条差异给 ref `path:line` 与 rust `path:line`。KNOWN 标注对照 `docs/auditF-cli-config-data.md`、`ROADMAP.md`、`PORT_REPORT.md`、`tools/RELEASE.md`、`MIGRATION.md`。
> 严重度：**S0** 破坏核心可用性 / **S1** 高（行为错误或用户可感知的功能缺失）/ **S2** 低（外观、边界、宽容度差异）。

## 0. 总览

| 层 | 参考侧盘点 | Rust 现状 | 结论 |
|---|---|---|---|
| CLI 命令面 | 49 个表面（根 TUI + 19 顶级命令 + 29 子命令） | 49/49 名称、别名、隐藏项齐全（`commands.ts` ↔ `cli.rs`） | **无缺失命令** |
| CLI flag/参数 | ~85 个（含 6 个全局） | 全部存在；另 5 个 rust-extra | 无缺失 flag；**7 类取值校验丢失**（§2.2） |
| 退出码 | 1（各错误）/130（run 中断、export 取消）/0 | 1/130 主线对齐；`session export` 交互取消路径整体缺失 | 基本 parity |
| core 配置管线 | 6 源优先级 + 变量替换 + normalize + watch | `oc-core/src/config.rs` 1:1（含 `OPENCODE_CONFIG(_CONTENT)` 每次 load） | parity；**9 个服务端 env 开关无人认领**（§3.4，NEW） |
| `cli.json`（TUI/CLI 配置） | 20 节 ~70 键 + flock 写回 + v1 迁移 + env overlay | `oc-core/src/cli_config/` 全量类型化 + 写回 + 迁移，TUI 已接线 | auditF 大项已修复；**`plugins[]` 通道无消费者**（§3.3，NEW S1） |
| schema（config/*） | 18 个子 schema | 18/18 字段级对齐（auditF 的 "provider +5 超集" 系误报，见 §4.1） | parity |
| schema（顶层文件） | 76 个 `.ts` | 归并为 ~30 个 `.rs`；仅 `v1/*`、`rpc.ts`（纯类型）无对应 | 归并式 parity；v1 桥整体缺席（§6.4） |
| util | 15 个运行时模块 | `oc-util` 15 个模块 + BOM/sha1 分散到 oc-core/oc-cli | `Glob.scan` 明示 deferred（§5） |
| 安装/打包 | npm 多目标包 + brew/aur + install 脚本 + bin/opencode(.cjs/.cjs2) | 单二进制 `opencode-rust` + GitHub release tarball + install.sh（参数化 URL） | 注册过的架构性分叉；install.sh 缺 5 项能力（§6.2） |
| v1→v2 / v2→rust 迁移 | v2 的 v1→v2 in-place 迁移 | `cli_config/v1.rs`（值级）+ `v2_migration.rs`（复制式，含 state 根） | auditF §3.3 已修复 |

**本轮 NEW（未见任何文档登记）共 6 项**：`--print-logs` 未接线（S1）、run 早期错误不走 JSON 格式（S1）、cli.json `plugins[]` 无消费者 + `plugin add` 不按 entrypoint 分流（S1）、server-process 9 个 env 开关无人认领（S1）、`debug paths db` 绕过 `OPENCODE_DB`/channel（S2）、OTEL 可观测性缺席（S2）。其余差异均有登记或在 auditF 中已知。

---

## 1. 命令 / flag 映射表

来源：ref `packages/cli/src/commands/commands.ts`（1-554）↔ rust `crates/oc-cli/src/cli.rs`（18-489）。判定：PORTED=对齐；PARTIAL=存在但语义不全；WRONG=接受但行为不同；EXTRA=rust 独有；MISSING=缺失。

### 1.1 根命令与全局 flag

| 项 | ref | rust | 判定 |
|---|---|---|---|
| CLI 名 | `OPENCODE_CLI_NAME ?? "opencode"`（commands.ts:13,38） | 硬编码 `opencode-rust`（cli.rs:21） | WRONG（注册分叉：共存隔离） |
| `--version` 输出 | `2.0.24`（version.ts 注入） | `opencode-rust v2.0.24`（cli.rs:22） | WRONG（注册分叉，release-smoke 以此为准） |
| `--version-rust` | — | cli.rs:29-30（隐藏，构建戳） | EXTRA（已登记） |
| `directory` 位置参数 | commands.ts:43-46 | cli.rs:48 | PORTED |
| `-c/--continue` / `-s/--session` / `--prompt` | commands.ts:47-57 | cli.rs:51-60；`--prompt`/`--auto` 已传入 `BootArgs`（main.rs:71-79 → misc.rs:47） | PORTED（auditF 1.2 #1/#2 **已修复**） |
| `--print-logs`（全局） | commands.ts:6-11；置 `OPENCODE_PRINT_LOGS=1`（framework/runtime.ts:98） | cli.rs:33-34 解析后**无人读取**；仅 env 生效（standalone.rs:112） | **WRONG / NEW S1**（§2.2-A） |
| `--standalone` / `--server` | 仅根+9 个命令有（ServerParams 展开） | clap global，全命令接受 | EXTRA 宽容（KNOWN auditF 1.2 #3，S2） |
| `--auto` / `--yolo` / `--dangerously-skip-permissions`（后二隐藏） | 根+run（commands.ts:26-36,415） | global；`effective_auto()` 折叠（cli.rs:67-85） | PORTED（作用域为超集，同上） |

### 1.2 子命令与 flag 逐项

| 命令 | ref flag/参数 | rust | 判定 |
|---|---|---|---|
| `upgrade`（别名 `update`） | `target`，`-m/--method` ∈ {curl,npm,pnpm,bun,yarn,vp,brew}（commands.ts:60-74；updater.ts:14） | cli.rs:90-97；`--method` 自由字符串；实际仅 curl 可用，github/local-file 为扩展（updater.rs:45-53,146-176） | PARTIAL（KNOWN：方法集收缩 + 2 个 EXTRA；choice 校验丢失） |
| `uninstall` | `-c --keep-config -d --keep-data --dry-run -f --force` | cli.rs:98-112 → misc.rs:79-156 | PORTED（包管理器 removal 命令行展示缺失，S2） |
| `acp` | —（commands.ts:99） | cli.rs:114 → `crate::acp`（20 文件对 20 文件，client/connection 合并入 transport.rs） | PORTED（R-03 done；本扫描未深审协议细节） |
| `api` | `request`(1..=2)、`-d/--data`、`-H/--header`(≤100)、`--param k=v` | cli.rs:116-129 → api.rs | PORTED；header 无 100 上限（EXTRA 宽容 S2）；查询串空格 `%20` vs ref `URLSearchParams` 的 `+`（api.rs:97-105 vs api.ts:96，S2） |
| `debug agents/config/paths` | `paths <name>` ∈ 10 值枚举（commands.ts:125-140） | cli.rs:288-299；name 自由字符串 | PORTED（枚举校验丢失，KNOWN S2）；`db` 值见 §2.2-E |
| `auth list/login/logout/export/import/switch` | `--format default\|json`、`target`、`--method`、`--answer`(≤100)、`credential`、`file` | cli.rs:301-344 → auth.rs（890 行，含 key/oauth/表单/账号选择） | PORTED（`--format` choice 校验丢失 S2；`--answer` 无上限 S2） |
| `mcp list/add/auth/logout` | `name`、`command…`（`--` 后）、`--url --header --env --global` | cli.rs:346-383 → mcp.rs（含 ref 全部校验消息 mcp.rs:207-227） | PORTED；`--force` EXTRA（已登记 D-01） |
| `plugin list/add/check/update/remove` | `--builtin`、`package`、`target` | cli.rs:385-419 → plugin.rs | PORTED；add/remove `--force` EXTRA；**add 目标文件分流缺失**（§3.3，NEW S1） |
| `models` | —（commands.ts:305-308） | misc.rs:178-209 | PORTED（`localeCompare` vs 字节序，S2 KNOWN） |
| `stats` | `--days≥0 --year∈[1970,9999] --all --project --models --tools --cost --full --limit≥1(默认5) --json` | cli.rs:153-184；u32 无范围校验 | PORTED（范围校验丢失，KNOWN S2；日历图简化的注册分叉；时区 `$TZ`||UTC vs `Intl` stats.rs:107-109，S2；30s 超时专用错误缺失 S2） |
| `mini` | `-c -s --fork --replay(可选bool) --replay-limit -m --agent --prompt --demo(隐藏)` | cli.rs:186-220（`--no-replay` 隐藏 EXTRA，为可选 bool 语义所需）；`mini.replay`/`mini.replay_limit` 回退已接（main.rs:134-163） | PORTED（auditF 1.2 #5 **已修复**） |
| `run` | `message… -c -s --fork -m --agent --format default\|json -f/--file(≤100) --title --thinking` + 权限组 | cli.rs:222-253 → run/ | PORTED（`--format` choice 校验丢失 S2；早期错误 JSON 缺失 §2.2-B；file 无 100 上限 S2） |
| `session list/delete/export/import` | `-n/--max-count(≥1) --format table\|json`、`sessionID`、`--sanitize`、`file`、`--directory` | cli.rs:421-453 → session.rs | PORTED（无 `less` 分页器、UTC 时间戳、export 交互选择器/130、import 无 SessionTransfer 校验 —— 全部 KNOWN/§2.2） |
| `service start/restart/status/stop/get/set/unset` | `key`、`value`、`env-value`、`name` | cli.rs:455-489 → service_cmds.rs | PORTED |
| `reload` | ServerParams | cli.rs:265 → misc.rs:212-223 | PORTED |
| `pair` | `--url`（http/https、无凭据/query/fragment 校验 commands.ts:522-534） | cli.rs:267-271；无校验 | PARTIAL（校验丢失 S2；QR 码缺失 KNOWN 注册 S2） |
| `serve` | `--hostname --port(int) --cors(非空,可重复) --service --stdio` | cli.rs:272-285；port 为 u16；cors 允许空串；`--service+--stdio` 冲突消息一致（main.rs:208-211 = serve.ts:12） | PORTED（hostname 仅 IP 字面量 §2.2-F KNOWN S1；port>65535 提前报错 vs ref 绑定时报错 S2；cors 非空校验丢失 S2） |

**任务书提到但 v2.0.24 参考侧不存在的命令**：`completion`（effect-CLI 无该命令；rust 侧亦无）、`github`（无；auth login 的 target 可接 well-known provider URL，双方一致）、`tui`（根命令即 TUI，双方一致）、`agent`（`debug agents`，双方一致）。

---

## 2. 逐命令行为差异（EXPECTED vs ACTUAL）

### 2.1 已确认修复的 auditF 旧项

| 旧编号 | 内容 | 证据 |
|---|---|---|
| auditF 1.2 #1 | 根 `--prompt` 丢弃 | rust misc.rs:47 `BootArgs{prompt,…}` + oc-tui/app.rs:4323-4335（含 startupPrompt 语义注释）——已接 |
| auditF 1.2 #2 | 根 `--auto` 系列不进 TUI | main.rs:68,76 `auto` → `BootArgs.auto`——已接 |
| auditF 1.2 #5 | mini replay 配置回退 | main.rs:134-163 精确实现 `flag ?? cli.json.mini.replay ?? true`（= mini.ts:37-38） |
| auditF §2.2/1.2#6 | cli.json 无写路径/无 v1 迁移/无 env overlay | oc-core cli_config/mod.rs（988 行：`get/update/merge/env_content/normalize_legacy_tabs/v1`）+ oc-tui config.rs:194-211、app.rs:4235-4283 写回接线 |
| auditF §3.3 | v2→rust 迁移跳过 state 根 | v2_migration.rs:44-59（service[-ch].json、model.json、session.json、`<channel>/tui/*.json`、prompt-history 家族） |

### 2.2 差异明细（NEW 优先）

**A. `--print-logs` 全局 flag 未接线 — NEW S1**
- EXPECTED（ref `framework/runtime.ts:98`）：解析到 `--print-logs` 即 `process.env.OPENCODE_PRINT_LOGS = "1"`，随后 standalone/serve 的 stderr 转发开启（描述 "server logs require --standalone"）。
- ACTUAL（rust）：`cli.rs:33-34` 解析进 `Cli.print_logs`，全树无读取点；仅手工 `OPENCODE_PRINT_LOGS=1` 生效（standalone.rs:112）。
- 影响：`opencode run --print-logs`、`opencode --standalone --print-logs` 等静默无效，排障日志拿不到。

**B. `run` 早期错误不遵守 `--format json` — NEW S1**
- EXPECTED（ref run.ts:67-69 + 248-262）：`run()` 的**所有**异常（含 fork 校验、file-not-found、"You must provide a message"、target 解析失败）都经 `reportRunError(input,…)`，`format==="json"` 时向 stdout 输出 `{"type":"error",…}` JSON 行。
- ACTUAL（rust run/mod.rs:347-355）：`run_inner` 的 Err 走 `report_error("",msg)` → 仅 `run_ui::error`（人类可读）；JSON 分支只存在于 noninteractive.rs:680-694（连接建立之后）。
- 影响：`opencode run --format json --file nope.txt` 输出人类文本，机器消费者解析失败。

**C. `plugin add` 不按 entrypoint 分流 + cli.json `plugins[]` 无消费者 — NEW S1**
- EXPECTED（ref plugin/add.ts:29-45）：`Host.resolve` 得 server/tui 入口；server → 全局 `opencode.json` `plugins`；tui → `Config.update` 写 **cli.json** `plugins`（tui config index.tsx:79-81 为 TUI 装载源）。
- ACTUAL（rust plugin.rs:125-175）：一律写全局 `opencode.json`；且 `oc_core::cli_config::Info::plugins`（cli.json）定义后在 TUI/CLI 均无读取点——TUI 的插件清单来自 opencode.json 系（plugin_boot.rs:105-147 `discover_config_files` 只找 `opencode.json(c)`）。
- 影响：v2 用户 cli.json 里的 TUI 插件指令在 rust 端被忽略；rust `plugin add` 装的 TUI-only 插件写错文件（v2 TUI 看不见）。服务端插件路径不受影响（PORT_REPORT："config plugins[] 双入口"即 opencode.json 通道）。

**D. server-process env 开关 9 项无人认领 — NEW S1（子项分级）**
- EXPECTED（ref server-process.ts:100-133）：`OPENCODE_SIMULATE`、`OPENCODE_MODELS_URL/PATH`、`OPENCODE_DISABLE_MODELS_FETCH`、`OPENCODE_CONFIG_PROJECT_DISABLE`/`OPENCODE_DISABLE_PROJECT_CONFIG`、`OPENCODE_FILEWATCHER_DISABLE`/`OPENCODE_DISABLE_FILEWATCHER`、`OPENCODE_GIT_BASH_PATH`（win）、`OPENCODE_DISABLE_FFF` 均改变服务端行为。
- ACTUAL（rust）：全树 grep 仅测试文件命中；`OPENCODE_CONFIG`/`OPENCODE_CONFIG_CONTENT`/`OPENCODE_CONFIG_DIR` 已接（config.rs:397-403、global_roots.rs:98-99），其余未接。
- 影响：`OPENCODE_DISABLE_PROJECT_CONFIG=1` 的用户仍会走项目配置向上查找（行为分叉，S1）；filewatcher/models 源控制失效（S1/S2）。注：rust 侧 server 为进程内启动，等价开关应落在 `handlers/serve.rs`/`oc-server::boot`。

**E. `debug paths` 的 `db` 值绕过 `OPENCODE_DB`/channel — NEW S2**
- EXPECTED（ref debug/paths.ts:12 + database-path.ts:1-11）：`paths.db = databasePath(global.data)`（`OPENCODE_DB`、`:memory:`、channel 文件名 `opencode-<ch>.db`、`OPENCODE_DISABLE_CHANNEL_DB`）。
- ACTUAL（rust misc.rs:357）：`roots.data.join(DB_FILENAME)` 恒为 `data/opencode.db`；正确的 `oc_core::database::path::default_location()`（path.rs:48-88 已完整实现）未被使用。
- 影响：设了 `OPENCODE_DB` 或非共享 channel 时，`debug paths db` 指向错误文件（仅显示面，故 S2）。另：键顺序与 ref 不同（ref: home,data,bin,log,repos,cache,config,state,tmp,db；rust: home,data,config,state,cache,tmp,db,bin,log,repos — misc.rs:350-361 vs global.ts:23-33，S2 外观）。

**F. 仍开放的 KNOWN 差异（复核确认未修）**

| # | 差异 | ref | rust | 级别 |
|---|---|---|---|---|
| F-1 | `serve --hostname` 仅 IP 字面量，非法值静默回退 127.0.0.1（service.json hostname 同） | server/process.ts:139-148 接受主机名 | boot.rs:269-272 `IpAddr::from_str`；serve.rs:66-72 | S1（auditF #9） |
| F-2 | `session list`：无 `less -R -S` 分页；`toLocaleString()` → 固定 UTC | session/list.ts:63-96,99-107 | session.rs:58-103（`format_local_time` 实为 UTC） | S2（auditF #8） |
| F-3 | `session export` 无 clack 交互选择器（无 TTY+无 id 时 ref 走 picker，取消退出 130） | session/export.ts:24-66 | session.rs:140-143 直接报错 | S2 |
| F-4 | `session import` 不做 `SessionTransfer` schema 校验（任意 JSON 直传）；`--directory` 不做 `path.resolve` | session/import.ts:28-34,39-43 | session.rs:198-230 | S2 |
| F-5 | `upgrade` 方法集：npm/pnpm/bun/yarn/vp/brew 仅"解释自身"；更新通道为 rust 自有 feed（`OC_RELEASE_TAG`/`OPENCODE_RELEASE_URL`），永不采用 TS release | updater.ts:14 vs updater.rs:45-53,97-118 | S1（注册分叉，RELEASE.md） |
| F-6 | `pair` 无 uqr QR 码 | pair.ts:26-33 | misc.rs:226（注释 REGISTERED） | S2 |
| F-7 | `--format`（run/session list/auth list）、`--method`（upgrade）、`debug paths <name>`、stats 范围、`pair --url`、`serve --cors` 非空 —— choice/range 校验全部丢失，坏值静默按默认渲染 | commands.ts:151,313-336,404,431,522-546,68 | cli.rs 对应自由类型 | S2（auditF #4/#6/#9） |
| F-8 | 全局 flag 作用域超集（所有子命令接受 `--standalone/--server/--auto…`） | effect 按命令注入 | clap global | S2（auditF #3） |
| F-9 | `api` 查询参数编码：空格 `+` vs `%20`；`-H` 无 100 上限 | api.ts:96,112 | api.rs:97-105,124 | S2 |
| F-10 | 版本不一致重启时 ref 有 `UpdatePreflight`（升级横幅/失败提示），rust 仅替换 | default.ts:24-52 | misc.rs:31-43 | S2 |
| F-11 | OTEL 可观测性（`OTEL_EXPORTER_OTLP_*`、`OPENCODE_CLIENT` 上报）无对应 — NEW S2 | index.ts:125-133 | 全树无 OTEL | S2 |
| F-12 | uninstall 不展示/执行包管理器 removal 命令与 "Shell PATH:" 行（shell 清理本体已做） | uninstall.ts:31-45 | misc.rs:130-146 | S2 |
| F-13 | `stats` 时区 `$TZ`||UTC（ref `Intl.DateTimeFormat`，无 TZ 时差一整个时区）；30s 超时专用错误文案缺失 | stats.ts:41,69-84 | stats.rs:107-109 | S2 |
| F-14 | channel 来源：ref 构建期注入（pinned 构建为 stable）；rust 运行时 `OPENCODE_CHANNEL` env，默认 latest | version.ts:3-8 | oc-util/channel.rs:26-31 | 注册分叉（影响 db 文件名/service 文件名，两侧行为一致地由各自 channel 推导） |

### 2.3 退出码与 TTY/stdin 判定小结

- `run`：1（错误）/130（二次 Ctrl-C / 用户中断）— noninteractive.rs:514-517,590-597 与 ref noninteractive.ts:536,680-684 对齐；stdin 非 TTY 读取合并（run/mod.rs:388 = run.ts:76）。
- `api`：HTTP 非 2xx → stderr `HTTP <status>` + 1（api.rs 尾段 = api.ts:49-52）。
- `acp`：失败 → 1（ref acp.ts:48 ↔ rust acp::run）。
- `session delete/list`、`auth *`、`plugin check/update`、`stats`、`mcp *`、`service *`：错误 1，双侧一致。
- `uninstall`：非 TTY 无 `--force` → 报错 1（misc.rs:107-113 = ref requireInteractive 文案）。
- TTY 判定：`util::stdin_is_tty()`（run）、`is_tty()`（uninstall）、TUI/mini 的 isatty 均与 ref `process.stdin.isTTY` 对应。

---

## 3. 配置项对比

### 3.1 `opencode.json(c)` 顶层（28 键）— parity

`packages/schema/src/config.ts:25-110` ↔ `crates/oc-schema/src/config/mod.rs:54-137`：`$schema shell model default_agent update share enterprise username permissions agents snapshots watcher formatter lsp media tool_output mcp compaction skills commands instructions references websearch plugins worktree warming providers experimental` 全部同名同 optional；`update∈{disable,notify,auto}`、`share∈{manual,auto,disabled}` 枚举一致（mod.rs:140-155）。

### 3.2 配置来源与优先级 — parity（含两处注册分叉）

ref config.ts:194-238 ↔ rust config.rs:433-525：
`wellknown(最低) → global 目录文档+Directory → 显式 OPENCODE_CONFIG → 向上直查文件（根先近后，同目录 json 先 jsonc 后= jsonc 胜）→ .opencode 项目目录 → OPENCODE_CONFIG_CONTENT(最高)`，`latest()`=findLast。变量替换 `{env:}`（缺失→空串）与 `{file:}`（`//` 注释行豁免、trim、JSON 转义、missing=error/empty）1:1（variable.rs:70-120 = variable.ts:15-52）。JSONC 容错（注释+尾逗Maria）经 json5。watch 计划+100ms 防抖+深等价静默（watch.rs:78,DEBOUNCE=100ms = config.ts:283-289）。`Config.update` 的 shell 补丁 JSONC 保形写（config.rs:694-728 = config.ts:329-349）。
注册分叉：① 物理根目录为 `opencode-rust` 后缀（global_roots.rs:33-36，共存隔离）；② channel 为运行时 env（§2.2 F-14）。

### 3.3 `cli.json`（CLI/TUI 配置）— 大项已补，plugins 通道悬空

- 类型：`oc-core/cli_config/mod.rs:75-332` 覆盖 ref tui/config/index.tsx:69-249 全部 20 节（theme/keybinds/plugins/leader/scroll/attention(+EXTRA `bell`)/diffs/terminal/prompt/session(10 键)/tabs(5 键)/mini(11 键)/debug/experimental/animations/mouse/cursor）；`validate()`（369-441）复刻全部枚举/约束（volume∈[0,1]、leader.timeout>0、scroll.speed≥0.001、sounds 键域、replay_limit>0、turn_tokens bool|"verbose"…）；未知键忽略、已知键类型错→整文档失效= 默认（= ref `decodeUnknownOption` 语义）。
- 读写：flock（`state/locks`，30s 上限= 注册的降级）、JSONC 保形 `update`、缺失文件播种 `$schema = https://opencode.ai/v2/cli.json`、`OPENCODE_CLI_CONFIG_CONTENT` 深合并 overlay（mod.rs:466-606 = config.ts:26-114）。
- v1 迁移：`terminal.copy_on_select→copy`、keybind 旧名、`tui.json`+`kv.json` 采纳（v1.rs）；注册分叉：ref 对重复 keybind 属性做原文手术，rust 值级重写（一次性丢注释，mod.rs:42-47 登记）。
- TUI 消费：oc-tui config.rs:194-211 读，app.rs:4235+（theme/mode/sidebar/grouping…）写回 —— auditF §2.2 主缺口已闭合。
- **悬空项（NEW S1）**：`Info.plugins`（cli.json `plugins[]`）无任何消费者；TUI 插件清单实际来自 opencode.json 系（plugin_boot.rs:105-147）。见 §2.2-C。

### 3.4 配置相关 env 面（差异表）

| env | ref 行为 | rust 现状 | 判定 |
|---|---|---|---|
| `OPENCODE_CONFIG` / `OPENCODE_CONFIG_CONTENT` | server-process.ts:117-118 → 每次加载的显式/内联源 | config.rs:397-403（Loader::new 每次） | PORTED |
| `OPENCODE_CONFIG_DIR` | Global.config 覆盖（index.ts:120） | global_roots.rs:98-99 | PORTED |
| `OPENCODE_CLI_CONFIG_CONTENT` | cli.json overlay（config.ts:32-34） | cli_config env_content（mod.rs:471-475） | PORTED |
| `OPENCODE_TEST_HOME` | 测试专用 home | global_roots.rs:77-79 | PORTED |
| `OPENCODE_DB` / `OPENCODE_DISABLE_CHANNEL_DB` | db 文件名/位置 | database/path.rs:48-88 | PORTED（`debug paths db` 显示面除外，§2.2-E） |
| `OPENCODE_PASSWORD`/`OPENCODE_SERVER_PASSWORD` | 密码 + `Env.session()` 过滤二者 | util.rs:19-32 | PORTED |
| `OPENCODE_SSH_ASKPASS_PORT` | askpass 隐藏入口（index.ts:20-23） | main.rs:30-32 | PORTED |
| `OPENCODE_PRINT_LOGS` | 由 `--print-logs` 置位 | 仅 env 生效 | **MISSING（flag 桥，§2.2-A）** |
| `OPENCODE_CONFIG_PROJECT_DISABLE`/`OPENCODE_DISABLE_PROJECT_CONFIG`、`OPENCODE_FILEWATCHER_DISABLE`/`OPENCODE_DISABLE_FILEWATCHER`、`OPENCODE_DISABLE_FFF`、`OPENCODE_MODELS_URL/PATH`、`OPENCODE_DISABLE_MODELS_FETCH`、`OPENCODE_SIMULATE`、`OPENCODE_GIT_BASH_PATH` | server-process.ts:100-133 | 无认领点 | **MISSING（§2.2-D）** |
| `OPENCODE_CHANNEL` | 构建期 define | 运行时 env（channel.rs） | 注册分叉 |
| `OTEL_EXPORTER_OTLP_*`、`OPENCODE_CLIENT`（上报名） | Observability 层 | 无 | MISSING（S2，NEW） |

---

## 4. Schema 文件映射表

### 4.1 `config/*` 子 schema（18/18）

| ref（packages/schema/src/config/） | rust（crates/oc-schema/src/config/） | 判定 |
|---|---|---|
| agent.ts（12 字段 + Color 正则 + mode 枚举） | agent.rs（字段逐一含 PositiveInt/Ruleset） | PORTED |
| command.ts | command.rs | PORTED |
| compaction.ts | compaction.rs | PORTED |
| experimental.ts | experimental.rs | PORTED |
| formatter.ts / lsp.ts / media.ts（+rust `max_base64_bytes` EXTRA）/ mcp.ts（=mcp.ts 顶层）/ model.ts（短串 `provider/model#variant` ↔ 显式，serde rename `providerID`）/ plugin.ts / policy.ts / reference.ts / tool-output.ts / warming.ts / watcher.ts / websearch.ts / worktree.ts | 同名 .rs | PORTED |
| provider.ts | provider.rs | **PORTED 1:1** —— 更正 auditF §2.1："rust +5 超集（timeout/header_timeout/chunk_timeout/transport/compaction）" 系误报，ref provider.ts:11-18 的 `Settings` 本就含这五项 |
| config.ts（根 + Patch/Document/Directory/Entry + config.updated 事件） | config/mod.rs:54-193 | PORTED |

### 4.2 顶层 schema 文件（76 → 归并）

ref 文件名 → rust 去向：`agent→agent.rs`、`command→command.rs（并入 misc/命令面）`、`connection/credential→integration.rs+misc.rs`、`event(-manifest)/durable-event-manifest/event-log→event.rs+events.rs`、`file-diff→session_support.rs`、`form→form.rs`、`identifier/id 族→identifier.rs+id.rs+session.rs`、`instruction(-entry)→session_support.rs/session_event.rs`、`integration(-id)→integration.rs`、`llm→common.rs`、`location(-event)→location.rs+events.rs`、`lsp-event/mcp-event/models-dev/server-event/tui-event/vcs-event/workspace-event/worktree-event/installation-event→misc_events.rs/events.rs`、`money→common.rs`、`permission(-saved)→permission.rs`、`project(-id)/workspace(-id)→session.rs/events.rs`、`prompt(-input)→prompt.rs`、`pty/persistent-pty→misc.rs+misc_events.rs（票据在 oc-server/tickets.rs）`、`question→misc.rs`、`reference→prompt.rs+config/reference.rs`、`session 族（error/fork/inbox/message/metadata/provider-context/revert/stats/status-event/transfer）→session*.rs 五模块`、`shell→shell.rs`、`skill→skill.rs`、`snapshot→session_support.rs`、`token-usage→common.rs/session.rs`、`tool→tool.rs`、`websearch→config/websearch.rs+misc_events.rs`、`schema.ts（optional/AbsolutePath 等 helper）→common.rs`、`vcs→location.rs/events.rs`、`worktree→events.rs+config/worktree.rs`。

**无对应者**：
| ref 文件 | 性质 | 判定 |
|---|---|---|
| `rpc.ts` | 纯 TS 类型（StandardSchema/JsonSchema 接口），无运行时 schema | N/A（无需移植） |
| `legacy-event.ts`/`filesystem-v1.ts`/`session-v1.ts`/`permission-v1.ts`（`v1/*`） | v1 兼容 wire（`command.executed` 等） | MISSING — 隶属 v1 桥（§6.4） |

---

## 5. Util 模块映射

| ref（packages/util/src） | rust | 判定 |
|---|---|---|
| hash.ts（`fast`=sha1、`sha256`） | oc-util/hash.rs 仅 sha256；sha1 分散：oc-cli/util.rs:82-87、oc-core/snapshot.rs:84（手写）、service_config.rs:62 | PORTED（分布式；行为等价，FNV/UTF-16 语义在 encode.rs 保真） |
| encode.ts（base64url 无填充、FNV-1a base36 checksum、sampled） | encode.rs（UTF-16 码元哈希、五窗采样，测试向量对齐 ref 测试） | PORTED |
| bom.ts（split/join/has/readFile/syncFile） | oc-core/file_mutation.rs:78-103（bom_split/join/has + 写保形） | PORTED（分布式） |
| binary.ts（search/insert） | 无独立对应（Rust 侧用 `Vec::binary_search_by` 等原生替代） | PORTED（等价原语，非逐文件） |
| glob.ts（scan/scanSync/match） | glob.rs 仅 `matches`（minimatch dot:true 子集；**不支持 brace/extglob**，rs:7 注释）；`scan` 注释明示 deferred | **PARTIAL S2**（ref 侧仅 fs-util.ts:160 使用 scan；核心发现管线未依赖） |
| flock.ts（acquire/release/全局 state 锁目录） | flock.rs（exclusive/shared/try/timeout + path） | PORTED |
| path.ts | pathx.rs（filename/truncate/tilde/encode 等） | PORTED |
| process.ts（AppProcess/selfCommand） | procutil.rs（进程组安全信号）+ 各 spawn 点 | PORTED（等价重构） |
| retry.ts（指数退避+瞬时错误探测） | retry.rs（RetryConfig/is_transient_message/retry_with） | PORTED |
| open.ts（openUrl/openPath） | oc-tui/store.rs:5188-5207 `open_url`（含 sink 测试缝）；openPath 场景未见独立封装 | PARTIAL（URL 面 PORTED，路径面未单列 S2） |
| npm.ts（sanitize/isRegistryPackage/isInstallablePackage/cacheKey/add/resolve/check/update/which） | bunman.rs（bun 定位/ensure）+ oc-plugin-host/context/install.rs（install/latest/resolve entry）+ oc-cli/plugin.rs | PORTED（分布式；`plugin add` 分流缺陷见 §2.2-C） |
| npm-config.ts | npm_config.rs | PORTED |
| global-roots.ts / global.ts | global_roots.rs（含 bin/log/repos/snapshot/locks/db/ensure） | PORTED（APP_NAME 注册分叉） |
| fs-util.ts | 拆散至 config.rs（walk_up/resolve_path）与各调用点 | PORTED（分布式） |
| observability*（OTEL/logging） | 缺 OTEL；日志布局为注册分叉（server.log/tui.log） | MISSING(S2)/注册分叉（auditF §3.2 日志项仍开放） |
| activity-calendar/session-title-fallback/patch/cross-spawn-spawner/artifact | 归并入 stats 渲染与 spawn 工具 | PORTED（等价） |

---

## 6. 安装 / 打包 / 迁移 / 更新通道 / 发布产物

### 6.1 单二进制 vs bun/npm（架构性注册分叉）

ref：`packages/cli/package.json` bin `opencode`/`opencode2`（.cjs 引导器→平台 npm 包），发布物为 `@opencode/cli-<os>-<arch>[-baseline][-musl]` npm 包 + brew（Core/tap）+ AUR（script/publish*.ts）。rust：单一 `opencode-rust` 二进制（Cargo.toml [[bin]]），GitHub release tarball+sha256 边车（release.yml:141-148），web 资产内嵌（R-04）。**v1 兼容桥（bin/opencode.cjs + run/v1.ts + schema v1/*）随 npm 包装面整体缺席** —— 单二进制 scope 下的注册缺席，无文档显式登记"v1 桥不做"，建议补登记或补实现。

### 6.2 install 脚本差异（ref 根 `install` 488 行 ↔ rust `install/install.sh` 286 行）

| 能力 | ref | rust | 判定 |
|---|---|---|---|
| 位置 `~/.opencode/bin`、PATH 提示、ASCII banner、GITHUB_PATH | install:69-70,390-487 | install.sh:86,206-285 | PORTED |
| 最新版本探测（opencode.ai update API + npm scope 回退） | install:169-201 | **无**（URL 必填，--url/$RELEASE_URL，注册：不采用 TS release） | 注册分叉 |
| 目标矩阵（os-arch-baseline-musl 检测、Rosetta、AVX2、alpine） | install:80-167 | 无（URL 模板 `{target}` 仅 os-arch） | MISSING（随单 linux-x64 产物，R-11 pending） |
| 下载进度条（curl trace+fifo）、tar 必备检查 | install:164-167,238-346 | 无（curl --retry 静默） | MISSING S2 |
| sha256 校验 | 无（npm 完整性隐含） | **EXTRA**（--sha256/边车，install.sh:98-107,189-198） | EXTRA（增强） |
| `opencode2` legacy shim 写入 | install:358-379 | 无 | MISSING（v1 桥面，S2） |
| 本地二进制安装 `-b` | install:49-56 | install.sh:69-70（另支持 tar.gz/zip 载荷 EXTRA） | PORTED+ |
| `--no-modify-path`、未知选项告警 | install:58-66 | install.sh:74-82 | PORTED |

### 6.3 更新通道 / 版本检查

ref：default(TUI) handler 内 updater 循环每 10min（default.ts:69-86），preflight 横幅；`--print-logs`→env；channel=stable。rust：仅 TUI+serve 路径 spawn 后台检查（main.rs:57-60，serve 转发 installation 事件 serve.rs:128-156）；dev 构建/禁用策略不跑；自解压替换仅显式 `upgrade`；`--version-rust` 构建戳。**注册分叉**（RELEASE.md/lane C），行为面差异 F-5/F-10 已列。

### 6.4 迁移

- v1→v2（cli.json）：双方等价（§3.3；ref migrate.ts:17-117 ↔ rust cli_config/v1.rs + mod.rs:630-669；原文手术 vs 值级重写的注册差异）。
- v2(TS)→rust：`v2_migration.rs`（1084 行）一次性复制：db（sqlite backup API + wal/shm 回退）、auth.json、config 平面文件、cache/npm+bin、**state 根全套**（service 注册/model.json 双写桥/session.json/tabs 转换/prompt-history 家族）、marker+latch+stale 锁。auditF §3.3 已闭合。
- 已知残留：prompt-history/frecency/stash 三文件**只有迁移写入、无运行时读写者**（grep 全树仅 v2_migration.rs 命中）→ 迁移搬来的历史无人消费（auditF §3.2 MISSING 项对 TUI 侧仍开放，S1 于 TUI 体验面）。

### 6.5 CI/发布流水线

rust `.github/workflows/ci.yml`（fmt/clippy/nextest 分层+PTY 串行）与 `release.yml`（web-assets 引用源 pinned 构建 → linux-x64 tarball → smoke → GitHub release）；`tools/release-smoke.sh` 校验 `--version` 精确串、`--version-rust` 戳、serve 沙箱随机端口 + /api/info + SIGTERM、run_e2e 对 release 二进制。ref 侧为 npm/brew/aur 多路 publish + verify-artifact/service-smoke。判定：流程等价性为注册 scope 差异；**平台矩阵仅 linux-x64**（KNOWN R-11）。

---

## 7. 缺失清单汇总

**缺失命令**：无（49/49）。
**缺失 flag**：无（5 个 EXTRA：`--version-rust`、`mcp add --force`、`plugin add/remove --force`、`--no-replay`（语义所需））。
**缺失配置项**：`opencode.json` 28/28；`cli.json` 20/20（`attention.bell` 为 rust EXTRA）。真正缺失的是**通道/开关**：cli.json `plugins[]` 消费者（NEW）、§3.4 的 10 个 env 开关（1 个 flag 桥 + 9 个服务端开关）、OTEL 面。
**缺失 schema 文件**：`v1/*` 四件 + `legacy-event.ts`（v1 桥）；`rpc.ts` 为纯类型 N/A。
**缺失 util 能力**：`Glob.scan`（deferred）、minimatch brace/extglob、openPath 独立封装、OTEL。
**缺失安装/打包能力**：平台矩阵（仅 linux-x64）、install 最新版探测/baseline-musl/进度条/opencode2 shim、v1 桥、npm/brew/aur 发布路。

---

## 8. KNOWN / NEW 归档索引

| 编号 | 摘要 | 级别 | 状态 |
|---|---|---|---|
| N-1 | `--print-logs` flag 未桥接到 env | S1 | **NEW** |
| N-2 | run 早期错误不输出 `--format json` 错误行 | S1 | **NEW** |
| N-3 | cli.json `plugins[]` 无消费者；`plugin add` 不按 server/tui entrypoint 分流 | S1 | **NEW**（PORT_REPORT 的 "config plugins[] 双入口" 只覆盖 opencode.json 通道） |
| N-4 | server-process 9 个 env 开关无人认领（PROJECT_DISABLE/FILEWATCHER/MODELS_URL…） | S1 | **NEW** |
| N-5 | `debug paths db` 绕过 OPENCODE_DB/channel；键序不同 | S2 | **NEW** |
| N-6 | OTEL 可观测性缺席 | S2 | **NEW** |
| K-1 | serve/hostname IP-only 静默回退 | S1 | KNOWN（auditF #9，未修） |
| K-2 | upgrade 方法集收缩 + rust 自有更新通道 + 默认端口分叉 | S1 | KNOWN（RELEASE.md/PORT_REPORT §3） |
| K-3 | stats 日历简化、时区来源、范围校验 | S2 | KNOWN（auditF #9） |
| K-4 | session list 分页器/本地时间；export 选择器/130；import 校验 | S2 | KNOWN（auditF #8） |
| K-5 | 枚举/范围校验丢失族（--format/--method/paths name/pair url/cors/header 上限） | S2 | KNOWN（auditF #4/#6） |
| K-6 | 全局 flag 作用域超集 | S2 | KNOWN（auditF #3） |
| K-7 | pair 无 QR；api 查询编码差异 | S2 | KNOWN/NEW 混合（QR=注册） |
| K-8 | prompt-history/frecency/stash 无运行时消费者 | S1(TUI 面) | KNOWN（auditF §3.2，半修：迁移已搬、读写未建） |
| K-9 | 日志布局（server.log/tui.log vs opencode[-ch].log、无 trim） | S2 | KNOWN（auditF §3.2） |
| K-10 | 二进制名/版本串/APP_NAME/channel 运行时化/默认端口 | — | 注册分叉（共存隔离族） |
| K-11 | 平台矩阵 linux-x64 only；install 探测/baseline/musl/进度条/opencode2 shim；v1 桥 | S1/S2 | KNOWN（R-11 pending）+ v1 桥建议补登记 |
| — | auditF 1.2 #1/#2/#5、§2.2 cli.json、§3.3 迁移洞 | — | **已修复**（§2.1） |

---

## 9. 覆盖清单（本次实际核对过的面）

- [x] commands.ts ↔ cli.rs 全命令/别名/隐藏 flag/参数形态（逐项表 §1）
- [x] index.ts 入口（askpass 分发、错误到 stderr、exit 链）↔ main.rs
- [x] handlers：default/misc（root/upgrade/uninstall/acp/mini/models/reload/pair/debug×3）、api、auth×6、mcp×4、plugin×5、session×4、service×7、stats、serve —— 精读输出格式与退出码
- [x] run/*（run.ts、noninteractive.ts 859 行事件机/JSON 流/130/SIGINT、ui）↔ run/{mod,noninteractive,tool_render}.rs（事件名 text/reasoning/step_start/step_finish/tool_use/error、promoted/inbox、reconcile 投影逐段比对）
- [x] session-target.ts ↔ run/mod.rs::resolve_session_target（fork/continue/缺失 id 变新 id/env 注入门控）
- [x] server-process/standalone/service-config/server-connection（mismatch replace/ignore/error、端口/文件名/channel、legacy sha1 文件名、versionBelongsToChannel）
- [x] core config.ts + discovery/variable/watch/normalize（抽查 normalize 旧键：snapshot→snapshots、mcp_timeout、preserve_recent_tokens；auditF 全量核对引用）
- [x] cli config（config.ts/schema.ts/migrate.ts + tui config index.tsx 20 节）↔ cli_config/{mod,jsonc,v1}.rs + oc-tui config.rs/app.rs 接线
- [x] schema config/* 18 文件字段级；顶层 76 文件去向映射；v1/rpc 缺席定性
- [x] util 15 模块（hash/encode/bom/binary/glob/flock/path/process/retry/open/npm/npm-config/global(-roots)/fs-util/observability）
- [x] install 脚本 488↔286 行逐能力；package.json bin/scripts；workflows（ci/release）；release-smoke.sh；RELEASE.md/PORT_REPORT/ROADMAP/MIGRATION 交叉标注
- [x] v2_migration.rs 头部语义 + prompt-history 消费者 grep
- [ ] ACP 20 文件协议细节（本轮仅文件级+既有 R-03 验收引用，未逐方法比对 —— 建议独立扫描）
- [ ] oc-tui 内部对 cli.json 20 节的逐键消费完整度（属 TUI 扫描面，本报告只确认了读取服务与写回接线）

— 审计员：Oracle（只读）；除本报告文件外未改动任何文件。
