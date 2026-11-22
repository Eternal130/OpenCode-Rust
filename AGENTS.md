# opencode-rust — Agent 工作入口

一句话：opencode v2.0.24（TS 参考实现）的 Rust 全量移植。绝对遵循开发规范——**权威源=本地私有册 `docs/GAPS-AND-PITFALLS.md`（git-ignored，不在克隆内；盘上路径同右）**，`tools/lane-brief.sh --all` 逐字输出其 §2.1-2.5 与速查表。公开分歧登记册=根级 `DIVERGENCES.md`。

## 硬纪律（违反即事故）

- 一切本地 cargo 构建/测试经 `tools/dev-gate.sh`（CPU/内存配额 + 负载闸；exit 3=负载拒启、exit 5=磁盘拒启——立即停，勿重试绕门）
- 禁 plain `cargo test` 作回归证据（canonical harness=nextest）
- 一切测试命令全量 tee 落盘 `.test-logs/<crate>-<时间>-<lane>.log`（唯一测试证据；日志丢失=证据丢失）——dev-gate 已内建自动 tee（lane 标签 `OC_GATE_LANE`；直跑的验证脚本仍需手动 tee）
- 每 lane 独立 worktree（`.worktrees/`）+ 串行合并回 main；禁碰主检出
- git 提交一律仓库 git config 既有身份（`tools/git-identity-hook.sh` 钉扎：作者/提交者必须一致匹配 config 层身份——禁 `--author` / `-c user.*` / `GIT_AUTHOR_*`）
- 在册缺口/豁免/判定权归属等制度见 GAPS §2.2（掩码裁决 SLA、覆盖台账、沙箱禁判定三裁定）

## 测试工具用法

### 1. dev-gate.sh — 构建/测试单入口

```sh
tools/dev-gate.sh 'cargo nextest run -p oc-tui --locked <过滤词>'   # 日常迭代，秒~分钟
```

每条 cargo 命令单独包一次。验证脚本（`tools/diff/*.py`、`tools/parity/*.sh`、`tools/guards/run.sh`）**直接跑、禁包 dev-gate**（锁套锁）。

**自动 tee**：dev-gate 将整条命令的完整输出（stdout+stderr，不截断）自动落盘 `.test-logs/<crates>-<时间>-<OC_GATE_LANE>.log`（`-p` 提取 crate 名、`--workspace`→workspace、默认标签 `gate`）——测试证据以该日志为准，**禁再手动接 tee**（双写）；直跑的验证脚本仍需手动 tee。

### 2. nextest 分层（`.config/nextest.toml`：threads=4、全局 retry=1；pty/be1 家族 retry=2）

| 层 | 命令 | 用途/耗时 |
|---|---|---|
| 渲染常设门 | `tools/dev-gate.sh 'cargo nextest run -p oc-tui --locked dual_engine_matches_frozen_oracle'` | 0.1s 冒烟 |
| 合并门 | `tools/dev-gate.sh 'cargo nextest run -p oc-tui -p oc-cli -p parity --locked --no-fail-fast'` | ~7min |
| 全仓门 | `tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'` | 收口必过 |
| PTY 族 | 上述命令 + `--profile pty` | 串行 test-threads=1，~28min |

首轮不过滤 + `--no-fail-fast`；红测试同轮 `--exact <name> -- --nocapture` 取回输出；flake 白名单见 GAPS §2.2#4（重试即过勿误判，禁自行放宽）。

### 3. realchain 真链路电池（四目标）+ 裁决检查器

```sh
tools/dev-gate.sh 'cargo nextest run -p oc-tui --test realchain_t9 --test realchain_pair --test realchain_toolloop --test realchain_w13 --locked --no-fail-fast'
python3 tools/diff/realchain/adjudicate.py .test-logs/diff-reports
```

测试只导出差异到 diff-reports/；判定权归控制者裁决注册表 `tools/diff/realchain/adjudications.json`（exit 0=全裁决 / 2=有未裁决=合并红 / 3=基建错）。改产品码后必须先重编 bin：`tools/dev-gate.sh 'cargo build -p oc-cli --bin opencode-rust'`——freshness 守卫会拦陈旧二进制。worktree 跑 omo 场景由 harness 自动回退主检出 plugin-payload。

### 4. guards — 反回归守卫（stdlib-only Python）

```sh
tools/guards/run.sh                          # run.sh 收录 10 门（CI 模式仅新违规红）
python3 tools/guards/acp_drift.py            # 独立守卫（G-27，third_party ACP 漂移）
python3 tools/guards/<name>.py               # 单跑
```

覆盖 config_consumers / event_schema / command_keymap / ledger_integrity（GAPS 台账完整性）/ process_safety 等——改 schema/事件/键位/台账后必跑对应守卫。

### 5. parity — 金帧与双端对拍

```sh
cd <opentui-rust 检出> && tools/diff/run-all.sh   # opentui 渲染差分全套 127 场景（直跑禁包 dev-gate）
```

`tools/parity/goldens/` = 334 金帧（重钉=stop-and-escalate，禁擅自重冻结）；树场景 capture/freeze 两脚本；详见 `tools/parity/README.md` 与 `NORMALIZATION.md`。

### 6. 其它

- `tools/lane-brief.sh --all`：派发词嵌规范全文的唯一合法来源（禁手抄压缩）
- `tools/RELEASE.md`：发布/合并链活定义（push 以 adjudicate exit 0 为前置）
- 差分脚本：`tools/diff/*.py`（HTTP/SSE/JSON-SCHEMA/ACP/TUI 全屏等，用法见各脚本头）
- 测试证据：`.test-logs/`（git-ignored）；flake 观察名单与白名单唯一权威=GAPS §2.2#4
