# C7a 结论: pty/persistent-pty/shell/formatter/ripgrep/watcher 七域移植高度忠实（OpaqueReason 全集逐一对齐、pty wire 协议帧格式与认证一致、ripgrep 参数构造逐字对齐），主要风险是非持久 pty 数据路径的 per-chunk lossy UTF-8 解码损坏多字节输出，以及 persistent-pty 协议超集共用版本号 7 的互操作隐患。

## 对比表

| 域 | ref | rust | 对齐度 |
|---|---|---|---|
| pty CRUD/attach | pty.ts(321L)：sessions map、BUFFER_LIMIT 2MiB、EXITED_LIMIT 25、attach cursor(-1/省略/绝对)、activate 前排队、事件 created/updated/exited/deleted | oc-core/pty.rs(1258L) | 高；UTF-16 游标/trim 字符边界/panic 隔离均按 ref 语义补齐(pty.rs:195-232,244-293) |
| pty 后端 | pty/pty.{node,bun,workerd}.ts（@lydell/node-pty/bun-pty/不可用） | environment/pty::LocalPtySpawner（portable-pty）+ PtySpawner seam | 架构等价；win32 ConPTY/CTRL+C 处理 ref 在 bun 后端(pty.bun.ts:7-20)，rust 无对应（win32 姿态自认 TODO） |
| pty 协议帧 | pty/protocol.ts：出站裸 UTF-8 + 0x00+JSON 元帧、REPLAY_CHUNK 64KiB、入站 invalid UTF-8 丢弃 | server 侧 websocket 适配（server 面已审过） | 本 lane 核对服务面，帧适配在 oc-server |
| connect-token | pty/ticket.ts：60s TTL、容量 10000、uuid、consume=invalidateWhen 精确三元组匹配 | server tickets.rs（routes 面已生成对齐） | 服务语义在 oc-server/persistent_pty 票据路径对齐（ticket 单次消费 daemon.rs:508-556） |
| persistent-pty 架构 | ref 同样是**外部 opencode-pty 二进制**（daemon.ts:228-244 spawn detached；binary 由 `#persistent-pty-binary` 解析）——rust oc-pty 独立 bin 非新增架构 | oc-pty bin(1020L) + oc-core 状态机(2485L) + transport(2117L) + wire(1056L)，另有 OC_PTY_DAEMON=0 in-process 后端 | 对齐良好；rust 把 ref 的外部产物状态机补成了第一方源码 |
| 帧格式/认证 | u32BE 长度前缀 JSON、MaxFrameBytes 8MiB、每帧 `{token, request}` 信封、二进制输出帧 `[0x00][start u64BE][end u64BE][data]`≥17B、ProtocolVersion 7 | wire.rs:36-157 逐字节同构；PTY_PROTOCOL_VERSION=7；per-frame token 校验(daemon.rs:385-395) | 一致 |
| 请求梯子 | daemon.ts:296-333 connect/discover/ping-echo/claim/start 双试/fresh-dir 重试/registration 重试/shutdown 5s drain | transport.rs:829-1016 同梯子（含 5s 超时、50ms 轮询、SIGTERM 自产子进程） | 一致 |
| handoff/snapshot | prepare_handoff→{ticket,expires_at}、adoption ladder（过期→discover→protocol→instance→claim）、migrate inode-preserving、失败告警弃置 | persistent_pty.rs:1220-1467 + transport.rs:1284-1453 双臂（arm A 活 daemon 票据 claim；arm B 死 daemon state.json restore——rust 扩展） | 对齐+扩展（见 D-4/D-5） |
| 孤儿处理 | ref daemon 外部产物无源码 | orphan-grace：连续 0 terminal 且无 owner → 60s 自退（args.rs:10, daemon.rs:241-253）；owner 断连回 OWNERLESS；shutdown DRAIN 先冲刷 Ok 帧 | rust 自有设计（无 ref 源可比） |
| shell 服务 | shell.ts(455L)：create(before hook)/list(仅 running)/get/wait/result/timeout/output(cursor+limit 65536)/remove、EXITED_LIMIT 25、7 天留存每小时清扫、timeout→TERM+3s 强杀、事件 created/exited/deleted | shell.rs(905L) 全套对齐；捕获文件名正则精确化(G-277②)、stdout/stderr 并发泵（防 64KiB 管道死锁）、进程组信号带 leader 存活防护(826-873) | 高 |
| shell select | select.ts:13-23 META 表、/etc/shells、$SHELL 校验、darwin zsh→bash→/bin/sh、args(cmd/-c/PS 参数)、login 集 {bash,dash,fish,ksh,sh,zsh} | shell/select.rs（oc-pty default_program 走 `select::environment()` daemon.rs:425-433，含 $SHELL 校验注释 G-277③）；pty.rs is_login_shell 集合与 ref login:true 集合一致(pty.rs:861-864) | 高（win32 gitbash/PS 分支为平台差异） |
| OpaqueReason | scan.ts:3-12 九值：command-substitution/compound-command/dynamic-command-name/dynamic-execution/heredoc/invalid-redirect/invalid-structure/unterminated-escape/unterminated-quote | scan.rs:80-107 九值 kebab-case 逐串一致；全文件 reason 产生点按 grep 全量对照（两类共 200+ 处均落在九值内） | 一致 |
| shell 解析器 | parse.ts=web-tree-sitter wasm + ARITY 表(100+ 命令) + CWD 集 + save 前缀；scan.ts=手写双方言扫描器 | parse.rs=**原生手写解析器**（ARITY 表对齐 parse.rs:49 起，scan/scan_portable/scan_legacy 三入口）；scan.rs=ref 手写扫描器直接移植(5366L) | 语义移植；tree-sitter→原生是实现差异 |
| formatter | builtins.ts 26 个内建（gofmt…dfmt，含 prettier/oxfmt/biome 依赖探测、ruff 三段探测、air --help 首行校验、uv format --help、pint vendor 路径）+ State set/remove + $FILE 替换 + 首个 exit 0 即真 | formatter.rs(1760L)：26 个名字与顺序逐一对齐（测试 builtin_table_has_the_exact_26_formatters_in_ref_order formatter.rs:1090） | 高 |
| ripgrep 服务 | ripgrep.ts：run(limit+1 截断、exit 0/1/2、exit2+regex parse error→InvalidPattern)、glob/find/grep 参数、normalizePath、MAX_SUBMATCHES 100、text 2000 clamp | tool/grep.rs:120-298 逐参数对齐（--glob=!**/.* 仅 ！hidden、--glob=!**/.git/**、--fixed-strings、--ignore-case、-- 分隔）；30s 超时+timeout_message；clamp 用 UTF-16 单位(G-282⑤)；exit2 非正则错误容忍部分结果 | 高 |
| rg 二进制获取 | binary.ts：系统 rg→global bin→下载 15.1.0 GitHub release（7 平台表） | ripgrep_runner.rs：平台表/URL/文件名逐项一致 + OPENCODE_RG_PATH/bundled/DISABLE 开关超集 | 一致+超集 |
| filesystem 读写列举 | filesystem.ts：read realpath 双重围栏、list 排序（目录先+locale）、write 可越界 | server handlers/fs.rs：contains 双围栏(fs.rs:132-145)、list/find 排序同规则 | 高 |
| protected | protected.ts：DARWIN_HOME/LIBRARY/ROOT、WIN32_HOME、linux 空表 | filesystem/protected.rs：三表逐项一致、平台门控一致、is_home lexical resolve | 一致 |
| watcher | watcher.ts：RcMap 结构键共享、file/entries(fs.watch)/directory(parcel+ignore)、disabled→空流 | filesystem/watcher.rs：引用计数共享 watch、File/Entries/Directory 三型(config/watch.rs:52-59)、disabled 立即 EOF | 高（后端 notify vs parcel，见 D-7） |
| location-watcher | location-watcher.ts：git `<gitdir>/HEAD`/hg `<store>/branch` 单文件 watch、aliases×ignore 过滤、policy observe reconcile | location_watcher.rs：resolve_target/policy/reconcile 逐段对齐，另触 activity | 高（+25ms 合并窗 D-6） |
| fs/find 搜索 | search.ts：fff 原生后端(VCS 且非 home) / ripgrep 索引 + fuzzysort.go、10s 刷新、home 排除 NAME/** | handlers/fs.rs:280-475：自建内存索引(10s)、VCS 根无预算(G-31)、fuzzy 贪心近似评分（自认 deviation）、**无 fff** | 中（见 D-8） |

## 行为差异清单

- **D-1 pty 输出缓冲单位**：ref BUFFER_LIMIT 以 JS string（UTF-16 单位）计 2MiB；rust 按字节 2MiB（pty.rs:276）。CJK 场景 rust 窗口约为 ref 一半。方向=更保守，无正确性问题。
- **D-2 pty.created/added 事件时序**：ref 在 `create` effect 内同步 publish 后返回（pty.ts:243）；rust `tokio::spawn` 异步 publish（pty.rs:675-685）。订阅者可能晚于 create 返回收到事件。
- **D-3 pty.exited exitCode 负值归零**：rust `u64::try_from(code).unwrap_or(0)`（pty.rs:661）；ref 透传 number（undefined 省略字段）。信号死亡显示为 0 而非缺失。
- **D-4 persistent-pty wire 超集（同版本号 7）**：rust 增加 `read`/`restore` op、`Attached.replay_chunked/replay_length`、`ReplayComplete`、`OwnerFrame{bus_event,owner_superseded}`、`service.boot` sidecar、chunked replay（>1MiB 走二进制帧重组，wire.rs:24-40）。ref 协议 v7 无这些面。同栈内自洽；跨二进制混用见 R-3。
- **D-5 adoption arm B**：ref ladder 只处理活 daemon（daemon 死=弃置重来，daemon.ts:403-428）；rust 增加死 daemon 的 `state.json` restore 臂（transport.rs:1380-1421）+ `OC_PTY_DAEMON=0` in-process 后端。恢复能力超出 ref，属增强分叉。
- **D-6 location-watcher 事件合并**：rust pump 对同一文件的连续更新做 25ms 合并窗、仅 publish 最后一类（location_watcher.rs:244-267，G-218）；ref 逐事件 publish（location-watcher.ts:69-70）。事件粒度变粗（有意的后端差异补偿）。
- **D-7 watcher 后端与 ignore**：ref @parcel/watcher + 调用方传入 ignore；rust notify + 固定 `DIRECTORY_IGNORE=[node_modules,.git,**/{node_modules,.git}/**]`（config/watch.rs:24）。ignore 语义相同方向（排除更多不会更少）。
- **D-8 fs/find 后端**：ref VCS 非 home 位置启用 fff 原生索引库 + fuzzysort.go 精确排序；rust 无 fff，索引自走目录树 + 贪心子序列近似评分（handlers/fs.rs:518-521 自注 deviation），并额外排除 `node_modules`（G-324 裁定分叉）、不再排除 `target`（G-278②）。排序结果与 ref 不保证一致；方向=结果集 ⊆ 语义等价集。
- **D-9 shell cwd 边界**：ref `input.cwd ?? location.directory`；rust 服务层拒绝空 cwd、由 HTTP 调用方解析默认（shell.rs:673-682，G-304②）。
- **D-10 rg 解析顺序超集**：rust 前置 `OPENCODE_RG_PATH`（lstat+uid 属主校验）与 exe-sibling、`OPENCODE_DISABLE_RG_DOWNLOAD` 开关；ref 仅 系统 rg→global bin→下载。方向安全。
- **D-11 清扫触发**：ref shell 清扫为 scoped fiber 每小时 + 进程退出即停；rust `spawn_cleanup_task` 首拍立即执行（shell.rs:97-111），间隔可配。行为轻微前置。

## 正确性风险

- **R-1（高）非持久 pty 读取路径 per-chunk lossy UTF-8**：pty.rs:623-638 reader 线程对每个 ≤8192B chunk 独立 `String::from_utf8_lossy`。多字节 UTF-8 序列跨 read 边界被替换为 U+FFFD 且**破坏后续该字符的字节**——输出含 CJK/emoji 的命令（git log、ls 中文文件名）时 replay 缓冲与订阅者收到永久损坏的文本。ref 由 node-pty 在解码层处理（不可拆代理对）。对比：persistent_pty.rs 全程字节缓冲（on_output:1516-1549），无此问题；修复方向=pty.rs 改字节缓冲或增量 UTF-8 解码器（已有 G-243① 边界回退先例可循）。
- **R-2（中）服务无 teardown**：ref pty.ts:128-134 与 shell.ts:141-151 都有 Layer finalizer（kill 全部 running 子进程、interrupt waiters）；rust `PtyService`/`Shell` 无 Drop 或显式 shutdown 钩子。服务器优雅停机时 running pty/shell 子进程不被显式 kill（pty master fd 随进程关闭会 HUP 子进程，实际可收敛，但 shell 捕获文件与 waiter 状态无终态化）。
- **R-3（中）协议版本号复用的互操作风险**：rust daemon 以 protocol 7（= ref 版本号）宣布，但 wire 面是超集。若 `OPENCODE_PTY_BIN` 指向 ref 的 opencode-pty 二进制（或反之），ref 客户端严格 union 会把 rust 的 `replay_complete`/`bus_event` 帧判为协议错误；rust 客户端对 ref daemon 的 `attached` 兼容。建议版本 bump 或能力协商位。
- **R-4（低）shell 组信号 pid 回收窗口**：signal_process_group（shell.rs:836-873）以 `/proc/<leader>` 存在为前置、逐 pid 校验 pgrp 后 kill；leader 在扫描与投递间死亡时组 id 可能回收——现有 live-leader 前置 + 逐成员复检已大幅收窄，残余窗口为 TOCTOU 性质，可接受。
- **R-5（低）daemon `own` 的 ticket 过期判定**：daemon 侧校验 `record.expires_at > unix_ms_now()`（daemon.rs:508-511），transport 侧 adopt 入口也先做过期检查（transport.rs:1286）——双侧判定用的是各自的时钟读取，正常无碍；仅时钟跳变时可出现 client 已判死、daemon 仍接受的单边窗口，后果限于一次多余 claim。
- **R-6（低）`prepare_handoff` 持锁做文件 I/O**：daemon.rs:585-600 在持有 DaemonState mutex 时执行 `machine.handoff()` 的同步原子写（state.json）+ 全量终端快照（base64 缓冲）。终端数多时其他 op（含 pump 的 bus_event 转发）在此期间停摆。代码内已有 TODO 自认；量级当前可接受。
- **R-7（低）persistent-pty restore 后的 `output` 窗口计数**：restore 时 `info.output = (buffer_cursor, buffer_cursor + buffer.len())`（persistent_pty.rs:1482），以字节计；`on_output` 同样按字节累加（:1519）——内部自洽。但 wire `WireTerminal.output_head/tail` 的语义在 ref 侧由外部 daemon 定义，若 ref 以别的单位计数，跨栈 attach 的 offset 请求会错位。同栈无影响。
- **R-8（低）`pty.exited` 的 exitCode 缺省**：ref 在 `exit === undefined` 时省略字段（pty.ts:233）；rust `Exited` schema 恒序列化 `exitCode`（pty.rs:659-663 用 `unwrap_or(0)`）。事件消费者在"被信号杀死"场景会看到 0 而非缺失。与 D-3 同源。

## 覆盖声明

**逐行读完（双侧）**：ref `pty.ts`、`pty/{protocol,pty,pty.node,pty.bun,ticket}.ts`、`persistent-pty/{index,daemon}.ts`、`shell.ts`、`shell/{select,result}.ts`、`formatter.ts`、`formatter/builtins.ts`、`ripgrep.ts`、`ripgrep/binary.ts`、`filesystem.ts`、`filesystem/{protected,watcher,location-watcher,search}.ts`；rust `oc-core/src/pty.rs`、`persistent_pty.rs`（1-2485，测试段 2021 起略读）、`persistent_pty/{wire,transport}.rs`（transport 1100-1800 详读、尾部辅助略读）、`oc-pty/src/{daemon,args}.rs`、`shell.rs`、`shell/parse.rs`（结构+ARITY 核对）、`tool/{grep,glob,ripgrep_runner}.rs`、`formatter.rs`（表+顺序核对）、`filesystem/{protected,watcher,location_watcher}.rs`、`server/handlers/fs.rs`（find/read 路径详读）。

**方法性核对（非逐行但全量枚举）**：`shell/scan.rs`（5366L）：OpaqueReason 九值逐串对照 + 全文件 200+ 处 `OpaqueReason::*` 产生点 grep 枚举确认均落在九值内、与 ref scan.ts 同类触发条件分布一致；未逐行验证每个触发条件的边界等价性（有 755L 测试文件 shell/tests.rs 存在）；`persistent_pty.rs` 2486 行起、`transport.rs` 1800 行起、`ripgrep_runner.rs` 260 行起（下载/解压细节）、`formatter.rs` 各 formatter 的 enabled 探测分支体：略读或按测试锚定。

**明确未覆盖**：`persistent_pty/frontend.rs`（583L，handlers 的 op 面）——未读；rust 独有 `read` 扩展 op 建议补审；ref 平台绑定产物（#persistent-pty-binary/#fff/#shell-parser-wasm、workerd 桩）无 rust 对应物需比对；oc-server 的 pty/persistent-pty/shell HTTP+websocket 端点属 server lane；shell 工具插件 timeout/background 元数据非本 lane 范围。

**结论要点**：七域移植质量高于平均水平——最难的 persistent-pty 协议梯子、双臂 adoption、wire 逐字节对齐均忠实；ripgrep 参数构造与 OpaqueReason 全集无一遗漏。需要动手的只有 R-1（pty.rs lossy UTF-8）与 R-3（协议超集版本协商），其余为已知且文档化的故意分叉。
