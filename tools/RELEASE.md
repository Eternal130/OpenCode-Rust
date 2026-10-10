# Release artifacts (Phase 10 packaging)

What ships for the Rust port of opencode v2.0.24, how it is built, and
how the optional runtime dependencies are handled.

## Artifact set

| Artifact | Path | Notes |
|---|---|---|
| Release binary | `cargo build --release -p oc-cli` → `target/release/opencode` (~46 MiB, thin-LTO + symbol-strip via the workspace `[profile.release]`) | The single command: CLI + server + TUI. Reports `opencode v2.0.24` (`--version`) and `opencode v2.0.24 (rust port build <git-short>, <profile> profile)` (`--version-rust`, hidden; stamped by `crates/oc-cli/build.rs`). |
| Installer | `install/install.sh` | Mirrors v2's installer UX shape (versioned download → `~/.opencode/bin/opencode`, chmod 755, shell-config PATH hint, ASCII banner). See below for the parameterization. |
| Release smoke | `tools/release-smoke.sh` | Gate for any release candidate; see below. |
| Release tarball | NOT in-repo — produce at packaging time: `tar -czf opencode-<version>-<target>.tar.gz package/opencode` (a `package/opencode` or `opencode` member; both the installer and `opencode upgrade` accept either, plus zip and raw-binary artifacts) | Publish alongside `<tarball>.sha256` sidecars (`sha256sum` format) — the installer auto-uses them. |

## Version stamping

* The workspace pins `version = "2.0.24"`; the binary reports
  **exactly** `2.0.24` (`env!("CARGO_PKG_VERSION")`) because service
  version negotiation is exact-string and the Rust build must be a
  drop-in peer of v2.0.24 (MIGRATION §2.4).
* `crates/oc-cli/build.rs` additionally stamps the git short hash +
  compilation profile for the hidden `--version-rust` flag.
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
Install destination `~/.opencode/bin/opencode`, mode 755, PATH hint
written to the detected shell rc (zsh/bash/fish/ash/sh lists identical
to v2), `$GITHUB_PATH` appended in Actions.

## `opencode upgrade` / `opencode uninstall` (real, Phase 10)

* **Auto-update stays default-OFF** (oracle gate 1 / incident-era
  decision): the Rust port ships no background update loop. Only the
  explicit `opencode upgrade` ever replaces the binary; the default
  invocation (no target) reports "already installed" and says so.
* Methods (`--method`):
  * `local-file` *(Rust-port extension, real)* — target is a path to a
    binary / tar.gz / zip on disk; payload extracted, staged, and
    atomically renamed over the running binary (unix self-replace;
    mode 755).
  * `github` *(wired, network-optional)* — downloads
    `$OPENCODE_RELEASE_URL` (with `{version}`/`{target}` placeholders),
    extracts, replaces. Without the env it is a clean typed error — it
    will NOT fall back to opencode.ai (TS-stream protection).
  * `curl` — auto-detected when the running binary IS
    `~/.opencode/bin/opencode`; behaves as `github` (same env).
  * `brew` — detected under a Cellar prefix; errors with the
    `brew upgrade opencode` guidance.
  * `npm`/`pnpm`/`bun`/`yarn`/`vp` — typed error: those streams carry
    TS releases this port must not adopt.
* `uninstall` (v2 flag surface: `--keep-config`, `--keep-data`,
  `--dry-run`, `--force`): stops every `service*.json` registration
  (safekill path), removes data/cache/state (+config unless kept),
  cleans the `~/.opencode/bin` PATH lines from shell configs, then
  self-unlinks the binary. Non-TTY without `--force` errors with v2's
  `requireInteractive` message.

## Bun handling (the plugin runtime dependency)

Decision (MIGRATION §2.3): **managed pinned download + checksum +
graceful degradation** — never embedded, single binary stays light.

* Resolution order (`oc_util::bunman::resolve`, shared by the plugin
  host): `OC_PLUGIN_BUN` override → `~/.cache/opencode/bin/bun` (the
  managed install) → `/usr/local/bin/bun` → `PATH` → `~/.bun/bin/bun`.
* When plugins are requested and NO bun exists
  (`PluginHost::start_detached_with` with entries, or
  `opencode plugin add`), `oc_util::bunman::ensure()` downloads the
  pinned **bun 1.3.10**:
  `https://github.com/oven-sh/bun/releases/download/bun-v1.3.10/bun-linux-x64.zip`
  * sha256 pinned in source (`oc-util/src/bunman.rs::BUN_SHA256`) =
    `f57bc0187e39623de716ba3a389fda5486b2d7be7131a980ba54dc7b733d2e08`,
    captured 2026-10-10 from the real artifact (39,328,732 bytes) via
    `curl -fsSL <url> -o zip && sha256sum zip`. Any other bytes are
    rejected (`EnsureError::Checksum`) before extraction.
  * Installed to `~/.cache/opencode/bin/bun`, mode 755, staged +
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

1. `--version` must print exactly `opencode v2.0.24`.
2. `--version-rust` must carry the build stamp.
3. `serve` boots in a sandboxed home on a RANDOM port (`--port 0`),
   `/api/info` answers `2.0.24` over Basic auth, clean SIGTERM exit.
   (Never the managed default port 49374 — enforced.)
4. The full FakeLlm e2e battery (`oc-cli` `run_e2e`, the standalone
   `run`/stdio-lease flows, incl. the upgrade/uninstall tests) runs
   against the RELEASE binary via `OPENCODE_SMOKE_BIN`.

Current status: ALL GREEN against `target/release/opencode`
(build `4c74e5f`, release profile).

## Gates

`cargo fmt --check` + `cargo clippy -D warnings` + `cargo nextest`
(green for this lane's crates: oc-util, oc-plugin-host, oc-cli — note
the workspace-wide fmt/clippy currently also touches the parallel
oc-server lane's in-flight files), `tools/release-smoke.sh` green.
