# S3a 结论: oc-cli 是 ref CLI 的高保真移植——命令树/flags 全量对齐、run/noninteractive 事件流逐语义复刻；主要分歧集中在升级通道整体换源（注册分歧）、TTY 权限交互提示（rust 扩展）与入口诊断信号（OTLP/heap/cpu-profile/EPIPE）缺席，另发现 mini 重连循环一处真实的无限重试风险。

## 命令树全量对照表

ref 锚点：`commands/commands.ts`（1-554）；rust：`cli.rs`（1-523）。

| 命令/子命令 | ref 定义 | rust 定义 | 判定 |
|---|---|---|---|
| 根 `opencode [directory] -c -s --prompt` | commands.ts:38-58 | cli.rs:27-64 | ✅ |
| `upgrade [target] -m/--method`（alias update） | :60-74 | cli.rs:257-265 | ✅；rust method 枚举多 `local-file`/`github`（➕注册扩展） |
| `uninstall -c -d --dry-run -f` | :75-98 | cli.rs:266-280 | ✅ |
| `acp` | :99 | cli.rs:282 | ✅（S3b 审） |
| `api <req...> -d -H(≤100) --param` | :100-116 | cli.rs:283-297 | ✅；ref `--param` 单值 optional，rust 可重复 Append（超集 R-6） |
| `debug agents/config/paths [10键]` | :117-143 | cli.rs:526-536 | ✅ 10 键一致 |
| `auth list --format / login --method --answer(≤100) / logout/export/import/switch` | :147-219 | cli.rs:541-580 | ✅ 参数全同 |
| `mcp list / add <name> [--url --header --env --global] [-- cmd] / auth / logout` | :225-258 | cli.rs:586-619 | ✅；➕rust `--force` |
| `plugin list --builtin / add / check / update / remove` | :261-304 | cli.rs:622-656 | ✅；➕add/remove `--force` |
| `models` | :305-308 | cli.rs:318-319 | ✅ |
| `stats --days --year --all --project --models --tools --cost --full --limit(5) --json` | :309-339 | cli.rs:320-352 | ✅ 含边界 |
| `mini -c -s --fork --replay/--no-replay --replay-limit -m --agent --prompt --demo(hidden)` | :340-375 | cli.rs:353-388 | ✅ 三态 replay 用 overrides_with 对 |
| `run [msg...] -c -s --fork -m --agent --format -f(≤100) --title --thinking` | :376-417 | cli.rs:389-421 | ✅ |
| `session list -n --format / delete / export --sanitize / import --directory` | :418-469 | cli.rs:658-690 | ✅（注：inv-rust-map 所记 rust 有 Move/Rename 不实——两侧都只有 4 子命令） |
| `service start/restart/status/stop/get/set/unset` | :471-512 | cli.rs:692-726 | ✅ |
| `reload` / `pair --url` | :513-536 | cli.rs:433-439 | ✅（--url 校验层位不同，语义同） |
| `serve --hostname --port --cors(非空,可重复) --service --stdio` | :537-550 | cli.rs:446-459 | ✅（--service+--stdio 互斥一致） |
| ➕rust 独有 | — | `completion <shell>`、`--version-rust` | rust-extra |

**全局 flags**：`--print-logs` ✅等价；`--standalone/--server`：ref 按命令声明，rust 一律全局（行为超集、无错误接受面，mcp.rs:18-22 G-274）；`--auto/--yolo/--dangerously-skip-permissions(hidden)`：ref 仅根+run，rust 全局 + `effective_auto()` 折叠（根/run 语义与 ref 一致）。

## 对比表（逐域）

| 域 | ref | rust | 判定 |
|---|---|---|---|
| 入口装配 | index.ts:81-147（Heap/CpuProfile listen、uncaught 兜底、OTLP Observability、stderr fatal 重定向） | main.rs:26-76（askpass 分流、print-logs、v2 迁移、autoupdate spawn、dispatch） | ✅ 主干等价；❌诊断信号缺失（D-7） |
| askpass 分流 | index.ts:20-23 | main.rs:29-31 | ✅ |
| framework | spec.ts/runtime.ts（懒加载 handlers） | main.rs dispatch + handlers/mod.rs | ✅ 等价 |
| 默认入口→TUI | default.ts（chdir、resolve(mismatch:replace)+preflight、findSession 探测、updater 10min 循环） | misc.rs:17-87 + boot_session_ids:523-535 | ✅；preflight 渲染降级（D-5）、更新循环换位 |
| api | api.ts | api.rs:114-246 | ✅ 逐行对齐（auth 头先行、-H 覆盖、URL set_path 不 join） |
| debug agents/config/paths | agents/config/paths/redact.ts | misc.rs:425-510,540-626 | ✅ redact 正则逐语义复刻 |
| auth 全家 | auth/*.ts + ui/integration-picker | auth.rs + auth_form.rs | ✅ loginChoices 优先级/opencode-go 置顶/key 后置、wellknown http(s) 探测（G-283⑤）、三流、cancel 收尾；clack→dialoguer UI 库换位 |
| mcp list/add/auth/logout | mcp/*.ts | mcp.rs:35-450 | ✅ add 校验矩阵一致；jsonc set_path 注释保留 |
| plugin list/add/check/update/remove | plugin/*.ts | plugin.rs:31-150 | ✅ 表格/版本 sha 截断/`-`/`*`/`opencode.` 过滤；update 为"注册分歧"（config-managed） |
| models | models.ts | misc.rs:275-308 | ✅（localecmp 对齐 CLDR） |
| stats | stats.ts（343L 全读） | stats.rs:1-220+ | ✅ 范围互斥、timezone IANA、tools 模式矩阵、COLORFGBG 调色板、本地午夜分桶 |
| run 主流程 | run.ts（266L 全读） | run/mod.rs（635L 全读） | ✅ fork 校验、stdin merge、`"quoted"` 拼接、10MiB/目录拒绝、mime 嗅别、title 50 截断 |
| noninteractive | noninteractive.ts（859L 全读） | noninteractive.rs:1-1327 | ✅ 事件流/键序、promoted/finalizing 状态机、inboxID 提升、execution 三终态、reconcile 分页 200、双击 Ctrl-C 130、JSON 行信封、global form 归属——逐语义对齐 |
| run ui | ui.ts | run_ui.rs | ✅ 含 `blank` 单次闩锁怪癖复刻 |
| session list/delete/export/import | session/*.ts | session.rs（509L 全读） | ✅ less -R -S 分页、export TTY 选择器、import URL/409/transfer 校验 |
| service * | service/*.ts + service-config.ts（283L 全读） | service_cmds.rs + service_config.rs | ✅ set/unset 六键、写前 stop、cors 校验、password 持久化；❗默认端口分歧（D-4） |
| serve | serve.ts + server-process.ts（212L 全读） | serve.rs（303L 全读）+ oc-server boot/lifecycle | ✅ 三模式、$HOME chdir、stdio 清密码、EADDRINUSE→incumbent 15s 轮询、注册+5s 自愈 |
| server-connection | server-connection.ts（100L 全读） | server_connection.rs:1-261 | ✅ 互斥、5s 健康探测、版本不匹配警告继续、mismatch 三态；➕B4-H3 build-marker（D-2） |
| standalone | standalone.ts（65L） | standalone.rs（298L） | ✅ 32B base64url 密码、`serve --stdio --port 0`、ready 行、EOF lease、TERM→3s KILL（G-158） |
| ssh-askpass | ssh-askpass.ts（40L） | ssh_askpass.rs（63L） | ✅ 端口/token/confirm/5min；prompt 参数位置差异（D-8，rust 论证编译形态） |
| mini | mini.ts（226L 全读）+mini-host.ts | mini.rs:1-160 | ✅ TTY 校验/fork 规则/mergeInput 顺序/replay 三态；❗重试条件见 R-1 |
| pair | pair.ts（58L 全读） | misc.rs:325-419 | ✅ disabled 拒绝、QR border2 缩进；链接拼接分歧 D-3 |
| upgrade/uninstall | upgrade.ts+updater.ts（538L 全读） | misc.rs:89-252 + updater.rs + update_feed + autoupdate | 语义框架 ✅（版本跳过、方法探测、包管理器命令表、removal 预览、绝不自删二进制 G-288⑤）；通道整体换源（D-2） |
| web-ui/app-assets/service-registration/retained-image | 各 ts | 无 oc-cli 对应（oc-server assets.rs/web 层承接；retained-image win32 专属） | 换位/平台性缺席 |
| node/* 双插件运行时、EPIPE、target | node/*.ts | 无（插件运行时=oc-plugin-host bun guest） | 换位，非缺失 |
| v1 兼容 run/v1.ts | 86L | 无 | rust 无 v1 SDK 边界，连带缺席（合理） |
| session-target | session-target.ts（171L 全读） | run/mod.rs:275-379 | ✅ findSession 仅 NotFound→None、latest 分页 50 递归、fork 缺会话报错；模型回退仅新建时 |

## 行为差异清单 D-n

- **D-1 TTY 权限交互提示（rust 扩展）**：ref 非交互 run 一律自动拒绝权限（noninteractive.ts:158-186）；rust 在 stdin+stdout 均 TTY 且非 CI 时弹 dialoguer once/always/deny（noninteractive.rs:257-305、370-401，kill-switch `CI`/`OPENCODE_NO_TTY_PROMPT`）。
- **D-2 升级/更新通道整体替换**：ref updater 走 `opencode.ai/update/api`+homebrew formula（updater.ts:251-307），root TUI 内 10 分钟更新循环+TUI 内更新对话框；rust 换自身 GitHub release feed（sha256 校验、dev-tag 关闸、`local-file/github` 方法、serve/TUI 才 spawn 后台检查、通知走 state 目录文件/`installation.*` 总线）。托管服务发现额外比较 `x-opencode-build`（server_connection.rs:169-196）——同版本旧 build 也判不兼容。
- **D-3 pair 链接拼接**：ref `new URL("/auth/connect/<code>", url)` 绝对路径替换 base path；rust 拼接保留 base path（反代场景有意偏差）。
- **D-4 默认服务端口**：ref `latest/dev/beta/next→0xc0de、local→0xc0df`（service-config.ts:35-39）；rust 用 `rust_default_port`（同 sha1 方案、错开 v2）——与 v2 共存刻意不兼容。
- **D-5 更换后台服务的提示渲染**：ref UpdatePreflight 的 opentui split-footer → rust stderr 文本行。
- **D-6 `api --param` 多值**：ref 单个 optional；rust 可重复 Append（超集）。
- **D-7 入口诊断面缺席**：ref 的 SIGUSR1 heap 快照/SIGPROF cpu-profile/OTLP 观测、EPIPE 吞护，rust main.rs 均无对应。
- **D-8 ssh-askpass prompt 参数位置**：argv[1]+空格 join vs ref `argv.slice(2)`（rust 修复 ref 潜在缺陷，注册偏差）。
- **D-9 rust-extra flags**：`completion`、`--version-rust`、`mcp add --force`、`plugin add/remove --force`。
- **D-10 run 标题更新失败被吞**：ref 失败即报错；rust `let _ =` 忽略（低危容错偏差）。
- **D-11 升级目标语义**：ref 无 target 时取 opencode.ai feed latest 比对跳过；rust 取自身 feed channel-eligible 最新版（dev 构建直接解释并退出）。

## 正确性风险 R-n

- **R-1（中）mini 无限重试非传输错误**：ref 仅对 `ClientError && reason==="Transport"` 走 reconnect 重试（mini.ts:157-180）；rust `resolve_mini_target`（mini.rs:122-147）把**任何**非 "Session not found" 的 `Err(String)` 都交给 reconnect 循环（250ms、无上限）。一次 5xx/逻辑错误会变成永久重试循环——transport 分类信息在返回 String 时已丢失。建议错误路径保留可重试标记。
- **R-2（低）api --param 重复键**：BTreeMap 后写覆盖先写。
- **R-3（低）D-1 无人值守风险**：expect/自动化脚本持有 TTY 且未设 CI 时，`opencode run` 在权限请求上阻塞等待人工；有 kill-switch 但需知晓。
- **R-4（低）title 更新静默失败**（D-10）。
- **R-5（待核）service stop/restart 的 pty 策略**：rust 用 `PtyStop::Clear`（service_cmds.rs:41,72）；ref `Service.stop(options)` 未显式传 pty、`managedService.restart` 显式 `handoff`（server-connection.ts:59）。建议 S2 client lane 核对 `Service.stop` 默认 pty 值。
- **R-6（低）`upgrade --method` 枚举扩展**（v2 七法全在，无破坏）。

## 覆盖声明

双侧**全文通读**：ref commands.ts、index.ts、framework/{spec,runtime}、run/{run,noninteractive,ui,v1}、services/{server-connection,standalone,service-config,service-registration,web-ui,retained-image,updater,updater-action}、commands/handlers/{default,upgrade,api,models,reload,pair,serve,stats,session/*,mcp/*,plugin/*,auth/{list,login,logout,export,import,switch,shared,account,form(至46行)},service/*,debug/*}、env/version/database-path/app-assets/cpu-profile/heap/session-target、ssh-askpass、mini.ts、mini-host.ts(头部)、server-process、node/*；rust cli.rs、main.rs、run_ui.rs、ssh_askpass.rs、util.rs、run/{mod,noninteractive(1-1327)}、services/{server_connection,standalone}、handlers/{mod,api,serve,service_cmds,session,misc,stats(1-220),auth(1-700),mcp(1-450),plugin(1-150),mini(1-160)}。

**部分/未读**：rust auth.rs 701-1518（logout/export/import/switch+测试）、auth_form.rs 全文、plugin.rs 151-863（add 安装/check/update/remove）、service_config.rs 正文（grep 键面核对）、updater.rs 121-2329 与 update_feed.rs 全文、autoupdate.rs 91+、mini.rs 161-398、tool_render.rs（ref 对应物在 tui 包，lane 外）；ref uninstall.ts 正文、config/{config,schema,migrate}.ts、ui/{integration-picker,prompt(部分),timeline.tsx}、update-preflight.tsx、form.ts 47 行以后。若需终裁建议补一轮窄读（尤其 plugin.rs update 的"注册分歧"与 updater.rs 安装原子性）。ACP 归 S3b。
