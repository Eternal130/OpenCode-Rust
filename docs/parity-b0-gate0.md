# B0-8 — Gate 0 dual-end gate environment（证据包）

- Lane: `lane/b0-gate0`（worktree `.worktrees/lane-b0-gate0`），base `main@2693ae8fbc77`
- Date: 2026-10-18 · Ledger: **B0-8** · Parity-LOOP: §2（红线）、§8.3 Gate 0、§8.5
- 证据目录: `/tmp/opencode/parity-evidence/B0-8/`（本文件引用的所有日志都在其中）
- 产品代码零改动；`tools/dev-gate.sh` 零改动；`/opt/opencode-ref`、`/tmp/opencode/ref` 只读使用。

## 1. 变更内容（仅 harness）

1. **`OC_GATE_STRICT=1` fail-not-skip** — 新模块 `tools/parity/src/gate.rs`：
   - `gate::strict()`：`OC_GATE_STRICT=1` 且未设 `OC_GATE_ALLOW_SKIP=1`（§8.3 的日常本地豁免；
     豁免运行不得进入 `verified` 证据）。
   - `gate::skip_or_fail(missing) -> bool`：唯一共享决策点。非严格 → 打印 skip note 并返回
     `true`（调用方 `return Ok(())`，行为与今天一致）；严格 → **panic FAIL**，错误信息点名
     缺什么（§8.5 “skip 不算 pass”）。
2. **bun 发现** — `reference.rs::bun_path()` 的 well-known 候选加入
   `/home/eternal130/.bun/bin/bun`（列在 `/root/.bun/bin/bun` 之后；`/root` 是 700，非 root
   永远探不到第一个，所以 lane 身份无需 env 即可发现 bun）。
3. **所有 bun/ref 可用性 self-skip 全部路由到共享检查**：
   - `src/reference.rs::skip_if_no_bun`（bun 缺失 / `PARITY_BUN` 指向不存在文件【仅严格模式】 /
     ref checkout 缺失且不可重建，三种原因分别点名）
   - `src/rust_server.rs::skip_if_no_cargo`（Rust 侧 boot 前置）
   - `tests/reference_info.rs`、`tests/scenarios.rs`（经 `skip_if_no_bun`，去除重复 note）
   - `tests/rust_scenarios.rs::ensure_ref_runtime_or_skip`（重建失败的三个分支）+ cargo 检查；
     `which_bun` 现在优先 `PARITY_BUN`
   - `tests/p10_common::reference_runnable`（经 `skip_if_no_bun`，覆盖全部 p10 双端文件）
   - `tests/p10_openapi.rs`（cargo + pinned `openapi.json` 缺失）、`tests/p10_cross_tui.rs`
     （ref TUI checkout + bun + cargo）、`tests/p10_error_paths.rs`、`tests/p10_db_interop.rs`（2 处）
   - `tests/m1_tui_smoke.rs`（ref TUI checkout 缺失）
   - 环境变量未设时行为不变（仍 skip）；opt-in 开关（`OC_P10`/`OC_TUI_SMOKE`/`OC_M1_TUI`/`OC_M2`）
     与 tmux 可用性 skip 不在 B0-8 的 bun/ref 范围内，保持原样（见 §7）。
4. **新测试 `tools/parity/tests/gate0_environment.rs`**：§8.3 Gate 0 的最小双端证明 + 逐运行记录
   （`gate_ran`/`ref_commit`/`bun_path`/`reference_boot`/`rust_boot`，`PARITY_GATE0_RECORD`
   指定落盘路径）。

## 2. 身份与 bun（全部经 `tools/dev-gate.sh` 以 eternal130 执行）

- `uid=1000 user=eternal130`；gate PATH =
  `/home/eternal130/.cargo/bin:/usr/local/sbin:...`（**不含 bun** — 所以 gate 命令必须显式
  `PARITY_BUN` 或依赖新加的 lane 发现）
- bun: `/home/eternal130/.bun/bin/bun`（root:root 755 的 staged 拷贝，eternal130 可执行；
  bun v1.3.10）→ `bun --version` 输出 `1.3.10`
- 无 env 变量的 lane 发现已验证：`env -u PARITY_BUN OC_GATE_STRICT=1` 下 `reference_info`
  绿（日志 `green-bun-discovery-noenv.log`）
- 日志：`env-identity.log`

## 3. 参考 checkout

- 可运行根 `/tmp/opencode/ref`：`packages/cli` 与 `node_modules` 均在（未重建、未改动）
- 只读源 `/opt/opencode-ref`：`git log -1` →
  `e7a34f09bfd9134dfade5a8ddb843f7030bc9a69 release: v2.0.24`
  （仓库 root-owned；gate 用户读取需逐命令 `git -c safe.directory=/opt/opencode-ref`，
  不改全局 git config）
- 日志：`env-identity.log`

## 4. 严格红证明（必须工件）

命令（经 dev-gate，eternal130）：

```bash
CARGO_TARGET_DIR=/tmp/opencode/target-lane-b0-gate0 \
PARITY_BUN=/nonexistent-and-absent OC_GATE_STRICT=1 \
cargo nextest run -p parity --test reference_info
```

结果：**FAIL**（exit 100，`Summary: 1 test run: 0 passed, 1 failed, 0 skipped`），
panic 信息点名缺失项：

```text
OC_GATE_STRICT=1: gate prerequisite missing — bun not found at
/nonexistent-and-absent (PARITY_BUN points at a nonexistent file); strict mode
forbids self-skip (fix the environment, or set OC_GATE_ALLOW_SKIP=1 for a local
non-verifiable run; see PARITY-LOOP.md §8.3 Gate 0)
```

工件：`red-strict.log`

## 5. 绿运行（最终代码，全部经 dev-gate）

| 运行 | 命令要点 | 结果 |
|---|---|---|
| 制式 reference boot | `PARITY_BUN=/home/eternal130/.bun/bin/bun OC_GATE_STRICT=1 cargo nextest run -p parity --test reference_info` | **1 passed, 0 skipped**（boot → GET /api/info 200 → golden/SSE → clean stop） |
| lane 发现（无 env） | 同上但 `env -u PARITY_BUN` | **1 passed, 0 skipped** |
| Gate 0 双端 | `... OC_GATE_STRICT=1 PARITY_GATE0_RECORD=... cargo nextest run -p parity --test gate0_environment --no-capture` | **1 passed**：bun --version、ref commit、reference boot 127.0.0.1:44609 → /api/info 200 → clean stop、rust boot 127.0.0.1:39605 → /api/info 200 → clean stop |
| 默认套件（非严格回归） | `PARITY_BUN=... cargo nextest run -p parity` | **103 passed, 9 skipped**（9 = 既有 opt-in ignore：OC_P10/OC_TUI_SMOKE/OC_M1_TUI/OC_M2） |

- 所有服务器端口均为 `127.0.0.1:<ephemeral>`（harness 的 free_port 带不可用 49374 保护）；停止走
  harness 自有 child 句柄 + `signal_group_filtered`。
- `cargo fmt -p parity -- --check`：OK。`cargo clippy -p parity --lib --tests -- -D warnings`：
  本 lane 触碰目标全部干净；唯一报错来自未触碰的 legacy bin 目标 `src/bin/rescan-tui.rs`
  （与 main@2550efe 字节一致，属既有债务，非本 lane 引入）。
- 日志：`green-reference-info.log`、`green-bun-discovery-noenv.log`、
  `green-gate0-dual-end.log`、`green-default-suite.log`

## 6. `gate_ran` 记录（§8.3 要求字段）

`gate-ran.json`（由 `gate0_environment` 在严格模式绿运行中写出）：

```json
{
  "gate_ran": true,
  "ref_commit": "e7a34f09bfd9134dfade5a8ddb843f7030bc9a69",
  "bun_path": "/home/eternal130/.bun/bin/bun",
  "bun_version": "1.3.10",
  "reference_boot": "ok",
  "rust_boot": "ok"
}
```

## 7. 边界与后续

- tmux 可用性 skip（`tui_smoke`/`m1_tui_smoke`/`p10_cross_tui` 的 tmux 检查）与 opt-in 开关
  保持 skip 语义：不在 B0-8 的 bun/ref 范围。p10 电池在 §8.4 严格命令下若 tmux 缺失仍会
  skip——建议 p10 电池 owner 后续把 tmux 一并路由到 `gate::skip_or_fail`。
- `rust_server::spawn_rust` 内部的 ref checkout 重建仍是 best-effort 日志（Rust-only 测试不依赖
  ref；双端测试的严格性由各自的 availability gate 保证）。
- 本 lane 不标记任何 `verified`；独立 verifier 复核。
