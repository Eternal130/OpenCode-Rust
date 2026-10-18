# Parity Scan 09 — 数据库、持久化与 Project/Worktree/Location 生命周期

- 日期：2026-10-18
- 参考：`/opt/opencode-ref`（opencode v2.0.24, TypeScript）
- 目标：`/opt/opencode-rust`（Rust 移植）
- 审计员：只读子代理（09-database）
- 方法：逐文件对照（schema DDL、48 条迁移逐条 SQL 级比对、运行时/pragma/事务、KV/State、project/worktree/workspace/location 生命周期、旧库导入），全部结论附双侧 `path:line`。

---

## 0. 总览

**结论：静态层（schema + 迁移 + 打开/pragma/事务语义）是全仓库最强的一块——19 表 + 48 迁移逐字节忠实，且叠加了数项超出参考实现的加固（跨进程 bootstrap 双检、WAL 切换重试、按列名解码的 NULL 容错、与真实 bun 建库的互操作测试）。动态层的缺口集中在 project/worktree/location 的"生命周期事件"侧：`Project.activate` 完全缺失、生产路径的 project 解析器挂 `NoEvents`（`worktree.resolved` 持久事件与 adopted 收养扫描在生产中是死代码）、project id 哈希用了 SHA-256 而参考是 SHA-1（跨实现共享 DB 时身份分裂）、`worktree.remove` 不执行 git 侧拆除、`worktree.refresh` 是 204 空桩、v1 内容级迁移（1135 行）未移植。**

严重度分布：S0 = 0；S1 = 6；S2 = 14+。

| # | 主题 | 判定 | 级别 | KNOWN/NEW |
|---|------|------|------|-----------|
| D1 | project id 哈希 SHA-256 vs SHA-1 | WRONG | S1 | NEW |
| D2 | `Project.activate`（60s 限频 time_active 提升 + project.updated）缺失 | MISSING | S1 | NEW |
| D3 | 生产解析器 `NoEvents`：`worktree.resolved` 事件无生产者 + adopted 扫描缺失 | MISSING | S1 | NEW |
| D4 | `Project.update` 忽略 canonical/commands、伪造响应、不发事件 | PARTIAL | S1 | NEW |
| D5 | `worktree.remove` 只删行：无 canonicalize、无 strategy 校验、不跑 `git worktree remove` | WRONG | S1 | NEW |
| D6 | `worktree.refresh` 空桩（TODO phase-9） | STUB | S1 | KNOWN（auditE 1.16 家族）|
| D12 | v1 内容迁移（session/message/part → session_v2/session_message）未移植，路由为假状态 | MISSING | S1 | NEW |
| D7–D22 | 见 §13 差异清单 | WRONG/PARTIAL/MISSING | S2 | 多为 NEW |
| D23 | bootstrap 加固（双检/重试/按名解码/互操作测试） | EXTRA（有益） | — | NEW |
| — | 19 表/48 迁移/pragma/锁/事务/路径规范化/旧 journal 播种 | MATCH | — | KNOWN（auditF 3.1）|

---

## 1. 表与迁移映射总表

### 1.1 表映射（v2 快照 → Rust）

参考侧快照：`packages/core/src/database/schema.gen.ts`（drizzle-kit 生成，首建库单事务执行）。Rust 侧：`crates/oc-core/src/database/bootstrap.rs` 的 `BOOTSTRAP_SQL`（逐语句、含内层空白的字节级转写，`bootstrap.rs:9-13` 注释明示保持 `sqlite_master` 文本一致）+ 行结构 `crates/oc-core/src/database/schema.rs`。

| # | 表 | ref（schema.gen.ts） | rust DDL（bootstrap.rs） | rust 行类型（schema.rs） | 判定 |
|---|----|----------------------|--------------------------|--------------------------|------|
| 1 | `account_state` | :8-14 | :16-21 | :486-494 (`AccountStateRow`) | MATCH |
| 2 | `account` | :15-26 | :24-34 | :471-484 (`AccountRow`) | MATCH |
| 3 | `control_account` | :27-39 | :36-47 | :496-508 (`ControlAccountRow`) | MATCH |
| 4 | `credential` | :40-52 | :49-59 | :455-469 (`CredentialRow`) | MATCH |
| 5 | `event_sequence` | :53-59 | :62-67 | :432-440 (`EventSequenceRow`) | MATCH |
| 6 | `event` | :60-70 | :69-78 | :419-430 (`EventRow`) | MATCH |
| 7 | `kv` | :71-78 | :80-86 | :510-519 (`KvRow`) | MATCH |
| 8 | `permission` | :79-89 | :88-97 | :442-453 (`PermissionRow`) | MATCH |
| 9 | `project_directory`（已废弃仍建表） | :90-100 | :99-107 | :268-277 (`ProjectDirectoryRow`) | MATCH |
| 10 | `project` | :101-117 | :110-124 | :248-266 (`ProjectRow`) | MATCH |
| 11 | `instruction_blob` | :118-123 | :127-130 | :398-405 | MATCH |
| 12 | `instruction_entry` | :124-135 | :133-142 | :386-396 | MATCH |
| 13 | `instruction_state` | :136-145 | :145-152 | :407-417 | MATCH |
| 14 | `session_inbox` | :146-157 | :155-164 | :372-384 | MATCH |
| 15 | `session_message` | :158-169 | :167-176 | :344-356 | MATCH |
| 16 | `session_pending` | :170-181 | :179-188 | :358-370 | MATCH |
| 17 | `session_v2`（36 列） | :182-222 | :191-229 | :301-342 | MATCH |
| 18 | `workspace` | :223-231 | :232-238 | :279-289 | MATCH |
| 19 | `worktree` | :232-241 | :241-248 | :291-299 | MATCH |
| + | journal 表 `"migration"` | migration.ts:37-39（`CREATE TABLE "migration"`）| bootstrap.rs:270-275（含 IF NOT EXISTS 变体）| schema.rs:521-528 (`MigrationRow`) | MATCH |

索引（16 个）与部分索引：`schema.gen.ts:242-277` ↔ `bootstrap.rs:250-265` —— `event_aggregate_seq_idx`（UNIQUE）、`event_aggregate_type_seq_idx`、`permission_project_action_resource_idx`（UNIQUE）、`session_inbox_session_delivery_seq_idx`、`session_inbox_session_enqueued_seq_idx`（UNIQUE）、`session_message_session_seq_idx`（UNIQUE）、`session_message_session_type_seq_idx`、`session_message_session_time_created_id_idx`、`session_message_time_created_idx`、`session_pending_session_delivery_seq_idx`、`session_pending_session_compaction_idx`（UNIQUE, WHERE type='compaction'）、`session_pending_session_admitted_seq_idx`（UNIQUE）、`session_v2_project_idx`、`session_v2_workspace_idx`、`session_v2_parent_idx`、`session_v2_time_suspended_idx`（WHERE time_suspended IS NOT NULL）—— **全部一致**。

外键/唯一约束：`account_state.active_account_id → account(id) ON DELETE SET NULL`、`event.aggregate_id → event_sequence ON DELETE CASCADE`、`permission/project_directory/worktree.project_id → project ON DELETE CASCADE`、`instruction_*/session_inbox/session_message/session_pending.session_id → session_v2 ON DELETE CASCADE`、`session_v2.project_id → project ON DELETE CASCADE`、复合主键 `control_account(email,url)`、`project_directory(project_id,directory)`、`worktree(project_id,directory)`、`instruction_entry(session_id,key)` —— **全部一致**。

字段级差异：**无 DDL 级差异**。行为级字段差异见 §6（`project.time_active` 插入默认值）与 §5（JSON 列的写入形状）。

### 1.2 迁移映射（48 条，顺序 = `migration.gen.ts` 数组序）

Rust `crates/oc-core/src/database/migrations.rs:111-538`（`MIGRATIONS`），journal 常量 `bootstrap.rs:278-327`（`MIGRATION_IDS`，48 项，与 `migration.gen.ts` 导入序一致；有测试锁定 `migrations.rs:1073-1079`）。每条迁移独立事务（`up` + journal 插入，`migrations.rs:616-629` ↔ `migration.ts:111-118`）。

| # | 迁移 id | ref 文件（migration/） | rust（migrations.rs） | 体 | 判定 |
|---|---------|------------------------|------------------------|----|------|
| 00 | 20260127222353_familiar_lady_ursula | …ursula.ts | :112-130 | 建v1七表+六索引 | MATCH |
| 01 | 20260211171708_add_project_commands | | :131-135 | ALTER project ADD commands | MATCH |
| 02 | 20260213144116_wakeful_the_professor | | :136-142 | 建 control_account | MATCH |
| 03 | 20260225215848_workspace | | :143-149 | 建 v1 workspace | MATCH |
| 04 | 20260227213759_add_session_workspace_id | | :150-157 | +列+索引 | MATCH |
| 05 | 20260228203230_blue_harpoon | | :158-165 | 建 account/account_state | MATCH |
| 06 | 20260303231226_add_workspace_fields | | :166-176 | +type/name/directory/extra −config | MATCH |
| 07 | 20260309230000_move_org_to_state | | :177-185 | org 迁移至 state | MATCH |
| 08 | 20260312043431_session_message_cursor | | :186-195 | 换 message/part 索引 | MATCH |
| 09 | 20260323234822_events | | :196-203 | 建 event/event_sequence | MATCH |
| 10 | 20260410174513_workspace-name | | :204-208 / :647-664 | `PRAGMA table_info` 探针（name 列有→"`name`" 否→"''"）| MATCH |
| 11 | 20260413175956_chief_energizer | | :209-218 | 建 session_entry | MATCH |
| 12 | 20260423070820_add_icon_url_override | | :219-226 | +列+回填 | MATCH |
| 13 | 20260427172553_slow_nightmare | | :227-240 | session_entry→session_message | MATCH |
| 14 | 20260428004200_add_session_path | | :241-245 | +path | MATCH |
| 15 | 20260501142318_next_venus | | :246-253 | +agent/model | MATCH |
| 16 | 20260504145000_add_sync_owner | | :254-258 | +owner_id | MATCH |
| 17 | 20260507164347_add_workspace_time | | :259-265 | +time_used | MATCH |
| 18 | 20260510033149_session_usage | | :266-278 | +6 usage 列 + json_extract 回填（SQL 逐字）| MATCH |
| 19 | 20260511000411_data_migration_state | | :279-285 | 建 data_migration | MATCH |
| 20 | 20260511173437_session-metadata | | :286-290 / :666-673 | 幂等探针（列已存在则跳过）| MATCH |
| 21 | 20260601010001_normalize_storage_paths | | :291-300 | 4 条 REPLACE/GLOB 规范化 | MATCH |
| 22 | 20260601202201_amazing_prowler | | :301-305 | DROP permission | MATCH |
| 23 | 20260602002951_lowly_union_jack | | :306-313 | 重建 permission+唯一索引 | MATCH |
| 24 | 20260602182828_add_project_directories | | :314-320 | 建 project_directory | MATCH |
| 25 | 20260603001617_…projection_indexes | | :321-331 | 索引更换 | MATCH |
| 26 | 20260603040000_…projection_order | | :332-342 | DELETE+ADD seq+索引 | MATCH |
| 27 | 20260603141458_session_input_inbox | | :343-350 | 建 session_input | MATCH |
| 28 | 20260603160727_jittery_ezekiel_stane | | :351-360 | IF NOT EXISTS 索引 | MATCH |
| 29 | 20260604172448_event_sourced_session_input | | :361-386 | 重建 session_input（含事务内 PRAGMA no-op，逐字保留 :375-377）| MATCH |
| 30 | 20260605003541_add_session_context_snapshot | | :387-393 | 建 session_context_epoch | MATCH |
| 31 | 20260605042240_add_context_epoch_agent | | :394-400 | +agent | MATCH |
| 32 | 20260611035744_credential | | :401-408 | 建 credential（v1 形状）| MATCH |
| 33 | 20260611192811_lush_chimera | | :409-417 | 重建 credential | MATCH |
| 34 | 20260612174303_project_dir_strategy | | :418-430 | +strategy 表重建 | MATCH |
| 35 | 20260622142730_simplify_session_context_epoch | | :431-439 | −3 列 | MATCH |
| 36 | 20260622170816_reset_v2_session_state | | :440-450 | 5×DELETE | MATCH |
| 37 | 20260622202450_simplify_session_input | | :451-463 | 清空+workspace 清空 | MATCH |
| 38 | 20260804233008_loose_psylocke | | :464-468 / :676-747 | 前置 V2 标记（20260730195856）→ rename 路（V1-only die :685-695）vs 重建路 | MATCH |
| 39 | 20260805200742_import_legacy_credentials | | :469-473 / :818-952 | 读 `<Global.data>/auth.json`：oauth/api/wellknown 三形、methodID 映射（openai→chatgpt-browser；github-copilot/opencode/xai→device；其余 oauth）、`cred_`+26 字符 id、`wellknown:sources` 并集 upsert | MATCH |
| 40 | 20260808023530_workspace_domain | | :474-481 | 重建 workspace（domain 形状）| MATCH |
| 41 | 20260811161259_execution_claim_attempts | | :482-488 | +resume_attempts | MATCH |
| 42 | 20260812181746_session_inbox | | :489-497 | 建 session_inbox+2 索引 | MATCH |
| 43 | 20260812213948_worktree | | :498-505 | 建 worktree + project_directory→worktree 回填（strategy CASE 映射逐字）| MATCH |
| 44 | 20260819222447_session_viewed_state | | :506-514 | +time_idle/time_viewed/idle_outcome | MATCH |
| 45 | 20260823191254_nullable_workspace_binding | | :515-524 | `foreign_keys:false`（唯一一条）表重建 | MATCH* |
| 46 | 20260910120000_clear_v1_session_permission | | :525-529 | UPDATE permission=NULL | MATCH |
| 47 | 20260923013825_project_time_active | | :530-537 | +time_active 并以 time_updated 回填 | MATCH |

\* m45：ref 在 `supportsForeignKeyToggle` 为 false 的 runtime（workerd DO）退化为 `PRAGMA defer_foreign_keys = ON`（migration.ts:119-131）；Rust 只实现 toggle 分支（migrations.rs:601-610）。Rust 无 workerd 目标，判定可接受（记录在案）。

### 1.3 引擎语义（bootstrap / applyOnly / 旧 journal 播种）

| 语义 | ref | rust | 判定 |
|------|-----|------|------|
| 分类：有 `session`/`session_v2` → 增量；有表无 session → die；空 → 单事务快照+journal+48 id | migration.ts:22-52 | migrations.rs:552-582 | MATCH |
| 快照单事务（DDL+建 journal+全部 id 一次写入） | migration.ts:34-46 | migrations.rs:560-577 | MATCH |
| **跨进程竞态**：分类在事务外读 | （无防护；TS 靠进程内 per-file 锁）| 事务内二次分类（双检），输者走增量路（migrations.rs:544-577，测试 :1036-1070）| EXTRA（有益）|
| `applyOnly`：建 journal（IF NOT EXISTS）→ 读已完成集合 → 逐条跑 | migration.ts:54-146 | migrations.rs:587-614 | MATCH |
| 旧 `__drizzle_migrations` 播种：name 列变体（bulk INSERT OR IGNORE）/ created_at 变体（`strftime('%Y%m%d%H%M%S',…)` 前缀匹配，不匹配 die） | migration.ts:62-104 | migrations.rs:956-985 | MATCH |
| `foreignKeys:false` 迁移：事务外 PRAGMA OFF/ON，恢复失败致命 | migration.ts:119-131 | migrations.rs:601-610（`Effect.ensuring(orDie)` ↔ 恢复失败返回 Err）| MATCH |

---

## 2. 数据库运行时（打开 / pragma / 锁 / 事务 / 并发）

| 项 | ref | rust | 判定 |
|----|-----|------|------|
| WAL / synchronous=NORMAL / busy_timeout=5000 / cache_size=-64000 / wal_checkpoint(PASSIVE) / foreign_keys=ON | database.ts:39-47（node 驱动 native 层先设 WAL，sqlite.node.ts:90）| mod.rs:320-332（busy_timeout 先武装 → WAL → 其余）| MATCH（顺序差异见 D23 加固）|
| WAL 切换 SQLITE_BUSY | 无重试 | 5 次退避重试（mod.rs:339-358）| EXTRA（有益）|
| 文件 0600（append-create 原子建文件 + chmod + -wal/-shm 容忍 ENOENT；win32 跳过）| database.ts:83-98 | mod.rs:360-382 | MATCH |
| 每文件 bootstrap 锁（`:memory:` 私有）| database.ts:57-65 | mod.rs:83-90 | MATCH |
| 单连接 + 信号量串行；事务全程持锁 | effect-sql 桥（SqlClient 单连接语义）| `Mutex<Connection>`+gate，`spawn_blocking`（mod.rs:67-90,177-224）| MATCH |
| 事务默认 BEGIN 模式 | **deferred**（drizzle 桥默认，session.ts:168；仅 bus.ts:433,652 显式 immediate）| **immediate**（mod.rs:47-56,192-199；注释称"所有调用点 immediate"——与事实不符）| **WRONG（D15, S2）**：单进程内无感（连接串行），多进程共享库时写锁获取时机不同（Rust 更安全但语义分歧）|
| 嵌套 SAVEPOINT `effect_sql_${id}` | session.ts:168 | `oc_core_${n}`（mod.rs:287-310）| MATCH（前缀不同，无影响）|
| COMMIT 失败显式 ROLLBACK（SQLite deferred 约束失败后保持事务打开）| session.ts:177-186 | mod.rs:227-251 | MATCH |
| `:memory:` 支持 | database.ts:67-80 | mod.rs:94-96 | MATCH |

**EXPECTED（D15）**：默认 `BEGIN DEFERRED`，仅 bus 两处显式 immediate。
**ACTUAL**：`Database::transaction` 默认 `BEGIN IMMEDIATE`（mod.rs:198），且 mod.rs:47-49 注释断言"every opencode call site passes immediate"——`grep behavior: "immediate"` 仅 2 处 / 14 个事务调用点。
**影响**：低（多进程时 Rust 主动取写锁反而减少 BUSY-on-upgrade；但注释误导后续维护者）。

---

## 3. 存储路径与文件布局

| 项 | ref | rust | 判定 |
|----|-----|------|------|
| DB 文件名方案：`OPENCODE_DB`（含 `:memory:`、相对名拼 data）→ channel 共享表 {latest,dev,beta,next,prod} 或 `OPENCODE_DISABLE_CHANNEL_DB∈{1,true}` → `opencode.db`，否则 `opencode-<sanitize(channel)>.db` | cli/database-path.ts:4-11 | database/path.rs:48-103（`db_location`/`engine_location`）| MATCH（rust 改进：空 `OPENCODE_DB` 忽略）|
| channel 来源 | 构建期 define（未定义时 dev 运行默认 `local`→`opencode-local.db`）| 运行期 env `OPENCODE_CHANNEL` 默认 `latest`（oc-util/channel.rs:19-29）| **WRONG（D24, S2, KNOWN=P10-DB1）**：匹配 npm 发行版（latest→opencode.db）；与"源码直跑的 ref（local）"不同文件 |
| XDG 根 | `opencode` | `opencode-rust` 后缀（oc-util/global_roots.rs:25-29）+ `legacy_v2_roots()` | DELIBERATE（PORT_REPORT §3；单向门迁移见 §11）|
| 路径列规范化：`storagePath`（win32 反斜杠→斜杠）/`absoluteColumn`（非绝对拒绝）/`directoryColumn`（空串保留）/`pathColumn`（任意）/`absoluteArrayColumn`（sandboxes JSON）| database/path.ts:6-95 | schema.rs:60-100（`storage_path`/`absolute_storage`/`directory_storage`）| MATCH |
| `fs.resolve`（realpath-存在否则词法绝对）| FSUtil | config.rs:324-339 `resolve_path` | MATCH |

---

## 4. KV 与 State

### 4.1 KV

| 能力 | ref（kv.ts） | rust | 判定 |
|------|--------------|------|------|
| get/set（upsert 刷 time_updated）/remove | :41-59 | `Tx::kv_get/kv_set/kv_delete`（schema.rs:665-690）+ `Database::kv_*`（:786-797）| MATCH |
| **scan**（prefix/after/limit；limit 夹 1..1000 默认 100；`prefixEnd` 码点递增；limit+1 探测 next 游标）| :60-90 | **无对应函数**（grep kv_scan/scan 前缀 → 无）| **MISSING（D10, S2）** |
| 命名空间已实现消费方 | `wellknown:sources`、`job.background/*`、models-dev 缓存、websearch provider | 相同键：integration.rs:200-248、jobs.rs:20（`job.background/`）、provider_catalog/refresh.rs:92-106、tool/websearch.rs:102-136 | MATCH |

D10 影响评估：ref 中 `scan` 的在树消费者只有 `job.ts:445`（后台任务分页），Rust 用专用 `LIKE 'job.background/%'` 查询等价覆盖（jobs.rs:561-578，无分页但全量列出）；另一消费者 repository-cache 整体缺失（D11）。故降为 S2。

### 4.2 State（transformable state 内核）

ref `state.ts`（252 行）：`create/get/invalidate/revision/transform/reload` + 注册组（失败隔离 disable）、batch/shutdown 通知合并、`reconcile`。Rust 无通用对应原语（grep `Transformable` → 无）；各服务（config snapshot、AgentRegistry、ToolRegistryState 等）各自实现可变状态。**MISSING（D16, S2）**——架构原语，具体消费者已分别移植，无直接持久化影响。

---

## 5. session/message/part 持久化格式

- v2 无独立 part 表：parts 内嵌在 `session_message.data` JSON（session/sql.ts:79-95）。Rust 同构：`SessionMessageRow.data` 原样 JSON 字符串（schema.rs:344-356），投影器 `session/projector.rs` 以事件→行投影写入（`session.created` INSERT 含 13 列 :91-107；列更新走 `update_session_column`）。
- `session_v2` 写入：`SessionRow::new`（schema.rs:567-617）默认值与 ref drizzle 默认一致（cost/tokens 0、resume_attempts 0、时间戳 now）。
- 排序/分页：`store.list` = `(time_updated, id)` 双列排他锚点分页、方向翻转（store.rs:151-237 ↔ ref session/store.ts:99-139）；`messages` = seq 分页、游标限会话内（store.rs:239-279 ↔ store.ts:140-178）。MATCH。
- 软删除：两侧均为硬 DELETE（`session.deleted` → `DELETE FROM session_v2`，projector.rs:141-145）；`time_archived/time_suspended` 为状态标记非软删过滤器。MATCH。
- `session.viewed`：monotone watermark `max(?1, coalesce(time_viewed,?1))` 且 `time_updated = time_updated` 抑制刷新（projector.rs:174-184 ↔ ref projector 以 `sql` 自赋值抑制 `$onUpdate`）。MATCH。
- `worktree.resolved` 投影（会话身份再归属）：**消费者完整**（projector.rs:804-883，含 previous/global/adopted 候选、`\uffff` 目录区间、adopted→canonical 目录、contains 过滤、path 相对化、time_updated 自赋值）——与 ref projector.ts:483-540 逐语义一致；**但生产无事件生产者**（见 D3）。

---

## 6. Project 生命周期

### 6.1 身份派生（id）

| 步骤 | ref（project.ts） | rust（project.rs） | 判定 |
|------|-------------------|--------------------|------|
| 标记上溯 `.git`/`.hg`（同目录 git 优先）| :337-342 | walk_to_marker :602-613 | MATCH |
| `Project.root`（纯文件系统）| :49-58 | root :459-470 | MATCH |
| remote 身份 `Hash.fast("git-remote:<host>/<path>")` | :265-271 + **util/hash.ts:4-7 = SHA-1** | :634-639 + **hash_fast :25-35 = SHA-256** | **WRONG（D1, S1）** |
| 目录身份 `Hash.fast("directory:<path>")` | :375 | :510 | 同上（D1） |
| `<store>/opencode` 缓存 id（previous；解析不回写）| :257-263 | cached_id :627-631 | MATCH |
| 根提交回退（排序取首）| :296-299（git rev-list）| root_commit_id :690-705（libgit2 revwalk）| MATCH |
| hg：缓存→roots(all()) 首节点→global；remote 推导双方均为 TODO | :303-335 | :709-725 | MATCH |
| URL 规范化：host 小写、去 `.git`/斜杠；`file:` 拒绝 | :273-294（WHATWG URL.parse 接受任意 scheme）| :643-687（仅 http/https/ssh/git + scp 启发式，多拒绝 `.` 开头 authority）| **WRONG（D21, S2）**：`git+ssh://…` 等 scheme ref 有 id、rust 回退缓存/根提交 |
| canonical（主 worktree）| :350-356 `git worktree list` 取 kind=main | :534-539 + common_directory :617-624（commondir 父目录启发式；git2 无 commondir API）| **WRONG（D20, S2）**：标准布局等价；GIT_COMMON_DIR/非标准 worktrees 父目录/bare 时分歧 |
| 空仓库 → `global` | :359 | :532 | MATCH |

**D1（S1, NEW）——EXPECTED**：`Hash.fast` = `createHash("sha1")`（40 hex）。
**ACTUAL**：`hash_fast` = SHA-256（64 hex，project.rs:22-24 注释自认"v2 uses sha1"并以"跨语言 id 不会跨主机持久化"辩护）。
**影响**：**id 恰恰在同一数据库内持久化**（`project.id` 主键、`session_v2.project_id`、`worktree.project_id`）。本移植的卖点之一是接手 v2 数据（`v2_migration.rs:26-31` 原样拷贝 v2 数据库），同一 checkout 在 ref 下是 sha1 id、在 rust 下是 sha256 id → 拷贝来的库中出现**重复 project 行**、旧会话挂在旧 id 下不被新解析认领、worktree 行按对方 id 查不到。修复成本极低（换 sha1 crate 或手写 sha1）。

### 6.2 持久化（upsert / canonical 重指 / worktree 行）

| 语义 | ref | rust | 判定 |
|------|-----|------|------|
| upsertProject：冲突仅刷新 vcs（setWhere：vcs 存在→ NULL或≠ 才写；vcs 空→ IS NOT NULL 清洗）；canonical 保留；sandboxes='[]' | project/sql.ts:36-56 | project.rs:233-268 `ON CONFLICT … WHERE project.vcs IS NOT excluded.vcs`（`IS NOT` 空安全不等价，语义相同）| MATCH |
| 插入时 `time_active` 默认 | **`$defaultFn(() => Date.now())` → 插入得 now**（project/sql.ts:33-36）| `ProjectRow::new` 置 0（schema.rs:550）且 upsert 显式绑 0 | **WRONG（D9, S2）**：新项目在 `ORDER BY time_active DESC` 里排最后 |
| canonical 重指（旧 canonical 不存在才换 + 发 project.updated）| project.ts:109-130 | project.rs:550-568（reassign 后经 events 回调——生产为 NoEvents，事件丢失，见 D3）| PARTIAL |
| worktree 行：`onConflictDoNothing` + 事件同事务 commit | project.ts:146-201（`commit: () => insert…`）| project.rs:579-596 先插行后发事件（无原子耦合；无 `announcing` 在途去重集）| **WRONG（D22, S2）** |
| adopted（无 vcs 且目录含于本次解析目录的旧项目，root 解析等于该目录）扫描并附在事件上 | project.ts:159-181,188 | **无**（`WorktreeResolved` 结构无 adopted 字段，project.rs:83-89）| **MISSING（并入 D3）** |
| `previous` 回退 global | project.ts:187 | project.rs:594 | MATCH |

### 6.3 list / update / activate（服务面）

| 语义 | ref | rust | 判定 |
|------|-----|------|------|
| `list`：`desc(time_active), asc(id)`，返回全 Info | project.ts:205-213 | oc-server handlers/project.rs:34-71：`ORDER BY time_active DESC`（无 id 次序），字段齐 | **WRONG（D8, S2）**（平局次序不定）|
| `update`：可改 canonical/name/icon/commands（空串→NULL 清除），返回更新后行 + 发 `project.updated` | project.ts:215-239（UpdateInput 含 canonical+commands，schema/project.ts:51-57）| handlers/project.rs:122-169：**只处理 name/icon.override/icon.color**，canonical/commands 静默丢弃；coalesce 语义（无空串清除）；**响应行为 fabricated**（canonical=""、time 全 0）；不发事件 | **PARTIAL（D4, S1）** |
| `activate`：60s 每项目限频，`time_active=now` 且 `time_updated` 自赋值不刷，发 `project.updated` | project.ts:60,241-255；location-lifecycle.ts 启动时 + 每个 session 事件 fork 激活（location-lifecycle.ts:33-45）| **全仓库无 `UPDATE project SET time_active`**（grep 仅 SELECT；announce.rs:9-10 注释提及该限频但无实现）| **MISSING（D2, S1）** |

D2 影响：`time_active` 冻结在插入值（迁移库=旧 time_updated；新库=0）→ 项目列表"最近使用"排序失效；boot/会话活动驱动的 `project.updated` 节奏缺失（Rust announce.rs 用 generation 代替，见 D14）。

---

## 7. Worktree 生命周期

| 语义 | ref | rust | 判定 |
|------|-----|------|------|
| 表/行写入（含 resolve 写入）| worktree/sql.ts + project.ts:193-199 | schema.rs:291-299 + project.rs:314-334 | MATCH |
| `list`：`desc(time_created), asc(directory)`，返回 `{directory, strategy?}` | worktree.ts:135-144 | handlers/project.rs:175-226：`ORDER BY directory`；且要求 projectID == 当前 location 解析的项目（:207-209），ref 只要求行存在（:115-119 404 仅当项目行缺）| **WRONG（D7, S2）** |
| `create`：source canonical 校验（必须是已登记行或 canonical）→ 父目录（显式 → config `worktree.directory`（`~/` 展开/相对 canonical）→ `<data>/worktree/<id[..6]>`）→ slug 名 + `-2..-10` 碰撞 → `git worktree add --detach -- <dir> <ref|HEAD>` → 行 replace 写入 + `worktree.updated` → commands.start（bash -lc / win shell，env OPENCODE_WORKTREE_BASE/PATH，失败即失败）| worktree.ts:204-255 + strategies.ts:24-31 + config/plugin/worktree.ts + git.ts:649-667 | handlers/project.rs:299-518：全链路在（canonical :261-269、config 父目录 :275-296、前缀 :378-379、碰撞 :394-411、git :416-451、行 :459-475、事件 :473-475、start :480-515）| **MATCH**（auditE #10 的"create 不跑 git"已修复——auditE 陈旧）|
| `remove`：canonicalize 输入 → 行须有 strategy（否则 InvalidDirectoryError）→ `git worktree remove [--force]` → 删行 → `worktree.updated` | worktree.ts:257-271 + git.ts:669-681 | handlers/project.rs:563-598：**只 `DELETE FROM worktree`**（原始字符串比较、不 canonicalize、不校验 strategy、不跑 git、force 仅改变 404 行为）| **WRONG（D5, S1）** |
| `refresh`：列已存行存在性 → 根集合（canonical + 无 strategy 行）→ 各策略 `list` 发现 → canonicalize/去重 → 单事务 upsert(非替换 setWhere)/remove 消失行 → 有变则 `worktree.updated` | worktree.ts:273-327 | handlers/project.rs:557-561：**`Ok(no_content())` 空桩**（注释 TODO(phase-9)）| **STUB（D6, S1, KNOWN=auditE 1.16 家族）** |
| 策略注册表（git 内建 + 插件 add/selected/configure）| worktree/strategies.ts | 无通用策略表（git 直写在 handler）| PARTIAL（S2；单策略下行为等价）|
| slug 生成 | util/slug.ts | handlers/project.rs:233-257（29 形容词×31 名词本地表）| MATCH（词表随机，形状一致）|

D5 影响：DB 行删除后磁盘目录与 git worktree 元数据残留（`git worktree list` 仍列出、prune 不会清）；symlink/带尾斜杠路径永不匹配行 → 404。ref 的 force 传给 git，rust 的 force 只抑制 404。

---

## 8. Workspace（workspace.ts / workspace/）

ref：`create`（逻辑行即时落库，provider 冲突 CreateConflict，注册表预检）→ `provision`（共享 attempt + keyed mutex + binding 持久化）→ `connect`（连接缓存 + `last_used_at` 刷新 + 每 spawn 计活动）→ 闲置挂起循环（20min 阈值/1min 轮询，`suspendForIdle` + `last_used_at` 回写 + 连接关闭）→ `destroy`（attempt 结算 NotFound + 连接关闭 + driver destroy（binding null 也调用）+ 删行）；driver 注册表默认**空**（workspace/driver.ts:85-88）。

rust：仅原始行助手 `insert_workspace/list_workspaces`（project.rs:406-441，测试用）+ schema 行类型（schema.rs:279-289）。无服务、无注册表、无闲置挂起、无 last_used_at 写入。**MISSING（D13, S2）**——鉴于 ref 默认注册表为空（create 必 ProviderNotFound），属休眠脚手架；`session_v2.workspace_id` 列的读写路径存在（store 过滤）。

---

## 9. Location / Instance 生命周期与多实例

| 语义 | ref | rust | 判定 |
|------|-----|------|------|
| 键：`(directory, workspaceID)`；win32 才 normalize | location-service-map.ts:41-46 | `LocationRef`：**全平台**反斜杠→斜杠（location.rs:76-79）| **WRONG（D18, S2）**（POSIX 合法反斜杠目录被并键）|
| 图缓存：`idleTimeToLive: ∞`（仅显式失效驱逐）；boot 失败驱逐+可重试 | location-services.ts:44-77 | LocationServiceMap（location_services.rs:215-338；get_or_build 失败不发布、占位竞争既有者胜）| MATCH |
| 失效：先摘路由再关停；借用者持旧图（old-graph survival）| location-services.ts:88-99 | invalidate = remove→shutdown（:293-297）；Arc 借用存活（测试 :625-649）| MATCH |
| reload（单点/全部：失效→重建，失败收集）| location-service-map.ts:13-28 | reload/reload_all（:303-322）| MATCH |
| 关停序列：permission.close → forms.close → rpc.close → publish `location.shutdown`（数据 {}、location 戳）| location-lifecycle.ts:47-58 | LocationGraph::shutdown（location.rs:916-931）| MATCH |
| **boot 时 `project.activate` + 会话事件再激活** | location-lifecycle.ts:33-45 | 无（并入 D2）| MISSING |
| 活动 TTL：60min/1min；session 事件 touch（仅活跃图）；每轮把未跟踪的活跃图 touch、清消失项；过期先 interrupt(reason=inactivity, awaitSettlement)，**再查活跃会话，有新工作则 re-touch 不驱逐** | location-activity.ts:31-95 | spawn_activity_sweeper（location_services.rs:371-388）+ LocationActivity（:141-197）：touch 为手动接线（:139-140 TODO lane-integration）；**活跃但未 touch 的位置永不过期**（expired 只扫 last_touch）；interrupt 后无再查 | **PARTIAL（D19, S2）** |
| Instance 55 节点图层 / `discovery:false` vanilla / 出生插件表 / replacements | instance.ts:73-107 | LocationGraph 精简图（location.rs:737-893；具体句柄 + 少量 trait 缝）| PARTIAL（D17, S2：vanilla/discovery 开关与出生插件未移植——宿主嵌入 API）|

---

## 10. 事件持久化（event/event_sequence）与总线

| 语义 | ref（bus.ts） | rust | 判定 |
|------|---------------|------|------|
| `event_sequence` upsert：`seq=max(old,新)`；owner 仅在旧为 NULL 且新非空时写入 | :404-415 | `Tx::event_sequence_upsert`（schema.rs:703-717，`coalesce(excluded.owner_id, old)`）| MATCH（语义等价）|
| 批量尾部 upsert：ref 直接 `set seq=finalSeq`（:645-646）；rust 走 max()+coalesce | :645-646 | bus.rs:808 | 等价（单调性更强）|
| 重放分歧/序列不匹配/事件已存在 → die | :354-395 | bus.rs:888-970（同类校验，错误串形状略异）| MATCH |
| 分页读（gt after / lte through，asc seq，尾行 seq 作游标跨空洞）| :775-800 | events_page（schema.rs:760-777）+ bus 流 | MATCH |
| 持久事件 `created` 列（m38 加入）默认 0 | schema | schema.rs:419-430 | MATCH |

**`worktree.resolved` 是唯一非 session 的持久事件**（registry.rs:226-240）：定义与投影两侧齐全，**生产无发布者**（见 D3）。`worktree.updated`（ephemeral）在 worktree create/remove 后有发布（handlers/project.rs:520-555）。

---

## 11. 旧库导入 / 迁移

| 语义 | ref | rust | 判定 |
|------|-----|------|------|
| 旧 drizzle journal 播种（两变体）| migration.ts:62-104 | migrations.rs:956-985 | MATCH（§1.3）|
| `20260805200742` legacy credentials 导入 | 同名文件 | migrations.rs:749-952（含 `Global.data` 注入 `MigrationOptions`，`open_default`/`with_global_data`）| MATCH |
| **v1 内容迁移**（v1 `session`/`message`/`part` → `session_v2`/`session_message`：消息/parts 变换、无效行 warnings、事件表分批清空、kv 进度态、`<data>` 下旧 json 存储导入、global 项目兜底行）| v1-migration.bun.ts（1135 行）；服务路由 `/api/experimental/migration/v1`（server/handlers/migration.ts），bun 运行时经 layer 挂接 | **未移植**：路由存在但恒答 `{"status":"completed"}`（oc-server/handlers/misc.rs:120-126，仅摸了一下 migration_ids）| **MISSING（D12, S1）** |
| v2 数据目录整体接管（DB/auth/config/npm/bin/state 根文档，单向拷贝+marker+文件锁）| 无对应（v2 本机原地）| v2_migration.rs:1-200+（auditF 3.3 的 state 根空洞已补：:36-48 拷贝 service/model/session 文档、channel storage、prompt history）| EXTRA（移植自设需求；auditF 该条已陈旧=已解决）|

D12 影响评估：Rust 侧正常用户拿到的库要么是快照新库、要么是 v2_migration 拷来的 v2 库——后者若源自 v1 时代且未在 v2 侧完成 v1 迁移，`loose_psylocke` 会以 `V1OnlyHistory` 拒绝（与 ref 相同）；但 ref 有迁移器可先行转换，rust 没有 → 此类库在 rust 下不可用且无出路。S1（窄场景、后果完全阻塞）。

---

## 12. 缓存失效

- **repository-cache**（远程参考库检出缓存，`<Global.data>/repos/...`，kv 日节流，flock）：ref repository.ts + repository-cache.ts，消费者 reference.ts:49-90。rust 无任何对应（grep `cachePath|RepositoryCache|RemoteReference` → 0）。**MISSING（D11, S2）**——参考/文档 lane 的依赖，跨 lane 记录。
- **config 缓存失效**：rust 用 announce.rs 指纹基线 + generation（announce.rs:303-323, 327-344），语义对应 ref 的 config watcher/reload（100ms 去抖 ↔ 250ms 轮询，已注记）。可接受。
- **`$cache.invalidate = cache.onMutate`**（drizzle 查询缓存，driver.ts:66）：rust 无查询缓存，无需失效。N/A。

---

## 13. 差异清单（判定 × 严重度 × KNOWN/NEW）

| ID | 判定 | 级别 | K/N | 摘要 | ref | rust |
|----|------|------|-----|------|-----|------|
| D1 | WRONG | **S1** | NEW | project id 哈希 SHA-256 ≠ SHA-1：共享/拷贝库中同 repo 双身份 | util/hash.ts:4-7；project.ts:270,375 | project.rs:22-35 |
| D2 | MISSING | **S1** | NEW | `Project.activate`（60s 限频 time_active + project.updated）不存在；boot/会话事件激活缺失 | project.ts:241-255；location-lifecycle.ts:33-45 | 全仓库无 `UPDATE … time_active` |
| D3 | MISSING | **S1** | NEW | 生产解析器挂 `NoEvents`：`worktree.resolved` 无生产者；adopted 收养扫描未实现（消费者齐全成死代码）| project.ts:159-199 | engine/mod.rs:630-636；project.rs:83-89；projector.rs:139,804 |
| D4 | PARTIAL | **S1** | NEW | `Project.update` 丢 canonical/commands、空串清除语义缺、响应行伪造、不发 project.updated | project.ts:215-239 | handlers/project.rs:122-169 |
| D5 | WRONG | **S1** | NEW | `worktree.remove` 不 canonicalize/不校验 strategy/不跑 `git worktree remove`/force 语义错位 | worktree.ts:257-271；git.ts:669-681 | handlers/project.rs:563-598 |
| D6 | STUB | **S1** | KNOWN(auditE 1.16) | `worktree.refresh` 空桩（发现/修剪/事务/事件全缺）| worktree.ts:273-327 | handlers/project.rs:557-561 |
| D12 | MISSING | **S1** | NEW | v1 内容迁移（1135 行变换）未移植；状态路由恒 completed | v1-migration.bun.ts | handlers/misc.rs:120-126 |
| D7 | WRONG | S2 | NEW | worktree.list 排序缺 `time_created DESC` 主键序；且只认当前 location 的项目 | worktree.ts:135-144,115-119 | handlers/project.rs:184,207 |
| D8 | WRONG | S2 | NEW | project.list 缺 `asc(id)` 平局次序 | project.ts:209 | handlers/project.rs:42 |
| D9 | WRONG | S2 | NEW | 新项目 time_active 插入默认 0（ref 为 now）| project/sql.ts:33-36 | schema.rs:550 |
| D10 | MISSING | S2 | NEW | KV.scan（前缀分页）未移植；唯一在树消费者已用 LIKE 等价覆盖 | kv.ts:60-90 | schema.rs:665-690；jobs.rs:561-578 |
| D11 | MISSING | S2 | NEW | Repository/RepositoryCache 子系统整体缺失（reference lane 依赖）| repository.ts；repository-cache.ts | 无 |
| D13 | MISSING | S2 | NEW | Workspace 服务（create/provision/connect/destroy/闲置挂起）未移植；ref 默认驱动注册表为空=休眠 | workspace.ts:78-326；driver.ts:85-88 | project.rs:406-441（仅行助手）|
| D14 | WRONG | S2 | NEW | announce 的 `project.updated` 载荷缺 name/vcs/icon/commands，且按 `worktree==location目录` 匹配 → 非 canonical 位置发布 `{}` | project.ts:129,237,254（全 Info）| announce.rs:216-271 |
| D15 | WRONG | S2 | NEW | 事务默认 BEGIN IMMEDIATE（ref 桥默认 deferred，仅 bus 两处 immediate）；注释失实 | drizzle session.ts:168；bus.ts:433,652 | mod.rs:47-56,192-199 |
| D16 | MISSING | S2 | NEW | State 内核（transform/batch/shutdown/注册组）无通用对应 | state.ts | 无（各服务各自实现）|
| D17 | MISSING | S2 | NEW | Instance `discovery:false` vanilla 模式与出生插件表未移植 | instance.ts:52-107 | location.rs:806-893 |
| D18 | WRONG | S2 | NEW | LocationRef 全平台反斜杠归一（ref 仅 win32）| location-service-map.ts:41-46 | location.rs:76-79 |
| D19 | PARTIAL | S2 | NEW | TTL sweeper：touch 未接线（活跃未跟踪位置永不过期）、interrupt 后无活跃再查 | location-activity.ts:31-95 | location_services.rs:139-140,371-388 |
| D20 | WRONG | S2 | NEW | canonical 主 worktree 推导用 commondir 父目录启发式（非 `git worktree list` main）| project.ts:350-356 | project.rs:534-539,617-624 |
| D21 | WRONG | S2 | NEW | remote URL 规范化只认 4 scheme（ref URL.parse 任意 scheme）；scp 启发式多拒 `.` 前缀 | project.ts:273-294 | project.rs:643-687 |
| D22 | WRONG | S2 | NEW | worktree 行与事件非同事务提交；无 `announcing` 在途去重 | project.ts:146-201 | project.rs:579-596 |
| D24 | WRONG | S2 | KNOWN(P10-DB1) | channel 默认 latest（env）vs 构建期 define（源码跑=local）| cli/version.ts + database-path.ts | oc-util/channel.rs:19-29 |
| D23 | EXTRA | — | NEW | bootstrap 加固：跨进程双检+WAL 重试+busy_timeout 先武装+按列名 NULL 容错解码+bun 实库互操作测试 | 无 | migrations.rs:544-577；mod.rs:321-358；schema.rs:20-32；tests/{bun,null_tolerance}.rs |

无 S0：未发现会损坏 Rust 自身数据或破坏库结构的缺陷；所有写路径约束/事务边界与 ref 一致或更严。

---

## 14. 与既有文档的对照（KNOWN 标定）

| 来源 | 陈述 | 本次核验 |
|------|------|----------|
| docs/auditF-cli-config-data.md:136-140（3.1 数据库全对等）| 19/19 表、48/48 迁移、文件名方案 | **成立**（本次逐 SQL 复核）|
| docs/auditF …:163（3.3 v2 迁移丢 state 根）| v2_migration 不拷 state 根 | **已过时**：v2_migration.rs:36-48 已拷 service/model/session/channel-storage/prompt-history |
| docs/auditE-core.md:150-155,206（1.16 worktree git 侧缺失）| create 只写行不跑 git | **已过时**（create 已跑 `git worktree add`，handlers/project.rs:416-451）；**refresh 空桩仍然成立**（D6）|
| docs/auditD-api.md:274,297 | `worktree.updated` 未发射 | **已过时**（create/remove 后有发布，handlers/project.rs:520-555）；但 `worktree.resolved`（持久）生产无发布者（D3，新发现）|
| ROADMAP.md:233 | "DB 19/19+48/48 migrations" | 静态层成立；本报告补动态生命周期缺口（D1-D6 等）|

---

## 15. 覆盖清单（本次检查过的文件）

**ref（全部读取/比对）**：database/{database,migration,migration.gen,schema.gen,schema.sql,path,sqlite,sqlite.node,v1-migration,v1-migration.noop,v1-migration.bun}.ts、database/migration/*.ts（48/48 全部）、database/drizzle/**（桥：driver/session/sqlite-core/effect/*）、effect-drizzle-sqlite/、kv.ts、kv/sql.ts、state.ts、instance.ts、instance/service.ts、repository.ts、repository-cache.ts、project.ts、project/{schema,sql}.ts、location.ts、location-{lifecycle,activity,services,service-map}.ts、workspace.ts、workspace/{sql,driver}.ts、worktree.ts、worktree/{sql,git,strategies,directory}.ts、bus.ts（事件持久化段）、session/sql.ts、session/projector.ts（worktree 投影段）、project.ts(schema 包)、config/plugin/worktree.ts、cli/database-path.ts、util/hash.ts、server/handlers/migration.ts。

**rust（全部读取/比对）**：oc-core/src/database/{mod,bootstrap,migrations,schema,path,error}.rs、database/tests/{mod,bun,null_tolerance}.rs、project.rs、location.rs、location_services.rs、v2_migration.rs、bus.rs（事件段）、bus/{registry,routing,sink}.rs（结构）、session/{store,projector}.rs（持久化相关段）、jobs.rs（kv 消费段）、engine/mod.rs（assemble 段）、config.rs（resolve_path 段）、oc-util/{global_roots,channel}.rs、oc-server/src/handlers/{project,session,misc,location}.rs、oc-server/src/announce.rs、oc-server/src/boot.rs（迁移挂点）。

**未尽事项**（建议移交对应 lane）：session_inbox/session_pending 的投递状态机细节（sessions lane）、permission/credential 表的服务层写入（credential lane 已有 handler，未见本扫描范围问题）、plugin host 的 kv-like scan（oc-plugin-host/supervisor.rs:1622 为 JS 侧自有实现，非 DB kv）。

---

## 16. 修复优先级建议（只读建议，不实施）

1. **D1**（一行改动级别）：`hash_fast` 换 SHA-1——解锁"接手 v2 库"这一既定目标的前提。
2. **D3 + D2**（同一处接线）：`Engine::assemble` 以真实事件汇替换 `NoEvents`，并补 `Project.activate`；`worktree.resolved`/adopted 扫描建议直接复用 ref project.ts:159-199 的形状（projector 消费者已就绪）。
3. **D5/D6**（worktree git 侧）：remove 走 `git worktree remove [--force]`；refresh 按 worktree.ts:273-327 移植（发现+修剪+单事务+事件）。
4. **D4**：project_update 补 canonical/commands/空串清除 + 返回真实行 + 发事件。
5. 其余 S2 按表处理；D12 视 v1 用户存量决定是否移植。
