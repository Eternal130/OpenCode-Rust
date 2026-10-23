# opencode-rust

[![ci](https://github.com/Eternal130/OpenCode-Rust/actions/workflows/ci.yml/badge.svg)](https://github.com/Eternal130/OpenCode-Rust/actions/workflows/ci.yml)

**A full Rust rewrite of [opencode](https://github.com/anomalyco/opencode) v2** — the AI coding agent's CLI, HTTP server, TUI, and plugin system, ported from upstream **[v2.0.24](https://github.com/anomalyco/opencode/commit/e7a34f09bfd9134dfade5a8ddb843f7030bc9a69)** with parity that is verified, not assumed: byte-equal OpenAPI contract, byte-equal TUI golden frames, and a database that reads and writes the upstream format in both directions.

Single native binary. No Node/Bun runtime for the app itself — Bun is only spawned as a plugin subprocess, and only when plugins or MCP servers are configured.

> Independent port, not affiliated with the upstream opencode project. Upstream is [MIT](https://github.com/anomalyco/opencode/blob/main/LICENSE); so is this.

## Scope

Ported from v2.0.24:

- **CLI** — 40/40 commands
- **HTTP server** — all 141 operations; the generated OpenAPI document is semantically identical to the pinned upstream contract (empty diff)
- **TUI** — full port on the [opentui-rust](https://github.com/Eternal130/opentui-rust) crates (solid renderer) over ratatui + taffy; 42 golden scenarios verified in tiers — 35 Tier-A byte-equal in both text grid and ANSI styling, plus Tier-B (256-color degradation), Tier-C1, and width-edge classes
- **Plugin system** — Bun-subprocess host; community plugins run unmodified (flagship [`oh-my-opencode-slim`](https://github.com/alvinunreal/oh-my-opencode-slim) v3.0.3 passes its own self-check through the real binary path: 12 agents / 12 tools / 8 skills registered, session round-trips verified)

Not ported (out of scope by design): the desktop app, GUI extensions, and the web UI frontend (static hosting + release asset embedding are ported; the app itself is not). All deferrals from the frozen report §3 have since landed (mini, ACP, worktree UI, sounds/OS notifications, C2 tier, tree-sitter highlighting, images, Bedrock/Vertex — beta.4–beta.7); the only open tail items are tracked in [PORT_REPORT.md](./PORT_REPORT.md) §0 and [ROADMAP.md](./ROADMAP.md): R-10 rmcp elicitation (blocked upstream) and R-11 platform matrix (unscheduled).

## Verified parity, not vibes

Every consistency claim is backed by a replay or golden artifact against the pinned upstream checkout:

- **Contract** — OpenAPI semantic diff vs v2.0.24: empty; error paths byte-equal 7/7
- **Interoperability** — the official v2.0.24 TUI runs against the Rust server (bidirectional swap tested); SQLite session DB is read/write compatible with v2 in both directions, including migration inheritance
- **LLM protocols** — 274 cassettes replayed event-exactly (121 interactions incl. HTTP failures) + 132 golden request bodies; native implementations for Anthropic, OpenAI chat/responses, Gemini, Mistral, Cohere; long tail via openai-compatible
- **TUI** — golden-frame harness (`tools/parity`): text + ANSI byte-equality on Tier A, 256-color degradation styles on Tier B, text grids on C1; CJK/emoji/ZWJ width pinned by measurement
- **Plugins** — registration, commands, tools, config presets, CodeMode interpreter, crash containment (kill -9 fuzzing), V1/V2 formats
- **Suite** — 3951/3951 tests, `clippy --workspace --all-targets -D warnings` clean, `rustfmt` clean

Performance (measured on Linux, vs upstream v2 on Bun): `--version` 3 ms vs 744 ms; server boot to listening 111 ms; idle RSS ~80 MB; release binary ~44 MB.

## Architecture

| Crate | Role |
|---|---|
| `oc-util` | Foundations: fs, process, strings, paths |
| `oc-schema` | Config schema, JSON/JSONC parsing, OpenAPI types |
| `oc-theme` | Theme engine — 63 themes byte-equal with upstream |
| `oc-ai` | LLM protocol SDK (Anthropic, OpenAI chat/responses, Gemini, Mistral, Cohere, Bedrock, Vertex, Copilot, Azure, xAI, openai-compatible) |
| `oc-core` | Session engine, projector/replay ledger, DB (drizzle-compatible), 14 tools, MCP, bus, config/permissions |
| `oc-syntax` | tree-sitter syntax highlighting (vendored v2 grammars) |
| `oc-plugin-host` | Bun-subprocess plugin host: registration, CodeMode, crash containment |
| `oc-server` | HTTP server, SSE, boot/composition root |
| `oc-client` | Client SDK for the server API |
| `oc-tui` | Terminal UI (opentui-solid renderer over ratatui + taffy) |
| `oc-tui-core` | Keymap / text-edit interaction primitives (opentui-keymap & opentui-core based) |
| `oc-cli` | The `opencode-rust` binary and its 40 commands |
| `tools/parity` | Parity harness: golden frames, scenarios, replays, acceptance batteries |
| `tools/openapi-gen` | OpenAPI contract generator |

Design docs: [MIGRATION.md](./MIGRATION.md) (plan & decision log), [PORT_REPORT.md](./PORT_REPORT.md) (delivery report & honest residual list), [docs/GAPS-AND-PITFALLS.md](./docs/GAPS-AND-PITFALLS.md) (open gaps & operational incident register).

## Build

Requires Rust stable.

```sh
cargo build --release
# binary: target/release/opencode-rust
```

Run:

```sh
opencode-rust            # TUI
opencode-rust serve      # HTTP server
opencode-rust run "..."  # headless
opencode-rust --help     # 40 commands
```

## Test

```sh
cargo nextest run
```

Opt-in acceptance batteries (need a Bun runtime and the pinned upstream checkout; the harness self-heals a reference build):

```sh
OC_P10=1 cargo nextest run -p parity        # cross-binary acceptance battery
OC_M2=1  cargo nextest run -p oc-tui        # full TUI workflow rehearsal
OC_TUI_SMOKE=1 cargo nextest run            # TUI smoke vs reference
```

Point the harness at your checkout/build with `PARITY_REF_ROOT` / `PARITY_BUN` if it isn't in the default location.

Packaging and release: see [tools/RELEASE.md](./tools/RELEASE.md) and [install/install.sh](./install/install.sh).

## Releases

Pre-release builds are published from `v2.0.24.beta.N` tags until acceptance is complete, then the final `v2.0.24` release replaces them. The binary always self-reports `2.0.24` (the parity pin) — the beta marker exists only in the tag and asset names.

Assets per release (the R-11 platform matrix, verified by the release workflow before publishing): `opencode-<version>-<target>` archives for `linux-x64`, `linux-arm64`, `linux-x64-musl`, `linux-arm64-musl` (static), and `darwin-arm64` (tarballs) plus `windows-x64` (zip, experimental build-only) — each with a `.sha256` sidecar. macOS Intel is not shipped (build from source). Every archive carries **two** binaries: `opencode-rust` and the `oc-pty` persistent-terminal daemon (persistent terminals are on by default; the binary spawns `oc-pty` via `OPENCODE_PTY_BIN` → exe-sibling → `PATH`). If your install somehow lacks the `oc-pty` sibling, `OC_PTY_DAEMON=0` forces the in-process backend for this one release window — see [tools/RELEASE.md](./tools/RELEASE.md) § oc-pty co-packaging. Install the latest beta:

```sh
curl -fsSL https://raw.githubusercontent.com/Eternal130/OpenCode-Rust/main/install/install.sh | bash -s -- \
  --version v2.0.24.beta.13 \
  --url https://github.com/Eternal130/OpenCode-Rust/releases/download/v2.0.24.beta.13/opencode-2.0.24.beta.13-linux-x64.tar.gz
```

Installed binaries can update themselves: `opencode-rust upgrade` checks the [releases feed](https://github.com/Eternal130/OpenCode-Rust/releases), verifies the tarball against its sha256 sidecar, and atomically replaces the binary (source builds are skipped cleanly). The check also runs in the background for the TUI/serve entry points — control it with the `update` config field (`"disable"`, `"notify"` the default, `"auto"`) or `OPENCODE_DISABLE_AUTOUPDATE=1`; every network failure is a silent no-op that never affects operation (see [tools/RELEASE.md](./tools/RELEASE.md) § Update channel).

## License

[MIT](./LICENSE). This project is a from-scratch Rust port of the MIT-licensed [opencode](https://github.com/anomalyco/opencode); it is an independent effort and is not affiliated with or endorsed by the upstream project.

---

## 中文说明

本项目是 [opencode v2](https://github.com/anomalyco/opencode)（钉版 **v2.0.24**，commit [`e7a34f0`](https://github.com/anomalyco/opencode/commit/e7a34f09bfd9134dfade5a8ddb843f7030bc9a69)）的**全量 Rust 重写**：CLI、HTTP 服务端、TUI、插件系统全部移植，桌面端与 Web UI 不在范围内。

一致性靠验收体系背书，不靠感觉：OpenAPI 契约与上游语义 diff 为空；官方 v2.0.24 TUI 可直连本服务端；会话数据库与 v2 双向互通；TUI 以 golden 帧按文本+ANSI 逐字节分级验收；LLM 协议 274 盒带逐事件重放；社区插件（旗舰 oh-my-opencode-slim）不修改直接可用。全套 3951 测试、clippy/rustfmt 零告警。

应用本体是纯 Rust 单二进制（约 44 MB，空闲内存约 80 MB）；Bun 仅在配置了插件或 MCP 服务器时作为子进程拉起（有旗舰插件满载时约 210 MB，无插件则完全不启动）。

协议：MIT（与上游一致）。详见上方英文部分与 [PORT_REPORT.md](./PORT_REPORT.md)。
