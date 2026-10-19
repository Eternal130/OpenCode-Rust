# PARITY LOOP — opencode-rust 后续开发执行手册

- **版本**：1.0（2026-10-18）
- **参考基线**：`/opt/opencode-ref`，opencode v2.0.24，commit `e7a34f09bf`（只读）
- **目标基线**：`/opt/opencode-rust`，workspace `2.0.24`，本文件落笔时 `main@31ff988`
- **审计输入**：`docs/parity-scan-2026-10-18/`（`00-MASTER.md` + `01`–`16`，386 条原始记录，去重后约 340 个独立差异）
- **状态文件**：`docs/parity-ledger.jsonl`（唯一任务队列）；`docs/parity-ledger.md` 若存在，只是生成视图，不可手改
- **适用期**：从本文件生效起，直到 §12 的退出条件全部满足

> **权威顺序**：ledger 的当前条目 > 本文件的流程与验收规则 > 各根因簇章节 > 扫描报告。
> 扫描报告只是证据库，不是任务清单。**不要用“再扫一遍”来定义完成。**

---

## 0. 怎么使用这份文件

### 0.1 编排者（orchestrator）

每个 Batch 开始时读一次 §0–§6、§8–§13；每次派活时只读：

- 目标 cluster 的小节；
- ledger 中该 cluster 的 `open / reproduced / fixed` 条目；
- 该条目的 `ref`、`rust`、`repro`、`test` 字段。

不要把 16 份报告重新塞进上下文。需要证据时按 `ref` / `rust` 的 `path:line` 定点打开。

**不要把整份文件交给一个 fixer 从头做。** 一个 iteration 只做一个 cluster 或一个 sub-lane；一个 lane 只拿自己的条目和 scope。整份文件是编排者的地图，不是单个 agent 的任务清单。

### 0.2 修复 lane（fixer）

每次只读 §2、§3、§6、§7、自己的 cluster 小节，以及被分配的 ledger 条目。禁止跨 cluster 顺手修。

### 0.3 验证 lane（verifier）

只读 §7、§8、§9 和被验证的 ledger 条目。verifier 不写产品代码，不修改 ledger 的 `fixed` 状态，只写 `verified` 或打回。

### 0.4 变更控制

以下内容一旦冻结，修改必须走 ADR + 用户确认，不能在循环中“顺手改”：

- 严重度定义（§3.3）；
- 状态机与证据要求（§3.4–§3.6）；
- 退出条件（§12）；
- 五组最小差分门禁的覆盖面（§8.3）；
- 红线与进程安全规则（§2）。

### 0.5 第一次循环从哪开始

**不要先修业务 bug。** 第一次循环只做 Batch 0 的 B0-1 + B0-8 + B0-2：

1. 把 `docs/parity-scan-2026-10-18/` 的 16 份报告导入 `docs/parity-ledger.jsonl`（已有 114 条种子；补齐剩余 + 去重），跑 `tools/parity-ledger.py validate` + `dedupe`，确认 386 → unique 的差异可解释。
2. 先做 B0-8：确认 lane 可执行 bun + 参考 checkout 可启动；`OC_GATE_STRICT=1` 下 Gate 0 必须红/绿明确，不允许 self-skip。
3. 跑一遍现有 `p10` 和 workspace 测试，把结果写进 `docs/parity-baseline.md`；五组新 gate 先建骨架，known-red 全部挂 ledger ID。
4. 只有 Batch 0 退出后，才按 §5 进入 Batch 1A。

---

## 1. 战略结论：从“扫描驱动”改为“契约 + 回归驱动”

### 1.1 一句话

**引擎主干已经高保真；真正的缺口是“最后一公里接线”和“外围用户表面”。下一阶段的核心不是再扫出多少条，而是把已知缺口变成可执行契约，把“接线完成”变成硬性验收。**

### 1.2 为什么不能继续扫描驱动

旧循环是：

```text
扫描 300 条 → 逐条修 → 再扫 300 条 → 再逐条修 → ……
```

它有三个结构性缺陷：

1. **扫描只能发现缺口，不能证明缺口已修**。静态 grep 看不到运行时接线、重启行为、并发和真实客户端解码。
2. **同一根因会产生多个 ID**。按 ID 逐条修会让不同 agent 重复改同一处，甚至互相破坏。
3. **“代码存在”被误当成“功能存在”**。这是本仓库历史上最贵的教训：实现存在、测试存在，但生产调用点不存在，用户侧仍然不可用。

### 1.3 新循环的定义

```text
扫描/审计 → 结构化 ledger（带 repro + test）→ 按根因簇修
   → 接线完成清单 → 独立验证 + 最小差分门禁
   → ledger 证据 → 下一簇
```

完成不是“报告里这条打了勾”，而是：

- ledger 状态为 `verified`；
- 有可执行复现；
- 有回归测试；
- 有独立 verifier；
- 有 commit 和测试输出；
- 该簇的接线清单逐项有证据。

### 1.4 本轮的总工作量判断

- 原始 386 条，去重后约 340 个独立差异。
- 其中 **6 个 S0**、约 **105 个 S1**、其余 S2/S3。
- S0/S1 不是 105 个独立工程，而是 **10 个根因簇**。按簇修，一次改动覆盖多条 ID。
- 先做 Batch 0（台账、门禁、安全护栏），再动产品代码。**Batch 0 没完成就修 bug，等于继续旧循环。**

---

## 2. 红线：先保证不会再杀掉宿主、污染数据、互相踩踏

这一节是硬约束。任何 lane 的 prompt 都必须原样带上 §2.1–§2.5。违反一次就可能丢掉整轮工作，甚至影响同机在线服务。

### 2.1 进程安全：PID 错位与“按名字杀进程”

#### 事故 A：PID 复用杀死系统服务

仓库里已有明确记录（`crates/oc-server/src/safekill.rs:1-30`）：机器重启后 PID 被回收，旧 registration 文件里的 PID 可能已经属于 `sshd` / `journald`；旧的 `stop()` 直接 `kill(pid)`，曾造成批量误杀。

**根因**：把“PID 整数”当成进程身份。PID 不是身份，`(pid, boot_id, start_time)` 才是；对无 sidecar 的外部记录，还要加 cmdline 白名单。

**现行正确机制**（必须复用，不得绕过）：

- `crates/oc-server/src/safekill.rs`
  - `Expectation::Generation { boot_id, start_time }`：自家 registration 必须 boot_id 匹配 + `/proc/<pid>/stat` 的 start_time 匹配；
  - `Expectation::Foreign`：外部记录必须 cmdline 的 basename 精确等于 `opencode` / `oc-server`；
  - 通过 `pidfd_open` + `pidfd_send_signal` 投递，避免检查与 signal 之间 PID 被回收；
  - 任何验证失败 → `Refused` / `Gone`，**不发信号**。
- `crates/oc-util/src/procutil.rs`
  - `signal_group_filtered(leader, signal, allowlist)`：只在 group leader 仍有 `/proc` 条目（live 或未 reap 的 zombie）时扫描组成员；只 signal cmdline 命中 allowlist 的成员；通过 pidfd 投递；
  - 明确禁止 `kill -TERM -PGID` 这种裸进程组信号。
- `crates/oc-cli/src/services/standalone.rs`
  - 只对自己的 `Child` 句柄发 SIGTERM/SIGKILL；stdin EOF → TERM → 3s → KILL。
- `tools/parity/src/reference.rs`
  - 参考服务器 `bun run` 会 re-exec 嵌套 bun，停止时对“自己 spawn 的进程组”做 SIGTERM→SIGKILL；这是允许的，因为 leader 是本次 spawn 的 child，且停止逻辑在 harness 内。

**禁止清单（无例外）**：

```text
pkill -f opencode
pkill -f oc-server
pkill -f bun
pkill -f node
pkill -f cargo
pkill -f nextest
pkill -f python
killall opencode
killall oc-server
killall bun
killall node
kill -9 $(pgrep ...)
kill -9 -<PGID>
fuser -k 49374/tcp
```

**这个清单是示例，不是穷举。** 唯一允许的停止方式见下；任何“按名字/模式杀任意进程”的行为都禁止，包括 `bun`、`node`、`cargo`、`nextest`、`python`、`cargo-watch`。宿主编排器、参考服务器、其它 lane 都可能以这些名字运行；按名字匹配等于随机杀宿主。

**为什么 `pkill -f opencode` 特别危险**：agent 自己就运行在 opencode 宿主里；按名字匹配会命中编排器、宿主 TUI、其它 lane，甚至当前会话。**杀测试服务器时按名字匹配，等于把宿主一起杀掉。**

**唯一允许的停止方式**：

1. 自己 spawn 的进程：持有 `Child` 句柄，`start_kill()` / `kill()` / `wait()`；测试里用 `kill_on_drop(true)` 兜底。
2. 生产 registration：走 `lifecycle::stop` → `safekill::safe_signal`，带 generation 期望。
3. 万不得已需要按进程组清理：只在 leader 仍被自己持有且未 reap 时，用 `signal_group_filtered` + cmdline allowlist；不得裸 PGID。
4. 看到 `refprobe.pid`、registration JSON、旧 pidfile：**只当提示，不当身份**；先校验 boot_id + start_time + cmdline，校验不过就删除/忽略，不发信号。

**孤儿测试服务器处理**：不要“清一遍”。先 `ss -ltnp` 找到端口，确认 cmdline / cwd / 启动时间 / 环境变量都属于本 lane；能通过 `Child` 或 registration 安全停止才停；否则保留现场并报告。宁可留一个孤儿，不可误杀宿主。

#### 事故 B：权限过宽 + root 运行

历史记录（`ROADMAP.md` §4.5）：root 直接跑 cargo 造成过三次文件所有权污染；一次 CPU 打满的 cargo build 影响同机 newapi/49374；共享 checkout 被另一个 lane 删除过文件。

**硬规则**：

- 所有本地 build/test 必须走 `tools/dev-gate.sh`：
  - 强制 `eternal130` 身份；
  - systemd transient scope `CPUQuota=400%`、`MemoryMax=10G`、`TasksMax=512`；
  - 1 分钟 load ≥ 6 拒绝启动；
  - `OPENCODE_DEV_GATE_DIR` 指向 lane 的 worktree。
- 禁止 root 直接跑 `cargo` / `cargo nextest` / `cargo fmt` / `cargo clippy`。
- 禁止 `sudo`、`systemctl`、`service`、`docker` / `podman`、`chown` / `chmod`（仓库内文件权限变更需 integrator + 用户确认）。
- 禁止修改 `/opt/opencode-ref`（只读参考）、`~/.config/opencode`、`~/.local/state/opencode`、`~/.local/share/opencode`、`/root/.cache/opencode`。
- 测试必须把 `HOME`、`XDG_CONFIG_HOME`、`XDG_DATA_HOME`、`XDG_STATE_HOME`、`XDG_CACHE_HOME`、`TMPDIR` 指向 sandbox；真实 v2 DB 只能复制到 sandbox 后读。
- 测试端口一律 `127.0.0.1:0`；**永远不绑定 49374**；需要固定端口的 helper 必须显式拒绝 49374。

#### 事故 C：工作树互相污染

- 每个写 lane 一个独立 git worktree：`.worktrees/lane-<cluster>`，分支 `lane/<cluster>`。
- `main` 对 lane 只读；只有 integrator 写 `main`。
- 每个 lane 使用自己的 `CARGO_TARGET_DIR=/tmp/opencode/target-<lane>`，避免 target 锁和磁盘互相踩。
- commit 早、commit 小；handoff 前 `git status --short` 必须干净。
- 禁止 `git clean -fdx`、`git reset --hard`、`git checkout -- .`、删除分支、force push。
- 合并串行：integrator 一次只 merge 一个 cluster，merge 后立刻跑 affected tests + 最小门禁；红则回退该 merge，不继续叠。

#### 事故 D：数据/临时目录污染

- `/tmp/opencode` 顶层是共享区：只能创建/删除本 lane 的 PID 唯一子目录（如 `/tmp/opencode/lane-wire-<pid>-<nanos>`）。
- 禁止 `rm -rf /tmp/opencode`、`rm -rf /tmp/opencode/*`。
- DNS 探针 `/tmp/opencode/dns-probe.*` 不是构建产物，禁止清理。
- 测试产生的真实用户数据目录写入 = 事故；发现后立即停止该 lane，保留现场，报告 orchestrator。

#### 事故 E：后台任务与宿主编排

`docs/host-issues.md` 记录了宿主 v2 平台的 H-1：子 agent 把长任务放后台、被重新唤醒后，第二次报告可能永远不回父会话。

**编排纪律**：

- 子 agent 禁止把 build/test/watch/scan 放后台；全部前台分块，单次工具调用 ≤45 分钟。
- orchestrator 自己的后台观察器允许，但要记录 task id。
- 任何曾经有后台任务的 child 完成后，父会话在下一次唤醒时用 `task_result` 复查一次；报告可能“已存但未推送”。
- 这是**宿主编排问题**，不要和产品侧的 `S-01`（后台 subagent 完成不 wake 父会话）混为一谈。两者都要修/规避，但层次不同。

#### 已知绕过护栏的脚本与债务（Batch 0 必须处理）

这些是已经存在的风险点，不是假设：

- `tools/parity/spec-p3/{ref_tti_probe,refbin_tti_probe,ours_tti_probe}.py` 和 `tools/parity/spec-ef2/f7_boot_trace.py` 仍使用 `pkill -9 -f` / `os.killpg(...SIGKILL)`，绕过 `safekill` / `procutil`。**在改成安全停止逻辑前，任何 lane 不得运行这些脚本**；它们应被标记为 legacy probe，或重写为只停止自己 spawn 的 child。
- `crates/oc-cli/tests/auth_e2e.rs` 目前是 `root:root`，eternal130 的 gate 无法编辑/fmt。**不要用 root 跑 cargo 来绕过**；由用户/integrator 修正所有权，或先把该测试从写 lane 的 gate 中隔离并登记。
- `crates/oc-core/src/integration/tests.rs` 的 OAuth 回调测试需要固定 `127.0.0.1:8080`；已有 skip-on-busy 语义（`:945-948`），Batch 0 要确认它真的 skip 而不是失败，并把“并行时可能 skip”写进测试说明；不得让并行 lane 互相踩。
- `tools/release-smoke.sh` 用裸 `$!` 杀自己刚 spawn 的 shell child；风险低，但应在 Batch 0 统一为 `Child` 句柄或 `signal_group_filtered`，保持规则一致。
- `tools/parity/spec-p3/*` 和 `f7_boot_trace.py` 是 root-owned；不要在写 lane 里 `chown`，交给有权限的人处理。

Batch 0 的 `process_safety` guard 必须把这些路径列入 allowlist（带 ledger ID、owner、到期 batch），并保证新代码不能新增同类模式。

### 2.2 lane 能力矩阵（最小权限）

| 角色 | 读 | 写 | Shell | 进程信号 | 网络 | Merge |
|---|---|---|---|---|---|---|
| scanner | 仓库 + ref 只读 | 无 | 只读命令（rg/git status/log/show） | 无 | 无 | 无 |
| fixer | 自己 scope + 依赖 | 仅 scope 内 | dev-gate + 测试 + 只读诊断 | 仅测试 harness 内自己的 child | 默认无 | 无 |
| verifier | 全仓库只读 | 无 | dev-gate + 测试 | 仅测试 harness 内自己的 child | 默认无 | 无 |
| integrator | 全仓库 | 仅 merge 冲突 | git + 门禁 | 无 | 无 | 有 |
| oracle | 全仓库只读 | 无 | 只读 | 无 | 无 | 无 |

补充：

- 若宿主工具不能按命令 allowlist 限制 `bash`，则默认**不给 fixer 任意 shell**；由 orchestrator 代跑测试并回传输出。
- `kill` / `pkill` / `killall` / `fuser` 对 fixer 和 verifier 一律 deny；测试代码里的 child 清理由 harness 负责。
- 外网、安装、系统服务、真实数据目录写入、49374 操作，必须用户显式批准。
- 唯一的常规例外是 C7 的 **live fixture window**（B0-9）：预先批准的 endpoint/credential sandbox + 时段；窗口外一律 cassette。

#### 权限配置原则（宿主支持时）

- **scanner**：`edit/write` 全 deny；`bash` 只允许 `rg`、`git status/diff/log/show`、`sed` 读取；不授予任何进程信号能力。
- **fixer**：`edit/write` 用 glob 限定到本 cluster 的写 scope；`bash` 默认 deny，只允许 `tools/dev-gate.sh`、测试命令、只读诊断；不授予 `kill/pkill/fuser`。
- **verifier**：`edit/write` 全 deny；`bash` 只允许测试/门禁/只读检查。
- **integrator**：`edit/write` 允许 `main` 的 merge 冲突；`bash` 允许 git merge/门禁；不允许顺手改功能。
- **oracle**：只读。
- 如果宿主只能给“全量 bash”，则把该角色降级为“orchestrator 代跑”，不要用“全量权限 + 口头约束”代替结构性限制。

### 2.3 每个 lane 的开工前检查

```bash
id -u                      # 必须是 eternal130，不是 0
pwd                        # 写 lane 必须在 .worktrees/lane-<cluster>
git status --short         # 必须干净
git log -1 --format='%H %s'
ss -ltn | grep ':49374 ' || true   # 只读观察；绝不 kill/fuser
```

不满足就停，不要“先干起来再说”。

### 2.4 每个 lane 的收工前检查

```bash
git status --short         # 干净
git log -1 --format='%H %s'
# 只清理本 lane 的 sandbox：/tmp/opencode/<lane>-<pid>-*
# 不改 /tmp/opencode 顶层，不清理别人的目录
```

### 2.5 事故处理

- 误杀/疑似误杀：立即停止所有 lane，保留 shell history / 日志 / `ss` / `ps` 现场，报告 orchestrator；不要“再杀一次确认”。
- 数据目录被写：停止 lane，记录路径和时间，报告；不要自行删除“恢复”。
- worktree 冲突：停止写入，`git status` + `git diff` 留证，由 integrator 裁决。
- 测试把宿主打挂：视为 S0 级流程事故，先修流程，再修产品。

---

## 3. Parity Ledger：唯一任务队列

### 3.1 文件与工具

- 权威数据：`docs/parity-ledger.jsonl`，一行一个 JSON 对象。
- 生成视图：`docs/parity-ledger.md`（只读；由脚本生成，不手改）。
- 校验/查询工具：`tools/parity-ledger.py`（已提供最小实现；Batch 0 扩展）
  - `validate`：schema、ID 唯一、状态迁移、证据完整性；`--exit` 额外执行退出条件；
  - `summary`：按 severity / status / cluster / owner 统计；
  - `next --cluster C`：按优先级返回可开工条目；
  - `dedupe`：按 fingerprint 找重复；
  - `evidence --id X`：追加证据；
  - `merge --id X --merge-commit SHA`：记录合并。

### 3.2 条目 schema

```json
{
  "id": "HTTP-H2",
  "source_id": "12:H2",
  "aliases": ["H2"],
  "fingerprint": "http|cursor.absent-vs-null|handlers/session.rs",
  "domain": "http",
  "cluster": "WIRE",
  "severity": "S1",
  "status": "open",
  "title": "分页 cursor 输出显式 null，v2 省略 absent 键",
  "symptom": "GET /api/session?limit=1 第二页 cursor 为 {\"previous\":null,\"next\":null}",
  "expected": "v2 optional 在 absent 时省略键；强类型 SDK 的 optional 不接显式 null",
  "actual": "Rust 恒序列化 previous/next，值为 null",
  "impact": "强类型客户端解码失败；分页游标不可用",
  "ref": ["packages/protocol/src/...:line"],
  "rust": ["crates/oc-server/src/handlers/session.rs:324", "crates/oc-server/src/handlers/message.rs:119"],
  "repro": "cargo nextest run -p oc-server -E 'test(cursor_absent_not_null)'",
  "test": "crates/oc-server/tests/parity_http_contract.rs::cursor_absent_not_null",
  "owner": "lane-wire",
  "upstream": "2.0.24",
  "wiring": {
    "implementation": false,
    "production_call_site": false,
    "config_env_consumer": "n/a",
    "event_publish_subscribe": "n/a",
    "db_read_write_migration": "n/a",
    "error_timeout_cancel": "n/a",
    "cross_process_restart": "n/a",
    "tui_cli_api_surface": true,
    "regression_or_differential_test": false,
    "ledger_evidence": false
  },
  "evidence": [],
  "merge_commit": "",
  "merged_at": "",
  "notes": "",
  "created": "2026-10-18",
  "updated": "2026-10-18"
}
```

字段规则：

- `id` 一经分配不再改；别名放 `aliases`。
- `source_id` 保留扫描报告编号，便于回溯；同一根因多个来源全部放 `aliases` / `notes`。
- `fingerprint` 用于去重：`domain + 症状码 + 首个 ref/rust 文件或符号`。**不要按标题去重。**
- `repro` 必须是可执行命令或明确的运行时步骤；没有 repro 的条目不能进入 `fixing`。
- `test` 必须指向将被创建/已存在的回归测试；没有 test 的条目不能进入 `fixed`。
- `wiring` 是 §7 的机器可读投影；`n/a` 必须写原因。
- `evidence` 是对象数组：commit、commands、result、counts、red_artifact、artifacts、verified_by、verified_at。
- `merge_commit` / `merged_at`：由 integrator 在 merge 后填写；exit 要求每个 `verified` 的 `merge_commit` 在 `main` 可达历史里。

### 3.3 严重度

| 级别 | 定义 | 例子 |
|---|---|---|
| S0 | 主流程断裂、数据丢失/损坏、安全/进程边界事故、无法恢复 | 凭据不进请求；TUI 表单假死；kill -9 后孤儿回合不恢复 |
| S1 | 用户可见功能不可用，或强类型契约不匹配导致客户端失败 | VCS 端点 schema 不符；插件 hook 生产不派发；PTY meta 帧错 |
| S2 | 边缘行为、保真度、性能、低频路径 | 排序细节、metadata 缺字段、错误文案 |
| S3 | 信封字段、文案、格式细节 | 多余字段、标点、日志格式 |
| needs-runtime | 静态无法确认，必须先跑运行时 | 未验证的并发/重启/真实网络行为 |

规则：

- 静态审计不能单独把一条判成 S0/S1 并直接开修；必须至少有可执行 repro，或标记 `needs-runtime` 并进入运行时队列。
- 不允许“为了显得严重”升级；也不允许“为了清账”降级。降级要有复核证据。
- 同一根因的多条 ID：保留一条 canonical，其余 `status=duplicate` + `duplicate_of`。

### 3.4 状态机

```text
open
  ├─► reproduced ─► fixing ─► fixed ─► verified
  ├─► needs-runtime ─► reproduced / accepted
  ├─► duplicate
  ├─► accepted
  └─► blocked
```

| 状态 | 谁能设 | 条件 |
|---|---|---|
| `open` | scanner / orchestrator | 有证据，未必有 repro |
| `reproduced` | fixer | repro 命令按描述失败，输出已记录 |
| `fixing` | fixer | 已 claim，worktree/branch 已建，写入 scope 已声明 |
| `fixed` | fixer | 代码 + 生产调用点 + 回归测试通过；commit 存在；**未独立验证** |
| `verified` | verifier | 从 committed HEAD 独立重跑 repro + test + 门禁；与 owner 不同人/会话 |
| `needs-runtime` | scanner / orchestrator | 静态无法确认；附运行时计划和环境 |
| `accepted` | oracle + 用户 | 有 ADR，写明为什么不可等价/永久分歧；不得当垃圾桶 |
| `duplicate` | orchestrator | 有 canonical ID 和 fingerprint 证据 |
| `blocked` | owner | 外部依赖/上游阻塞；写清解除条件 |

反规则：

- fixer 不得把条目设为 `verified`。
- scanner 不得把条目设为 `fixed`。
- 没有 `test` 不得 `fixed`；没有独立 verifier 不得 `verified`。
- `accepted` 不得因为“修起来麻烦”而使用；必须是设计/上游/范围决定。
- 回归可以把 `verified` 打回 `open`，必须附新证据和 commit。

### 3.5 证据包（phantom commit 的反面）

每个 `fixed/verified` 条目至少包含：

```json
{
  "commit": "<40-hex>",
  "branch": "lane/<cluster>",
  "commands": [
    "tools/dev-gate.sh 'cargo nextest run -p oc-server -E ...'",
    "tools/dev-gate.sh 'cargo clippy ...'"
  ],
  "result": "pass",
  "counts": "42/42 passed",
  "red_artifact": "/tmp/opencode/parity-evidence/<id>/red.log",
  "artifacts": ["/tmp/opencode/parity-evidence/<id>/..."],
  "verified_by": "lane-verify",
  "verified_at": "2026-10-18T..."
}
```

`red_artifact` 是修复前 repro 失败输出的路径/摘要；没有它，无法证明“红→绿”，`fixed/verified` 不算完成。

Commit message 必须包含 ledger ID：

```text
parity(WIRE): HTTP-H2 omit absent cursor keys

Ledger: HTTP-H2, HTTP-H1
Repro: crates/oc-server/tests/parity_http_contract.rs::cursor_absent_not_null
Tests: cargo nextest run -p oc-server -E 'test(parity_http_contract)'
Gate: tools/dev-gate.sh '...'
Ref: packages/protocol/src/...:line
```

证据写入 ledger：

```bash
# fixer：设为 fixed
python3 tools/parity-ledger.py evidence --id HTTP-H2 \
  --commit "$(git rev-parse HEAD)" --branch lane/wire \
  --command "tools/dev-gate.sh 'cargo nextest run -p oc-server -E \"test(parity_http_contract)\"'" \
  --counts "18/18 passed" --result pass \
  --red-artifact /tmp/opencode/parity-evidence/HTTP-H2/red.log

# verifier：独立复核后设为 verified（verified_by 必须不同于 owner）
python3 tools/parity-ledger.py evidence --id HTTP-H2 \
  --commit "$(git rev-parse HEAD)" --branch lane/wire \
  --command "tools/dev-gate.sh 'cargo nextest run -p oc-server -E \"test(parity_http_contract)\"'" \
  --counts "18/18 passed" --result pass \
  --red-artifact /tmp/opencode/parity-evidence/HTTP-H2/red.log \
  --verified --verified-by lane-verify
```

以下情况视为 **phantom commit（未完成）**：

- 只有报告文字，没有 commit；
- commit 不在 `main` 可达历史里；
- 只有测试改动，没有生产调用点；
- 测试被 `#[ignore]` / skip / 删除断言来“变绿”；
- 声称跑过门禁，但没有输出/计数；
- verifier 与 owner 是同一会话；
- 只改了 golden 让 diff 消失，没有解释；
- “实现存在”但 grep 不到生产调用方。

### 3.6 去重与计数

- 先按 `fingerprint` 分组，再人工确认。
- 同一根因的多个 ID：canonical 保留最完整证据，其余 `duplicate`。
- 每轮 Batch 结束重新生成 `summary`；Burn-down 看 **unique canonical**，不看原始 ID 数。
- 报告里的 386 与 ledger 的 unique 数必须能解释差异（重复/合并/降级/needs-runtime）。
- L0 的导入用脚本解析报告表格 + 人工抽查，不要让一个 agent 用上下文硬读 16 份报告；解析器的输出必须可重复、可 diff。
- **orphan check**：每个 ledger cluster 必须映射到 §5 的至少一个 batch line item，或标记 `accepted/duplicate`；否则 L0 不算完成。

---

## 4. 十个根因簇

> 原则：**按簇修，不按 ID 逐条修。** 一个簇一个 owner、一个写通道、一组共享测试。簇内条目共享设计，避免“修了 A 又破坏 B”。

| # | Cluster | 目标 | 主要 S0/S1 | 写通道（首要） | 关键验证 |
|---|---|---|---|---|---|
| C1 | `CRED` | 凭据 → 解析 → 请求通用化 | 04:B-1/B-2/B-3, A-3/A-4/A-5, D-6/D-8 | `oc-core/engine/resolver.rs`, `oc-core/integration/*`, `oc-server/handlers/model.rs` | 登录后请求头/401；keyless zen；console |
| C2 | `PLUGIN` | guest 插件完整接线 | 06:#1/#2, 08:D-21/D-18/D-33/D-35/D-14/D-30 | `oc-plugin-host/*`, `oc-server/plugins.rs`, `oc-core/hooks.rs`, `oc-tui/plugin_*` | 真实 omo-slim；hook/KV/MCP/skills/UI/RPC |
| C3 | `RUNTIME` | 生命周期、唤醒、恢复、重连 | 01:S-01/L-01/P-01/P-02, 14:TD-04 | `oc-core/session/{subagent_completion,store,jobs}.rs`, `oc-server/boot.rs` | 后台唤醒；kill -9；SSE resync |
| C4 | `WIRE` | HTTP/SSE/PTY/VCS 线格式 | 12:H0–H5, 05:F1–F4, 11:#1–4, 15:EV-1, 09:D5 | `oc-server/handlers/*`, `tools/openapi-gen`, `oc-schema` | OpenAPI 校验；cursor；PTY 帧；VCS schema |
| C5 | `CONFIG` | config/env/cli.json 消费 | 13:C-7/C-8/C-1/T-1/T-3/T-4, 07:N-1–N-4/K-1 | `oc-core/config/*`, `oc-core/cli_config/*`, `oc-tui/config.rs`, `oc-cli` | 每个 leaf 有消费者；env inventory；retention |
| C6 | `TUI` | 主 TUI 交互面 | 14:TD-16/17/18/19, TD-01/02/03/05/07, 06:#3–#17 | `oc-tui/app.rs`, `oc-tui/store.rs`, `components/*` | 表单/权限/会话路由/终端面板/PTY 帧 |
| C7 | `AI` | provider/传输/超时 | 03:M1–M5/M10/B2, 04:Azure/Cohere/Mistral, 16:C1/C2 | `oc-ai/*`, `oc-core/engine/resolver.rs` | 协议路由矩阵；WS；Azure；Copilot responses；timeout |
| C8 | `DB` | 身份/生命周期/迁移 | 09:D1–D6/D12, D7–D22 | `oc-core/database/*`, `oc-core/session/projector.rs`, `oc-server/handlers/project.rs` | SHA-1；activate；worktree；v1 迁移；真实 v2 库 |
| C9 | `SURFACES` | 缺失表面 | 10:P0/P1, 11:#5–#14 | `oc-core/tool/*`, `oc-tui/*`, `.github/workflows` | browser；LaTeX/Mermaid；平台 CI |
| C10 | `TOOLS-EXEC` | 工具/ shell 执行运行时 | 02:C1–C5/A1/A2, 11:#7 | `oc-core/shell.rs`, `oc-core/tool/*`, `oc-core/hooks.rs` | stdout/stderr 并发；timeout；shell hooks；session_move |

### 4.1 依赖图

```text
CRED ───────────────► AI providers（请求鉴权）
PLUGIN ─────────────► TUI plugin slot / keymap
RUNTIME ────────────► TUI reconnect / permission resync
WIRE ───────────────► TUI wire ingest / client decode
CONFIG ─────────────► TUI surfaces / CLI behavior
TUI-FORM ───────────► TUI daily interactions
DB ─────────────────► 消费 RUNTIME 的 claims/turns 语义；DB 在 1B 的 `session/store.rs` seam 合并后 rebase（箭头不是“DB 先于 RUNTIME”）
SURFACES ───────────► last（依赖上面的工具/UI 基础）
TOOLS-EXEC ─────────► independent（shell/tool runtime）
```

### 4.2 C1 `CRED` — 凭据 → 解析 → 请求

**目标**：`auth login` 得到的 key/OAuth 必须进入 `ModelResolver` 和实际请求；console/zen 的特殊降级必须与 v2 一致；`/api/model` 的可用性判断读凭据。

**Ledger 种子**：

- S0：`04:B-1`（通用凭据不进入解析/鉴权）、`04:B-2`（console provider 无 token）、`04:B-3`（keyless zen 无 public key 降级，需运行时复现）。
- S1：`04:A-1`（catalog 刷新不传播到 resolver/integration）、`04:A-3`（variant overlay）、`04:A-4`（catalog provider apiKey/baseURL）、`04:A-5`（env 方法过滤/vertex/bedrock 激活）、`04:D-6`（ChatGPT/Codex 凭据整形）、`04:D-8`（Poe token 字段）、`03:V3`（ADC service_account）。
- 关联：`03:B-6` 的 header/chunk timeout 归 C7，不在本簇重复修。

**写 scope**：

- `crates/oc-core/src/engine/resolver.rs`
- `crates/oc-core/src/integration/{credential,console,copilot,vertex,providers}.rs`
- `crates/oc-core/src/provider_catalog/{facade,variant,overlay}.rs`
- `crates/oc-server/src/handlers/{model,credential,integration}.rs`
- 只读依赖：`crates/oc-schema/src/{provider,integration}.rs`

**复现/测试**：

- `crates/oc-core/tests/credential_wiring.rs`
  - `active_credential_reaches_resolver`
  - `console_provider_gets_token`
  - `keyless_zen_uses_public_key`
  - `credential_switch_invalidates_resolver`
- `crates/oc-server/tests/credential_request.rs`
  - fake provider 断言 `Authorization` / `api-key` / baseURL；
  - 无凭据 → 401 的 wire shape；
  - `/api/model` availability 与凭据一致。

**Exit**：

- 至少一个真实登录流（OAuth 或命令流）→ resolver → fake provider 请求头，全链通。
- console/zen 两条特殊路径有运行时测试。
- `/api/model` 不再只看 env。
- credential switch 后 resolver 不缓存旧值。

### 4.3 C2 `PLUGIN` — guest 插件接线

**目标**：插件注册的 hooks、KV、MCP、skills、UI slot、RPC 全部进入生产路径；不是“host 内可见”，而是“引擎/ TUI 真实消费”。

**Ledger 种子**：

- S0：`06:#1`（TUI UI slot/router/keymap）、`06:#2` + `08:D-33`（插件 MCP 不进真实注册表）。
- S1：`08:D-21`（guest hooks 不进引擎分发链）、`08:D-18`（KV 落 `/tmp` sandbox，重启即失）、`08:D-35`（插件 skills 不进引擎）、`08:D-14`（RPC 全链 STUB）、`08:D-30`（远程 MCP OAuth）、`08:D-01`（目录自动发现）、`08:D-07`（per-location 激活）、`08:D-10`（ctx.session 方法缺失）、`08:D-11`（ctx.permission 缺失）、`08:D-13`（serve 模式事件源）、`08:D-02`（热重载）、`08:D-31`（MCP protocol pin/旧 SSE-only 传输）、`08:D-34`（凭据切换重连 MCP）、`01:M-02`（http/ws hook 不 dispatch）。

**写 scope**：

- `crates/oc-plugin-host/src/{supervisor,ui,registrations,host_js,lib}.rs`
- `crates/oc-plugin-host/src/host/host.js`
- `crates/oc-server/src/plugins.rs`
- `crates/oc-core/src/{hooks,plugin_apply}.rs`
- `crates/oc-tui/src/{plugin_ui,plugin_boot}.rs`
- `crates/oc-tui/src/app.rs` / `components/session_screen.rs` 的 slot mount seam：**由 C6a 先落**，C2c 只消费，不在同一 batch 抢 `app.rs`。
- 只读依赖：`crates/oc-schema/src/config/plugin.rs`

**复现/测试**：

- `crates/oc-plugin-host/tests/flagship.rs` / `flagship_tui.rs`（真实 omo-slim，若环境有）
- `crates/oc-server/tests/plugin_guest_wiring.rs`
  - hook 在 `session.prompt` / `tool.execute.before` / `shell.create.before` 的生产路径被调用；
  - KV 写入 → 重启 → 读回；DB 表/前缀/大小/并发/失败回滚；
  - 插件 MCP 出现在 `/mcps` 并建立连接；
  - 插件 skills 出现在引擎技能表；
  - `/api/rpc` 返回真实结果。
- `crates/oc-tui/tests/plugin_ui_slot.rs`
  - sidebar slot 渲染、router.navigate、keymap.layer；
  - 点击/按键命中。

**Exit**：

- `Bridge::dispatch_hook` 有生产调用方（grep 证明，非测试）。
- KV 跨进程重启可读；DB 表存在；namespace 前缀与 v2 一致。
- 插件 MCP 进入真实 registry，`/mcps` 可见，能调用一次工具。
- 插件 skills 可被 prompt/@skill 使用。
- TUI slot 在真实 app 路径可见、可交互。
- `/api/rpc` 不再是 500。
- 真实 omo-slim 加载并完成一次完整交互（若 CI 无插件，用 fixture + 本地 release gate）。

### 4.4 C3 `RUNTIME` — 生命周期、唤醒、恢复

**目标**：后台 subagent 完成会唤醒父会话；崩溃/重启会恢复孤儿 turn 和 pending background；SSE/WS 重连会 resync；prompt hook/skill 在生产路径生效。

**Ledger 种子**：

- S0 等价：`01:S-01`（completion 后不 wake 父会话）、`01:L-01`（restart sweep 未接线）。
- S1：`01:P-01`（session.prompt hook 不 dispatch）、`01:P-02`（prompt 附带 skills 不解析）、`14:TD-04`（重连 resync 缺失）、`01:K-01`（compaction payload-too-large 免费重发死代码）。
- 边界：`01:M-01` 已合并到 `AI-M1`（canonical，alias `01:M-01`），不要在 RUNTIME 重复修；`01:M-02` 归 C2（PLUGIN）的 hook dispatch，本簇只修生命周期调用点。

**写 scope**：

- `crates/oc-core/src/session/subagent_completion.rs`
- `crates/oc-core/src/session/store.rs`（只动 claim/release/resume 段）
- `crates/oc-core/src/jobs.rs`
- `crates/oc-core/src/session/inbox.rs`
- `crates/oc-server/src/boot.rs`
- `crates/oc-core/src/engine/mod.rs`（wake seam）
- `crates/oc-core/src/hooks.rs` 与 `crates/oc-server/src/handlers/session.rs`：**只在 PLUGIN/WIRE 的 1A seam 合并后**由 RUNTIME 接行为；hot file 单写者规则见 §10.3。
- 只读依赖：`crates/oc-core/src/session/execution.rs`

**复现/测试**：

- `crates/oc-core/tests/runtime_recovery.rs`
  - `background_completion_wakes_idle_parent`
  - `boot_sweep_releases_orphan_claims`
  - `pending_background_replayed_after_restart`
  - `resume_attempts_increment_and_budget`
- `crates/oc-tui/tests/reconnect_resync.rs`
  - 断线期间 permission/form 事件在重连后补回；
  - session children / message tail 重新同步；
  - “Session not found” 收尾。

**Exit**：

- 父会话空闲时，后台子任务完成 → 无需用户再发消息，父会话消费结果（运行时测试）。
- `kill -9` 后重启 → 孤儿 turn 恢复 / claim 释放 / pending background 补发。
- 断线重连 → permission/form 不丢。
- `session.prompt` hook 和 `@skill` 在生产路径有测试，不是 test-only。

### 4.5 C4 `WIRE` — HTTP/SSE/PTY/VCS 线格式

**目标**：所有路由的请求/响应符合 OpenAPI；cursor/optional 语义与 v2 一致；PTY meta 帧/关闭码正确；VCS 三端点返回 schema 形状；事件 payload 可 round-trip。

**Ledger 种子**：

- S1：`12:H0`（请求校验中间件整体缺失）、`H1`（message cursor/order）、`H2`（cursor null vs absent）、`H3`（MCP `uriTemplate`）、`H4`（session.stats 形状）、`H5`（WorktreeError.name）、`H6`（session cursor 丢 search/parentID/order/subpath，与 H2 同簇）。
- S1：`05:F1`（PTY meta 三重偏差）、`F2`（正常退出 4404）、`F3`（persistent-PTY resized checkpoint）、`F4`（WS ticket Origin 门）。
- S1：`11:#1`–`#3`（VCS base/branch/diff）、`09:D5`（worktree.remove 不跑 git）。
- S1：`15:EV-1`（persistent-pty.added terminal 错形）。
- S2 关联：`12` 的 cursor 内容丢 search/order、`credential.create` activate 默认值、`permission.saved.list` 缺省、`session.active` 域、`session.context` compaction 过滤、export sanitize、instruction 上限、`shell.remove` 幂等；`15` 的 filesystem.changed、session.usage.recorded 泄漏、credential.switched null；`05` 的 F5–F22。

**写 scope**：

- `crates/oc-server/src/handlers/{session,message,mcp,project,pty,persistent_pty}.rs`
- `crates/oc-server/src/routes.gen.rs`（**生成文件，禁止手改**）
- `crates/oc-server/src/handlers/schema_check.rs`（或新的生成式 validator）
- `tools/openapi-gen/*`（生成校验表/客户端类型）
- `crates/oc-schema/src/*`（只加必要类型）
- 只读依赖：`tools/openapi-gen/openapi.json`（pinned）

**复现/测试**：

- `crates/oc-server/tests/parity_http_contract.rs`
  - `cursor_absent_not_null`
  - `message_cursor_order_rules`
  - `session_cursor_preserves_query`（search/order/subpath 在 cursor 中保留）
  - `mcp_resource_template_key`
  - `session_stats_shape`
  - `worktree_error_name_literal`
  - `all_routes_reject_wrong_payload_type`（生成式矩阵）
- `crates/oc-server/tests/parity_pty_frames.rs`
  - meta = replay → binary `0x00 + {"cursor":...}`；
  - 64KiB 分块；
  - 正常退出 close code 1000，attach 失败 4404；
  - persistent-pty resized checkpoint。
- `crates/oc-server/tests/parity_vcs_wire.rs`
  - base 必填 source、无 base 503；
  - branch 返回 `string[]`；
  - diff 单条/分支/committed/10MB 封顶；
  - worktree.remove 真跑 git + 脏树/force。
- `crates/oc-server/tests/parity_event_payloads.rs`
  - 每个公共事件 JSON round-trip；
  - `persistent-pty.added` 的 `terminal` 是 `{cols,rows}` / `{head,tail}`；
  - `session.usage.recorded` 不进公共 SSE。

**Exit**：

- OpenAPI 141 路由的请求校验覆盖率从 2/141 提升到 **141/141**（或有逐条 allowlist 的例外）；
- 响应 schema 校验在测试里跑完所有 handler；
- cursor 不再输出显式 null；
- PTY 帧与 v2 客户端兼容；
- VCS 三端点 + worktree.remove 通过强类型客户端；
- 事件 payload 全部 round-trip。

**设计门**：H0 不要手写 139 个 validator。先做一个 `openapi-gen` 生成校验表的 spike，由 oracle 评审；生成器输出必须可 diff、可测试、可增量。H0 归 C4b（独立 sub-lane），**不阻塞 C4a 的 cursor/PTY/VCS/EV 修复**；C4b 可以滑到 Batch 2，但 release 前必须完成。

### 4.6 C5 `CONFIG` — config/env/cli.json 消费

**目标**：schema 里有的键，要么有生产消费者，要么显式 accepted（写原因）；env 变量同理；managed-policy 和 session-retention 真正生效。

**Ledger 种子**：

- S1：`13:C-7`（managed-policy 运行时整体缺失）、`C-8`（TUI session-retention 缺失）、`C-1`（compaction auto/keep/buffer 零接线）、`T-1`（cli.json session.permissions 读侧缺失）、`T-3`（session.* 死键）、`T-4`（mini 8 键 dead）。
- S1：`07:N-1`（--print-logs）、`N-2`（run JSON 错误）、`N-3`（cli.json plugins）、`N-4`（9 个 env 开关）、`K-1`（--hostname）、`K-2`（upgrade 方法集/通道）、`K-8`（迁移来的 history/frecency/stash 文件无消费者）。
- S2：`13` 的 warming/watcher.ignore/diffs/scroll/prompt/debug/animations/experimental/tabs.indicators/session.new_location 等 dead key；`C-9`/`T-2`/`TD-24`（new_location 缺失，同一根因）；env inventory 缺口。

**写 scope**：

- `crates/oc-core/src/config/*`
- `crates/oc-core/src/cli_config/*`
- `crates/oc-tui/src/config.rs`
- `crates/oc-cli/src/handlers/*`
- `crates/oc-core/src/session/runner/compaction.rs`（只动配置消费）
- `crates/oc-server/src/boot.rs`（policy/retention 启动）

**复现/测试**：

- `crates/oc-core/tests/config_consumers.rs`
  - 每个 leaf：设置值 → 观察行为（或 accepted 理由）；
  - managed-policy：provider.use 裁剪 + permission 拒绝；
  - session retention：by_session 逐出、session.deleted 清缓存；
  - compaction auto:false / keep.tokens / buffer。
- `crates/oc-tui/tests/config_consumers.rs`
  - mini 8 键、thinking/verbosity/tps/markdown/scrollbar/grouping、new_location。
- `crates/oc-cli/tests/cli_config.rs`
  - --print-logs 真的输出；run JSON 早期错误；cli.json plugins；9 个 env；hostname。

**Exit**：

- `tools/guards/config_consumers.py` 基线归零（或全部 allowlist 有 ledger ID + 理由）。
- env inventory diff 无“新增未认领”。
- retention 长跑内存有界。
- managed-policy 两个执行面都有测试。

### 4.7 C6 `TUI` — 主 TUI 交互面

**目标**：表单/权限不再假死；会话路由、终端面板、slash、tab、复制、cost/live、Zed 等日常交互可用；重连不丢状态。

**Ledger 种子**：

- S0：`14:TD-16`（form.created 无 frame/press 分支）、`TD-17`（FieldKind 缺 boolean/number/integer，整表单丢）。
- S1：`TD-18`（权限两段机错误）、`TD-19`（edit diff 预览缺失）、`TD-01`（六类 wire 消息被丢）、`TD-02`（消息导航 5 条死）、`TD-03`（session.parent 缺失）、`TD-05`（挂载预算/历史分页）、`TD-07`（队列 undo/steer 语义）。
- S1：`06:#3`–`#17`（垂直 tab、TerminalPane、open.menu、会话列表、过滤、DialogSkill/Stash/Image、slash、live/cost、shell !、Zed、选区复制、mini inspector）；`TD-25`（垂直 rail 搜索按钮 + 半行边框，属 #3 子项）。
- 关联：`13:C-8` 的 TUI 逐出在 ledger 中 canonical 放 CONFIG，本簇只消费 `session.deleted` 清 tab 缓存（`TD-23` 是它的 TUI 表现别名）。
- S2：`14` 的 AssistantRetry/BackgroundToolHint/TurnTokenUsage/toggles/rename/copy/queue dock/new_location/eviction/Experiments/permission 展示行/表单深度。

**写 scope**：

- `crates/oc-tui/src/app.rs`（**单一写者**）
- `crates/oc-tui/src/store.rs`（**单一写者**）
- `crates/oc-tui/src/components/footer/{form,permission}.rs`
- `crates/oc-tui/src/components/session_screen.rs`
- `crates/oc-tui/src/terminal_pane*`（若存在）
- `crates/oc-tui/src/components/command_palette.rs`
- `crates/oc-tui-core/*`（仅交互原语）
- 只读依赖：`crates/oc-schema/src/form.rs`

**复现/测试**：

- `crates/oc-tui/tests/pty_form.rs`
  - form.created → footer/全屏可见；
  - 字段类型 boolean/number/integer 正确反序列化；
  - 键位接管、composer 禁用；
  - reply/cancel 后恢复；
  - 重连后表单补回。
- `crates/oc-tui/tests/pty_permission.rs`
  - always 不多一次确认；esc 语义；自由文本 reject 只对当前会话。
- `crates/oc-tui/tests/pty_session_route.rs`
  - `up` 回父会话；六类 wire 消息不丢；过滤 6 项语义。
- `crates/oc-tui/tests/pty_terminal_pane.rs`
  - `<leader>t/down/up`；内嵌终端输出/输入/resize。
- `crates/oc-tui/tests/pty_daily.rs`
  - 7 个 slash、垂直 tab、选区复制、cost/live、shell !、Zed、DialogSkill/Stash/Image。

**Exit**：

- TD-16/17 的 PTY 测试在真实 app 路径通过，不是 golden-only。
- 表单期间 composer 不能输入；reply/cancel 后恢复。
- 权限 always/esc/reject 与 v2 一致。
- 断线重连权限/表单不丢。
- 5 个核心 TUI flow 进入 §8 的门禁。

### 4.8 C7 `AI` — provider/传输/超时

**目标**：协议路由矩阵正确；WS/Azure/Cohere/Mistral/Copilot responses/Bedrock 凭证链/timeout 全部接线；不是“代码存在但运行时不可达”。

**Ledger 种子**：

- S1：`03:M1`（WS + OpenResponses 续链）、`M2`（媒体执行层）、`M3`（google-interactions）、`M4`（google-speech TTS）、`M5`（xai-responses）、`M10`（Bedrock SigV4 默认凭证链）、`B2`（mantle store/include）、Azure 错线、Cohere/Mistral 未接线、`03:5.1` + `04:B-6`（header/chunk/total timeout）、Vertex messages URL/鉴权（`03:V2`，16 已实锤：无 base 时静默落 `api.anthropic.com` + ADC bearer → 401）。
- S1：`16:C1`（Copilot responses lane 21/34 模型打错端点）、`C2`（reasoning_opaque 多轮续链）、`16` 的 `X-Interaction-Type/Id`、Claude-on-Copilot `/v1/messages`、utility title、providerOptions/User-Agent、prompt_cache_key；`04:P-06`（Copilot 动态 /models 同步）。
- S2/设计：`04:P-12`（npm provider 运行时安装，需 ADR-0006）、M6/M7/M8/M9、variant overlay 与 C1 共享部分。

**写 scope**：

- `crates/oc-ai/src/protocols/*`
- `crates/oc-ai/src/route/*`
- `crates/oc-ai/src/media/*`
- `crates/oc-core/src/engine/resolver.rs`（只读/协作，与 C1 串行）
- `crates/oc-core/src/provider_catalog/{facade,variant}.rs`（与 C1 串行）

**复现/测试**：

- `crates/oc-ai/tests/transport_matrix.rs`
  - 每个 provider/protocol → endpoint/auth/headers；
  - WS vs HTTP；
  - 超时（header/chunk/total）用本地 stall server。
- `crates/oc-ai/tests/azure.rs` / `cohere_mistral.rs` / `copilot_responses.rs`
  - URL/`api-version`/`api-key`；runtime reachable；reasoning 续链。
- `crates/oc-ai/tests/bedrock_credentials.rs`
  - Static/Env/profile/SSO/IMDS 链的优先级。
- `crates/oc-ai/tests/timeouts.rs`
  - 挂起响应头/停滞 chunk 在预算内报错，不无限等待。

**Exit**：

- 9 个 aisdk shim 不再是 `UnsupportedProtocol`（或有 accepted ADR-0006）。
- Copilot responses 模型走对端点；Copilot `/models` 动态同步可用。
- Azure/Cohere/Mistral 至少一组真实 fixture；若没有用户批准的 live window，则必须是 cassette + ADR 明确“runtime reachability deferred”，不能口头声称已验证。
- 超时语义与 v2 默认 300s 一致，且有测试。
- prompt_cache_key 在支持的 provider 上发送。

**Live fixture window（Batch 0 定义）**：用户预先批准一组 endpoint/credential sandbox + 可运行时段；只有该窗口内允许 fixer/verifier 发外网请求；窗口外一律 cassette。每次 live 运行记录 endpoint、时间、结果，不记录凭据。

### 4.9 C8 `DB` — 身份/生命周期/迁移

**目标**：与 v2 共享/导入数据库时 project id、迁移、session 内容、worktree 生命周期一致；真实 v2 库能接管并继续。

**Ledger 种子**：

- S1：`09:D1`（project id SHA-256 vs SHA-1）、`D2`（Project.activate 缺失）、`D3`（worktree.resolved 无生产者）、`D4`（Project.update 忽略 canonical/commands）、`D5`（worktree.remove 只删 DB 行；**与 WIRE 的 `VCS-4` 同一条，canonical 在 WIRE，DB 只配合行/迁移**）、`D6`（worktree.refresh 空桩）、`D12`（v1 内容迁移未移植）。
- S2：`D7`–`D22`（排序、KV.scan、Repository、Workspace、announce、事务、TTL、路径规范化等）。

**写 scope**：

- `crates/oc-core/src/database/*`
- `crates/oc-core/src/session/projector.rs`
- `crates/oc-core/src/project*`
- `crates/oc-server/src/handlers/project.rs`
- `tools/parity/tests/p10_db_interop.rs` / 新 `p10_db_import.rs`

**复现/测试**：

- `crates/oc-core/src/database/tests/*`
  - SHA-1 project id 与 v2 一致；
  - activate/time_active；
  - worktree remove/refresh 真跑 git；
  - v1 内容迁移；
  - 事务/排序/路径。
- `tools/parity/tests/p10_db_import.rs`
  - 真实 v2 库副本导入；project id 不产生双行；旧会话可继续；
  - 迁移 journal 0 rerun；
  - 表内容/迁移版本对比。

**Exit**：

- 真实 v2 库导入 + 继续会话通过（release gate）。
- project id 不再双行。
- worktree.remove 真删 git worktree，脏树/force 语义正确。
- v1 内容迁移有测试，不再是“状态路由恒 completed”。

### 4.10 C9 `SURFACES` — 缺失表面

**目标**：补主程序 P0 表面和 P1 生态；平台矩阵至少多一个 CI 目标。不要拉回 desktop/web/gui-extensions。

**Ledger 种子**：

- P0：`10` 的 `plugin-browser` agent `browser` 工具；TUI 内建 LaTeX + Mermaid 渲染。
- P1：npm SDK、VS Code 扩展、GitHub Action、npm/AUR/brew 安装、macOS/Windows/aarch64 二进制；`07:K-11` 安装脚本的 target matrix/latest probe/opencode2 shim。
- P2/P3：containers/simulation/storybook/http-recorder/codegen/script/drizzle/nix/identity；console/enterprise/stats/posts/www。

**Exit**：

- browser 工具注册 + 可调用；
- LaTeX/Mermaid 在 TUI 中渲染，不再占位；
- 至少一个非 linux-x64 的 CI 构建目标；
- 生态项按用户排期，不阻塞主流程退出。

### 4.11 C10 `TOOLS-EXEC` — 工具/shell 执行运行时

**目标**：shell stdout/stderr 并发；timeout TERM→KILL；shell hooks/config；session_move；tool output/metadata；progress 事件。

**Ledger 种子**：

- S1：`02:C1`（stdout/stderr 顺序读，stderr>64KB 互锁）、`C2`（shell.create.before 不派发）、`C3`（config shell 键被忽略）、`C4`（portable_shell_scanner 硬编码）、`C5`（subagent_depth 硬编码）、`A1`（opencode namespace 清空）、`A2`（session_move 恒失败）。
- S2：`C6`–`C13`、`D1`（结构化 output 降级）、`D2`（tool_output config）、`D4`（fs.find 朴素子串）。

**写 scope**：

- `crates/oc-core/src/shell.rs`
- `crates/oc-core/src/shell/*`
- `crates/oc-core/src/tool/*`
- `crates/oc-core/src/hooks.rs`（与 C2 串行）
- `crates/oc-core/src/session/runner/tool*`（只动调用点）

**复现/测试**：

- `crates/oc-core/tests/shell_streams.rs`
  - 交错 stdout/stderr 顺序；
  - stderr >64KB 不互锁；
  - timeout TERM→3s→KILL；
  - 进程组孤儿清理。
- `crates/oc-core/tests/tool_runtime.rs`
  - shell.create.before 生产派发；
  - config shell 生效；
  - session_move 真移动；
  - namespace/广告名正确；
  - output/metadata 不丢。

**Exit**：

- shell 双流并发测试通过；
- timeout 不误报、不挂死；
- shell hooks/config 生产生效；
- session_move 可用。

### 4.12 子 lane 拆分（大簇不要一个 agent 硬扛）

一个“根因簇”不等于一个 agent 从头做到尾。簇内可以拆 sub-lane，但必须满足：

- 每个 sub-lane 一个 owner、一个写 scope；
- 同一 hot file 仍然只有一个写者；
- merge 仍按 cluster 计，不按 sub-lane 计；
- sub-lane 之间不共享未提交的中间状态。

| Cluster | Sub-lane | 主要内容 | 可并行条件 |
|---|---|---|---|
| C2 PLUGIN | C2a core | hooks dispatch + KV DB + RPC | 不碰 `plugin_ui.rs`/`host.js` UI 段 |
| C2 PLUGIN | C2b mcp-skills | MCP registry/OAuth + skills | 不碰 `hooks.rs` |
| C2 PLUGIN | C2c tui-slot | slot/router/keymap + `/mcps` 可见 | 等 C6a 的 `app.rs`/`session_screen.rs` mount seam + C2b registry 合并 |
| C3 RUNTIME | C3a wake-recovery | S-01 + L-01 + jobs/boot | 不碰 TUI data |
| C3 RUNTIME | C3b reconnect-hooks | TD-04 + P-01/P-02 | 不碰 `session/store.rs` |
| C4 WIRE | C4a http-contract | H0–H6 + 请求校验生成器 | 不碰 PTY/VCS |
| C4 WIRE | C4b pty | F1–F4 + EV-1 | 不碰 session handler |
| C4 WIRE | C4c vcs | VCS-1–4 + worktree.remove | 与 C8a 串行 `project.rs` |
| C4 WIRE | C4d events | EV/15 payload + 公共流 | 不碰 handler body |
| C5 CONFIG | C5a keys-env | dead key + env + cli.json | 不碰 policy |
| C5 CONFIG | C5b policy-retention | C-7/C-8/C-1 | 不碰 TUI store 的 retention 消费点 |
| C6 TUI | C6a form-permission | TD-16/17/18/19 | `app.rs`/`store.rs` 单写者 |
| C6 TUI | C6b session-route | TD-01/02/03/05/07 | 等 C6a 释放 `app.rs` |
| C6 TUI | C6c daily | #3–#17 | 不与 C6a/C6b 同文件 |
| C7 AI | C7a transport-timeout | WS + 5.1 + hooks 接线 | 不碰 provider catalog |
| C7 AI | C7b providers | Azure/Cohere/Mistral/Copilot/Bedrock | 与 C1 串行 `facade.rs`/`resolver.rs` |
| C7 AI | C7c media | M2/M3/M4/M5 | 独立 `media/` |
| C8 DB | C8a identity-worktree | D1–D6 + 真实库导入 | 与 C4c 串行 `project.rs` |
| C8 DB | C8b v1-migration | D12 | 不碰 identity |
| C10 TOOLS | C10a shell | C1–C5 | 不碰 registry |
| C10 TOOLS | C10b registry | A1/A2 + metadata | 不碰 shell |

---

## 5. 批次计划

> 每个 Batch 最多 5 个根因簇合并；一个 cluster 一个写者；合并前跑 affected tests + 最小门禁。

### Batch 0 — 先建立可运营的闭环（不修产品 bug）

| ID | 任务 | 产出 | 验收 |
|---|---|---|---|
| B0-1 | 台账导入 | `docs/parity-ledger.jsonl` + `tools/parity-ledger.py` | `validate --paths` 0 错；`summary` 计数可解释；S0/S1 有 repro/test 或 needs-runtime |
| B0-2 | 去重 | fingerprint + duplicate 映射 | 约 40 条同根因合并；canonical 唯一 |
| B0-3 | 门禁基线 | `docs/parity-baseline.md` + 5 个 gate 测试骨架 | 每个 gate 可运行；known-red 都有 ledger ID |
| B0-4 | 静态守卫 | `tools/guards/*` + CI job | 8 个 guard 可运行；allowlist 有 ledger ID/owner/expiry |
| B0-5 | ADR 骨架 | `docs/adr/0001..0004` | 永久分歧/范围有书面决定 |
| B0-6 | lane 模板 | 本文件 §10 + prompt 模板 | 每个 lane 有 scope/deny/evidence 模板 |
| B0-7 | 测试隔离债 | 固定 8080 的 skip-on-busy 语义、root-owned `auth_e2e.rs`、legacy `pkill` 脚本 | 每个风险有 ledger ID/owner/到期；不阻塞主流程 |
| B0-8 | 双端 gate 运行环境 | lane 可执行 bun + 预装 ref checkout + Gate 0 预检 | `OC_GATE_STRICT=1` 下参考侧缺失必须红；证据含 ref 执行日志 |
| B0-9 | live fixture 窗口 | endpoint/credential sandbox 白名单 + 运行时段 | 窗口外禁网；窗口内可复现；凭据不落盘 |

**Batch 0 退出**：不改变产品行为；台账、门禁、守卫、ADR 全部可运行。未完成不得进入 Batch 1。

### Batch 1A — S0 主流程（3 簇并行）

| Cluster | 内容 | 依赖 | 合并门 |
|---|---|---|---|
| C1 CRED | B-1/B-2/B-3 | 无 | 登录→resolver→请求头；console/zen |
| C2 PLUGIN | D-21/D-18/D-33/D-35 + TUI slot | TUI slot 等 C6a 先落 `app.rs`/`session_screen.rs` 的 mount seam；其余可并行 | hook/KV/MCP/skills/UI/RPC |
| C6a TUI-FORM | TD-16/TD-17 | 无 | 表单可见/可答/composer 禁用/重连 |

### Batch 1B — S0 恢复与线格式（5 簇；C4b 可滑到 Batch 2）

| Cluster | 内容 | 依赖 | 合并门 |
|---|---|---|---|
| C3 RUNTIME | S-01/L-01/P-01/P-02/TD-04 | 1A 的 hook/session seam | 后台唤醒；kill -9；resync |
| C4a WIRE-PROTOCOL | H1–H6/F1–F4/VCS/EV-1 | B0 门禁 | cursor；PTY/VCS schema；事件形状 |
| C4b WIRE-VALIDATION | H0（141 路由校验生成器） | C4a + openapi-gen spike + oracle 评审 | 生成式校验表 + 矩阵测试；可滑到 Batch 2，不阻塞其它 |
| C10 TOOLS-EXEC | C1–C5/A1/A2 | 1A 的 hooks seam | 双流/timeout/hooks/session_move |
| C7a AI-TIMEOUT | 5.1/B-6 + stall server | 无 | header/chunk/total timeout |

### Batch 2 — P1 用户日常（≤5 簇）

| Cluster | 内容 |
|---|---|
| C6b TUI-DAILY | #3–#17、TD-01/02/03/05/07/18/19、14 深扫项 |
| C7b AI-PROVIDERS | M1–M5/M10、Azure、Cohere/Mistral、Copilot C1/C2、Bedrock |
| C5 CONFIG | C-7/C-8/C-1/T-1/T-3/T-4、N-1–N-4/K-1、dead keys |
| C9a SURFACES | browser、LaTeX/Mermaid |
| C4b WIRE-DETAIL | 12/15 的 S2 字段、event payload 细节 |

### Batch 3 — P2 一致性与外围（≤5 簇）

| Cluster | 内容 |
|---|---|
| C8 DB | D1–D6/D12、D7–D22、真实 v2 库 |
| C2b PLUGIN-RESIDUAL | 热重载、MCP OAuth/pin、guarded id、集成 provider |
| C4c EVENT/TOOL/CORE 细节 | event payload、tool metadata、截断/清理、snapshot/reference/image、vcs info/status、websearch、codemode、workspace/environment 缝（11:#5–#14） |
| C9b PLATFORM | 多平台 CI/二进制（至少一个非 linux-x64） |
| C5b POLICY/RETENTION | managed-policy 企业面、session retention 长跑 |

### Batch 4 — P3 生态（按用户排期）

- npm SDK、VS Code、GitHub Action、安装渠道。
- console/enterprise/stats/posts/www。
- desktop/web/gui-extensions 保持 out-of-scope。

### 5.1 合并切片

- Batch 1A / 1B 的簇在 batch window 内**串行 merge**（一次一个 cluster），每个 merge 后跑测试；不是一把 merge 多个分支。
- 每簇一个分支/ worktree；integrator 串行 merge。
- merge 后立刻跑：affected crate tests + `p10_http_contract`（或对应 gate）+ fmt/clippy。
- 红则回退该 merge，不继续叠下一个。

### 5.2 风险与缓解

| 风险 | 症状 | 缓解 |
|---|---|---|
| 门禁恢复暴露大量失败 | 第一批 gate 全红，无法推进 | B0 只建骨架 + known-red；按 cluster 分批清零，不要求一次全绿 |
| 双端 gate 自跳过 | bun 不可执行/ref 未装，测试 pass 但没跑参考侧 | Gate 0 + `OC_GATE_STRICT=1` fail-not-skip + B0-8；证据含 ref 启动日志 |
| 真实插件/参考仓库/bun 不可用 | p10/TUI gate skip，误以为绿 | gate 输出必须区分 `pass/skip/fail`；release gate 要求真实环境；fixture 只做日常回归 |
| H0 生成器过大 | 139 路由校验拖垮 Batch 1B | 先 spike：生成表 + 2 条路由；oracle 评审后再全量 |
| `app.rs`/`store.rs` 单写者瓶颈 | TUI 子 lane 排队 | C6a/C6b/C6c 串行 hot file；其它 cluster 并行 |
| 台账导入 386 行耗上下文 | L0 变成“读报告马拉松” | 脚本解析表格 + 人工抽查；不把报告塞进 agent 上下文 |
| 上游 v2.0.25 漂移 | 修着修着混版本 | ledger `upstream` 固定 2.0.24；v2.0.25 单独 diff 分支 |
| 真实 v2 库隐私/损坏 | 测试写真实数据 | 只复制到 sandbox；原库只读；导入测试前后校验 hash |
| 长跑/并发 flake | gate 红但不稳定 | 先重复 3 次分类；flaky 必须 ledger 化，不用 retry 掩盖 |
| 范围膨胀 | 顺手修 S2/S3 | §13 禁止；S1 未清不追文案 |

---

## 6. 每轮循环协议

### 6.1 选活

```text
ready = status ∈ {open, reproduced}
        AND deps satisfied
        AND (repro 存在 OR severity=S0 且已有运行时复现计划)
        AND cluster 没有活跃 writer
sort by severity (S0 > S1 > S2), cluster priority, created
```

WIP 限制：

- 1 个 writer / cluster；
- 最多 3 个活跃 cluster；
- 最多 5 个 cluster / merge batch；
- `needs-runtime` 条目集中到运行时窗口，不在每个 iteration 零散跑。

Definition of Ready（缺一不进 `fixing`）：

| 项 | 要求 |
|---|---|
| repro | 可执行命令或明确运行时步骤；当前会失败 |
| expected / actual | 各有 `ref` / `rust` 的 `path:line` |
| test | 已命名的回归测试文件 + case |
| scope | 写路径、hot file 锁、依赖已确认 |
| duplicate | 不是已合并的重复条目 |
| runtime | 跨进程/并发/重启类有运行时计划，不接受纯静态判断 |

### 6.2 Claim

1. 在 ledger 把条目设为 `fixing`，写 `owner`、`branch`、`worktree`。
2. 建 worktree：`.worktrees/lane-<cluster>`，分支 `lane/<cluster>`。
3. 声明写 scope；检查 hot file 锁（§10.3）。
4. 记录 baseline：`git log -1`、`git status`、相关测试现状。

### 6.3 复现（没有 repro 不许修）

- 先写测试或最小复现脚本，确认它按 `actual` 失败。
- 记录：命令、退出码、关键输出、环境。
- 如果复现不了：
  - 标记 `needs-runtime`，写清运行时计划和需要什么环境；
  - 或标记 `duplicate` / `accepted`（附理由）；
  - **不要“照着报告改代码”**。

### 6.4 根因

- 在簇内找到共享机制；不要按 ID 打补丁。
- 如果发现新根因：
  - 新增 ledger 条目 + fingerprint；
  - 若属于同簇，纳入本簇；
  - 若跨簇，停下，交给 orchestrator 重新分 lane；
  - 不静默扩 scope。

### 6.5 实现

- 最小改动；优先补生产调用点，不重构高保真核心。
- 不做“为了更合理”的 hash/ID/时间/optional 改动；只有 ledger 有 ref 证据且带迁移方案时才改。
- 不添加 test-only 调用点；生产路径必须真实走到。
- 生成文件（`routes.gen.rs`、client types）用生成器，不手改。

### 6.6 接线清单

逐项填 §7；不适用的写 `n/a + 原因`。任何一项为空，不得 `fixed`。

### 6.7 自测

- affected crate tests；
- fmt + clippy（touched crates）；
- 对应最小门禁子集；
- 如果是跨进程/重启/并发类，必须跑运行时测试，不接受“静态看起来对”。

### 6.8 提交

- commit message 含 ledger ID（§3.5）。
- 一个 commit 只做一个逻辑改动；不要把多个簇混在一起。
- push 到 lane 分支；不 force push。

### 6.9 独立验证

- verifier 用**新会话**，从 committed HEAD 开始；
- 重跑 repro + test + 门禁；
- 检查生产调用点（grep 证明）；
- 检查 wiring 清单；
- 通过 → `verified`；不通过 → 打回 `open`/`fixing`，附失败输出。
- verifier 不改代码；发现问题交回 fixer。

### 6.10 合并

- integrator 串行 merge；
- merge 后跑 affected tests + 门禁；
- 更新 ledger 的 `merged_at` / `merge_commit`（`python3 tools/parity-ledger.py merge --id <ID> --merge-commit <sha>`）；
- 删除 worktree（确认无未提交改动）。

### 6.11 一轮结束的产出

- 1–5 个 `verified` 条目；
- 0 个 phantom commit；
- 更新的 burn-down；
- 没有未解释的 known-red 新增；
- 下一轮 `next` 列表。

**时间盒**：一个条目超过 2 个 45 分钟前台块仍没有可执行 repro，或修复后仍无法稳定验证，标记 `needs-runtime` / `blocked`，写清卡点，释放 worktree，交给下一轮；不要在一个条目上无限钻。

---

## 7. Definition of Done：接线完成清单

### 7.1 通用清单（每条都要打勾或写 n/a + 原因）

| # | 项 | 证据 |
|---|---|---|
| 1 | 实现代码 | diff + 单元测试 |
| 2 | 生产调用点（不是测试专用） | `rg` 输出 / 调用链 |
| 3 | config / env / cli.json 消费者 | 设置值 → 行为测试 |
| 4 | 事件发布 + 订阅方 | publish + consume 测试 |
| 5 | DB 读/写/迁移 | 表/行/迁移测试 |
| 6 | 错误、超时、取消路径 | 失败路径测试 |
| 7 | 跨进程/重启行为 | kill -9 / restart 测试 |
| 8 | TUI/CLI/API 表面 | PTY / CLI / HTTP 测试 |
| 9 | 回归测试 / 双端差分 | test 文件 + 失败前/通过后 |
| 10 | parity ledger 证据 | commit + commands + counts |

### 7.2 簇专项清单

#### CRED

- [ ] 登录方法写入 credential store；
- [ ] resolver 读取 active credential；
- [ ] 请求带 key/header/baseURL；
- [ ] console/zen 特殊路径；
- [ ] `/api/model` availability；
- [ ] credential switch 使缓存失效；
- [ ] 401/refresh/error 路径；
- [ ] fake provider 请求头断言。

#### PLUGIN

- [ ] guest hook 注册进 `HookRegistry`；
- [ ] 生产 dispatch 调用方存在；
- [ ] KV 落 DB，跨重启可读；
- [ ] KV 前缀/大小/并发/失败回滚；
- [ ] 插件 MCP 进真实 registry + `/mcps`；
- [ ] 插件 skills 进引擎技能表；
- [ ] TUI slot/router/keymap 真实渲染/交互；
- [ ] `/api/rpc` 真实分发；
- [ ] crash/restart/hot reload；
- [ ] 真实 omo-slim 或 fixture 旗舰测试。

#### RUNTIME

- [ ] completion 后 `execution.wake(parent)`；
- [ ] 父会话空闲时消费，不需要用户再发消息；
- [ ] boot sweep 调 `list_suspended` / `release_child_claims` / `pending_background`；
- [ ] `resume_attempts` 预算；
- [ ] `kill -9` 后恢复；
- [ ] SSE/WS reconnect resync（permission/form/children）；
- [ ] prompt hook/skill 生产路径。

#### WIRE

- [ ] 141 路由请求校验（或逐条 allowlist）；
- [ ] 响应 schema 校验；
- [ ] cursor absent 不输出 null；
- [ ] message cursor/order 规则；
- [ ] PTY meta binary/64KiB/close code；
- [ ] VCS 三端点 schema；
- [ ] worktree.remove 真跑 git；
- [ ] event payload round-trip / 公共流过滤。

#### CONFIG

- [ ] 每个 leaf 有消费者或 accepted 理由；
- [ ] env inventory 无新增未认领；
- [ ] cli.json 读/写/迁移；
- [ ] managed-policy 两个执行面；
- [ ] session retention 内存有界；
- [ ] compaction 配置生效；
- [ ] upgrade 方法/通道按 v2 语义；
- [ ] 迁移来的 history/frecency/stash 有运行时读写。

#### TUI

- [ ] form.created 可见/可答；
- [ ] boolean/number/integer 字段；
- [ ] composer 禁用/恢复；
- [ ] permission always/esc/reject；
- [ ] session.parent / wire 六类消息；
- [ ] terminal pane；
- [ ] slash/tab/copy/cost/live/Zed；
- [ ] reconnect 不丢 permission/form。

#### AI

- [ ] provider/protocol 路由矩阵；
- [ ] WS transport；
- [ ] Azure URL/header/api-key；
- [ ] Cohere/Mistral runtime reachable；
- [ ] Copilot responses/reasoning；
- [ ] Bedrock 凭证链；
- [ ] header/chunk/total timeout；
- [ ] usage/cost/prompt_cache_key。

#### DB

- [ ] SHA-1 project id；v2 导入不双行；
- [ ] Project.activate；
- [ ] worktree remove/refresh 真 git；
- [ ] Project.update canonical/commands/事件；
- [ ] v1 内容迁移；
- [ ] 迁移 journal 0 rerun；
- [ ] 真实 v2 库副本导入。

#### SURFACES

- [ ] browser 工具注册/调用；
- [ ] LaTeX/Mermaid 渲染；
- [ ] 非 linux-x64 CI；
- [ ] 生态项按排期。

#### TOOLS-EXEC

- [ ] stdout/stderr 并发；
- [ ] timeout TERM→KILL；
- [ ] shell.create.before；
- [ ] config shell；
- [ ] session_move；
- [ ] output/metadata/progress。

---

## 8. 验证体系

### 8.1 测试层级

| 层级 | 目的 | 频率 | 例子 |
|---|---|---|---|
| 单元 | 纯逻辑/边界 | 每次 commit | resolver、schema、cursor |
| Crate 集成 | 真实 handler/store | 每次 iteration | `oc-server/tests/*` |
| 双端差分 | 与 v2 同请求/同操作 | 每批 merge / release | 五组 gate |
| 运行时/soak | 并发/重启/断线/长跑 | release | 30min、kill -9、reconnect |
| 平台 | OS/架构 | release | CI matrix |

### 8.2 默认测试纪律

- 默认测试不依赖网络、bun、真实凭据、tmux；需要的用 opt-in gate。
- 端口 `127.0.0.1:0`；永不 49374。
- `HOME`/`XDG_*`/`TMPDIR` sandbox。
- 确定性：FakeLlm、固定 clock/id 能力、归一化只允许已登记规则。
- 失败重试不能掩盖 flaky；flaky 必须 ledger 化（`test_status: flaky`），不能静默 `#[ignore]`。

### 8.3 最小双端差分门禁（只恢复 5 组，不恢复全量 rig）

> 2026-10-17 的“差分测试退役”裁定仍然适用于 26-script corpus、sweeps、termatrix profiles、ref-cache、verify-lane rig phases。本文件只恢复 **5 组高价值、低维护** 的门禁，作为“深”验证；静态守卫做“广”。

#### Gate 0 — 环境预检（不通过就红，不允许 self-skip）

这是所有双端 gate 的前置条件，也是本计划最大的“假绿”风险：

- lane 用户（eternal130）必须能执行 bun；不能只依赖 `/root/.bun/bin/bun`。`tools/dev-gate.sh` 的 PATH 要包含 lane bun，或 gate 命令显式设 `PARITY_BUN`。
- 参考 checkout 必须已安装到可启动状态（`/tmp/opencode/ref` 或 `PARITY_REF_ROOT`，含 `node_modules`）；`/opt/opencode-ref` 必须保持只读。
- Rust server 和参考 server 都能在 sandbox 中启动、`GET /api/info` 就绪、干净退出。
- **batch-merge / release 模式下，参考侧不可用必须 fail，不得 skip**。日常本地可以用 `OC_GATE_ALLOW_SKIP=1` 允许 skip，但 skip 结果不能进入 `verified` 证据。
- 每次 gate 运行必须记录：`gate_ran: true`、`ref_commit`、`bun_path`、`reference_boot: ok`、`rust_boot: ok`。
- 证据包必须包含“参考侧真的执行过”的日志；只有 Rust 侧输出 = 未验证。

Batch 0 的 B0-8 负责这一项；**Gate 0 不通过，Batch 0 不得退出，后面五组 gate 也不得报绿。**

#### Gate 1 — HTTP/SSE 契约

- 文件：`tools/parity/tests/p10_http_contract.rs`
- 复用：`src/httpdiff.rs`、`src/scenario.rs`、`src/sse.rs`、`tests/scenarios.rs`、`tests/rust_scenarios.rs`
- 覆盖：
  - session/message 分页 cursor（absent vs null、order 规则）；
  - 错误信封；
  - 请求校验矩阵（141 路由）；
  - 事件 payload（permission/form/pty/shell/execution）；
  - 公共 SSE 过滤。
- 断言：同请求双端比 status/header/body（JSON 归一）；事件序列按 canonical grouping 比。

#### Gate 2 — DB

- 文件：`tools/parity/tests/p10_db_interop.rs` + 新 `p10_db_import.rs`
- 覆盖：
  - v2 写 NULL-heavy 行 → Rust 打开（journal 0 rerun）→ 继续 → v2 再打开；
  - 真实 v2 库副本导入；
  - project id / 迁移版本 / 表内容；
  - worktree remove。
- 断言：表 dump 归一后 diff；project id 不双行；journal 不变。

#### Gate 3 — Tools

- 文件：`tools/parity/tests/p10_tools.rs`
- 覆盖：
  - read/edit/glob/grep/patch 的 content/metadata；
  - shell stdout/stderr/exit/timeout；
  - permission ask；
  - 文件副作用。
- 断言：同输入双端比 content/metadata/权限请求/文件副作用。

#### Gate 4 — TUI 五流程

- 文件：`tools/parity/tests/p10_tui_frames.rs`
- 复用：`src/tui_diff.rs` 的 `PtyChild` / `Grid`；不恢复 26-script corpus。
- 五流程：
  1. 启动/home；
  2. 发消息（FakeLlm 固定回复）；
  3. 工具调用；
  4. 权限/表单；
  5. 会话切换。
- 断言：chrome 区域 EXACT；模型文本 STRUCTURAL；每步 cell diff；固定 80x24 + 120x30。

#### Gate 5 — CLI

- 文件：`tools/parity/tests/p10_cli.rs`
- 覆盖：
  - `--version` / `--help`；
  - `run --format json` 成功/早期错误；
  - `serve --hostname localhost`；
  - `auth list` / `models` 的 TTY/非 TTY 分支；
  - `session list` 分页/本地时间；
  - `--print-logs`。
- 断言：stdout/stderr/exit code/TTY 分支；路径/端口/时间归一。

### 8.4 门禁运行方式

```bash
# 单测 + 集成（每次 iteration）
tools/dev-gate.sh 'cargo nextest run -p oc-server -E "test(parity_)"'

# 全 workspace（batch merge / release）
tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'

# Phase-10 battery（batch merge / release；严格模式禁止 self-skip）
tools/dev-gate.sh 'OC_P10=1 OC_GATE_STRICT=1 cargo nextest run -p parity --locked --run-ignored only -E "test(p10_)"'

# TUI 五流程（batch merge / release）
tools/dev-gate.sh 'OC_P10=1 OC_GATE_STRICT=1 cargo nextest run -p parity --locked --run-ignored only -E "test(p10_tui_frames)"'

# fmt / clippy
tools/dev-gate.sh 'cargo fmt --all -- --check'
tools/dev-gate.sh 'cargo clippy --workspace --all-targets --locked -- -D warnings'
```

Worktree：

```bash
OPENCODE_DEV_GATE_DIR=/opt/opencode-rust/.worktrees/lane-wire \
  tools/dev-gate.sh 'CARGO_TARGET_DIR=/tmp/opencode/target-lane-wire cargo nextest run -p oc-server'
```

### 8.5 known-red 规则

- 门禁第一次恢复时，记录 `known-red.json`：每条 mismatch 必须映射 ledger ID、owner、expiry batch。
- 门禁失败时，新 mismatch 立即红；known-red 只能减少，不能增加。
- **skip 不算 pass**：`OC_GATE_STRICT=1` 下参考侧不可用 / 测试未执行 = fail；只有 `gate_ran: true` 才计入绿色。
- 不允许用 `--skip` / 删除 case / 放宽归一化来变绿；放宽归一化必须改 `NORMALIZATION.md` 并评审。
- known-red 过期未修 → 门禁失败。

### 8.6 运行时/Soak gate（release）

> soak 由 orchestrator 前台分块运行（单次 ≤45 分钟），不要让子 agent 把长跑放后台（host H-1）；结果落到证据包。

- 30 分钟长会话 + 周期 prompt/tool；RSS/FD 无无界增长。
- 断线重连：SSE/WS、permission/form resync。
- `kill -9` 服务器 mid-turn → 重启 → 孤儿 turn 恢复、pending background 补发。
- 真实插件 omo-slim 加载 + 完整交互。
- 真实 v2 库副本导入 + 继续会话。
- 以上全部通过才算 release-ready。

---

## 9. 静态防复发守卫

> 静态守卫不是“再扫一遍”。它们只检查固定模式，防止已修问题复发；比全量扫描便宜，且不会漏。

### 9.1 守卫清单

| Guard | 检查 | 失败条件 | 产物 |
|---|---|---|---|
| `config_consumers` | schema 有键、代码无读取点 | 新增 dead key | `tools/guards/config_consumers.py` |
| `hook_dispatch` | `register_*` 存在、`dispatch_*` 无生产调用 | 新增 registered-but-undispatched | `hook_dispatch.py` |
| `route_validation` | 路由表 vs 校验覆盖 | 覆盖率下降 / 新增未校验路由 | `route_validation.py` |
| `event_schema` | publish 前 schema 校验 + 每事件 round-trip | 新事件无 schema/round-trip | `event_schema.py` |
| `env_inventory` | ref/清单中的 `OPENCODE_*` vs Rust 认领点 | 新增未认领 env | `env_inventory.py` |
| `command_keymap` | 注册命令/键位 vs handler | 注册即死/no-op | `command_keymap.py` |
| `ledger_integrity` | JSONL schema/状态/证据 | `fixed` 无 test/evidence；`verified` 无 verifier | `ledger_integrity.py` |
| `process_safety` | 裸 kill/pkill/killall/fuser；按名字杀 bun/node/cargo/nextest/python；绕过 dev-gate 的 sudo/systemd-run | 新增违规 | `process_safety.py` |

### 9.2 allowlist 规则

`tools/guards/allowlist/<guard>.txt` 每行：

```text
pattern | reason | ledger-id | owner | review-date
```

- 每个 allowlist 条目必须有 ledger ID 和到期 batch。
- 到期未清 → CI 失败。
- 不允许 `*` 通配整类；不允许无 reason。
- 初始基线用 `tools/guards/baseline/<guard>.json` 记录“当前已存在的违规集合”，不是把 139 条未校验路由全部写成 allowlist。守卫的语义是：**基线集合只能缩小；新增违规立即红**。allowlist 只放“明确接受、有 ADR/到期”的例外。

### 9.3 CI

- `.github/workflows/ci.yml` 增加 `guards` job（或在 `lint` 里跑 `tools/guards/run.sh`）。
- `guards` 不依赖 bun/ref/真实数据。
- 新违规 = 红；已有 allowlist 只减不增。
- 每次 Batch 结束更新 baseline 计数。

### 9.4 守卫与 ledger 的关系

- 守卫发现的**新**缺口 → 新 ledger 条目，不直接修。
- 守卫的 allowlist 条目必须指向 ledger 的 `open/needs-runtime`。
- 守卫通过不等于功能完成；它只防复发。

---

## 10. 多 agent 协作纪律

### 10.1 角色分离

- **scanner**：只产出证据和 repro；不修，不标 fixed。
- **fixer**：只修一个 cluster；加回归测试；不标 verified。
- **verifier**：独立复跑；只标 verified/打回；不改代码。
- **integrator**：串行 merge；只解决冲突；不顺手改功能。
- **oracle**：架构/ADR/高风险评审；不写产品代码。

**禁止**：scanner 和 fixer 共用同一个“完成”判断；fixer 自证；verifier 与 owner 同会话。

### 10.2 派活模板（必须包含）

```text
你是 lane-<cluster> 的 fixer。
只处理 ledger 中 cluster=<C> 的条目：<IDs>。
基线：main@<sha>；worktree：<path>；branch：<name>。
写 scope：<paths>。
禁止：<deny list>；禁止 kill/pkill/killall/fuser；禁止 root；禁止真实数据目录；禁止 49374。
先复现：<repro command>；失败输出写入 ledger。
实现后逐项填 §7 接线清单。
验证：<affected tests> + <gate subset> + fmt/clippy。
提交：parity(<cluster>): <ID> ...；输出 git log -1 + 测试计数。
不得标记 verified；由 lane-verify 复核。
不要后台跑长任务；单次命令 ≤45 分钟。
```

### 10.3 Hot file 锁

| 文件 | 默认 owner | 规则 |
|---|---|---|
| `crates/oc-core/src/hooks.rs` | PLUGIN | 1A 先落 dispatch seam；RUNTIME/TOOLS-EXEC 在 1A merge 后串行 |
| `crates/oc-core/src/session/store.rs` | RUNTIME | 1B 先落 claims/turns seam；DB 在 1B merge 后 rebase 串行 |
| `crates/oc-core/src/jobs.rs` | RUNTIME | 单写者 |
| `crates/oc-server/src/handlers/session.rs` | WIRE | 1B 先落 wire seam；RUNTIME 在 1B merge 后串行 |
| `crates/oc-server/src/plugins.rs` | PLUGIN | 单写者 |
| `crates/oc-plugin-host/src/supervisor.rs` | PLUGIN | 单写者 |
| `crates/oc-tui/src/app.rs` | TUI | 单写者 |
| `crates/oc-tui/src/store.rs` | TUI | 单写者 |
| `crates/oc-server/src/routes.gen.rs` | generated | 只由 `openapi-gen` 生成 |
| `Cargo.toml` / `Cargo.lock` | integrator | 功能 lane 不手改 |

如果两个 cluster 必须改同一 hot file：

1. 先做一个最小 seam commit（integrator 或指定 owner）；
2. 再并行；
3. 或者串行，后一个 rebase。

### 10.4 并行规则

- 可并行：不同 crate / 不同文件 / 无数据依赖。
- 不可并行：同一 hot file、同一 DB 表/迁移、同一生成文件、同一测试 gate 的 golden。
- 每批 ≤5 个 cluster；每 cluster 1 writer。
- verifier 可以并行读，但不与 writer 改同一文件。

### 10.5 小批量合并

- 每批≤5 个根因簇。
- merge 前：affected crate test + fmt/clippy + 对应 gate。
- merge 后：workspace test + p10（batch 级）。
- 红则回退，不叠。

### 10.6 上下文管理

- 每个 fixer 只拿自己的 ledger 条目 + cluster 小节；不要把 16 份报告塞进去。
- verifier 用新会话，避免继承 fixer 的假设。
- 同一 cluster 的后续修复可以复用原会话；跨 cluster 不复用。
- 扫描 agent 与修复 agent 不复用会话。

### 10.7 禁止 phantom commit

见 §3.5。额外要求：

- 报告必须附 `git log -1 --format='%H %s'`。
- 报告必须附测试输出/计数，不是“我跑了”。
- 报告必须附 `git status --short`（确认没有未提交改动）。
- 没有证据 = 未完成；不要用“应该可以”结案。

---

## 11. 上游版本策略

### 11.1 固定基线

- 当前所有 ledger 条目 `upstream = "2.0.24"`。
- 参考源码 `/opt/opencode-ref`，commit `e7a34f09bf`。
- 不把 v2.0.25 的行为混进 2.0.24 的修复；否则会出现“部分 0.24、部分 0.25”的混合态。

### 11.2 v2.0.25 diff 分支

- 单独维护 `upstream/v2.0.25-diff` 分支。
- 每个上游 release 做一次分类：

| 类别 | 例子 | 动作 |
|---|---|---|
| 行为/契约变更 | API status/body、事件 payload、路由选择 | 新增 ledger 条目，排期修 |
| schema 变更 | OpenAPI 字段 optionality、config key | 重新生成 + 门禁 |
| 仅内部重构 | 文件移动、性能、命名 | 不追 |
| 新功能 | 2.0.25 新增能力 | 用户排期前不做 |

- 只跟进影响契约的变更；内部重构不追。
- 任何“永久分歧”写成 ADR，不再每轮重吵。

### 11.3 ADR 清单（Batch 0 建立）

- `docs/adr/0001-plugin-process-isolation.md`：bun guest / 进程隔离 / RPC/KV 边界。
- `docs/adr/0002-system-player-audio.md`：声音用系统播放器，非进程内 miniaudio。
- `docs/adr/0003-sdk-embedded-server.md`：SDK 内嵌服务器形态由 CLI lifecycle 承担。
- `docs/adr/0004-scope-desktop-web.md`：desktop/web/gui-extensions out-of-scope。
- `docs/adr/0005-hook-copy-semantics.md`：hook copy-in/copy-out 与 v2 可观察等价。
- 后续按需：npm provider 运行时安装、平台矩阵优先级、managed-policy 企业面。

ADR 模板：

```markdown
# ADR-NNNN: 标题
- 状态：proposed / accepted / superseded
- 日期：
- 上游版本：
- 背景：
- 决定：
- 后果：
- 证据（path:line / 测试）：
- 复审日期：
```

### 11.4 不要做的事

- 不要为了“更合理”改 hash/ID/时间格式/字段 optional；只有 ledger 有 ref 证据且带迁移方案时才改。
- 不要用 v2.0.25 的行为去“修” v2.0.24 的测试。
- 不要把 accepted 当垃圾桶；每个 accepted 必须有 ADR。

---

## 12. 退出条件（怎么判断可以停）

不要用“扫描无新发现”作为完成标准。用下面这些：

### 12.1 缺口

- [ ] S0 = 0，且每个 S0 都有运行时回归测试。
- [ ] S1 burn-down 到 0；剩余全部 `accepted` 且有 ADR。
- [ ] `needs-runtime` 队列清空，或全部有明确运行时结论。
- [ ] `duplicate` 全部指向 canonical；没有孤儿。
- [ ] 不存在超过一个 Batch 未复审的 `blocked`；不存在无 owner heartbeat 的 `fixing`。
- [ ] ledger 中不存在 `fixed` 无 test/evidence、`verified` 无独立 verifier 的条目。

### 12.2 门禁

- [ ] Gate 0 通过，且五组最小差分门禁全绿：HTTP/SSE、DB、Tools、TUI 五流程、CLI。
- [ ] 每次绿色 gate 都有参考侧实际执行的证据（`gate_ran: true` + `ref_commit` + 启动日志），不是 skip。
- [ ] 现有 p10 battery（`tools/parity/goldens/p10/VERIFICATION.md`；当前 6/6，含 stable-channel lane）全绿。
- [ ] workspace nextest 全绿；fmt/clippy `-D warnings` 全绿。
- [ ] 静态守卫无新增违规；allowlist 到期项清零。
- [ ] known-red 清零或有 ADR。

### 12.3 运行时

- [ ] 真实插件（omo-slim）能加载并完成一次完整交互。
- [ ] 真实 v2 库能导入并继续会话。
- [ ] 30 分钟长会话 + 断线重连 + `kill -9` 恢复通过。
- [ ] 表单/权限在重连后不丢。
- [ ] 后台 subagent 完成能唤醒父会话。

### 12.4 平台/发布

- [ ] linux-x64 之外的平台至少有一个 CI 构建目标。
- [ ] release smoke 通过（`tools/release-smoke.sh` 或等价）。
- [ ] 每个 `verified` 条目都有 `merge_commit`，且该 commit 在 `main` 可达历史里。
- [ ] 没有 phantom commit；证据包完整。
- [ ] README/ROADMAP 的范围声明与实际一致（Web UI 不再写“不在范围”）。

### 12.5 一句话

**当“每个已知缺口都有契约、每个修复都有回归、每个完成都有独立证据”时，才停。**

退出判定必须跑：

```bash
python3 tools/parity-ledger.py validate --paths --exit
```

0 错才算账面完成；再叠加 §12.2–§12.4 的运行时/平台证据。

### 12.6 保证边界（必须诚实写进 release note）

这套策略保证的是 **“已知差异的处理质量”**，不是 **“差异集合的完备性”**。

| 维度 | 能保证 | 不能保证 |
|---|---|---|
| Soundness | 每个 `fixed/verified` 有 repro、red artifact、test、独立 verifier、merge commit；不会假绿 | — |
| Completeness | 386 条审计记录全部入账、去重、逐条处置 | 审计没发现的差异；真实用户长尾 |
| Contracts | HTTP/SSE、DB、Tools、TUI 五流程、CLI 双端门禁 | 全量 TUI corpus、所有 provider 真实网络、所有插件 |
| Runtime | 30 分钟会话、kill -9、重连、真实旗舰插件、真实 v2 库 | 小时级 soak、网络分区、磁盘满、极端并发 |
| Platform | 至少一个非 linux-x64 CI | Windows ConPTY / macOS 全行为 |
| Details | S0/S1 行为一致；S3 文案/时序显式 `accepted` | 未登记细节的“像素级/毫秒级”完全一致 |

因此 release note 必须写清：

- **verified scope**：哪些契约/流程/平台真的跑过；
- **accepted divergences**：每条对应 ADR；
- **not-covered matrix**：哪些面没验证；
- **residual risk**：owner + 复审日期。

如果要求比“可发布”更强的保证，追加 release gate：

1. Web UI 的浏览器端 E2E（若 Web UI 在范围内）；
2. 第二个平台 CI（Windows 或 macOS）；
3. top-N 真实插件 corpus（不只 omo-slim）；
4. 真实 provider canary 矩阵（live fixture window）；
5. 2–4 小时 soak + 网络分区/磁盘满/极端并发；
6. release-only 的全量 TUI corpus 差分（一次性，不恢复日常 rig）；
7. 真实用户 dogfood 验收。

**结论**：这套策略可以把“一致性”从口号变成可审计、可收敛、可停止的契约；它不能数学证明“所有细节完全等价”。要更强保证，靠的是扩大上面的验证面，而不是再扫一遍。

---

## 13. 明确不要做什么

1. **不要重写已经高保真的核心**：drain/step/retry/compaction/event-sourcing/DB schema/路由表/主题资产。只加 seam 和调用点。
2. **不要在 S1 未清时追 S2 文案**。S3 文案冻结，除非它是某条 S1 测试的一部分。
3. **不要为“更合理”改 hash/ID/时间格式/字段 optional**。只有 ledger 有 ref 证据 + 迁移方案时才改。
4. **不要用静态 grep 代替运行时验证**。`rg` 找不到调用方只能证明“当前树没有”，不能证明功能可用。
5. **不要让扫描 agent 和修复 agent 共用“已完成”判断**。scanner 只产证据；fixer 只修；verifier 独立。
6. **不要把 out-of-scope 的 desktop/web/gui-extensions 反复拉回队列**。
7. **不要恢复全量差分 rig**。只恢复 5 组门禁；26-script corpus/sweeps/termatrix 继续退役。
8. **不要按进程名/裸 PID 杀进程**。见 §2。
9. **不要 root 跑 cargo / 绕过 dev-gate**。见 §2。
10. **不要共享 worktree 写文件 / 在 main 上直接改**。
11. **不要添加 test-only 调用点**。生产路径必须真实走到。
12. **不要用 skip/ignore/删断言/放宽归一化来变绿**。
13. **不要在一个 commit/merge 里混多个簇**。
14. **不要把 accepted 当垃圾桶**。没有 ADR 的 accepted 一律打回。
15. **不要混合上游版本**。2.0.24 的 ledger 只用 2.0.24 证据。
16. **不要在没有 repro 的情况下“照着报告改”**。改对了是运气，改错了是新债务。

---

## 附录 A：Ledger 模板与示例

### A.1 新建条目模板

```json
{
  "id": "<DOMAIN-CODE>",
  "source_id": "<report:ID>",
  "aliases": [],
  "fingerprint": "<domain>|<symptom-code>|<file-or-symbol>",
  "domain": "<http|tui|plugin|ai|db|config|runtime|tool|surface>",
  "cluster": "<CRED|PLUGIN|RUNTIME|WIRE|CONFIG|TUI|AI|DB|SURFACES|TOOLS-EXEC>",
  "severity": "S0|S1|S2|S3|needs-runtime",
  "status": "open",
  "title": "",
  "symptom": "",
  "expected": "",
  "actual": "",
  "impact": "",
  "ref": [],
  "rust": [],
  "repro": "",
  "test": "",
  "owner": "",
  "upstream": "2.0.24",
  "wiring": {
    "implementation": false,
    "production_call_site": false,
    "config_env_consumer": "n/a",
    "event_publish_subscribe": "n/a",
    "db_read_write_migration": "n/a",
    "error_timeout_cancel": "n/a",
    "cross_process_restart": "n/a",
    "tui_cli_api_surface": "n/a",
    "regression_or_differential_test": false,
    "ledger_evidence": false
  },
  "evidence": [],
  "merge_commit": "",
  "merged_at": "",
  "notes": "",
  "created": "2026-10-18",
  "updated": "2026-10-18"
}
```

### A.2 证据追加示例

```json
{
  "commit": "0123456789abcdef0123456789abcdef01234567",
  "branch": "lane/wire",
  "commands": [
    "tools/dev-gate.sh 'cargo nextest run -p oc-server -E \"test(parity_http_contract)\"'"
  ],
  "result": "pass",
  "counts": "18/18 passed",
  "red_artifact": "/tmp/opencode/parity-evidence/HTTP-H2/2026-10-18T11-40-00-red.log",
  "artifacts": ["/tmp/opencode/parity-evidence/HTTP-H2/2026-10-18T12-00-00.log"],
  "verified_by": "lane-verify",
  "verified_at": "2026-10-18T12:10:00Z"
}
```

---

## 附录 B：簇 → 扫描 ID 种子映射

> 当前 `docs/parity-ledger.jsonl` 是 **114 条 canonical 种子**（6 S0 + 106 S1 + 2 S2），由 2026-10-18 的扫描主报告与 01–16 明细生成；L0 负责把 16 份报告全部导入、去重并补齐 S2/S3。下表用于核对“不要漏簇”。

| Cluster | S0 / S0 等价 | S1 种子 | S2 重点 |
|---|---|---|---|
| CRED | 04:B-1, B-2, B-3 | 04:A-1, A-3, A-4, A-5, D-6, D-8, 03:V3 | A-6, A-7, A-8, A-9, D-4, D-5, D-7, D-9, D-10 |
| PLUGIN | 06:#1, 06:#2/08:D-33 | 08:D-21, D-18, D-35, D-14, D-30, D-01, D-07, D-10, D-11, D-13, D-02, D-31, D-34, 01:M-02 | D-03, D-04, D-05, D-15, D-16, D-22, D-23, D-24, D-40 |
| RUNTIME | 01:S-01, 01:L-01 | 01:P-01, P-02, K-01, 14:TD-04 | L-02, L-03, L-05, L-06, P-03, P-04 |
| WIRE | — | 12:H0–H6, 05:F1–F4, 11:#1–#3, 09:D5, 15:EV-1 | 12 S2, 05:F5–F22, 15 S2, 09:D4 |
| CONFIG | — | 13:C-7, C-8, C-1, T-1, T-3, T-4, 07:N-1–N-4, K-1, K-2, K-8 | 13 dead keys/env, 13:C-9/T-2/14:TD-24 (new_location), 07:F-3–F-13 |
| TUI | 14:TD-16, TD-17 | 14:TD-18, TD-19, TD-01, TD-02, TD-03, TD-05, TD-07, 06:#3–#17, TD-25 | 14 深扫 14 项, 06:#18–#34, TD-23 (alias C-8) |
| AI | — | 03:M1–M5, M10, B2, 03:5.1/04:B-6, 03:V2, Azure, Cohere/Mistral, 16:C1/C2, 04:P-06 | M6–M9, 04:P-12 (ADR-0006), 16 细节 |
| DB | — | 09:D1–D6, D12 | D7–D22 |
| SURFACES | 10:P0 browser/LaTeX/Mermaid | 10:P1 npm SDK/VS Code/GitHub Action/安装/平台, 07:K-11 | 10:P2/P3 |
| TOOLS-EXEC | — | 02:C1–C5, A1, A2 | C6–C13, D1/D2/D4 |

---

## 附录 C：Lane Prompt 模板

### C.1 Scanner（只读）

```text
你是 scanner。只读 /opt/opencode-rust 和 /opt/opencode-ref，不写任何文件。
任务：在 <domain/cluster> 内找缺口，输出候选 ledger 条目。
每条必须带：symptom、expected、actual、ref(path:line)、rust(path:line)、repro（可执行或运行时步骤）、建议 test。
静态无法确认的标 needs-runtime，不要强行判 S0/S1。
不要修代码；不要跑 build；不要 kill 任何进程；不要碰真实数据目录/49374。
输出后由 orchestrator 去重、定级、分配。
```

### C.2 Fixer（单簇）

见 §10.2。额外：

```text
先读 docs/PARITY-LOOP.md §2/§3/§6/§7 和你的 cluster 小节。
只改 <scope>；hot file <file> 已由 <owner> 锁定，不要动。
如果发现跨簇根因，停下并报告，不要扩 scope。
```

### C.3 Verifier（独立）

```text
你是 verifier，与 fixer 不同会话。只读 + 跑测试，不改代码。
对象：ledger 条目 <IDs>，commit <sha>。
重跑：<repro> + <test> + <gate>；检查生产调用点、wiring 清单、证据包。
通过：写 verified + evidence；失败：打回 open/fixing，附失败输出。
不要接受“测试通过但生产无调用方”；不要接受 skip/ignore/删断言。
不要 kill 非自己 spawn 的进程；不要碰真实数据目录/49374。
```

### C.4 Integrator

```text
你是 integrator。只做串行 merge，不写功能代码。
一次 merge 一个 cluster：<branch>。
merge 前确认 verifier 已 verified、worktree 干净、commit 在分支上。
merge 后跑 affected tests + <gate> + fmt/clippy；红则回退该 merge。
更新 ledger 的 merged_at/merge_commit；不顺手改功能。
```

---

## 附录 D：命令与路径速查

### D.1 基线

```bash
git -C /opt/opencode-rust log -1 --format='%H %s'
git -C /opt/opencode-rust status --short
git -C /opt/opencode-rust worktree list
```

### D.2 Worktree

```bash
git -C /opt/opencode-rust worktree add .worktrees/lane-wire -b lane/wire main
cd /opt/opencode-rust/.worktrees/lane-wire
OPENCODE_DEV_GATE_DIR=$PWD tools/dev-gate.sh 'CARGO_TARGET_DIR=/tmp/opencode/target-lane-wire cargo nextest run -p oc-server'
```

### D.3 测试

```bash
tools/dev-gate.sh 'cargo nextest run -p oc-server -E "test(parity_)"'
tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'
tools/dev-gate.sh 'OC_P10=1 OC_GATE_STRICT=1 cargo nextest run -p parity --locked --run-ignored only -E "test(p10_)"'
tools/dev-gate.sh 'cargo fmt --all -- --check'
tools/dev-gate.sh 'cargo clippy --workspace --all-targets --locked -- -D warnings'
```

### D.4 只读诊断

```bash
ss -ltnp | grep 49374 || true
ps -eo pid,ppid,pgid,lstart,cmd | grep -E 'oc-server|opencode' | grep -v grep
```

只读；不要把输出管道到 `kill`。

### D.5 证据目录

```bash
/tmp/opencode/parity-evidence/<ledger-id>/<timestamp>.log
/tmp/opencode/target-<lane>/
/tmp/opencode/<lane>-<pid>-<nanos>/
```

---

## 附录 E：前 10 个 iteration 的具体起点

| # | 任务 | 产出 |
|---|---|---|
| 1 | B0-1 台账导入 | `docs/parity-ledger.jsonl` + `tools/parity-ledger.py` |
| 2 | B0-2/3/4 基线、门禁、守卫 | `docs/parity-baseline.md` + `tools/guards/*` |
| 3 | B0-5/6/7 ADR、lane 模板、测试隔离债 | `docs/adr/0001..0005` + 风险 allowlist |
| 4 | CRED B-1 | 登录→resolver→请求头测试 |
| 5 | CRED B-2/B-3 | console/zen 运行时测试 |
| 6 | PLUGIN D-21 | hook 生产 dispatch |
| 7 | PLUGIN D-18/D-33/D-35 | KV/MCP/skills |
| 8 | TUI TD-16/TD-17 | form PTY 测试 |
| 9 | RUNTIME S-01 | 后台唤醒测试 |
| 10 | RUNTIME L-01 | kill -9 恢复测试 |

之后按 §5 的 Batch 1B/2/3 继续。

---

## 附录 F：本文件的维护

- 每次 Batch 结束更新：`版本`、`目标基线`、`known-red` 计数、`allowlist` 到期项。
- 不静默修改 §2/§3.4/§7/§8.3/§12；需要修改时写 ADR + 用户确认。
- 如果某条规则在实战中反复造成摩擦，先记录到 `docs/parity-loop-friction.md`，Batch 结束统一评审；不要在循环中随手放宽。
- 本文件不是审计报告；审计证据仍以 `docs/parity-scan-2026-10-18/` 为准。
