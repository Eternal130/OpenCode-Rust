# opencode-rust — Agent Working Entry

One-liner: a full Rust port of opencode v2.0.24 (the TypeScript reference implementation). Absolutely follow the development standards — **the authoritative source is the local private book `docs/GAPS-AND-PITFALLS.md` (git-ignored, not present in a clone; the on-disk path mirrors the repo layout)**; `tools/lane-brief.sh --all` prints its §2.1–2.5 and the quick-reference table verbatim. The public divergence register is the root-level `DIVERGENCES.md`.

## Absolute Principle (owner ruling, 2026-11-24)

**Everything is faithfully ported from the reference — architecture, UI, UX, and all behavior — except (a) this port's supersets and (b) the reference's inherent bugs.** Architecture may differ only where the development language inherently requires it. The experience surface allows **zero gaps**: every behavior must fully match the reference or be a partial superset (e.g. higher framerate, smoother animation) — deferring animation or opt-in features as "later" is not an exemption. Exemptions fall into exactly three classes: ① this port's supersets (including intentional corrections of ref bugs) ② the reference's inherent bugs ③ language/platform-inherent differences (e.g. V8 inspector, workerd embedding). Every exemption cited in `DIVERGENCES.md` must name its class; the re-triage under this ruling lives there.

## Hard discipline (violations are incidents)

- All local cargo builds/tests go through `tools/dev-gate.sh` (CPU/memory caps + load gate; exit 3 = load refusal, exit 5 = disk refusal — stop immediately, never retry around the gate)
- Plain `cargo test` is forbidden as regression evidence (canonical harness = nextest)
- All test commands are fully logged to `.test-logs/<crate>-<time>-<lane>.log` (the only test evidence; a lost log is lost evidence) — dev-gate auto-tees built in (lane tag `OC_GATE_LANE`; directly-run verification scripts still need a manual tee)
- Every lane uses its own worktree (`.worktrees/`) and merges back to main serially; never touch the main checkout
- Commits always use the identity configured in the repo's git config (`tools/git-identity-hook.sh` pins it: author and committer must both match the config-file identity — `--author` / `-c user.*` / `GIT_AUTHOR_*` are forbidden)
- Register regimes (open-gap SLAs, the coverage ledger, the sandbox-no-judgment rulings) live in GAPS §2.2

## Test tooling usage

### 1. dev-gate.sh — the single entry point for builds/tests

```sh
tools/dev-gate.sh 'cargo nextest run -p oc-tui --locked <filter>'   # day-to-day iteration, seconds to minutes
```

Wrap exactly one cargo command per invocation. Verification scripts (`tools/diff/*.py`, `tools/parity/*.sh`, `tools/guards/run.sh`) run **directly — never wrapped in dev-gate** (lock-in-lock).

**Auto-tee**: dev-gate logs each gated command's complete output (stdout+stderr, untruncated) to `.test-logs/<crates>-<time>-<OC_GATE_LANE>.log` (crate names taken from `-p` flags, `--workspace` → `workspace`, default tag `gate`). That log is the test evidence — **do not tee manually on top** (double write); directly-run verification scripts still need a manual tee. **Console echo (2026-11-28)**: the full stream always goes to the log; when stdout is not a TTY (subagents, shell capture) the console gets a bounded view instead — ≤200 lines are echoed whole (≤600 on failure), larger outputs show the Summary/FAIL/TRY/SLOW/error lines (+ the last 40 lines on failure) and the log path. `OC_GATE_QUIET=0` forces the full stream, `=1` the digest. Never filter or truncate inside the gated command — that shrinks the run AND the log; read details from the log path.

### 2. nextest tiers (`.config/nextest.toml`: threads=4, global retry=1; pty/be1 families retry=2)

| Tier | Command | Purpose / time |
|---|---|---|
| Standing render gate | `tools/dev-gate.sh 'cargo nextest run -p oc-tui --locked dual_engine_matches_frozen_oracle'` | 0.1s smoke |
| Merge gate | `T9_LANE_SHARDS=4 tools/dev-gate.sh 'cargo nextest run -p oc-tui -p oc-cli -p parity --locked --no-fail-fast'` | ~17min |
| Workspace gate | `tools/dev-gate.sh 'cargo nextest run --workspace --locked --no-fail-fast'` | required at closure |
| PTY family | the above + `--profile pty` | serialized test-threads=1, ~28min |

First round unfiltered + `--no-fail-fast`; red tests are re-run in the same round with `--exact <name> -- --nocapture` to capture output; the flake whitelist lives in GAPS §2.2#4 (retry-passes = not a regression; never loosen it on your own).

### 3. realchain battery (five targets) + adjudication checker

```sh
T9_LANE_SHARDS=4 tools/dev-gate.sh 'cargo nextest run -p oc-tui --test realchain_t9 --test realchain_pair --test realchain_toolloop --test realchain_w13 --test realchain_g454 --locked --no-fail-fast'
python3 tools/diff/realchain/adjudicate.py .test-logs/diff-reports
```

The realchain family serializes on a **sharded cross-process flock**: `T9_LANE_SHARDS=<N>` (owner ruling 2026-11-23, default 4; pid-bucketed lock files, ~746s → ~420s for the battery). Unset or `1` = the historical single lock (byte-identical behavior; CI without the env self-serializes). `PTY_LANE_SHARDS` (pty family) stays default-off pending a soak.

The tests only export differences to diff-reports/; judgment belongs to the controller's adjudication registry `tools/diff/realchain/adjudications.json` (exit 0 = all adjudicated / 2 = unadjudicated remains = merge red / 3 = infrastructure error). After changing product code, rebuild the binary first: `tools/dev-gate.sh 'cargo build -p oc-cli --bin opencode-rust'` — the freshness guard rejects stale binaries. When running omo scenarios from a worktree, the harness automatically falls back to the main checkout's plugin-payload.

### 4. guards — anti-regression guards (stdlib-only Python)

```sh
tools/guards/run.sh                          # run.sh collects 10 guards (CI mode: only new violations are red)
python3 tools/guards/acp_drift.py            # standalone guard (G-27, third_party ACP drift)
python3 tools/guards/<name>.py               # run one
```

Coverage includes config_consumers / event_schema / command_keymap / ledger_integrity (register integrity) / process_safety and more — after touching schema/events/keybinds/ledgers, run the corresponding guard.

### 5. parity — golden frames and dual-end diffing

```sh
cd <opentui-rust checkout> && tools/diff/run-all.sh   # full opentui render diff, 127 scenarios (run directly, never via dev-gate)
```

`tools/parity/goldens/` = 334 golden frames (re-pinning = stop-and-escalate; never re-freeze on your own); tree scenarios use the capture/freeze script pair; see `tools/parity/README.md` and `NORMALIZATION.md`.

### 6. Misc

- `tools/lane-brief.sh --all`: the only legal source for embedding the standards verbatim into dispatch prompts (hand-copied compressions forbidden)
- `tools/RELEASE.md`: the living definition of the release/merge chain (push requires adjudicate exit 0)
- Diff scripts: `tools/diff/*.py` (HTTP/SSE/JSON-SCHEMA/ACP/TUI fullscreen etc.; usage in each script's header)
- Test evidence: `.test-logs/` (git-ignored); the flake watchlist and whitelist are authoritative only in GAPS §2.2#4
