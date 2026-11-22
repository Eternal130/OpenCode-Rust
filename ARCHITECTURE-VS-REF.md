# Architecture vs the Reference (opencode v2.0.24: TypeScript reference → Rust port)

One-line overview: the reference is a bun monorepo (`packages/*`, TS); the port is a cargo workspace of 14 crates (539k lines of source, measured 2026-11-22 under `crates/*/src`), mapped package-by-package with every structural divergence carrying a documented ruling (see `DIVERGENCES.md`).

## Two tree topologies

| | Reference (TypeScript) | Rust port |
|---|---|---|
| Build | bun monorepo, `packages/*` | cargo workspace, 14 crates (`crates/*`) |
| Size | ~247k lines of TS core paths | 539,175 lines of source (src, excluding tests) |
| Tests | vitest / bun-test | nextest four-tier gates + 12 guards + parity 334 golden frames + the realchain four-target battery |

## Layer mapping (ref package ↔ rust crate)

| ref package | rust crate (src lines, measured 2026-11-22) | Notes |
|---|---|---|
| core | oc-core (184,989) + oc-ai (79,646) | Split: the LLM SDK (providers/protocols/stream) becomes oc-ai |
| tui | oc-tui (211,299) + oc-tui-core (4,778) + oc-plugin-host (16,352) | oc-tui-core = cross-crate contract; oc-plugin-host carries the ref supervisor duties (bun guest process management + TSX shim bridge) |
| server | oc-server (51,985) + oc-http (394) | oc-http extracts the TLS/trust stack (platform-verifier + webpki fallback) |
| cli | oc-cli (31,897) + oc-pty (4,563) | oc-pty is the standalone PTY daemon (below) |
| sdk / client | oc-client (22,037) | Generated: `tools/openapi-gen` produces it from the sha256-pinned openapi.json |
| theme | oc-theme (4,068) | |
| schema (embedded Effect Schema) | oc-schema (9,356) | Hand-written serde + schemars, goldens guard drift (G-16) |
| util | oc-util (5,176) | |
| syntax highlighting (embedded tree-sitter) | oc-syntax (1,880) | Vendored grammar+queries; the highlighting engine rides opentui's TreeSitterClient |
| — (no ref counterpart) | vendored: ACP×2, tree-sitter grammars×4, oc-uqr (`third_party/`) | ACP carries serde-delta patches |

## Key structural decisions (decision → why → divergence pointer: G# entries live in `DIVERGENCES.md`)

1. **Render engine**: ref embeds opentui (Solid TSX) → the port uses the cross-repo opentui-rust (git rev `0f7fb04` pinned: opentui-core/keymap/solid) + oc-tui. Anti-drift = the dual-engine golden oracle + tree-scenario diffing over 334 golden frames (`tools/parity/`). → Closed (historical gaps G-1..G-464 all cleared; current state in `DIVERGENCES.md`).
2. **Plugin runtime**: ref runs a worker per Location → the port runs one bun guest for the whole process (G-289) plus the bounded TSX shim: the render tree is flattened over IPC into a `{title,rows,clickable}` row shape; the markdown renderer registration chain and panel slot have landed, narrowing the declaration surface (G-28).
3. **HTTP service**: ref embeds workerd (fetch.ts/workerd.ts) → the port is native axum `boot_and_serve`, no JS-runtime embedding target (G-163).
4. **Schema**: ref Effect Schema → hand-written serde + schemars + golden diffing against drift; `/openapi.json` is sha256-pinned, upgrades go through a manual baseline-bump (G-16, the long-term drift-maintenance pit).
5. **Client SDK**: ref hand-writes its sdk → the port generates oc-client via `tools/openapi-gen` from the pinned openapi.json (wire shapes guaranteed by a single source of truth).
6. **PTY**: oc-pty is a standalone daemon (Unix-socket transport; the historical design doc left the tree — full copies in the out-of-repo archive); override-path owner hardening and the sha256-pinning exemption are documented trade-offs (G-240).
7. **Vendored**: `third_party/` holds ACP×2 (agent-client-protocol / -schema, with serde-delta patches), tree-sitter grammars×4 (kotlin/toml/vim/vue), and oc-uqr — upstream-pinned, patches evidenced.
8. **Verification system**: ref vitest/bun-test → nextest four-tier gates (standing gate → diff → merge gate → serialized pty) + 12 guards (`tools/guards/run.sh`) + parity 334 golden frames + the realchain four-target battery with the controller adjudication registry (`tools/diff/realchain/adjudications.json` + `adjudicate.py`). See `AGENTS.md`.

## Build and toolchain

- Rust `1.99.0` pinned (`rust-toolchain.toml`); repo-level incremental=false enforced.
- `tools/dev-gate.sh` single entry: systemd scope (CPUQuota=400% / MemoryMax=6G / TasksMax=512) + load/disk gates; verification scripts (diff/parity/guards) run directly, never wrapped in the gate.
- Four CI pipelines (`.github/workflows/`): `ci.yml` / `runtime-gates.yml` / `release.yml` / `realchain-diff.yml`.
- Diff tooling family: `tools/diff/*.py` (HTTP/SSE/JSON-SCHEMA/ACP/TUI fullscreen), `tools/parity/` (goldens), `tools/diff/realchain/` (real-chain battery + adjudication).

## Pointers

- Known-divergence register → `DIVERGENCES.md`; development standards → the local private book `docs/GAPS-AND-PITFALLS.md` (git-ignored)
- Agent entry (standards extraction via `tools/lane-brief.sh`) → `AGENTS.md`
- Historical ADRs / audit ledgers / design docs → the local out-of-repo archive (not committed; the complete adjudication history is not distributed with the public repo — the 2026-11-22 privacy cleanup rewrote history; full copies live in local backups and the archive)
