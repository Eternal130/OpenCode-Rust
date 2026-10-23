# Release artifacts (Phase 10 packaging)

What ships for the Rust port of opencode v2.0.24, how it is built, and
how the optional runtime dependencies are handled.

## Artifact set

| Artifact | Path | Notes |
|---|---|---|
| Release binary | `cargo build --release -p oc-cli` → `target/release/opencode-rust` (~46 MiB, thin-LTO + symbol-strip via the workspace `[profile.release]`) | The single command: CLI + server + TUI. Reports `opencode-rust v2.0.24` (`--version`) and `opencode-rust v2.0.24 (rust port build <git-short>, <profile> profile)` (`--version-rust`, hidden; stamped by `crates/oc-cli/build.rs`). The `/api/info` version stays `2.0.24` (protocol parity). |
| PTY daemon binary | `cargo build --release -p oc-pty` → `target/release/oc-pty` | The persistent-terminal daemon (Phase 9). CO-PACKAGED with the main binary in every release archive — the phase-3 gate ruling; see "oc-pty co-packaging" below. |
| Installer | `install/install.sh` | Mirrors v2's installer UX shape (versioned download → `~/.opencode/bin/opencode-rust`, chmod 755, shell-config PATH hint, ASCII banner). Installs the `oc-pty` sibling when the payload carries it. See below for the parameterization. |
| Release smoke | `tools/release-smoke.sh` | Gate for any release candidate; see below. |
| Release tarball | NOT in-repo — produce at packaging time: `tar -czf opencode-<version>-<target>.tar.gz package/opencode-rust package/oc-pty` (windows: a zip with `opencode-rust.exe` + `oc-pty.exe`) | Two-member archives (the installer and `opencode-rust upgrade` accept the `package/opencode-rust` or `opencode-rust` member shape, plus zip and raw-binary artifacts) published per platform target alongside `<archive>.sha256` sidecars (`sha256sum` format) — the installer auto-uses them. See "Platform matrix (R-11)" for the target list. |

## oc-pty co-packaging (phase-3 gate ruling)

Persistent terminals are default-ON and route through the `oc-pty`
daemon. The spawn resolution order is `OPENCODE_PTY_BIN` → the `oc-pty`
sitting NEXT TO the main executable (exe-sibling) → `PATH`
(`oc_core::persistent_pty::transport::resolve_binary_path`). A released
install without the sibling would 503 every persistent-terminal op, so
the gate ruling binds the two binaries together:

* Every release archive carries BOTH `package/opencode-rust` AND
  `package/oc-pty` (windows: both `.exe` members at the zip root).
* The release workflow builds `-p oc-cli -p oc-pty` for every matrix
  target, and the packaging smoke asserts BOTH members extract from the
  archive and the main binary runs `--version` from inside it.
* `install/install.sh` installs both into `~/.opencode/bin` when the
  payload carries the sibling. A lone main binary (legacy beta tarballs,
  raw `--binary` installs) still installs — with a note, never a
  failure.

**One-release escape hatch (user-facing)**: `OC_PTY_DAEMON=0` forces
the in-process PTY backend for exactly ONE release window (design doc
`docs/superpowers/specs/2026-10-23-pty-daemon-design.md` R3) — no
`oc-pty` process is ever spawned and the HTTP/TUI surface is
identical. Set it if a release arrives without a usable sibling binary
(e.g. a manual single-binary copy); the flag is scheduled for
retirement next release.

**Registered gap CLOSED (p4 follow-up)**: `opencode-rust upgrade`
refreshes BOTH members — when the release archive carries
`package/oc-pty` (all current tarballs do) the sibling is co-extracted
and installed beside the main binary with the same atomic
stage/chmod/rename mechanics. Archives without the member (legacy
beta tarballs, raw `--binary` upgrades) skip the sibling silently; a
skewed install there can still fall back to `OC_PTY_DAEMON=0`.

## Identity: binary name, isolated data dirs, one-way v2 migration

The Rust port ships under its OWN identity so it can be installed next to
an upstream v2 binary without a shadow war (REGISTERED divergence —
PORT_REPORT §3):

* **Binary name**: `opencode-rust` (`[[bin]]` in `crates/oc-cli`,
  clap `--version` banner `opencode-rust v2.0.24`, `--version-rust`
  stamp prefix likewise). The wire-visible version stays `2.0.24`
  (`/api/info`, service negotiation — protocol parity is unaffected).
* **Isolated XDG dirs** (`oc_util::global_roots::APP_NAME`):
  `~/.config/opencode-rust`, `~/.local/share/opencode-rust`,
  `~/.cache/opencode-rust`, `~/.local/state/opencode-rust`. Env
  overrides (`OPENCODE_CONFIG_DIR`, `XDG_*`, `OPENCODE_TEST_HOME`,
  `OPENCODE_DB`) keep working unchanged. The plugin npm cache and the
  managed pinned bun follow (`<cache>/opencode-rust/npm`,
  `<cache>/opencode-rust/bin/bun`).
* **One-way first-launch migration**
  (`oc_core::v2_migration::ensure_migrated`, called from the CLI entry
  and `boot_and_serve`): if upstream v2 directories exist
  (`~/.config/opencode`, `~/.local/share/opencode`, `~/.cache/opencode`
  — resolved under the same env rules), they are COPIED into the
  isolated dirs once — databases (`opencode.db` + channel-suffixed
  variants, via the sqlite backup API with a byte-copy fallback),
  `auth.json`, flat config files, and the `npm/`+`bin/` cache subtrees.
  `log/` and `repos/` are not copied. Sources are NEVER moved, deleted,
  or modified (v2's own v1→v2 migration was in-place; ours is a copy
  precisely BECAUSE the directories are isolated). A marker file
  `<data>/v2-migration.json` latches the one-way door; a lock file
  under `<state>/locks` serializes concurrent entry points. Any hard
  failure warns on stderr and continues boot.

## Version stamping

* The workspace pins `version = "2.0.24"`; the binary reports
  **exactly** `2.0.24` (`env!("CARGO_PKG_VERSION")`) because service
  version negotiation is exact-string and the Rust build must be a
  drop-in peer of v2.0.24 (MIGRATION §2.4). The `--version` banner is
  `opencode-rust v2.0.24` — the binary-name divergence is registered
  (see "Identity" above); the version STRING itself is unchanged.
* `crates/oc-cli/build.rs` additionally stamps the git short hash +
  compilation profile for the hidden `--version-rust` flag, plus
  `OC_RELEASE_TAG` (the release workflow's git tag, `"dev"` locally)
  — the update channel's release identity.
* **Request User-Agent** (the provider-facing `User-Agent` header,
  v2 `app.ts:24-26` `opencode/{channel}/{version}/{name}`): the
  engine stamps it from
  `AppMetadata::detected()` (oc-core `session/runner/model_request.rs`)
  — version = the same workspace `2.0.24` pin, name = `OPENCODE_CLIENT`
  ?? `"cli"` (v2 `version.ts:8` artifact fallback), channel = the ONE
  workspace channel source (`oc_util::channel`, the runtime port of
  v2's build-time `OPENCODE_CHANNEL` define — P10-DB1). The parity rig
  pins `OPENCODE_CHANNEL=stable` for our server exactly as it passes
  `--define OPENCODE_CHANNEL="stable"` to the reference, so both
  stacks' request-path UAs read `opencode/stable/2.0.24/cli`
  (corpus3 #48; asserted in the tui-diff request-path report).
* Never report a TS-stream version (`2.0.25+`) — see the upgrade
  policy below.

## Install script (`install/install.sh`)

Differences from v2's `install` script, both deliberate:

1. **The tarball URL is parameterized** (`--url`, `$RELEASE_URL`;
   `{version}`/`{target}` placeholders supported). There is NO
   opencode.ai latest-version probe: this stream must never adopt TS
   releases beyond the 2.0.24 parity pin. Running without a URL (and
   without `--binary`) is a hard error explaining exactly that.
2. **Checksum verification**: `--sha256` / `$SHA256`, or a
   `<url>.sha256` sidecar when fetchable; a mismatch aborts before
   anything is installed. Without any checksum the installer warns
   loudly and continues (matching v2's no-verification behavior).

Also supported, matching v2's flags: `--version <v>`, `--binary <path>`
(local binary OR tarball OR zip), `--no-modify-path`, `-h/--help`.
Install destination `~/.opencode/bin/opencode-rust`, mode 755, PATH hint
written to the detected shell rc (zsh/bash/fish/ash/sh lists identical
to v2), `$GITHUB_PATH` appended in Actions.

## `opencode-rust upgrade` / `opencode-rust uninstall` (real, Phase 10 + lane C)

* **Update channel (lane C)**: `opencode-rust upgrade` with NO target
  queries the port's own GitHub releases feed and installs the newest
  channel-eligible release (see "Update channel" below). A
  policy-driven background checker (`update: disable|notify|auto`)
  announces or auto-installs; the failure-isolation contract — network
  failures NEVER affect program operation — is a hard requirement of
  the lane. Source/dev builds (`RELEASE_TAG == "dev"`) are told they
  are source builds and skipped cleanly.
* Methods (`--method`):
  * `local-file` *(Rust-port extension, real)* — target is a path to a
    binary / tar.gz / zip on disk; payload extracted, staged, and
    atomically renamed over the running binary (unix self-replace;
    mode 755). No network, no checksum requirement (offline installs).
  * `github` *(wired, network-optional)* — with an explicit target,
    downloads `$OPENCODE_RELEASE_URL` (with `{version}`/`{target}`
    placeholders), verifies against the `<url>.sha256` sidecar when
    one is reachable, extracts, replaces. Without the env it is a
    clean typed error — it will NOT fall back to opencode.ai
    (TS-stream protection).
  * `curl` — auto-detected when the running binary IS
    `~/.opencode/bin/opencode-rust`; behaves as `github` (same env).
  * `brew` — detected under a Cellar prefix; errors with the
    `brew upgrade opencode` guidance.
  * `npm`/`pnpm`/`bun`/`yarn`/`vp` — typed error: those streams carry
    TS releases this port must not adopt.
* Extraction accepts the `package/opencode-rust` member (lane B
  naming, what the release tarballs carry) with legacy `opencode`
  kept as a fallback for the beta.1-era tarballs, plus zip and
  raw-binary artifacts.
* `uninstall` (v2 flag surface: `--keep-config`, `--keep-data`,
  `--dry-run`, `--force`): stops every `service*.json` registration
  (safekill path), removes data/cache/state (+config unless kept),
  cleans the `~/.opencode/bin` PATH lines from shell configs, then
  self-unlinks the binary. Non-TTY without `--force` errors with v2's
  `requireInteractive` message.

## Update channel (lane C)

**Feed**: `https://api.github.com/repos/Eternal130/OpenCode-Rust/releases`
(`?per_page=20`, `User-Agent: opencode-rust-updater`,
`Accept: application/vnd.github+json`). The feed is the DEFAULT for
`upgrade` with no target and for the background checker;
`OPENCODE_RELEASE_URL` keeps overriding the explicit-target download
path exactly as before (tests/harness depend on it).

**Release identity**: `build.rs` stamps `OC_RELEASE_TAG` (the release
workflow sets it to `github.ref_name`); local/source builds get
`"dev"` and NEVER touch the network for updates. `--version-rust`
appends `, release <tag>` for stamped builds.

**Channel rule**: a build from a beta tag (`v2.0.24.beta.1`) sees ALL
releases (betas + finals); a build from a final tag sees only
`prerelease == false` releases. Tags parse as
`v?(\d+)\.(\d+)\.(\d+)(?:\.beta\.(\d+))?`; final > beta for the same
triple, betas order numerically. Unparseable tags and releases
without the expected asset (`opencode-<version>-linux-x64.tar.gz`,
target a constant for now) are never candidates. GitHub 403/429 rate
limits read as "no update", not an error.

**sha256 policy**: every feed-driven install (manual latest AND
`update: auto`) REQUIRES the `.sha256` sidecar (the tarball asset URL
+ `.sha256`, `sha256sum` two-space format, case-insensitive) and
refuses on mismatch or unreachable sidecar — all-or-nothing: download
→ verify → extract → atomic sibling-rename. The one exception is the
EXPLICIT-target `github`/`curl` path, which keeps its pre-lane-C
contract (verify when a sidecar is reachable, proceed with a note
when not) so offline/harness installs against a bare tarball keep
working.

**Config** (`update` field, v2 semantics, in the global
`opencode.json(c)` / legacy `config.json`):

* `"disable"` — never check.
* `"notify"` *(default when unset)* — check and announce.
* `"auto"` — check and install (verified), announce the result.

The legacy v1 `autoupdate` key maps the same way as oc-core's
normalize (`false`→disable, `true`→auto, `"notify"`→notify).
`OPENCODE_DISABLE_AUTOUPDATE` (any truthy value: `1`, `true`, …)
forces disable above everything.

**Announcement channels**: `notify`/`auto` messages go to stderr once
per detected version — except inside the TUI, where they are appended
to `~/.local/state/opencode-rust/update-notice.log` instead (printing
over the TUI screen would corrupt it). The background checker runs
only for the long-running entry points (default/TUI and `serve`), on
a single detached thread, re-checking every 10 minutes.

**Failure isolation (hard requirement)**: the background check never
adds boot latency and never affects operation. Feed requests are
bounded (5 s connect / 15 s total, one retry after 2 s); ANY failure —
DNS, connect refused, TLS garbage, HTTP error, parse error, rate
limit — is a silent no-op (no stderr noise, no exit-code impact, no
panic — the thread body is `catch_unwind`-wrapped). Repeated failures
accumulate nothing: one thread, fixed resources, PID-unique
`update-download-*` temps under the cache dir cleaned on failure and
swept on the next run. The explicit `opencode-rust upgrade` command
is the exception: it reports the network error clearly and exits
non-zero, with the installed binary untouched.

## Bun handling (the plugin runtime dependency)

Decision (MIGRATION §2.3): **managed pinned download + checksum +
graceful degradation** — never embedded, single binary stays light.

* Resolution order (`oc_util::bunman::resolve`, shared by the plugin
  host): `OC_PLUGIN_BUN` override → `~/.cache/opencode-rust/bin/bun` (the
  managed install) → `/usr/local/bin/bun` → `PATH` → `~/.bun/bin/bun`.
* When plugins are requested and NO bun exists
  (`PluginHost::start_detached_with` with entries, or
  `opencode-rust plugin add`), `oc_util::bunman::ensure()` downloads the
  pinned **bun 1.3.10**:
  `https://github.com/oven-sh/bun/releases/download/bun-v1.3.10/bun-linux-x64.zip`
  * sha256 pinned in source (`oc-util/src/bunman.rs::BUN_SHA256`) =
    `f57bc0187e39623de716ba3a389fda5486b2d7be7131a980ba54dc7b733d2e08`,
    captured 2026-10-10 from the real artifact (39,328,732 bytes) via
    `curl -fsSL <url> -o zip && sha256sum zip`. Any other bytes are
    rejected (`EnsureError::Checksum`) before extraction.
  * Installed to `~/.cache/opencode-rust/bin/bun`, mode 755, staged +
    atomically renamed, under a flock so concurrent bootstraps
    serialize.
* Failure of ANY step (offline, missing curl/wget/unzip, checksum,
  filesystem) degrades to a disabled plugin subsystem with one clear
  log line — the core product is unaffected.
* Kill switch: `OPENCODE_DISABLE_BUN_DOWNLOAD=1` (sandboxes, CI,
  air-gapped hosts). The workspace test profile sets it globally
  (`.config/nextest.toml`) so the suite never fetches.
* Only `linux-x64` is managed; other platforms resolve a system bun or
  stay disabled (the pin is about node-compat behavior, which is
  x64/linux where the parity corpus lives).
* Re-pinning procedure: download the new zip once, `sha256sum` it,
  update `BUN_VERSION`/`BUN_URL`/`BUN_SHA256` (one const block), and
  note the capture date. The fixture test
  (`checksum_verification_rejects_wrong_bytes`) guards that the pin
  and the committed fixture zip can never collide.

## Release smoke (`tools/release-smoke.sh`)

Run before shipping any candidate (argument: path to the release
binary; builds one if missing):

1. `--version` must print exactly `opencode-rust v2.0.24`.
2. `--version-rust` must carry the build stamp.
3. `serve` boots in a sandboxed home on a RANDOM port (`--port 0`),
   `/api/info` answers `2.0.24` over Basic auth, clean SIGTERM exit.
   (Never the managed default port 49374 — enforced.)
4. The full FakeLlm e2e battery (`oc-cli` `run_e2e`, the standalone
   `run`/stdio-lease flows, incl. the upgrade/uninstall tests) runs
   against the RELEASE binary via `OPENCODE_SMOKE_BIN`.

Current status: ALL GREEN against `target/release/opencode-rust`
(build `4c74e5f`, release profile).

## Gates

`cargo fmt --all --check` + `cargo clippy --workspace --all-targets --
-D warnings` + `cargo nextest` (green for this lane's crates: oc-util,
oc-plugin-host, oc-cli — note the workspace-wide fmt/clippy currently
also touches the parallel oc-server lane's in-flight files),
`tools/release-smoke.sh` green.

The clippy invocation must stay byte-equivalent to CI's (`.github/
workflows/ci.yml`: `cargo clippy --workspace --all-targets -D warnings
--locked`): local gates that omit `--all-targets` or `-D warnings`
leave exactly the drift CI then rejects (the lane/lint lesson — ~45
pedantic warnings accumulated in `tools/parity` because local muscle
memory ran bare `cargo clippy`).

### Accelerated verification protocol (lane/accel — the lane gate tiering)

Renderer-migration lanes iterate the tui-diff rig dozens of times a
day; the full matrix (both sizes × `--runs 2`, live bun per
invocation) is the FINAL bar, not the inner loop. The tiered protocol
is scripted by **`tools/parity/tui-diff/verify-lane.sh`** (see
`tui-diff/README.md` for the full contract):

* **`iterate`** (minutes): fmt + nextest for the affected crates
  (oc-tui, oc-tui-core, parity) + the rig's `--quick` mode (`--runs 1`,
  single default size 80x24) on the lane's scripts, with the
  **ref-capture cache** ON — the ref (bun) side is identical across our
  flag changes, so its fingerprint-keyed settled grids are replayed
  (per step, multi-run-confirmed stable only; unstable steps stay
  live) instead of re-settling bun every invocation. Cache at
  `tui-diff/cache/ref/` (gitignored), warmed once per fingerprint by
  `--measure-stability 5` — the cache must never change verdicts.
* **`final`** (the merge gate): CI-equivalent fmt+clippy, full
  workspace nextest, the serialized pty profile, the release budget
  gate, and the rig at BOTH sizes × `--runs 2` with the cache
  DISABLED — final verdicts are live verdicts.

Rule: **iterate with `--quick`, FINAL verification runs the full
matrix (both sizes × `--runs 2`)** — never merge off `--quick` alone.

### PTY invariant fuzz (`crates/oc-cli/tests/pty_fuzz.rs`)

A seeded, human-plausible key driver over the real TUI binary (pty +
FakeLlm-style stub) checking screen invariants after every key:
alive / no escape-response garbage (`rgb:` leak) / non-void screen /
escape-recoverability / bounded cell corruption. Failures dump the
exact reproducer to `/tmp/opencode/pty-fuzz-fail-s<seed>.txt`.

- CI smoke (serialized pty job): `pty_fuzz_smoke` — 3 fixed seeds ×
  120 keys.
- Nightly-style long pass (local, ~20× the smoke):
  `cargo nextest run -p oc-cli --run-ignored only -E 'test(pty_fuzz_long)'`
  — 20 seeds × 250 keys; sweep custom seeds/lengths via
  `OC_PTY_FUZZ_SEEDS=41,42,43 OC_PTY_FUZZ_KEYS=400`. Reproduce any
  dump by replaying its hex key list (each line = one key write).

---

## GitHub Actions release pipeline (added for the beta program)

The repo ships two workflows:

- **`.github/workflows/ci.yml`** — push/PR gate: `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -D warnings --locked`, and the
  full `cargo nextest run --workspace` suite (2191 tests) on
  `ubuntu-latest`. One CI-only environment step: `mkdir -p /tmp/opencode`
  (the scratch root tests stage their PID-unique sandboxes under; the dev
  box pre-creates it, runners do not). Toolchain pinned via
  `rust-toolchain.toml` (1.99.0 = the version every gate ran on).
- **`.github/workflows/release.yml`** — tag-triggered (`v*`):
  `web-assets` builds the embedded UI archive (unchanged, R-04), then a
  **platform matrix `build` job** (R-11) builds
  `cargo build --release -p oc-cli -p oc-pty --locked --target <rust
  target>` per target (with `OC_RELEASE_TAG` stamped for the update
  channel and `OPENCODE_APP_ARCHIVE` wired from the web-assets
  artifact), packages a tarball (zip on windows) + `sha256sum` sidecar
  per target, smoke-tests BOTH packaged binaries (`--version` from
  inside the archive; `oc-pty` exec'd with a bad flag and REQUIRED to
  fail with its usage exit 2 — it has no `--version`), and uploads
  per-target artifacts. A final tag-only **`publish` job** collects
  every artifact and cuts ONE GitHub release; a matrix leg that did
  not produce an artifact is called out as _(absent)_ in the notes
  table — never silently dropped. Tags containing `beta` publish as
  **prerelease**.

### Version scheme during acceptance

Tags: `v2.0.24.beta.1`, `v2.0.24.beta.2`, … until acceptance; the final
release is plain `v2.0.24`. The tag version (leading `v` stripped) names
the assets. **The binary itself keeps reporting version `2.0.24`** (as
`opencode-rust v2.0.24`) — the upstream-parity pin is a registered
decision (client/server handshake, plugin host `app_version`, golden
`--version`); beta numbering lives in git/Release metadata only.

### Consumer URL shape

```
https://github.com/Eternal130/OpenCode-Rust/releases/download/<tag>/opencode-<version-without-v>-linux-x64.tar.gz
```

Feed it to the installer via `--url` (or `$RELEASE_URL`); there is still
no default URL by design (the 2.0.24 pin must never adopt TS releases).

## Platform matrix (R-11)

Every target below is a matrix leg of the release workflow's `build`
job; each produces a `opencode-<version>-<target>` archive (+ `.sha256`
sidecar) carrying BOTH binaries, uploaded as its own artifact and
published by the `publish` job.

| Target | Asset suffix | Runner | Toolchain | Status |
|---|---|---|---|---|
| linux x86_64 (glibc) | `linux-x64` | `ubuntu-latest` | native | **shipped** since Phase 10; the suffix is FROZEN (update-channel asset expectation) |
| linux aarch64 (glibc) | `linux-arm64` | `ubuntu-24.04-arm` | native | new (R-11) |
| linux x86_64 (musl, static) | `linux-x64-musl` | `ubuntu-latest` | `musl-tools` (musl-gcc) | new (R-11); watch item below |
| linux aarch64 (musl, static) | `linux-arm64-musl` | `ubuntu-24.04-arm` | `musl-tools` (musl-gcc) | new (R-11); same watch item |
| macOS aarch64 (Apple silicon) | `darwin-arm64` | `macos-latest` | native | new (R-11). macOS x86_64 (Intel) is NOT shipped — Intel Macs build from source |
| windows x86_64 | `windows-x64` (zip) | `windows-latest` | MSVC | new (R-11), **EXPERIMENTAL build-only** — see KNOWN-ISSUE-R11-WINDOWS |

Notes:

* **Target naming** follows the installer's `{target}` convention
  (`uname -s` lowercased + `uname -m` mapped: x86_64→x64,
  aarch64→arm64), so `--url …/{target}` templates resolve on every
  shipped unix platform — except the `-musl` suffixes, which `uname`
  cannot produce: musl assets are direct-download only.
* **Runner strategy**: native runners wherever they exist (no cross
  toolchains); `ubuntu-24.04-arm` requires the public-repo free ARM
  runner tier (a private repo would need paid larger runners). musl
  has no native runner — the standard `musl-tools` recipe (musl-gcc
  for the host arch) compiles for the HOST arch under musl
  (x86_64-musl on the x64 runner, aarch64-musl on the arm
  runner), so those binaries still smoke-run natively.
* **R10a (rmcp → aws-lc-rs)**: `aws-lc-sys` compiles C, so every
  builder needs a **C compiler for the target**. Framing correction
  (verified against the aws-lc-sys 0.46 builder): the non-FIPS crate
  uses the **cc builder by default** — the cmake builder is only
  selected for FIPS, `AWS_LC_SYS_NO_ASM`, or bindgen-requiring
  setups — and the crate ships pregenerated bindings, so **neither
  cmake nor bindgen is ever required** here. The workflow's cmake
  install steps are kept as harmless over-insurance (the cmake
  builder remains the fallback path if cc's dependency check fails).
  The musl legs additionally wire
  `CC_<target>`/`CARGO_TARGET_<target>_LINKER` to `musl-gcc` —
  musl-tools (musl-gcc) is the true musl dependency. Windows
  additionally sets `AWS_LC_SYS_PREBUILT_NASM=1` on the build step
  (windows-latest has no NASM; the non-FIPS MSVC build hard-fails
  its asm step without the prebuilt objects).
* **KNOWN-ISSUE-R11-MUSL (watch item)**: `aws-lc-sys` under musl is
  the historically painful seam. The legs are wired strict (NO
  `continue-on-error`): a red musl leg fails the matrix, `publish`
  does not run, and the release is blocked — the honest signal. IF a
  tagged run fails inside `aws-lc-sys`'s C build in a way that is
  upstream-infeasible from the logs alone, flip the leg to
  `experimental: true` (continue-on-error) and pin the failing
  `aws-lc-sys` version + error here; the other platforms then publish
  with the musl rows visibly _(absent)_ in the notes.
* **KNOWN-ISSUE-R11-WINDOWS**: first-ever Windows build. The workspace
  has unix-only seams (oc-pty's Unix-socket wire, safekill's
  pidfd/procfs, `std::os::unix` in oc-core) that may need cfg-gating
  before `oc-cli`/`oc-pty` link on MSVC. The leg is registered
  EXPERIMENTAL (`continue-on-error`): it runs on every tag, and the
  windows asset ships only once the leg actually goes green (the notes
  table shows the absence until then). Tests are not attempted
  (build-only, per the R-11 ruling). First-run evidence (2026-10-24,
  dispatch run 39998955269): aws-lc-sys BUILDS on windows-latest with
  `AWS_LC_SYS_PREBUILT_NASM=1`; the leg fails in workspace crates on the
  predicted unix-seam class — `std::os::unix`/`std::os::fd` imports,
  `libc` refs, `Permissions::from_mode`/`.mode()`, `as_raw_fd`
  (E0433/E0432/E0599 in oc-core/oc-pty). Fix path = a cfg-gating sweep;
  no crypto/toolchain blockers.
* **runtime-gates.yml keeps its own `platform-build` macOS job** (the
  §12.4 SURF-PLATFORM dev-loop leg: whole-workspace `cargo build` on
  `macos-latest`, dispatch-gated). The two are complementary, not
  duplicated: runtime-gates proves the workspace COMPILES on macOS
  during development; release.yml's `darwin-arm64` leg produces the
  macOS release artifact. Consolidating them would couple the dev-loop
  proof to tag-only execution — deliberately not done (stated ruling).


## Web UI asset archive (R-04)

The reference web app (anomalyco/opencode `packages/app`, Solid/Vite/PWA)
publishes no standalone artifact at v2.0.24 — the dist is built from a
**pinned ref** in CI and embedded into the binary (ROADMAP §R-04,
decided 2026-10-11). There is deliberately **no local-ref build
discipline exception**: CI is the executor; locally you only consume.

### Pipeline (`.github/workflows/release.yml`)

1. **`web-assets` job** (also runnable alone via `workflow_dispatch` for
   iteration): fetch the pinned ref tarball
   (`https://github.com/anomalyco/opencode/archive/<APP_REF>.tar.gz`,
   curl with retries), `oven-sh/setup-bun`, `bun install` at the ref
   root, `cd packages/app && VITE_OPENCODE_SERVER_MODE=origin bun run
   build`, then `bun tools/web-assets/package.ts --dist
   ref/packages/app/dist --ref <APP_REF> --out app-archive.json`
   (node:zlib **brotli q6** + base64 per file, self-describing JSON with
   provenance). Uploaded as the `app-archive` artifact; the bun install
   cache is `actions/cache`-keyed on the ref.
2. **Platform matrix `build` job** (`needs: [web-assets]`, R-11 — the
   old single `linux-x64` job is gone): each target leg downloads the
   artifact, exports `OPENCODE_APP_ARCHIVE=<path>` so `oc-server/build.rs`
   stages it into `OUT_DIR` for compile-time embedding, builds
   `oc-cli` + `oc-pty`, packages + exec-smokes the archive (§ Platform
   matrix), and uploads a per-target artifact.
3. **`publish` job** (tag-only): collects every per-target artifact and
   cuts ONE GitHub release; the release notes carry the Web-UI size
   line (bytes/files/ref) composed from the downloaded
   `app-archive.json` — there is no separate `web-ui-summary.md` step.

**Fail-loud guard**: if `web-assets` did not succeed, the build job
dies before `cargo build` — a broken app build must never ship a
silently UI-less binary. Emergency escape hatch (documented): the
`skip_web` workflow_dispatch input, or the `WEB_ASSETS_SKIP=true`
**repository variable** (tag pushes have no inputs context) — either
skips `web-assets` AND suppresses the guard, producing an API-only
binary. Revert the variable immediately after the emergency.

### Bumping the pinned ref

`APP_REF` lives once, in the workflow `env:` block. A bump is a
deliberate change (it swaps the shipped UI): dispatch `web-assets`
alone first, inspect the size line, run the local smoke below against
the produced archive, then cut the tag.

### Local browser smoke (orchestrator/manual — no CI needed)

Runtime override accepts an **archive file OR an unpacked dist dir**,
so neither the packager nor a release build is required:

```sh
# 1. Clone the reference app at the pinned ref and build it (network +
#    bun; one-off, NOT part of any gate):
git clone https://github.com/anomalyco/opencode /tmp/opencode/ref
git -C /tmp/opencode/ref checkout e7a34f09bfd9134dfade5a8ddb843f7030bc9a69
bun install --cwd /tmp/opencode/ref
VITE_OPENCODE_SERVER_MODE=origin bun run --cwd /tmp/opencode/ref/packages/app build

# 2a. Serve the unpacked dist directly (simplest):
export OPENCODE_APP_ASSETS=/tmp/opencode/ref/packages/app/dist

# 2b. Or package it first (production-shaped, brotli q6):
bun tools/web-assets/package.ts \
  --dist /tmp/opencode/ref/packages/app/dist \
  --ref e7a34f09bfd9134dfade5a8ddb843f7030bc9a69 \
  --out /tmp/opencode/app-archive.json
export OPENCODE_APP_ASSETS=/tmp/opencode/app-archive.json

# 3. Serve (dev-gate wrapper for the cargo build; the env var rides
#    through):
tools/dev-gate.sh 'cargo run -p oc-cli -- serve --port 49374'
# stderr must show: opencode-rust: web ui: N files (ref e7a34f0...)

# 4. Open http://127.0.0.1:49374 (set OPENCODE_PASSWORD first; login is
#    opencode / <password>, or a pairing link per the auth flow).
```

A bad `OPENCODE_APP_ASSETS` path fails boot loudly (exit, stderr
message). With neither override nor embedded archive the server is
API-only — the historical default every test build pins.

## TUI differential parity job (future CI — lane/diff-rt, not wired yet)

The side-by-side TUI differential (`tools/parity/tui-diff/`, the
`OC_TUI_DIFF` gate) needs the **runnable v2.0.24 checkout** at
`/tmp/opencode/ref` (bun + node_modules), exactly like the local
parity harness self-heals it (`tools/parity/src/reference.rs`:
tar copy from the pinned `/opt/opencode-ref` source, then
`bun install --frozen-lockfile`). When CI grows a job for it, model it
on `web-assets`:

1. **`ref-checkout` job**: fetch the pinned ref tarball (same URL +
   `APP_REF` env as `web-assets`), `oven-sh/setup-bun`, untar to
   `ref/`, `bun install --frozen-lockfile` at the ref root (cache on
   the ref hash). Export a `PARITY_REF_ROOT` pointing at it and upload
   as the workspace for the next job (or cache it — ~2 GB with
   node_modules; the cache route matches `web-assets`' philosophy).
2. **`tui-diff` job** (manual/nightly cadence, NOT per-PR — a full
   corpus ×2 runs ×2 sizes takes ~1–2 h): `PARITY_BUN` pointed at the
   runner's bun (GitHub runners install bun world-readable via
   `oven-sh/setup-bun`, so the local `/root` permission problem does
   not apply), then `cargo run -p parity --bin tui-diff -- --runs 2`
   under the usual caps. The runner exits 0 as long as the RIG
   executed (boots, settles, captures) — content mismatches are the
   REPORT, not a red X; the current backlog lives in
   `tools/parity/tui-diff/REPORT.md`. Once the backlog is burned down,
   flip the `--strict` flag (exits 1 on any driver error or
   deterministic MISMATCH step) and move the job to per-PR.

Why not per-PR today: the mismatch backlog is the point of the rig —
every deterministic mismatch is a fix-wave work item, and CI red on a
known backlog teaches nothing. The `OC_TUI_DIFF=1` nextest gate
(boot-home only) is the cheap per-PR variant once the ref checkout is
cached on the runner.
