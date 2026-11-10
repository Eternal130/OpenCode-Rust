# C2b 结论: 会话服务域（store/inbox/projector/message-updater/fork/revert/move/transfer/subagent/title/stats/execution/restart）为高保真移植，多数差异为文档化的有界偏离（G-226 合成失败行、title 重试、usage fan-out 串行化），未发现会导致数据损坏的正确性缺陷；少量低风险口径差（files 空数组归一化、shell 失败通知 delivery、completion_text 回扫）。

## 对比表

| ref 文件(:行) | rust 对应 | 保真度 | 备注 |
|---|---|---|---|
| store.ts:53-251 | store.rs:145-542 | 高 | list/messages/claim/release/countResume 逐条对齐（含 anchor 双列分页、time_updated 钉住）；create 移自 session.ts:262-318，`SessionAlreadyProjected`→Existing 竞态同构 |
| store.ts + session.ts:262-318 | store.rs:574-736 `create_inner` | 高 | 子会话继承 location/metadata/permissions；`explicit_defaults`(G-250) 为 rust 显式化参数，ref 靠 transfer 绕过 Session.create |
| prompt.ts:29-91 prepare() | runner/prompt_prepare.rs + server handlers/attachment.rs→session/prompt.rs | 高(拆分) | hook→skill 分流顺序一致；drain 侧 backstop+PreparedRing 为 rust 新增防双发机制 |
| prompt.ts:93-231 materialize | prompt.rs:33-412 | 高 | 20MiB 双检（stat 期报 uri、解码后报 label）、目录列举排序、text/plain 行切、image normalize 回退臂全对齐 |
| inbox.ts 全 550L | inbox.rs 全 853L | 高 | promote 控制序重排（compaction 前插、move 不跨越）、admitCompaction 复用、first-admission-wins、projected 投影四函数逐条对应 |
| session.ts:319-359 fork（命令半） | fork.rs:74-153 | 高 | boundary/newest 回退、404/empty 分类、继承“最新”instruction 值+entry 快照（key ASC） |
| projector.ts:106-226 projectFork | projector.rs:678-813 | 高 | `parent_id=NULL+fork_session_id`、settled-only 过滤三条件、`{eventID}_{seq}` 消息 id、reserveSequence、500 批 |
| revert.ts:21-114 | revert.rs:107-389 | 高 | original 复用/plan first-wins/restore 合并/files=false；legacy V1 行解码为 rust 补充（ref session-revert.ts 同源） |
| move.ts 全 179L | handlers/session.rs:940-1113 + inbox.rs | 高 | ~ 展开/相对解析、孤儿直达 Moved+取消在锁内、wake；runner 装配失败探测为有界偏离 |
| transfer.ts 全 336L | transfer.rs 全 643L | 高 | isSettled/commit-hook 原子导入/reserveSequence/计数回填/sanitize 全 redact kind 逐字对齐 |
| subagent-job.ts 全 | subagent_job.rs + subagent_jobs_adapter.rs | 高 | `{child}:{startedAt}` 一代一观察者、forkIn→tokio::spawn |
| subagent-completion.ts 全 | subagent_completion.rs | 高 | 信封逐字节、resume:false/暂存 revert 抑制 wake（session.ts:310 对应）、metadata source/childID/agent/state |
| title.ts 全 157L | runner/title.rs | 高(1 处偏离) | 8k/2k/100 截断、untitled 回退标题 iso 串、primary 回退、usage-on-interrupt Drop guard（G-279③） |
| stats.ts 全 436L | handlers/session.rs:2477-2949 | 高 | SQL 形状、fork 过滤、compaction usage 事件、median/streak/day_ordinal；windows 31d 分批为 ref 内部优化未复刻（等价） |
| usage.ts 全 | runner 侧 SessionUsage::record/calculate（C2a 域） | — | 口径（safe/finit/tier 选取）由 C2a 覆盖 |
| run-coordinator.ts 全 196L | execution.rs:116-509 | 高 | doorbell 合并最宽 scope、settlement 窗口（loop_done 对应 owner===undefined）、stopping 拒 joiner、successor 先于 waiter 释放 |
| execution.ts 全 195L | execution.rs:511-1113 | 高 | claim/release commit hook 同事务、terminal 分类、interrupt-resume 的 nextPromotable steer/control 复醒 |
| execution/restart.ts 全 240L | jobs.rs:631-1037 | 高 | prepareResume 先计数、超限 terminalize、shell/subagent 恢复顺序（shell 先）、suspended 集抑制 wake、suspended.clear() 收尾 |
| projector.ts:434-788 全部 bus.project | projector.rs:90-419 | 高 | 每个事件一张表逐条对上（moved/renamed/viewed max 水位/idle max(+1) 溢出/revert.committed 双删+instruction reset/compaction.ended epoch） |
| message-updater.ts 全 487L | message_updater.rs 全 968L | 高 | 全事件分派表逐 arm 对应；G-112 files 键存在语义有专测 |
| history.ts / context.ts / provider-context.ts | runner/context.rs（C2a 域） | — | 边界 Boundary/steps 语义由 C2a 覆盖；rust inbox.rs:657 decode_message_row 即 decodeMessageRow 对应 |
| shell.ts:10-27 + session.ts:179-222 | handlers/session.rs:1700-1821 + shell/result.rs | 高 | timeout 0+background metadata、evt_ 派生 id、ended 后 userNotification resume:false |
| skill.ts / command.ts / session.ts:223-245 | handlers（skill.activated 2240 / command 路径） | 中(未逐行) | 消息通道（bus 事件投影）确认存在；command 经 PromptInput→prompt 通道同构 |
| system-prompt.ts | runner/system_prompt.rs（C2a 域） | — | ${OPENCODE_TOOL_GUIDANCE} 按工具注入 |
| environment.ts | env.rs SessionEnv | 高 | 进程内 map get/set/clear；remove_tree clear 对应 |
| error.ts / to-session-error.ts | execution.rs:89-109 + 各错误枚举 | 高 | 类型映射集中于 server 层；interrupted→aborted/"Interrupted"、defect→unknown 对应 fallback 行 |
| affinity.ts:7-8 | runner 侧（C2a，tests_usage_affinity.rs 存在） | — | parentID??fork.sessionID??id |
| instructions.ts:46-93 load | engine/instructions.rs + runner/context.rs:830 | 高 | in-flight claim/已注入 ledger（synthetic metadata）/相对路径描述 |
| instruction-state.ts / instruction-entry.ts | projector.rs:1135-1247 + engine/instructions.rs | 高 | apply/initialize/advanceEpoch/reset 与 10 行批初始化对齐 |
| info.ts / sql.ts / event.ts / message.ts / schema.ts | database/schema.rs + oc-schema | 高 | fromRow 列映射/re-export 薄层 |
| session_share 表 | migrations.rs:122 + upgrade_battery.rs | 高 | 两侧均“仅建表无写方”（ref 唯一写入在 v1-migration.bun.ts:797），无 share 服务事件 |
| —（ref 无） | state_transform.rs | 有界 | G-5：仅 agent 域、急切重建、失败隔离到 state transforms（文档化三偏离） |
| —（ref 无） | hooks.rs（会话相关半） | 对应 | plugin/hooks.ts 域：session prompt/request/context/compaction/title + ws/http hook 面 |

## 行为差异清单 D-n

- **D-1 files 空数组归一化**：ref `prompt.ts:54` 保留 `files: []`；rust `prompt.rs:404-406` 把空数组折叠为 `None`。存储的 user 消息 `files` 键出现与否不同（wire 差一档，消费方均按缺省处理）。
- **D-2 附件流水线拆分与并发**：ref 在 `prepare()` 内以 concurrency 8 并发物化附件（prompt.ts:54）；rust `materialize_files` 顺序执行且由 HTTP handler 调用（prompt.rs:399-412）。纯语义等价（失败即整体失败），仅 IO 交织与背压不同。
- **D-3 title 乐观并发重试**：ref 序列不匹配即放弃（title.ts:145-147，一次性）；rust 重试至多 20 次、50ms 退避（title.rs:306-353）。rust 在 ref 会放弃的场景可能最终改名成功。
- **D-4 G-226 合成失败回合行**：ref 失败 terminal 只发 `session.execution.failed`（无 assistant 行）；rust 对未上浮错误补发 `session.step.started/failed` 合成一个 assistant 错误行并按“当前 turn 唯一行”去重（execution.rs:897-1037）。文档化有界偏离：Rust 转录多一行，导出/interop 可见。
- **D-5 中断机制**：ref fiber interrupt（finalizer 保证运行）；rust shutdown watch + stop bell，60s `UNWIND_TIMEOUT` 后放弃 drain 并归类 Interrupted（execution.rs:421-437）。放弃时 drain future 被丢弃，极端 wedged 场景的清理可能不完整（ref 无此路径）。
- **D-6 usage fan-out 因果序**：ref 靠单流 runForEach 协作调度保证 `usage.updated` 先于 terminal；rust 新增 per-session 串行链 + `wait_usage_fanout`（projector.rs:426-496, execution.rs:821），并在 settled 前显式 await。为恢复同一可观测序的 rust 专用机制。
- **D-7 sweep 启动期守卫**：rust 增 `list_suspended_before`/`release_child_claims_before`（store.rs:398-523，RUNTIME-SWEEP-RACE）；ref 靠进程级协议（restart.ts:44-52）。注意 `jobs.rs:1003 resume_suspended_sessions` 本体仍用无守卫的 `list_suspended()`；`_before` 变体是否被 boot 实际调用未在本 lane 验证。
- **D-8 revert stage 快照短路**：rust 在 diff 前比较 `after == original` 相同则直接空列表（revert.rs:154-157）；ref 无条件 `snapshot.diff`（revert.ts:44）。语义等价（同树 diff 为空），省一次 diff。
- **D-9 消息行 time_updated 口径**：ref `$onUpdate(()=>Date.now())` 写入时墙钟；rust 钉到事件 created（projector.rs:1111-1115，G-279①）。同事务内两值近似相等，时钟回拨时不同。
- **D-10 shell 启动失败通知 delivery**：rust `Delivery::Queue`（handlers/session.rs:1764）；ref `synthetic()` 默认 `"steer"`（session.ts:296）。该通知 resume:false 不唤醒，但下轮 drain 的提升边界不同（steer 可在步边界插入，queue 只在 idle 边界）。
- **D-11 move 源不可用探测收窄**：ref 将 runner 装配任意失败视为 unavailable（move.ts:104-116）；rust 只探测“目录消失”（handlers/session.rs:1019-1033，文档化 bounded deviation）。装配失败但目录存在的会话在 rust 走 404/500 而非孤儿直达迁移。
- **D-12 slug 词表**：rust 本地 30×30 词表（store.rs:118-137，RC-09，契约测试钉住）；ref `Slug.create()` 上游大词表。ID 不透明，仅碰撞概率不同。
- **D-13 completion_text 回扫**：ref 取 desc 前 20 条中**第一条**完成且无错 assistant，其文本为空即返回 NO_TEXT（subagent-job.ts:46-51 + subagent-completion.ts:10-19）；rust 空文本时**继续向旧扫描**（subagent.rs:508-509 continue）。极少数场景 rust 会拿到更旧的回复文本。
- **D-14 stats 时区回退**：rust 非法/未知 IANA 时区回退 UTC（handlers/session.rs:2900-2901）；ref `Intl.DateTimeFormat` 构造即抛（orDie → 500）。
- **D-15 remove 有界等待**：rust 删除前 `await_idle` 限时 10s 后强制删（mod.rs:59,82）；ref 无限等待。drain wedged 时 rust 先删（配合 D-5 的 60s 放弃，删除窗口内仍可能有写入落进已删聚合）。
- **D-16 instructions 批内逐行插入**：ref `instruction-entry` 10 条一批多值 INSERT；rust 批内逐行 execute（projector.rs:1233-1244）。原子性同为外层事务覆盖，等价。

## 正确性风险 R-n

- **R-1（低，D-4）**：G-226 合成 assistant 行使 Rust 导出的会话在 ref 侧导入/重放时多一条消息；若 ref 端有按消息计数或 golden 对比的下游会失配。属已注册偏离，建议在 transfer 导出侧保持原样即可（现状如此）。
- **R-2（低，D-5+D-15）**：60s 放弃 drain + 10s 强制删除的组合窗口内，被放弃但仍在跑的 drain 若后续完成写入，事件会投到已删除 session 聚合（mod.rs:69-70 注释自认）。建议确认 publish 对不存在聚合的兜底。
- **R-3（低，D-7）**：若 boot 路径未接上 `list_suspended_before`，与共享数据库的并存进程可能被 sweep 误恢复；`_before` 变体存在但调用点未在本 lane 证实（建议复核 boot.rs:1241 附近）。
- **R-4（低，D-3）**：title 20×50ms 重试与用户手动改名的竞态由“每 attempt 重读 current 行”守卫（title.rs:312-327），已闭环；仅行为面差异非正确性。
- **R-5（极低，D-10/D-13）**：边角口径差，无数据损坏路径。
- **R-6（信息）**：determinism 抽象对 fork/revert/seq 无影响：事件 seq 由 DB `event_sequence` 单调分配，fork 消息 id 由事件 id 派生（两侧同构）；session id slug 见 D-12。时间戳生产路径为 SystemClock ≡ Date.now。

## 覆盖声明

- **ref 侧已全读（逐行）**：store.ts, projector.ts, history.ts, prompt.ts, inbox.ts, revert.ts, move.ts, transfer.ts, subagent-job.ts, subagent-completion.ts, title.ts, context.ts, stats.ts, usage.ts, shell.ts, skill.ts, command.ts, provider-context.ts, system-prompt.ts, instructions.ts, instruction-entry.ts, instruction-state.ts, run-coordinator.ts, execution.ts, execution/restart.ts, environment.ts, diff.ts, error.ts, to-session-error.ts, affinity.ts, info.ts, session.ts（fork/create/remove/shell/skill/prompt/synthetic/revert 段全读），session/event.ts, message.ts, schema.ts（2-9 行 re-export，确认无逻辑）。sql.ts 仅核对了被 store/projector 引用的表形状。
- **rust 侧已全读**：session/{prompt,execution,fork,revert,diff,inbox,store,projector,transfer,subagent_job,subagent_completion,message_updater,mod}.rs, subagent_jobs_adapter.rs, jobs.rs（restart 段逐行、job 内核抽样）, state_transform.rs（头部+API 面）, hooks.rs（API 面+SessionPromptEvent）, runner/{title,prompt_prepare}.rs, subagent.rs(completion_text), shell/result.rs(通知段), handlers/session.rs 的 stats/move/shell/view/wait/fork 段。
- **按边界排除（非静默跳过）**：runner/ 循环主体、compaction、context.rs、system_prompt、retry、generate（C2a/C3 lane）；diff.rs 只精读前 200 行 + 与 ref 逐段比对计划结构（TurnPlan/anchor/step/switch 查询形状一致，两段式拆分为 rust 结构选择）。
- **测试文件**：仅按需查阅，未逐行。
- **未验证点（明示）**：① boot.rs 是否调用 `list_suspended_before`（R-3）；② handlers 中 command/skill 执行路径仅确认消息通道存在，未逐行比对 commands.execute 的参数映射。
