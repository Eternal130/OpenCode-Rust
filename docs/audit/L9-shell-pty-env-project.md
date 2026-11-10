# L9 Shell / PTY / 环境 / 项目 — 审查报告

> 审查方式：两侧逐文件逐行阅读，结论均以代码为准（不信任文档）。证据格式 file:line。
> 参照：/opt/opencode-ref (v2.0.24 TS) · 移植：/opt/opencode-rust

### 1. 覆盖证明

实际逐行阅读的文件（两侧）：
- **Shell**: ref `shell.ts`(455L)、`shell/select.ts`(227L)、`shell/parse.ts`(402L)、`shell/result.ts`(81L)、`shell/scan.ts`(2409L，全文结构+全部函数签名+关键实现走读) ↔ rust `shell.rs`(878L)、`shell/select.rs`(381L)、`shell/parse.rs`(634L)、`shell/result.rs`(241L)、`shell/scan.rs`(5366L，结构+函数级映射核验，TS 每个函数均有同名 Rust 对应：`finish_bash_word`/`scan_bash_keyword`/`bash_ansi_quote`/`scan_powershell` 等 60+ 项)。
- **PTY**: ref `pty.ts`(321L)、`pty/ticket.ts`(56L) ↔ rust `pty.rs`(1270L 全文)、`oc-server/src/tickets.rs`。
- **Persistent-PTY**: ref `persistent-pty/index.ts`(419L)、`daemon.ts`(690L，协议/handoff/ladder 关键段全文) ↔ rust `persistent_pty.rs`(3173L 全文)、`persistent_pty/{frontend,transport,wire}.rs`(结构+pub 面)、`oc-pty/{lib,main,args,daemon}.rs`。
- **Environment**: ref `environment/{index,environment,driver,unavailable,files,local,exec-defaults,memory}.ts`(全 632L) ↔ rust `environment/{mod,driver,local,files,exec_defaults,memory,pty,stdio,workspace,unavailable}.rs`。
- **Project/Workspace**: ref `project.ts`(390L)、`workspace.ts`(336L) ↔ rust `project.rs`(2065L 全文)、`environment/workspace.rs`(256L)。
- **Instance/App/Global**: ref `instance.ts`(181L)、`app.ts`、`instance/service.ts`、`util/global.ts`+`global-roots.ts` ↔ rust `oc-util/global_roots.rs`、`engine/mod.rs`（装配面）。
- **Integration**: ref `integration.ts`(834L 全文)、`integration/connection.ts` ↔ rust `integration/{mod,types,flows,credential,providers,providers_enterprise,copilot,vertex,console,jwks,wellknown_config}.rs`（mod.rs 1507L 全文+子模块 pub 面核验）。
- **WebSearch**: ref `websearch.ts`(头部100L) ↔ rust `tool/websearch.rs`+`websearch_providers.rs`（存在性+结构）。

注：审查域清单中"environment 检测（tmux/ssh/container/CI/wsl）"在 v2.0.24 参照版 core 内**不存在**（grep 全 core 无 TMUX/WSL 标志检测；v2 的 environment/ 是执行平面抽象），该子项按"参照版无此项"处理。

### 2. 逐项对比表

**Shell 服务（shell.ts ↔ shell.rs）**

| 功能点 | Ref (file:line) | Rust (file:line) | 状态 | 差异说明 |
|---|---|---|---|---|
| create 流程/hook 前置 | shell.ts:254-275 | shell.rs:557-587 | ✅ | hooks.trigger 在 before 之前，env 注入 TERM/OPENCODE_TERMINAL 一致 |
| 会话环境继承 | shell.ts:258-272 | shell.rs:564-568 | ✅ | session env ?? process.env；Rust `env_clear+envs` 与展开 process.env 等价（但丢弃非 UTF-8 环境变量，unix 影响极小）⚠️低 |
| cwd 默认 | shell.ts:265 `input.cwd ?? location.directory` | shell.rs:571 `unwrap_or_default()`(空→进程 cwd) | ⚠️ | 服务层默认不同；tool/shell.rs:317-320,670 在调用层填 location 目录，实际路径已兜底 |
| spawn 细节 | shell.ts:299-307 (detached, stdin ignore, forceKillAfter 3s) | shell.rs:655-670 (process_group(0), stdin null, kill_on_drop(false)) | ✅ | unix 进程组=detached 语义；win ref 不 detached，rust win 用 start_kill |
| 输出捕获（合并流+文件+行数） | shell.ts:323-354 | shell.rs:699-756 | ✅ | 双管道并发 pump（v2 是合并 `handle.all`；顺序差已注释论证）；行数流式统计一致 |
| 退出观察 | shell.ts:410-422 exitCode 流+KilledBySignal→signal | shell.rs:764-793 try_wait 20ms 轮询+signal_name | ✅ | 轮询替代事件流，语义等价 |
| 超时（重置/清除/TERM→3s→KILL） | shell.ts:390-406 | shell.rs:334-352, 489-518 | ✅ | escalation 为 detached 任务，即使 finish future 取消也触发 |
| 进程组 kill | (cross-spawn handle.kill) | shell.rs:799-846 signal_process_group（/proc 扫描+leader 存活校验） | 🔵 | 加固实现：防 pgrp 复用误杀 |
| EXITED_LIMIT=25 保留/LRU | shell.ts:32,377-382 | shell.rs:35,536-550 | ✅ | |
| output 分页/utf8 | shell.ts:198-226 | shell.rs:355-387 | ✅ | ref `toString("utf8")` ↔ `from_utf8_lossy`，等价度可接受 |
| result 截断通知 | shell.ts:228-252 | shell.rs:391-453 | ✅ | `total-shown+1` 公式、`(no output)`、file 路径一致 |
| 7 天保留清理（每小时） | shell.ts:83-117 | shell.rs:34-110 | ✅ | 文件名 regex `sh_[0-9a-f]{12}.*\.out` 逐字符复刻 |
| wait 移除时 NotFound | shell.ts:166-167 | shell.rs:277 (done.send(None)) | ✅ | |
| 事件 created/exited/deleted | shell.ts:424,372,169 | shell.rs:794,535,279 | ✅ | |

**ShellSelect / Parse / Scan**

| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| META 表（deny/login/ps） | select.ts:13-23 | select.rs:11-20 | ✅ | |
| /etc/shells 列表 | select.ts:110-114 | select.rs:197-214 | ✅ | ref Set 去重 ↔ dedup（相邻去重，非全局；重复项不相邻时残留，影响≈0） |
| win32 发现链 pwsh→powershell→gitbash→COMSPEC | select.ts:95-108 | select.rs:187-194,245-261 | 🟡 | `win_list` 未含 gitbash；`full()` 的 bash→gitbash 重定向已实现 (select.rs:145-150) |
| gitbash 推导 `<git>/../../bin/bash.exe`+size>0 | select.ts:125-132 | select.rs:89-98,170-185 | ✅ | |
| resolve（config→SHELL→fallback，compat 过滤） | select.ts:179-193 | select.rs:278-292 | ✅ | 丢弃 ref 的 per-bin 缓存（行为等价，性能差） |
| args（cmd/`ps -NoLogo…`/`-c`） | select.ts:164-169 | select.rs:296-311 | ✅ | |
| ARITY 表（161 项）+prefix | parse.ts:23-161,374-380 | parse.rs:43-86 | ✅ | 逐项核对一致 |
| legacy tree-sitter (bash/powershell) | parse.ts:174-206 | parse.rs:516-634 | 🟡 | bash 分支 keep 列表漏 `command_name_expr`；PS command_elements 内所有子节点一律 Word，丢失 `command_parameter` 类型 → PS legacy 路径 `-Path` 检测弱化 |
| portable 扫描（声明跳过/foreach 块/redirect 计数/save 边界） | parse.ts:208-287 | parse.rs:103-249 | ✅ | 含 G-190 backtick 续行 separator 复刻 |
| directoryArgs（--、-Path 正则、$env:/`$HOME`/`$PWD`/`$PSHOME`） | parse.ts:305-372 | parse.rs:361-509 | ✅ | POWERSHELL_PATH_FLAG_RE 用枚举表实现等价 |
| scan.ts 便携扫描器（bash/zsh/posix + PowerShell，Opaque 9 类原因） | scan.ts 全文 | scan.rs 全文（函数级 1:1） | ✅ | 5366L 对 2409L，逐函数对应 |
| MAX_INPUT=64K/DEPTH=32 预算 | scan.ts:89-90 | scan.rs:304 spend() | ✅ | |

**PTY（非持久）**

| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| create（默认 shell resolve config、login `-l`、TERM、win32 LC_ALL） | pty.ts:166-184 | pty.rs:534-586 | ✅ | is_login_shell 集合与 META.login 一致（含 fish/dash） |
| 2MiB 缓冲窗口+绝对 cursor | pty.ts:12,205-222 | pty.rs:34,243-292 | ✅ | UTF-16 计数复刻 JS `string.length`（G-116①），trim 回退字符边界 |
| attach（-1 尾部/省略全量/钳制；pending 激活回放；detach） | pty.ts:260-311 | pty.rs:781-847 | ✅ | |
| exited 保留 25 / remove / update(title+resize) | pty.ts:15,142-158,247-253 | pty.rs:36,689-709,729-767 | ✅ | |
| 订阅者 panic 隔离 | pty.ts:210-216 try/catch | pty.rs:254-273 catch_unwind | ✅ | |
| create 默认尺寸 | pty.ts:184（spawn 未传尺寸，node-pty 默认） | pty.rs:576 `(80,24)` | ⚠️ | node-pty 默认行列与 (80,24) 可能不同；实际影响限于未传 size 的首个空白终端 |
| PtyTicket（60s TTL、容量 10k、scope 匹配消费） | pty/ticket.ts:9-52 | oc-server/src/tickets.rs | ✅ | |

**Persistent PTY**

| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| 服务面 list/get/create/write/resize/control/input/snapshot/read/terminate/shutdown/handoff/attach | index.ts:142-349 | persistent_pty.rs:815-1354 + frontend.rs:182-540 | ✅ | 全 op 覆盖；id `pty_persistent_<n>`、cwd 默认 `/`、80x24 默认一致 |
| daemon 进程外托管（跨重启存活） | daemon.ts（外部 opencode-pty 二进制） | oc-pty crate（main/args/daemon.rs 1019L）+ transport(2118L)+wire(1056L) | ✅ | 默认走 daemon（frontend.rs:51 `OC_PTY_DAEMON`，=0 为进程内逃生口）；orphan-grace 自退、owner 连接、bus_event 帧 |
| 协议版本=7、service.json/lock 注册 | daemon.ts:8,33-39 | persistent_pty.rs:509,1733-1751 | ✅ | |
| prepare_handoff 票据+30s TTL、sidecar/state.json | daemon.ts:356-380 | persistent_pty.rs:1216-1254,513 | ✅ | 进程内模式以 state.json 快照替代活 daemon（有注释论证） |
| 接替梯（过期→不可达→协议→实例→锁→票据→快照） | daemon.ts:403-428 | persistent_pty.rs:1388-1463 | ✅ | 失败统一告警文案一致；inode 保持迁移 rename 一致 |
| restore：恢复 replay 窗口、子进程死亡=exited | daemon.ts（daemon 死亡语义） | persistent_pty.rs:1469-1510 | ✅ | 进程内模式子进程必死；daemon 模式下终端真存活（parity 靠 oc-pty） |
| attach replay{requested/available/end/truncated}+generation/takeover+ControllerChanged | index.ts:326-349 | persistent_pty.rs:1281-1354 | ✅ | §12Q3 ControllerChanged 为文档化新增（🔵） |
| current[session] 控制器选择/失效清理 | index.ts:140,209-249,268-274 | persistent_pty.rs:1059-1091,1134-1155 | ✅ | |
| read lines 1..=65535 校验 | index.ts:265-267 | persistent_pty.rs:1127-1133 | ✅ | |
| removeVisibleExit（仅可见终端退出自动移除） | index.ts:311-324 | persistent_pty.rs:986-1002,1628-1657 | ✅ | |
| snapshot/checkpoint（§7 v1 二进制格式+CRC32） | （外部二进制产物） | persistent_pty.rs:1100-1116 + wire::encode_checkpoint | 🔵 | 版本化二进制格式为移植侧新增的确定性编码（对齐 daemon 字节） |
| OSC 0/2 标题、前台进程探测 | daemon.ts:107-117 | persistent_pty.rs:1875-2018（TIOCGPGRP+/proc）, 939-975（500ms 探测） | ✅ | unix-only；win32 TODO（与整体 win32 姿态一致） |

**Environment**

| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| per-location 绑定（workspace→driver / local） | environment.ts:20-30 | environment/mod.rs:68-107 | ✅ | orDie→Err(String) 语义映射 |
| Files 契约（read/stat/list/move/mkdir/remove+WrongKind/NotFound） | files.ts:33-75, local.ts:28-110 | files.rs + local.rs:155-257 | ✅ | lstat 语义、symlink 报告、move 入目录拼接、rm force 均一致 |
| typeFollowing | files.ts:64-75 | files.rs `type_following` | ✅ | |
| exec-defaults sh 脚本（exit 44/45/46、LC_ALL=C、dd range、find -printf） | exec-defaults.ts:17-160 | exec_defaults.rs:17-80+ | ✅ | 脚本逐字复刻；64MiB/64KiB 上限一致 |
| HostSpawner（并发 drain+stdin 并写+cap kill） | util collectStream | local.rs:33-134 | ✅ | cap 触发即 kill 防管道死锁 |
| memory driver | memory.ts | memory.rs(387L) | ✅ | |
| unavailable spawner | unavailable.ts | unavailable.rs | ✅ | |
| cross-spawn shim（win .cmd/PATHEXT/npm_execpath） | util/cross-spawn-spawner | local.rs:20-28 | 🔴 | 文档化不移植（仅影响 win32） |

**Project / Workspace / Instance / Global**

| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| root（.git/.hg 向上、仅文件系统） | project.ts:49-58 | project.rs:1221-1232 | ✅ | |
| git 身份 remote→cached→rootCommit | project.ts:265-299,337-364 | project.rs:1283-1322,1489-1560 | ✅ | URL/scp 归一化、SHA1 `Hash.fast`（DB 兼容）、根提交排序一致 |
| canonical=主 worktree（porcelain 首块） | project.ts:350-356 | project.rs:1296-1313,1454-1480 | ✅ | G-232 修正为 porcelain 首块 |
| hg 发现（cached→`hg log roots`） | project.ts:303-335 | project.rs:1255-1271,1564-1580 | ✅ | HGPLAIN=1 一致 |
| 无 VCS 目录身份 hash | project.ts:374-379 | project.rs:1272 | ✅ | |
| upsert（冲突仅刷 vcs）/canonical 消失才替换 | project.ts:108-130 | project.rs:559-594,596-613,1324-1342 | ✅ | |
| worktree.resolved 事件+adoption 扫描（markerless 收养） | project.ts:132-201 | project.rs:1346-1389,1402-1418 | ⚠️ | ref 将行插入与事件**原子提交**（commit 回调）；Rust 先插行后发事件——崩溃窗口内事件永久丢失（下次 resolve 见已有行跳过发布）。低概率、可观察到 |
| activate 60s 限频、不动 time_updated | project.ts:241-255 | project.rs:396-421 | ✅ | |
| update（None 保留/空串清空） | project.ts:215-239 | project.rs:428-491 | ✅ | |
| worktree 策略注册表+git 策略 | worktree/{strategies,git}.ts | project.rs:911-1144 | ✅ | `--detach`、common-dir remove、dirty→force_required |
| workspace create/provision/connect/destroy、CreateConflict、null-binding destroy | workspace.ts:217-323 | environment/workspace.rs:157-255 | ✅ | |
| workspace 共享 provision attempt/KeyedMutex/连接池/空闲挂起 janitor(20min/1min) | workspace.ts:85-215,254-290 | workspace.rs:3-10（缺失清单自述） | 🔴 | 见缺失汇总#1 |
| instance 服务图（49 节点 location graph、vanilla discovery 开关） | instance.ts:52-160 | engine/mod.rs（装配） | 🟡 | 图式装配存在；逐节点 DI 等价性未在本域全量核验（AI/工具节点属其他域） |
| Global 路径（XDG data/cache/config/state、bin=cache/bin、log=data/log、repos、tmp、OPENCODE_CONFIG_DIR） | util/global.ts:12-58, global-roots.ts | oc-util/global_roots.rs:100,112-121,166 | ✅ | 含 snapshot 目录（🔵新增）；目录创建集一致 |

**Integration**

| 功能点 | Ref | Rust | 状态 | 差异说明 |
|---|---|---|---|---|
| 注册表 editor（update/remove/method.update/same-slot 替换） | integration.ts:293-356 | integration/mod.rs:263-347 | ✅ | |
| 10min attempt/1min 终态保留/30s scrub 双循环 | integration.ts:237-239,509-556 | mod.rs:43-47,1250-1300 | ✅ | |
| oauth connect/status/complete/cancel（completing/persisting 门闩、CodeRequired、scope 关闭） | integration.ts:558-608,752-795 | mod.rs:681-869 | ✅ | auto 模式 connect 时即 completing；G-115③ 取消中止在途交换（🔵加固） |
| command connect（spawn+stderr 逐块入 message+exitCode 判定） | integration.ts:610-674 | mod.rs:953-1133 | ✅ | |
| 凭据标签自动编号（name、name 2…） | integration.ts:358-367 | mod.rs:635-680 | ✅ | |
| 连接投影（credential 逆序+env 名存在）+运行时 status | integration.ts:369-395,744-750 | mod.rs:379-410,611-633 | ✅ | |
| resolve/refresh（5min 边距刷新并回写） | integration.ts:696-714 | mod.rs:499-547(+force_refresh 🔵) | ✅ | |
| key 连接（表单校验、无表单拒答案、configuration） | integration.ts:715-738 | mod.rs:571-610 | ✅ | |
| integration.updated 事件 | integration.ts:355 | mod.rs:351-355 | ✅ | |
| 内建 provider 族（catalog key/env + OAuth 提供方 + copilot/vertex/jwks/console） | （各插件包注册） | integration/providers*.rs、copilot.rs 等 | ✅ | 移植侧内联；含 enterprise 子集 |

### 3. 关键差异与风险 Top 5

1. **workspace 生命周期缺失（🔴）**：ref workspace.ts:85-215 的共享 provision attempt、KeyedMutex、连接池、20min 空闲挂起 janitor、destroy-vs-provision 竞态结算全部未移植（workspace.rs:3-10 自述"待非本地 driver"）。当前只有 local provider 时无行为差异，但任何远程/沙箱 provider 接入即出现重复 create、无法挂起、destroy 竞态泄露。
2. **project 事件原子性（⚠️）**：ref 的 `worktree.resolved` 行插入与事件 commit 原子（project.ts:190-199）；Rust project.rs:1354-1388 先插行后发事件，崩溃窗口后事件永不补发（stored 行短路）→ session 项目归属投影可能落后。
3. **延迟 SIGKILL 升级的 pid 复用窗口（⚠️低概率）**：shell.rs:506-512 在 TERM 后 3s 无条件对原 pgid 发 SIGKILL；虽然 signal_process_group（:809-845）有 leader 存活校验，但若子进程已被 reap 且 pid 在 3s 内被复用为新 leader，/proc 校验无法区分（ref 经由受管 handle kill，无此窗口）。建议 escalate 前核对子进程尚未退出。
4. **PS legacy 解析器类型丢失（🟡）**：parse.rs:585-634 在 tree-sitter legacy 路径将 command_elements 子节点一律记为 Word，丢失 `command_parameter` 类型 → PowerShell legacy 扫描中 `cd -Path x` 的目录提取退化（select 目录参数仍过滤 `-` 前缀，兜底可用）。portable 路径不受影响。
5. **服务层 cwd/env 默认与 select win 差异（🟡）**：① shell.rs:571 cwd 默认空（进程 cwd）≠ ref location.directory（调用层已兜底，但服务 API 语义漂移）；② 非 UTF-8 环境变量在 `std::env::vars()` 处静默丢弃；③ select.rs win 列表缺 gitbash、ref resolve 的 per-bin 缓存省略；④ pty 默认尺寸 (80,24) 与 node-pty 隐式默认可能不同。均为低危组合差异。

### 4. 缺失文件/功能汇总

| # | 缺失项 | Ref 证据 | 状态 |
|---|---|---|---|
| 1 | workspace 空闲挂起 janitor / 连接池 / 共享 provision / destroy 竞态结算 | workspace.ts:85-215,254-290,291-301 | 🔴（grep 全 crates 确认无 idle/suspend 调用方；driver trait 已留 `suspend_for_idle` 挂点） |
| 2 | cross-spawn win32 shim（.cmd/.bat/PATHEXT/npm_execpath） | util/cross-spawn-spawner.ts | 🔴 文档化不移植（win32 姿态） |
| 3 | persistent-pty 前台进程探测 win32 路径 | （外部二进制） | 🟡 unix-only |
| 4 | shell/scan.ts 的 `MAX_INPUT_LENGTH` 拒绝路径差异 | scan.ts:89 | ✅（scan.rs spend 预算覆盖，语义等价） |
| 5 | websearch.ts 独立服务（KV provider 选择/cooldown/preferred per session） | websearch.ts:76-120 | 🟡 以 `tool/websearch{,_providers}.rs` 形态存在，KV/事件面未逐行核验（建议 tool 域复审） |

**总体结论**：本域 Rust 移植完成度很高——shell/scan 逐函数对齐、PTY 双轨（in-process + oc-pty daemon）与 handoff 梯完整、environment 执行平面与 exec-defaults 脚本逐字复刻、project 解析身份链与 DB 语义（含 SHA1 兼容）一致、integration host 全 op 覆盖且多处带加固性偏差。主要缺口集中在 workspace 生命周期（#1）与三处低危语义漂移（风险 2-5）。
