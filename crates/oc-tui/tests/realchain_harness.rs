//! CHANNEL 9 (real-chain) W1 harness — the machine half of the six
//! beta.15 symptom reproductions.
//!
//! Topology (the approved W1 design — the ONLY stub is the replay
//! gateway; everything else is the production assembly):
//!
//! ```text
//! real PTY keystrokes → real TUI process (the `opencode-rust`
//!   binary, production boot: oc-cli root handler → managed
//!   background oc-server via lifecycle::ensure — real config
//!   discovery, real storage, sandboxed HOME/XDG roots)
//!                          ↓ HTTP (loopback)
//!               replay gateway (tools/diff/realchain/replay_gateway.py)
//!               — speaks the real anthropic-messages wire protocol,
//!                 matches by normalized request hash, replays
//!                 recorded SSE bodies byte-for-byte; strict 404 on a
//!                 miss.
//! ```
//!
//! Boot topology conclusion (read from the code, not guessed): the
//! production TUI NEVER embeds the server — `oc-cli`'s root handler
//! resolves a server through `services::server_connection` (explicit
//! `--server`, `--standalone` stdio lease, or the MANAGED background
//! daemon via `oc_server::lifecycle::ensure`, registration under
//! `$XDG_STATE_HOME/opencode/service.json`). This harness uses the
//! managed-daemon path (the default a real user gets): `opencode-rust
//! service start` under the sandbox env, then the TUI attaches to the
//! same registration.
//!
//! Sandbox discipline: every root lives under
//! `/tmp/opencode/t9w1-<pid>-<tag>` (HOME,
//! `XDG_CONFIG/DATA/STATE/CACHE`); the child env is scrubbed of real-user OPENCODE_* vars; a
//! cross-process flock serializes the tests (nextest runs each test
//! in its own process — the in-process `pty_lock` pattern cannot
//! reach across). Teardown is graceful: exit chord → wait → the
//! binary's own `service stop` under the same sandbox → gateway child
//! handle teardown. No naked PIDs, no signal-by-name utilities.

#![allow(unsafe_code)] // forkpty/ioctl/pre_exec are libc FFI by nature
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
// Each test compile unit includes this shared harness but uses a
// different subset of the lane API.
#![allow(dead_code)]
// Test-harness infra lints (see realchain_pair.rs's list for the
// rationale): `{:?}` forensics in panics, wire-shape names in docs,
// explicit bridge steps.
#![allow(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::items_after_statements,
    clippy::manual_assert,
    clippy::map_or_identity,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::must_use_candidate,
    clippy::needless_continue,
    clippy::redundant_closure_for_method_calls,
    clippy::semicolon_if_nothing_returned,
    clippy::similar_names,
    clippy::unnecessary_debug_formatting
)]

use std::ffi::CString;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

// ---------------------------------------------------------------------------
// Workspace roots / logs
// ---------------------------------------------------------------------------

/// The worktree root (`crates/oc-tui/tests` → up three: tests, oc-tui, crates).
fn worktree_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

/// The production binary built from this worktree. Honors an explicit
/// `CARGO_TARGET_DIR` (W8: lane verification on a disk-tight box reuses
/// a sibling lane's build cache; cargo's own `CARGO_BIN_EXE_*` paths
/// follow the same env, so the harness must agree with it).
fn opencode_bin() -> PathBuf {
    opencode_bin_pub()
}

/// G-454 forensics — the production binary path for out-of-harness
/// service launches (stderr-captured daemon starts).
pub fn opencode_bin_pub() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => Path::new(&dir).join("debug/opencode-rust"),
        None => worktree_root().join("target/debug/opencode-rust"),
    }
}

// ---------------------------------------------------------------------------
// Binary freshness guard (W3 hardening ①)
// ---------------------------------------------------------------------------

/// Fail fast when `target/debug/opencode-rust` is older than any source
/// it is built from — a stale binary silently turns real-chain asserts
/// into false reds/greens. Sources = every `crates/**/src` file, every
/// crate manifest, and the workspace `Cargo.toml`/`Cargo.lock`.
pub fn ensure_fresh_binary() {
    let bin = opencode_bin();
    let meta = std::fs::metadata(&bin).unwrap_or_else(|_| {
        panic!(
            "production binary missing: {} (build it via tools/dev-gate.sh first)",
            bin.display()
        )
    });
    let built = meta.modified().unwrap_or_else(|_| panic!("mtime of {}", bin.display()));
    let root = worktree_root();
    let newest_path = std::cell::RefCell::new(None);
    let newest_time = std::cell::RefCell::new(None);
    let consider = |path: PathBuf, mtime: std::time::SystemTime| {
        let better = match *newest_time.borrow() {
            Some(best) => mtime > best,
            None => true,
        };
        if better {
            *newest_path.borrow_mut() = Some(path);
            *newest_time.borrow_mut() = Some(mtime);
        }
    };
    for entry in walkdir_shallow(&root.join("crates")) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() && name == "target" {
            continue;
        }
        if path.is_file()
            && (Path::new(&name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                || name == "Cargo.toml")
        {
            if let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) {
                consider(path, mtime);
            }
        }
    }
    for name in ["Cargo.toml", "Cargo.lock"] {
        let path = root.join(name);
        if let Ok(mtime) = path.metadata().and_then(|m| m.modified()) {
            consider(path, mtime);
        }
    }
    if let (Some(path), Some(mtime)) = (newest_path.into_inner(), newest_time.into_inner()) {
        // 2s grace: mtime granularity + touch-vs-write skew.
        assert!(
            mtime <= built + std::time::Duration::from_secs(2),
            "stale binary: {} (mtime {mtime:?}) is newer than {} \
             (mtime {built:?}) — rebuild via \
             tools/dev-gate.sh 'cargo build --locked -p oc-cli' before rerunning",
            path.display(),
            bin.display()
        );
    }
}

/// Minimal directory walker (no external deps): yields every file under
/// `root`, skipping build-output and non-binary source dirs (`target`,
/// `tests`, `benches`, `examples`, `src/bin` stays in — it links).
fn walkdir_shallow(root: &Path) -> Vec<std::fs::DirEntry> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !matches!(name.as_str(), "target" | "tests" | "benches" | "examples") {
                    stack.push(path);
                }
            } else {
                out.push(entry);
            }
        }
    }
    out
}

/// One `.test-logs` artifact file per harness instance.
fn log_file(tag: &str) -> PathBuf {
    let dir = worktree_root().join(".test-logs");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    dir.join(format!("t9-{tag}-{stamp}-t9w1.log"))
}

/// The cross-process lane lock (nextest = one process per test; a
/// std Mutex cannot serialize those). Held for the whole test.
pub struct LaneGuard {
    _file: File,
}

#[must_use]
pub fn lane_lock() -> LaneGuard {
    let path = Path::new("/tmp/opencode").join("t9w1-realchain.lock");
    let file = File::create(&path).unwrap_or_else(|_| panic!("open {}", path.display()));
    unsafe {
        let rc = libc::flock(file.as_raw_fd(), libc::LOCK_EX);
        assert_eq!(rc, 0, "flock {}", path.display());
    }
    LaneGuard { _file: file }
}

// ---------------------------------------------------------------------------
// Sandbox
// ---------------------------------------------------------------------------

pub struct Sandbox {
    pub root: PathBuf,
    pub project: PathBuf,
    pub gateway_port: u16,
    /// The `--redact` sandbox-root substitution handed to the gateway
    /// (PID-unique paths hash equal across runs).
    pub service_port: u16,
    /// W7 fast execution: when set, this dir's `shims/` is prepended
    /// to the sandbox PATH (the recorded-command PATH shims; see
    /// `tools/diff/realchain/shim_extract.py`).
    pub shim_dir: Option<PathBuf>,
}

impl Sandbox {
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        // The server's global config dir is the SHARED v2 one,
        // `$HOME/.config/opencode` (boot.rs dogfood2 bug 3 — it
        // deliberately ignores XDG_CONFIG_HOME for config).
        self.root.join("home/.config/opencode/opencode.json")
    }

    /// The port's global DATA root (`$XDG_DATA_HOME/opencode-rust`) —
    /// where `opencode.db` and the legacy `auth.json` live (the W13
    /// migration scenarios preset both).
    #[must_use]
    pub fn data_root(&self) -> PathBuf {
        self.root.join("xdg-data/opencode-rust")
    }

    /// The LEGACY v2 (upstream `opencode`) data root in this sandbox —
    /// the one-way v2 migration's source (`oc_core::v2_migration`).
    #[must_use]
    pub fn legacy_data_root(&self) -> PathBuf {
        self.root.join("xdg-data/opencode")
    }

    /// The legacy v2 auth.json path inside this sandbox.
    #[must_use]
    pub fn auth_json(&self) -> PathBuf {
        self.legacy_data_root().join("auth.json")
    }

    /// The port's database file inside this sandbox.
    #[must_use]
    pub fn db_path(&self) -> PathBuf {
        self.data_root().join("opencode.db")
    }

    #[must_use]
    pub fn env(&self) -> Vec<(String, String)> {
        let s = &self.root;
        vec![
            ("HOME".into(), s.join("home").to_string_lossy().into_owned()),
            ("XDG_CONFIG_HOME".into(), s.join("xdg-config").to_string_lossy().into_owned()),
            ("XDG_DATA_HOME".into(), s.join("xdg-data").to_string_lossy().into_owned()),
            ("XDG_STATE_HOME".into(), s.join("xdg-state").to_string_lossy().into_owned()),
            ("XDG_CACHE_HOME".into(), s.join("xdg-cache").to_string_lossy().into_owned()),
            ("TERM".into(), "xterm-256color".into()),
            // A unique daemon port per sandbox: the dev-channel default
            // (0xc0de) would collide if two sandboxes ever overlapped.
            ("COLUMNS".into(), "120".into()),
            ("LINES".into(), "36".into()),
        ]
    }

    /// Scrubbed child env: sandbox vars in, real-user OPENCODE_* out.
    #[must_use]
    pub fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for (key, value) in std::env::vars() {
            // Keep PATH/locale so the binary can exec python-free helpers;
            // drop everything OPENCODE_* (never inherit real creds/config).
            if key.starts_with("OPENCODE") || key == "HOME" || key.starts_with("XDG_") {
                continue;
            }
            cmd.env(key, value);
        }
        for (key, value) in self.env() {
            cmd.env(key, value);
        }
        if let Some(shims) = &self.shim_dir {
            prepend_path(&mut cmd, &shims.join("shims"));
        }
        cmd.current_dir(&self.project);
        cmd
    }

    /// Write the global config: the custom `replay` provider
    /// (anthropic wire family via `package: "@ai-sdk/anthropic"`)
    /// pointing at the gateway, plus its model entries. `plugins` =
    /// config `plugins[]` specs (the W5 real-plugin rig).
    pub fn write_config(&self, extra_models: &[(&str, &str)], default_model: &str) {
        self.write_config_ext(extra_models, default_model, &[])
    }

    pub fn write_config_ext(
        &self,
        extra_models: &[(&str, &str)],
        default_model: &str,
        plugins: &[&str],
    ) {
        let mut models = serde_json::Map::new();
        models.insert("claude-opus-4-7".into(), serde_json::json!({ "name": "claude-opus-4-7" }));
        for (id, name) in extra_models {
            models.insert((*id).into(), serde_json::json!({ "name": name }));
        }
        let mut config = serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "model": default_model,
            "autoupdate": false,
            "providers": {
                "replay": {
                    "name": "Replay Anthropic",
                    "package": "@ai-sdk/anthropic",
                    "settings": {
                        "baseURL": format!("http://127.0.0.1:{}/v1", self.gateway_port),
                        "apiKey": "replay-sandbox-key"
                    },
                    "models": models
                }
            }
        });
        if !plugins.is_empty() {
            config["plugins"] = serde_json::json!(plugins);
        }
        let path = self.config_path();
        std::fs::create_dir_all(path.parent().unwrap()).expect("config dir");
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
            .unwrap_or_else(|e| panic!("write config: {e}"));
        // Unique daemon port per sandbox (service.json — `ServiceConfig.Info.port`).
        let service = serde_json::json!({ "port": self.service_port });
        let service_path = self.root.join("xdg-config/opencode-rust/service.json");
        std::fs::create_dir_all(service_path.parent().unwrap()).expect("service config dir");
        std::fs::write(&service_path, serde_json::to_string_pretty(&service).unwrap())
            .unwrap_or_else(|e| panic!("write service config: {e}"));
    }

    /// The daemon registration written by `lifecycle::register` —
    /// `service start` prints only the URL; the password lives here.
    #[must_use]
    pub fn registration(&self) -> serde_json::Value {
        for base in ["opencode-rust", "opencode"] {
            for name in ["service.json", "service-dev.json", "service-beta.json"] {
                let path = self.root.join("xdg-state").join(base).join(name);
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        if value.get("url").is_some() {
                            return value;
                        }
                    }
                }
            }
        }
        serde_json::Value::Null
    }
}

#[must_use]
pub fn worktree_dir(rel: &str) -> PathBuf {
    worktree_root().join(rel)
}

/// Committed synthetic seed corpus (rust-engine request shapes).
#[must_use]
pub fn corpus_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus")
}

/// Committed synthetic seed corpus (ref-engine request shapes).
#[must_use]
pub fn corpus_ref_dir() -> PathBuf {
    worktree_dir("tools/diff/realchain/corpus-ref")
}

#[must_use]
pub fn sandbox(tag: &str, gateway_port: u16) -> Sandbox {
    let root = Path::new("/tmp/opencode").join(format!("t9w1-{}-{tag}", std::process::id()));
    for dir in ["home", "xdg-config", "xdg-data", "xdg-state", "xdg-cache", "project"] {
        std::fs::create_dir_all(root.join(dir)).expect("sandbox dir");
    }
    std::fs::write(root.join("project/README.md"), "# t9 sandbox project\n").expect("project file");
    let service_port = 20_000 + (std::process::id() % 20_000) as u16;
    Sandbox {
        root: root.clone(),
        project: root.join("project"),
        gateway_port,
        service_port,
        shim_dir: None,
    }
}

/// Prepend `dir` to a prepared command's PATH (the W7 shim lane: the
/// recorded-command shims must shadow the real tools inside the
/// sandbox, and the daemon's own bash-tool spawns inherit this PATH).
fn prepend_path(cmd: &mut Command, dir: &Path) {
    let inherited = std::env::var("PATH").unwrap_or_default();
    let current = cmd
        .get_envs()
        .find(|(key, _)| *key == "PATH")
        .and_then(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()));
    let base = current.unwrap_or(inherited);
    cmd.env("PATH", format!("{}:{}", dir.display(), base));
}

// ---------------------------------------------------------------------------
// Replay gateway child
// ---------------------------------------------------------------------------

pub struct Gateway {
    pub port: u16,
    child: Child,
    log_path: PathBuf,
}

/// The default gateway pace tier (W8 CI hook): the `T9_PACE` env
/// override, default `instant` (the historical single-write shape).
/// Scenarios that pin a fidelity mode (P1-P3) pass their mode to
/// [`Gateway::spawn_paced`] explicitly and are NOT affected.
#[must_use]
pub fn default_pace() -> String {
    std::env::var("T9_PACE")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "instant".to_owned())
}

impl Gateway {
    /// Spawn the gateway; `record_dir` = bootstrap seeding mode (writes
    /// generated cassettes — sandbox traffic only, no user data). The
    /// pace tier is [`default_pace`] (the `T9_PACE` env override,
    /// default `instant` — the historical shape).
    #[must_use]
    pub fn spawn(tag: &str, record_dir: Option<&Path>) -> Gateway {
        Gateway::spawn_paced(tag, record_dir, &default_pace())
    }

    /// W5 timing fidelity: spawn with an explicit `--pace` mode
    /// (`instant` keeps the historical single-write behavior).
    pub fn spawn_paced(tag: &str, record_dir: Option<&Path>, pace: &str) -> Gateway {
        Gateway::spawn_ext(tag, &[], record_dir, None, pace, "digest")
    }

    /// W13 content-boundary lane: digest-mode gateway with EXPLICIT
    /// seed files (record mode) — each scenario's synthetic payload
    /// (CJK wrap / emoji ZWJ / long-line / empty reply) is its own
    /// `--seed-reply` body, recorded once into the committed
    /// `corpus-w13{,-ref}` dirs by the `w13x_*` bootstrap tests.
    #[must_use]
    pub fn spawn_payload_seeded(
        tag: &str,
        record_dir: Option<&Path>,
        seed_reply: &Path,
        seed_title: &Path,
    ) -> Gateway {
        Gateway::spawn_ext_seeds(
            tag,
            &[],
            record_dir,
            None,
            Some((seed_reply, seed_title)),
            &default_pace(),
            "digest",
        )
    }

    /// W7 turn-anchored mode: `--match turn` with the tool-loop corpus
    /// dirs; `bootstrap` = (record_dir, seed_turns_dir) enables the
    /// one-shot scripted-seed recording of the committed corpus.
    pub fn spawn_turn(
        tag: &str,
        tool_corpus: &[PathBuf],
        bootstrap: Option<(&Path, &Path)>,
    ) -> Gateway {
        let seed = bootstrap.map(|(record, turns)| (record.to_path_buf(), turns.to_path_buf()));
        let (record_dir, seed_turns) = match seed {
            Some((record, turns)) => (Some(record), Some(turns)),
            None => (None, None),
        };
        Gateway::spawn_ext(
            tag,
            tool_corpus,
            record_dir.as_deref(),
            seed_turns.as_deref(),
            // The lane's pinned grids are INSTANT-pace only (W9: a
            // T9_PACE slow tier streams the same content correctly
            // but shifts the spinner-frame phase and the footer
            // usage row — paced-grid fidelity is the P1-P3 lane's
            // surface, not this one), so the pace is pinned here and
            // deliberately ignores the T9_PACE override.
            "instant",
            "turn",
        )
    }

    fn spawn_ext(
        tag: &str,
        extra_corpus: &[PathBuf],
        record_dir: Option<&Path>,
        seed_turns: Option<&Path>,
        pace: &str,
        match_mode: &str,
    ) -> Gateway {
        Gateway::spawn_ext_seeds(tag, extra_corpus, record_dir, seed_turns, None, pace, match_mode)
    }

    /// The general spawner: `seeds` overrides the default
    /// `--seed-reply`/`--seed-title` bodies (the W13 payload lane);
    /// `None` keeps the channel-9 defaults.
    fn spawn_ext_seeds(
        tag: &str,
        extra_corpus: &[PathBuf],
        record_dir: Option<&Path>,
        // W7: turn-script bootstrap dir (--seed-turns; digest mode
        // ignores it).
        seed_turns: Option<&Path>,
        seeds: Option<(&Path, &Path)>,
        pace: &str,
        match_mode: &str,
    ) -> Gateway {
        let root = worktree_root();
        let log_path = log_file(&format!("gateway-{tag}"));
        let mut cmd = Command::new("python3");
        cmd.arg(root.join("tools/diff/realchain/replay_gateway.py"));
        cmd.arg("--port").arg("0");
        for corpus in [
            root.join("crates/oc-ai/test-fixtures/recordings"),
            root.join("tools/diff/realchain/corpus"),
            // W3: ref-side request shapes (recorded from the ref 2.0.24
            // engine; the title call normalizes to the SAME digest on both
            // ends and rides the shared corpus above).
            root.join("tools/diff/realchain/corpus-ref"),
            // W13: the content-boundary payloads (both engines' request
            // shapes for the cjk/emoji/longline/empty prompts).
            root.join("tools/diff/realchain/corpus-w13"),
            root.join("tools/diff/realchain/corpus-w13-ref"),
        ] {
            cmd.arg("--corpus").arg(&corpus);
        }
        // W7: the tool-loop corpora ride AFTER the shared ones (later
        // dirs override earlier on digest clashes; the turn table
        // unions every tagged interaction).
        for corpus in extra_corpus {
            cmd.arg("--corpus").arg(corpus);
        }
        // PID-unique sandbox paths must hash equal across runs. The ref
        // lane's root (`…-<tag>ref`) is a strict prefix extension of the
        // rust root, so it must be redacted FIRST.
        for redaction in [
            format!("/tmp/opencode/t9w1-{}-{}ref", std::process::id(), tag),
            format!("/tmp/opencode/t9w1-{}-{}", std::process::id(), tag),
        ] {
            cmd.arg("--redact").arg(format!("{redaction}:__SANDBOX__"));
        }
        cmd.arg("--log").arg(&log_path);
        // Miss forensics (G-431 follow-ups): the normalized request of
        // every strict miss lands next to the gateway log for corpus
        // digest-drift diffing.
        cmd.arg("--dump-misses").arg(log_path.with_file_name(format!("gateway-{tag}-misses")));
        if let Some(dir) = record_dir {
            // W13: `seeds` overrides the seeded reply/title bodies (the
            // content-boundary payloads); `None` keeps the channel-9
            // defaults.
            cmd.arg("--seed-reply").arg(seeds.map_or_else(
                || root.join("tools/diff/realchain/seeds/anthropic-text.txt"),
                |(reply, _)| reply.to_path_buf(),
            ));
            // W3 hardening: title-generator misses seed a DISTINCT text so
            // the session title never echoes the main-turn reply needle.
            cmd.arg("--seed-title").arg(seeds.map_or_else(
                || root.join("tools/diff/realchain/seeds/title-sse.txt"),
                |(_, title)| title.to_path_buf(),
            ));
            cmd.arg("--record-dir").arg(dir);
        }
        if let Some(dir) = seed_turns {
            cmd.arg("--seed-turns").arg(dir);
        }
        if match_mode != "digest" {
            cmd.arg("--match").arg(match_mode);
        }
        if pace != "instant" {
            cmd.arg("--pace").arg(pace);
        }
        // stderr rides the same log file (spawn/import errors land there).
        let err_sink = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("open gateway log");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::from(err_sink));
        let mut child = cmd.spawn().expect("spawn replay gateway");

        // Read stdout lines until the READY line (with a deadline).
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut stdout = child.stdout.take().expect("gateway stdout");
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            let mut pending = String::new();
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                        while let Some(pos) = pending.find('\n') {
                            let line = pending[..pos].to_owned();
                            pending.drain(..=pos);
                            if tx.send(line).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        let mut port = None;
        while Instant::now() < deadline {
            if let Ok(line) = rx.recv_timeout(Duration::from_millis(500)) {
                eprintln!("[gateway] {line}");
                if let Some(rest) = line.strip_prefix("READY") {
                    for part in rest.split_whitespace() {
                        if let Some(value) = part.strip_prefix("port=") {
                            port = value.parse::<u16>().ok();
                        }
                    }
                    if port.is_some() {
                        break;
                    }
                }
            }
        }
        let port = port.unwrap_or_else(|| panic!("the replay gateway never reported READY"));
        Gateway { port, child, log_path }
    }

    #[must_use]
    pub fn stats(&self) -> Value {
        let (status, body) = http_get(&format!("http://127.0.0.1:{}/__stats", self.port), None)
            .unwrap_or((0, String::new()));
        serde_json::from_str(&body).unwrap_or(json_stats_fallback(status))
    }

    #[must_use]
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        // Graceful: close stdin/stdout (the server exits on EOF-free
        // SIGTERM through its own handle); then reap.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn json_stats_fallback(status: u16) -> Value {
    Value::String(format!("gateway stats unavailable (http {status})"))
}

// ---------------------------------------------------------------------------
// Tiny HTTP GET/POST (loopback server-API asserts)
// ---------------------------------------------------------------------------

/// Minimal standard base64 (no crate dep in the test env).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let triple = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(triple >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[triple as usize & 63] as char } else { '=' });
    }
    out
}

#[must_use]
pub fn http_get(url: &str, password: Option<&str>) -> Option<(u16, String)> {
    http_request("GET", url, None, password)
}

#[must_use]
pub fn http_post(url: &str, body: &str, password: Option<&str>) -> Option<(u16, String)> {
    http_request("POST", url, Some(body), password)
}

fn http_request(
    method: &str,
    url: &str,
    body: Option<&str>,
    password: Option<&str>,
) -> Option<(u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (host_port, path) = rest.split_once('/')?;
    let mut stream = TcpStream::connect(host_port).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut request =
        format!("{method} /{path} HTTP/1.1\r\nhost: {host_port}\r\nconnection: close\r\n");
    if let Some(password) = password {
        let encoded = base64_encode(format!("opencode:{password}").as_bytes());
        let _ = write!(request, "authorization: Basic {encoded}\r\n");
    }
    if let Some(body) = body {
        let _ =
            write!(request, "content-type: application/json\r\ncontent-length: {}\r\n", body.len());
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    if let Some(body) = body {
        stream.write_all(body.as_bytes()).ok()?;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let status = response.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok())?;
    let payload = response.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
    Some((status, payload))
}

// ---------------------------------------------------------------------------
// The TUI child on its own PTY
// ---------------------------------------------------------------------------

pub struct TuiProcess {
    pub child: Child,
    master: File,
    pub raw: Arc<Mutex<Vec<u8>>>,
    vt: Arc<Mutex<vt100::Parser>>,
}

impl TuiProcess {
    /// fork a PTY, exec the production binary on its slave (setsid +
    /// TIOCSCTTY — the TUI demands a real controlling terminal).
    #[must_use]
    pub fn spawn(sandbox: &Sandbox, cols: u16, rows: u16) -> TuiProcess {
        ensure_fresh_binary();
        let command = sandbox.command(&opencode_bin());
        TuiProcess::spawn_command(command, cols, rows)
    }

    /// fork a PTY, exec an arbitrary prepared command on its slave (the
    /// ref-TUI pair lane reuses the PTY machinery with a bun command).
    #[must_use]
    pub fn spawn_command(mut command: Command, cols: u16, rows: u16) -> TuiProcess {
        let master = unsafe {
            let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(fd >= 0, "posix_openpt");
            libc::grantpt(fd);
            libc::unlockpt(fd);
            File::from_raw_fd(fd)
        };
        let slave_path = unsafe {
            let name = libc::ptsname(master.as_raw_fd());
            assert!(!name.is_null(), "ptsname");
            std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
        };
        let slave_c = CString::new(slave_path).expect("slave path nul");
        let slave_fd = unsafe { libc::open(slave_c.as_ptr(), libc::O_RDWR) };
        assert!(slave_fd >= 0, "open pty slave");
        unsafe {
            let winsize = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
            libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &winsize);
        }

        command.stdin(unsafe { Stdio::from_raw_fd(libc::dup(slave_fd)) });
        command.stdout(unsafe { Stdio::from_raw_fd(libc::dup(slave_fd)) });
        command.stderr(unsafe { Stdio::from_raw_fd(libc::dup(slave_fd)) });
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
                Ok(())
            });
        }
        let child = command.spawn().expect("spawn opencode-rust");
        unsafe {
            libc::close(slave_fd);
        }

        let raw: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let vt: Arc<Mutex<vt100::Parser>> = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let reader = master.try_clone().expect("clone master");
        let feed_raw = Arc::clone(&raw);
        let feed_vt = Arc::clone(&vt);
        let start = Instant::now();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let _ = start;
                        feed_vt.lock().expect("vt").process(&chunk[..count]);
                        feed_raw.lock().expect("raw").extend_from_slice(&chunk[..count]);
                    }
                }
            }
        });
        TuiProcess { child, master, raw, vt }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).expect("write keys");
        self.master.flush().expect("flush keys");
    }

    #[must_use]
    pub fn raw_len(&self) -> usize {
        self.raw.lock().expect("raw").len()
    }

    /// Wait for `needle` in the raw stream at/after byte `from`.
    #[must_use]
    pub fn wait_for(&self, needle: &[u8], from: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let raw = self.raw.lock().expect("raw");
            if raw[from.min(raw.len())..].windows(needle.len()).any(|w| w == needle) {
                return true;
            }
            drop(raw);
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// The current screen text (rows joined by \n) from the VT parser.
    #[must_use]
    pub fn screen_text(&self) -> String {
        let parser = self.vt.lock().expect("vt");
        let screen = parser.screen();
        let mut out = String::new();
        for row in screen.rows(0, u16::MAX) {
            out.push_str(&row);
            out.push('\n');
        }
        out
    }

    /// The app's exit footprint in the raw stream.
    #[must_use]
    pub fn exited_alt_screen(&self, from: usize) -> bool {
        const LEAVE: &[u8] = b"\x1b[?1049l";
        self.wait_for(LEAVE, from, Duration::from_secs(1))
    }

    /// The graceful exit chord (adapted from the vt lane's CI-hardened
    /// chord: escapes clear dialogs, ctrl+u clears a dirty draft, then
    /// the double ctrl+c).
    pub fn exit_chord(&mut self) {
        self.write(b"\x1b\x1b\x15\x03\x03");
    }

    /// Graceful teardown: chord, wait, escalate only through the OWN
    /// child handle if the app is wedged.
    pub fn shutdown(&mut self, grace: Duration) {
        self.exit_chord();
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// The managed server (production boot step 1)
// ---------------------------------------------------------------------------

pub struct ManagedServer {
    pub url: String,
    pub password: Option<String>,
}

/// `opencode-rust service start` under the sandbox env — the exact
/// production managed-daemon path (`lifecycle::ensure`); the TUI then
/// attaches to the same registration.
pub fn service_start(sandbox: &Sandbox) -> ManagedServer {
    service_start_with_env(sandbox, &[])
}

/// W6 症状① — `service start` with EXTRA daemon env (the slow-boot
/// scenario's `OPENCODE_TEST_SSE_FIRST_FRAME_DELAY_MS` injection:
/// a daemon that is healthy on `/api/info` but holds the event
/// stream's first frame).
pub fn service_start_with_env(sandbox: &Sandbox, extra_env: &[(String, String)]) -> ManagedServer {
    // Defensive pre-clean: a previous run's daemon on this sandbox's
    // fixed port would wedge `ensure` (its own graceful stop path).
    service_stop(sandbox);
    let mut command = sandbox.command(&opencode_bin());
    command.args(["service", "start"]);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let mut child =
        command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("spawn service start");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut url = None;
    let mut early_exit: Option<String> = None;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A clean fast exit prints the endpoint URL on stdout.
                let out = child.stdout.take().map(|mut s| {
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf);
                    buf
                });
                let err = child.stderr.take().map(|mut s| {
                    let mut buf = String::new();
                    let _ = s.read_to_string(&mut buf);
                    buf
                });
                if status.success() {
                    if let Some(found) = out
                        .as_deref()
                        .and_then(|text| text.lines().find(|l| l.starts_with("http://")))
                    {
                        url = Some(found.trim().to_owned());
                    }
                }
                early_exit =
                    Some(format!("service start exited ({status}): out={out:?} err={err:?}"));
                break;
            }
            Ok(None) => {
                // Registration file already tells us the endpoint once the
                // daemon registered (before the probe even finishes).
                let reg = sandbox.registration();
                if reg.is_object() {
                    if let Some(found) = reg.get("url").and_then(|v| v.as_str()) {
                        if !found.is_empty() {
                            url = Some(found.to_owned());
                            break;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => break,
        }
    }
    // Let `service start` finish its probe (prints the URL, exits 0).
    let _ = child.wait();
    let url = url.unwrap_or_else(|| {
        panic!(
            "the managed server never registered; {}",
            early_exit.as_deref().unwrap_or("no early exit observed")
        )
    });
    let password =
        sandbox.registration().get("password").and_then(|v| v.as_str()).map(str::to_owned);
    ManagedServer { url: url.trim_end_matches('/').to_owned(), password }
}

/// `opencode-rust service stop` under the sandbox env — the daemon's
/// own graceful stop path.
pub fn service_stop(sandbox: &Sandbox) {
    let _ = sandbox.command(&opencode_bin()).args(["service", "stop"]).output();
}

/// W6 harness hardening — the T9 lane's panic-safe daemon cleaner:
/// between `service_start` and the `Rig` taking ownership, a panic
/// (or an early assert) would strand the daemon on the sandbox's
/// fixed port and wedge every later lane run. The pair lane's Rig
/// Drop covered its own construction window; the T9 lane's rig
/// builds the TUI AFTER the daemon starts, so this guard owns the
/// gap — `forget` it once the owning struct's Drop takes over.
pub struct ServiceClean<'a> {
    sandbox: &'a Sandbox,
    armed: bool,
}

impl<'a> ServiceClean<'a> {
    #[must_use]
    pub fn new(sandbox: &'a Sandbox) -> Self {
        Self { sandbox, armed: true }
    }

    /// Hand ownership to a struct whose own Drop stops the service.
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ServiceClean<'_> {
    fn drop(&mut self) {
        if self.armed {
            service_stop(self.sandbox);
        }
    }
}

/// The home wordmark's opening block (boot-settled marker, same needle
/// family as the vt lane).
pub const WORDMARK: &[u8] = "█▀▀█".as_bytes();

// ---------------------------------------------------------------------------
// W3 — the ref 2.0.24 lane: the SAME replay gateway, a ref-shaped
// sandbox, and the ref TUI booted through its own production topology.
//
// Ref boot topology (read from the code + probed, not guessed):
// `bun packages/cli/src/index.ts` (default handler) →
// ServerConnection.resolve → ServiceConfig.options → selfCommand()
// spawns the managed background server (`bun <entry> serve --service`,
// registration under `$XDG_STATE_HOME/opencode/service-local.json`).
// Constraints the staging honors:
//   * `selfCommand()` (cli/src/util/process.ts) only re-attaches the
//     entrypoint when `basename(execPath)` == "bun" — a binary named
//     `bun-1.4.2` silently spawns `bun serve` (bun's "Script not found")
//     — so the staged runtime copy must be named exactly `bun`;
//   * ref's server assembly imports tui TSX through bun's default react
//     JSX runtime, which the tree does not ship — the @opentui/solid
//     preload (the channel-8 driver convention) must be active, and it
//     is propagated to the self-spawned server via a `bunfig.toml`
//     `preload` entry in the sandbox cwd (the spawn inherits the cwd);
//   * ref's global config dir is `$XDG_CONFIG_HOME/opencode` (NOT the
//     rust port's `$HOME/.config/opencode` dogfood quirk) and the
//     provider package is the workspace-native
//     `@opencode/ai/providers/anthropic`.
// ---------------------------------------------------------------------------

/// Default ref checkout (read-only; its `node_modules` are used).
const REF_ROOT_DEFAULT: &str = "/opt/opencode-ref";

#[must_use]
pub fn ref_root() -> PathBuf {
    Path::new(&std::env::var("T9_REF_ROOT").unwrap_or_else(|_| REF_ROOT_DEFAULT.to_owned()))
        .to_path_buf()
}

/// The pinned bun runtime path (`T9_BUN` override; default the MAIN
/// checkout's untracked `tools/bin/bun-1.4.2` — this worktree shares no
/// untracked helpers).
#[must_use]
pub fn bun_runtime() -> PathBuf {
    std::env::var("T9_BUN")
        .map_or_else(|_| PathBuf::from("/opt/opencode-rust/tools/bin/bun-1.4.2"), PathBuf::from)
}

/// Skip guard for the pair lane (W4 Important ②): `Some(reason)` when
/// the ref checkout or the pinned bun runtime are absent — the calling
/// test returns early (skip), matching the repo's bun-test skip
/// convention. `None` = the lane is available; `ref_stage` then enforces
/// the remaining contracts (version, preload) as hard fails.
#[must_use]
pub fn ref_lane_skip_reason() -> Option<String> {
    let root = ref_root();
    let entry = root.join("packages/cli/src/index.ts");
    if !entry.exists() {
        return Some(format!(
            "ref checkout unavailable: {} missing (set T9_REF_ROOT)",
            entry.display()
        ));
    }
    let preload = root.join("packages/tui/node_modules/@opentui/solid/scripts/preload.js");
    if !preload.exists() {
        return Some(format!("ref solid preload unavailable: {} missing", preload.display()));
    }
    let bun = bun_runtime();
    if !bun.exists() {
        return Some(format!(
            "pinned bun runtime unavailable: {} missing (set T9_BUN)",
            bun.display()
        ));
    }
    None
}

/// Everything the ref lane needs in the sandbox.
///
/// CI semantics (W4): when the ref checkout or the pinned bun runtime
/// are absent — the pair lane's environment dependencies, which CI does
/// not provision — every pair test SKIPs through [`ref_lane_skip_reason`]
/// (a skip with the reason, never a red). On machines that DO carry the
/// lane (`T9_REF_ROOT` default `/opt/opencode-ref`, `T9_BUN` default the
/// main checkout's `tools/bin/bun-1.4.2`), the tests run directly; a
/// present-but-wrong bun version still fails (a contract, not an
/// availability gap).
pub struct RefStage {
    /// The staged bun runtime (a COPY named exactly `bun` — see the
    /// `selfCommand()` contract above).
    pub bun: PathBuf,
    /// `packages/cli/src/index.ts` of the ref tree.
    pub entry: PathBuf,
    /// The ref sandbox project (cwd for every ref child; carries the
    /// `bunfig.toml` preload).
    pub project: PathBuf,
}

/// Verify the ref tree + runtime contract, then stage the runtime copy
/// and the preload bunfig under `…-<tag>ref/`.
#[must_use]
pub fn ref_stage(sandbox: &Sandbox) -> RefStage {
    let root = ref_root();
    let entry = root.join("packages/cli/src/index.ts");
    assert!(entry.exists(), "ref entry missing: {} (set T9_REF_ROOT)", entry.display());
    let preload = root.join("packages/tui/node_modules/@opentui/solid/scripts/preload.js");
    assert!(preload.exists(), "ref solid preload missing: {}", preload.display());

    // Runtime + version contract: the pinned bun must exist and report
    // the ref `packageManager` version (1.4.2). Default: the MAIN
    // checkout's untracked tools/bin (this worktree shares no untracked
    // helpers); override with T9_BUN. Availability was pre-checked by
    // [`ref_lane_skip_reason`]; a wrong version is a CONTRACT failure.
    let runtime_src = bun_runtime();
    assert!(runtime_src.exists(), "pinned bun runtime missing: {}", runtime_src.display());
    let version = std::process::Command::new(&runtime_src)
        .arg("--version")
        .output()
        .expect("query bun version");
    let version_text = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    assert_eq!(version_text, "1.4.2", "pinned bun version contract (ref packageManager)");

    let bin_dir = sandbox.root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("ref bin dir");
    let bun = bin_dir.join("bun");
    if !bun.exists() {
        std::fs::copy(&runtime_src, &bun).unwrap_or_else(|e| panic!("stage bun: {e}"));
        set_exec_bit(&bun);
    }
    // `opencode` fallback wrapper on PATH (the client default command
    // `opencode serve --service` — used when selfCommand is bypassed).
    let wrapper = bin_dir.join("opencode");
    std::fs::write(&wrapper, format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", bun.display()))
        .expect("write opencode wrapper");
    set_exec_bit(&wrapper);

    let project = sandbox.root.join("project");
    std::fs::create_dir_all(&project).expect("ref project dir");
    std::fs::write(project.join("README.md"), "# t9 ref sandbox project\n").expect("project file");
    // The preload MUST reach the self-spawned `serve --service` child;
    // bunfig.toml in the inherited cwd is the one channel both the TUI
    // and the managed server read.
    std::fs::write(project.join("bunfig.toml"), format!("preload = [\"{}\"]\n", preload.display()))
        .expect("write bunfig");

    RefStage { bun, entry, project }
}

fn set_exec_bit(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).expect(" staged file").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).expect("chmod staged file");
}

impl Sandbox {
    /// The unique port the ref managed daemon binds — G-461: pinned
    /// per-sandbox (hash of the sandbox root into 50000-60000, a band
    /// disjoint from BOTH the rust sandbox's `service_port` range
    /// 20000-40000 and the legacy ref band 40000-50000), with a
    /// bind-probe fallback that walks past any incumbent listener
    /// BEFORE the first boot (a stray user-run `ref serve --service`
    /// DETACH child on the old shared/fixed port once EADDRINUSE'd
    /// every leg). Cached: once written, the sandbox reuses its port.
    #[must_use]
    pub fn ref_service_port(&self) -> u16 {
        // Reuse the staged pin when write_ref_config already ran (the
        // probe below would otherwise walk past OUR OWN live daemon
        // on a re-call and silently re-point the sandbox).
        let pin = self.root.join(".ref-service-port");
        if let Ok(text) = std::fs::read_to_string(&pin) {
            if let Ok(port) = text.trim().parse::<u16>() {
                return port;
            }
        }
        // Deterministic per-sandbox base: hash the sandbox ROOT path
        // (unique per test/tag), not the PID (all sandboxes in one
        // test process share it).
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.root.hash(&mut hasher);
        let mut port = 50_000 + (hasher.finish() % 10_000) as u16;
        // Incumbent probe: never hand out a port something already
        // listens on (a user's manually-source-run ref service, or a
        // neighbor sandbox's daemon). Walk forward until bind works.
        for _ in 0..2_000 {
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
                std::fs::write(&pin, port.to_string()).expect("pin ref service port");
                return port;
            }
            port = 50_000 + ((u32::from(port) + 1 - 50_000) % 10_000) as u16;
        }
        panic!("no free ref service port in 50000-60000 for {}", self.root.display());
    }

    /// Ref-shaped env: the rust env plus the staged `bin` first on PATH
    /// (so any `opencode` fallback resolves to the staged runtime) and
    /// `NODE_ENV=production` (keeps bun out of jsx-dev lookups).
    #[must_use]
    pub fn env_ref(&self, stage: &RefStage) -> Vec<(String, String)> {
        let mut env = self.env();
        let path = std::env::var("PATH").unwrap_or_default();
        let mut filtered = env.iter().position(|(k, _)| k == "PATH");
        let bin_dir = stage.bun.parent().unwrap().to_string_lossy().into_owned();
        let new_path = format!("{bin_dir}:{path}");
        if let Some(index) = filtered.take() {
            env[index].1 = new_path;
        } else {
            env.push(("PATH".into(), new_path));
        }
        env.push(("NODE_ENV".into(), "production".into()));
        env
    }

    /// A prepared ref child command (TUI, `service stop`, …): scrubbed
    /// env, cwd = the ref project (bunfig/preload apply).
    #[must_use]
    pub fn command_ref(&self, stage: &RefStage, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear();
        for (key, value) in std::env::vars() {
            if key.starts_with("OPENCODE") || key == "HOME" || key.starts_with("XDG_") {
                continue;
            }
            cmd.env(key, value);
        }
        for (key, value) in self.env_ref(stage) {
            cmd.env(key, value);
        }
        if let Some(shims) = &self.shim_dir {
            prepend_path(&mut cmd, &shims.join("shims"));
        }
        cmd.current_dir(&stage.project);
        cmd
    }

    /// The ref global config: `$XDG_CONFIG_HOME/opencode/opencode.json`
    /// with the replay provider on the workspace-native anthropic
    /// package (same gateway, same models as the rust side).
    pub fn write_ref_config(&self, extra_models: &[(&str, &str)], default_model: &str) {
        self.write_ref_config_ext(extra_models, default_model, &[])
    }

    pub fn write_ref_config_ext(
        &self,
        extra_models: &[(&str, &str)],
        default_model: &str,
        plugins: &[&str],
    ) {
        let mut models = serde_json::Map::new();
        models.insert("claude-opus-4-7".into(), serde_json::json!({ "name": "claude-opus-4-7" }));
        for (id, name) in extra_models {
            models.insert((*id).into(), serde_json::json!({ "name": name }));
        }
        let mut config = serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "model": default_model,
            "autoupdate": false,
            "providers": {
                "replay": {
                    "name": "Replay Anthropic",
                    "package": "@opencode/ai/providers/anthropic",
                    "settings": {
                        "baseURL": format!("http://127.0.0.1:{}/v1", self.gateway_port),
                        "apiKey": "replay-sandbox-key"
                    },
                    "models": models
                }
            }
        });
        if !plugins.is_empty() {
            config["plugins"] = serde_json::json!(plugins);
        }
        let path = self.root.join("xdg-config/opencode/opencode.json");
        std::fs::create_dir_all(path.parent().unwrap()).expect("ref config dir");
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
            .unwrap_or_else(|e| panic!("write ref config: {e}"));
        // G-461 — the ref channel-local managed service's port comes
        // from the ServiceConfig file `service-local.json`
        // (`filename("local")` in the ref's service-config.ts) under
        // the SAME XDG config dir — the OLD write stuffed `port` into
        // `cli.json`, which the service-config reader NEVER reads, so
        // every leg bound the FIXED channel default 0xc0df (49375)
        // and one stray user-run `serve --service` DETACH child
        // EADDRINUSE'd the whole lane. The pin is per-sandbox with a
        // pre-boot incumbent probe (see `ref_service_port`).
        let service = self.root.join("xdg-config/opencode/service-local.json");
        std::fs::write(
            &service,
            serde_json::to_string_pretty(&serde_json::json!({ "port": self.ref_service_port() }))
                .unwrap(),
        )
        .unwrap_or_else(|e| panic!("write ref service-local.json: {e}"));
        // The TUI's own config is cli.json — `debug.devtools: false`
        // pins the release shape (channel-local builds default the
        // devtools bar ON, app.tsx:457).
        let cli = serde_json::json!({ "debug": { "devtools": false } });
        let cli_path = self.root.join("xdg-config/opencode/cli.json");
        std::fs::write(&cli_path, serde_json::to_string_pretty(&cli).unwrap())
            .unwrap_or_else(|e| panic!("write ref cli.json: {e}"));
    }

    /// The ref managed-server registration (`service-local.json`,
    /// `$XDG_STATE_HOME/opencode/`), if the ref TUI already booted one.
    #[must_use]
    pub fn ref_registration(&self) -> serde_json::Value {
        let path = self.root.join("xdg-state/opencode/service-local.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<serde_json::Value>(&text).unwrap_or(Value::Null),
            Err(_) => Value::Null,
        }
    }

    /// W7 权限自动化 — pre-authorize the tool loop through the REAL
    /// config `permission` block (the same v2 semantic both engines
    /// parse): bash/edit/write/read allowed, so the scenario's tool
    /// calls run straight through with no permission dialog on either
    /// end. Mutates the already-written config in place.
    pub fn preauthorize_tools(&self, ref_side: bool) {
        let path = if ref_side {
            self.root.join("xdg-config/opencode/opencode.json")
        } else {
            self.config_path()
        };
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read config for preauth {}: {e}", path.display()));
        let mut config: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse config: {e}"));
        config["permission"] = serde_json::json!({
            "bash": "allow",
            "edit": "allow",
            "write": "allow",
            "read": "allow",
        });
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
            .unwrap_or_else(|e| panic!("write preauth config: {e}"));
    }

    /// W13 permission-flow rig: the config `permission` block ASKS for
    /// the tool-loop actions on both ends (the default policy allowed
    /// them silently — the ask dialog needs the explicit "ask").
    pub fn ask_tools(&self, ref_side: bool) {
        let path = if ref_side {
            self.root.join("xdg-config/opencode/opencode.json")
        } else {
            self.config_path()
        };
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read config for ask {}: {e}", path.display()));
        let mut config: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse config: {e}"));
        config["permission"] = serde_json::json!({
            "bash": "ask",
            "edit": "ask",
        });
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap())
            .unwrap_or_else(|e| panic!("write ask config: {e}"));
    }
}

/// Spawn the ref TUI on its own PTY. The managed ref server starts
/// ITSELF on first boot (the production topology); teardown must call
/// [`ref_service_stop`].
#[must_use]
pub fn ref_tui_spawn(sandbox: &Sandbox, stage: &RefStage, cols: u16, rows: u16) -> TuiProcess {
    let mut command = sandbox.command_ref(stage, &stage.bun);
    command.arg(&stage.entry);
    TuiProcess::spawn_command(command, cols, rows)
}

/// The ref daemon's own graceful stop path (`service stop` under the
/// same sandbox env + cwd). MUST run after every ref TUI leg — the
/// managed server is a detached child that outlives the TUI.
pub fn ref_service_stop(sandbox: &Sandbox, stage: &RefStage) {
    let _ =
        sandbox.command_ref(stage, &stage.bun).arg(&stage.entry).args(["service", "stop"]).output();
}

// ---------------------------------------------------------------------------
// W5 — the real-plugin rig (symptom ⑦: plugin-added sidebar content).
//
// The user's real machine carries REAL installed plugins under
// `~/.cache/opencode/npm/<pkg>@<key>/<stamp>/node_modules/<pkg>` (the
// `opencode plugin add` layout both engines share). The rig stages
// READ-ONLY COPIES of those trees into each sandbox's cache root and
// names them in the config `plugins[]` array — the REAL config
// discovery path on both ends (resolve.rs for the port,
// plugin/module.ts + Npm for the ref), never driver injection.
// ---------------------------------------------------------------------------

/// One real plugin payload: (source dir NAME under the payload registry
/// root, config spec). The registry root is the repo-local, git-ignored
/// `tools/diff/realchain/plugin-payload/` (W8: payload persistence —
/// /tmp staging died with reboots and was invisible to the worktree's
/// lane isolation), override with `T9_PLUGIN_PAYLOAD_ROOT`. Provision
/// it with `tools/diff/realchain/fetch_plugin_payload.sh` (casts the
/// host's real `~/.cache/opencode` install trees). Privacy: these are
/// published npm packages, not user data.
pub const PLUGIN_SPECS: &[(&str, &str)] = &[
    // (registry dir name, config spec)
    ("oh-my-opencode-slim", "oh-my-opencode-slim@3.0.3"),
    ("superpowers", "superpowers@7.0.0"),
];

/// The plugin payload registry root (see [`PLUGIN_SPECS`]).
///
/// Rider (Lane D pitfall, 2026-11-20): the registry is git-ignored, so
/// it exists ONLY in the main checkout — a worktree lane's local path
/// is absent and every plugin scenario silently skipped (a 0.004s PASS
/// fake-green). The default resolution therefore falls back to the
/// MAIN checkout's copy via `git rev-parse --git-common-dir` (the
/// shared `.git` of the worktree's repo) when the worktree-local path
/// does not exist. `T9_PLUGIN_PAYLOAD_ROOT` still wins outright.
#[must_use]
pub fn plugin_payload_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("T9_PLUGIN_PAYLOAD_ROOT") {
        return Path::new(&dir).to_path_buf();
    }
    let local = worktree_dir("tools/diff/realchain/plugin-payload");
    if local.is_dir() {
        return local;
    }
    // Worktree-local payload absent — fall back to the main checkout
    // (the shared git dir's parent). Any git failure keeps the local
    // path (and the skip guard below reports it loudly).
    let main = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--git-common-dir")
        .current_dir(worktree_root())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            // In the main checkout itself this yields ".git" — the
            // local path check above already covered that case.
            if dir == ".git" {
                None
            } else {
                worktree_root().join(dir).canonicalize().ok()
            }
        })
        .flatten()
        .and_then(|git_dir| git_dir.parent().map(Path::to_path_buf))
        .map(|main_root| main_root.join("tools/diff/realchain/plugin-payload"));
    if let Some(main) = &main {
        if main.is_dir() {
            return main.clone();
        }
    }
    local
}

/// Skip guard for the plugin rig: `Some(reason)` when the real plugin
/// payload trees are absent on this machine (never a red). Every skip
/// is APPENDED to `.test-logs/skips.log` (rider: a silent 0.004s PASS
/// fake-green once hid the missing payload for a whole lane — the
/// eprintln callers' output is captured away by nextest on PASS).
pub fn plugin_lane_skip_reason() -> Option<String> {
    let reason = plugin_lane_skip_reason_at(&plugin_payload_root())?;
    let log = worktree_dir(".test-logs");
    let _ = std::fs::create_dir_all(&log);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log.join("skips.log"))
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "[plugin-payload] {reason}")
        });
    Some(reason)
}

fn plugin_lane_skip_reason_at(root: &Path) -> Option<String> {
    for (name, spec) in PLUGIN_SPECS {
        let source = root.join(name);
        if plugin_payload_stamp(&source).is_none() {
            return Some(format!(
                "real plugin payload unavailable: no <stamp>/node_modules under {} \
                 (spec {spec}; run tools/diff/realchain/fetch_plugin_payload.sh first)",
                source.display()
            ));
        }
    }
    None
}

#[cfg(test)]
mod rider_payload_root_tests {
    use super::*;

    /// Rider (Lane D pitfall) — the git-ignored payload registry lives
    /// only in the main checkout: in a lane worktree the local path is
    /// absent and the root MUST fall back to the main checkout's copy
    /// (a silent skip once fake-greened a whole omo lane).
    #[test]
    fn payload_root_falls_back_to_main_checkout() {
        let local = worktree_dir("tools/diff/realchain/plugin-payload");
        let root = plugin_payload_root();
        if local.is_dir() {
            // Main-checkout shape: the local copy wins outright.
            assert_eq!(root, local);
        } else {
            assert!(
                root.is_dir(),
                "worktree-local payload absent: fallback must resolve the main checkout copy (got {})",
                root.display()
            );
            assert_ne!(root, local);
        }
    }

    /// Skip semantics survive without a payload supply: an empty root
    /// yields a reason naming the missing tree (never a red).
    #[test]
    fn payload_skip_reason_names_missing_root() {
        let tmp = std::env::temp_dir().join(format!("t9-payload-empty-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("tmp payload root");
        let reason = plugin_lane_skip_reason_at(&tmp)
            .expect("empty payload root must produce a skip reason");
        assert!(reason.contains("oh-my-opencode-slim"), "reason names the spec dir: {reason}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// The newest `<stamp>` generation under a payload registry entry that
/// carries `node_modules` (the install layout both engines share).
fn plugin_payload_stamp(source: &Path) -> Option<PathBuf> {
    let mut stamps: Vec<PathBuf> = std::fs::read_dir(source)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("node_modules").is_dir())
        .collect();
    stamps.sort();
    stamps.pop()
}

/// Copy one install tree into the sandbox read-only (files 0444, dirs
/// 0555 — the payload is never mutated by either engine's resolve
/// path, and a stray write must fail loudly).
fn copy_tree_readonly(source: &Path, target: &Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt;
    if source.is_file() {
        std::fs::copy(source, target)
            .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", source.display(), target.display()));
        let mut perms = std::fs::metadata(target).expect("staged file").permissions();
        perms.set_mode(if writable { 0o644 } else { 0o444 });
        std::fs::set_permissions(target, perms).expect("chmod staged file");
        return;
    }
    std::fs::create_dir_all(target).unwrap_or_else(|e| panic!("mkdir {}: {e}", target.display()));
    let mut perms = std::fs::metadata(target).expect("staged dir").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(target, perms).expect("chmod staged dir");
    for entry in std::fs::read_dir(source).expect("read plugin payload dir").flatten() {
        copy_tree_readonly(&entry.path(), &target.join(entry.file_name()), writable);
    }
}

/// The staged install generation stamp (any fixed value — the
/// resolve paths sort by stamp and this is the only one).
const PAYLOAD_STAMP: &str = "1791474452027";

/// Stage every real plugin payload into the sandbox's install cache
/// and return the config `plugins[]` specs. `ref_side` picks the cache
/// root: the port's `$XDG_CACHE_HOME/opencode-rust/npm` vs the ref's
/// `$XDG_CACHE_HOME/opencode/npm`.
pub fn stage_plugins(sandbox: &Sandbox, ref_side: bool) -> Vec<String> {
    stage_plugins_g454(sandbox, ref_side, None, false)
}

/// G-454 discriminator staging — `only` restricts the staged set to one
/// registry name (the single-plugin confounder), `writable` stages files
/// 0644 instead of 0444 (the read-only-artifact confounder).
pub fn stage_plugins_g454(
    sandbox: &Sandbox,
    ref_side: bool,
    only: Option<&str>,
    writable: bool,
) -> Vec<String> {
    let base = if ref_side { "opencode" } else { "opencode-rust" };
    let npm_root = sandbox.root.join("xdg-cache").join(base).join("npm");
    let mut specs = Vec::new();
    for (name, spec) in PLUGIN_SPECS {
        if let Some(want) = only {
            if *name != want {
                continue;
            }
        }
        let (_, version) = spec.rsplit_once('@').expect("pinned plugin spec");
        // `@org/pkg@ver` safety: the package name is everything before
        // the LAST '@'.
        let package = spec[..spec.len() - version.len() - 1].to_owned();
        let stamp_dir = plugin_payload_stamp(&plugin_payload_root().join(name))
            .unwrap_or_else(|| panic!("plugin payload vanished after the skip check: {name}"));
        let target = npm_root.join(format!("{package}@{version}")).join(PAYLOAD_STAMP);
        if !target.join("node_modules").is_dir() {
            copy_tree_readonly(&stamp_dir, &target, writable);
        }
        specs.push((*spec).to_owned());
    }
    specs
}

// ---------------------------------------------------------------------------
// W5 — teardown hardening: sweep this process's sandbox registry on
// lane release (the inotify-leak lesson: a panic path can strand
// sandbox trees; the sweep runs unconditionally, errors tolerated).
// ---------------------------------------------------------------------------

fn sandbox_registry_sweep() {
    let prefix = format!("t9w1-{}-", std::process::id());
    let Ok(entries) = std::fs::read_dir("/tmp/opencode") else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) {
            let _ = remove_tree(&entry.path());
        }
    }
}

fn remove_tree(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        // Clear the read-only bits the plugin staging set (a plain
        // remove_dir_all fails on 0555 dirs).
        for entry in std::fs::read_dir(path)?.flatten() {
            let _ = remove_tree(&entry.path());
        }
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
        std::fs::remove_dir(path)
    } else {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(path, perms)?;
        std::fs::remove_file(path)
    }
}

impl Drop for LaneGuard {
    fn drop(&mut self) {
        sandbox_registry_sweep();
    }
}

// ---------------------------------------------------------------------------
// Grid normalization + pair comparison (W3: differential philosophy
// extended to the real chain — both ends render into equal-size PTYs,
// the settled TEXT grids are normalized, then compared byte-for-byte)
// ---------------------------------------------------------------------------

// Which environmental normalizations actually FIRED (W12: the export
// must list everything the normalizer stripped, so the adjudication
// side sees it). Accumulated per test process (nextest isolates each
// test in its own process), read by the diff exporter.
thread_local! {
    static FIRED_NORMALIZATIONS: std::cell::RefCell<std::collections::BTreeSet<&'static str>> =
        const { std::cell::RefCell::new(std::collections::BTreeSet::new()) };
}

fn mark_fired(name: &'static str) {
    FIRED_NORMALIZATIONS.with(|set| {
        set.borrow_mut().insert(name);
    });
}

/// The names of every normalization that fired SINCE THE LAST EXPORT
/// (M-1, review fix: a per-report snapshot, not a process-lifetime
/// accumulation — the set is drained by each export, so each report
/// lists exactly what its own grids triggered, provided the export
/// follows the state's normalize calls; empty = raw grids, nothing
/// stripped).
#[must_use]
pub fn normalization_names() -> Vec<String> {
    FIRED_NORMALIZATIONS.with(|set| {
        std::mem::take(&mut *set.borrow_mut()).into_iter().map(|name| name.to_owned()).collect()
    })
}

/// Normalize a settled screen grid for cross-end comparison:
/// * middle-ellipsized path prefixes (`/…/openco…/`) re-expanded so the
///   root substitution below can fire
/// * sandbox roots (each end runs in its own PID-unique root) → `__ROOT__`
/// * the composer placeholder suggestion (`Ask anything… "…"`) picks via
///   `Math.random` (ref home route) → `__PROMPT__`
/// * the bottom-right version/channel pill (ref-from-source builds say
///   `local`, release builds the semver) → `__VERSION__`
/// * volatile identifiers (`ses_…`, `msg_…`, `evt_…`) → `__ID__`
/// * clock faces (`HH:MM`, `HH:MM:SS`) → `__TIME__`
/// * trailing row whitespace
///
/// These remove ENVIRONMENT artifacts (not bug judgments — W12/2.2#12:
/// every fired one is exported in the report's
/// `normalizations_applied`).
#[must_use]
pub fn normalize_grid(grid: &str, roots: &[&str]) -> String {
    let mut out = String::with_capacity(grid.len());
    for line in grid.lines() {
        let mut row = line.trim_end().to_owned();
        if row != line {
            mark_fired("trailing-whitespace");
        }
        // Middle-ellipsized paths come in more than one truncation
        // shape (`/…/openco…/t9w1-…` at 120 cols, `/…/opencode/t9w1-…`
        // in the wider sidebar footer): re-expand ANY `/…*/t9w1-`
        // prefix so the root substitution below can fire.
        if let Some(start) = row.find("/…") {
            if let Some(tail) = row[start..].find("/t9w1") {
                let end = start + tail;
                row = format!("{}{}{}", &row[..start], "/tmp/opencode", &row[end..]);
                mark_fired("ellipsis-path-reexpand");
            }
        }
        if row.contains("/…/openco…/") {
            row = row.replace("/…/openco…/", "/tmp/opencode/");
            mark_fired("ellipsis-path-reexpand");
        }
        for root in roots {
            if !root.is_empty() && row.contains(root) {
                row = row.replace(root, "__ROOT__");
                mark_fired("sandbox-roots");
            }
        }
        let after_roots = row.clone();
        row = replace_placeholder(&row);
        if row != after_roots {
            mark_fired("composer-prompt-suggestion");
        }
        if row.contains("__ROOT__") {
            // Path-length differences (PID-unique roots pre-replacement)
            // shift adjacent column padding; collapse them on BOTH ends
            // identically.
            let before = row.clone();
            collapse_spaces(&mut row);
            if row != before {
                mark_fired("root-padding-collapse");
            }
        }
        let before_pill = row.clone();
        row = replace_version_pill(&row);
        if row != before_pill {
            mark_fired("version-pill");
        }
        let before_ids = row.clone();
        row = replace_id_shapes(&row);
        if row != before_ids {
            mark_fired("wire-id-shapes");
        }
        let before_clocks = row.clone();
        row = replace_clocks(&row);
        if row != before_clocks {
            mark_fired("clocks");
        }
        let before_timings = row.clone();
        row = replace_timings(&row);
        if row != before_timings {
            mark_fired("timings");
        }
        // The timing tokens replace variable-width runs (`96ms` →
        // `__MS__`, `666.7 tok/s` → `__TOKS__`), so any content sharing
        // the row on the OTHER side of the tokens (the p4-long tier:
        // the sidebar onboarding card renders on the transcript's
        // `Build · model · …` rows) shifts by the per-side delta —
        // collapse the space runs on BOTH ends (the same shape as the
        // __ROOT__ path-length rule above).
        if row.contains("__MS__") || row.contains("__TOKS__") || row.contains("__SECS__") {
            let before = row.clone();
            collapse_spaces(&mut row);
            if row != before {
                mark_fired("timing-padding-collapse");
            }
        }
        // G-434 (documented masking): the free/no-auth model tier in
        // the palette lists models.dev-catalog entries whose VINTAGE
        // differs between the ref tree's snapshot.txt and the port's
        // embedded catalog (both ends list their own era's free
        // models; production syncs one live catalog). The row's model
        // NAME is masked on both ends; the tier column and row count
        // stay asserted.
        if row.trim_end().ends_with(" Free") {
            let trimmed_len = row.trim_end().len();
            let head = &row[..trimmed_len];
            if let Some(name_start) = head.find(|c: char| !c.is_whitespace()) {
                let tier_at = head.len() - " Free".len();
                row = format!("{}__FREELIST__{}", &row[..name_start], &head[tier_at..]);
                mark_fired("freelist-mask");
            }
        }
        out.push_str(&row);
        out.push('\n');
    }
    out
}

/// Reply meta timings (`56ms`, `122.4 tok/s`, `3.6s` turn durations)
/// are wall-clock artifacts (W5: the paced-stream duration differs by
/// fractions of a second between ends — same schedule, jittered
/// render clocks).
fn replace_timings(row: &str) -> String {
    let chars: Vec<char> = row.chars().collect();
    let is_digit =
        |chars: &[char], index: usize| chars.get(index).is_some_and(char::is_ascii_digit);
    let mut out = String::with_capacity(row.len());
    let mut index = 0;
    while index < chars.len() {
        if is_digit(&chars, index) {
            let mut end = index;
            while is_digit(&chars, end)
                || (chars.get(end) == Some(&'.') && is_digit(&chars, end + 1))
            {
                end += 1;
            }
            // number span [index, end)
            let suffix: String = chars[end..(end + 6).min(chars.len())].iter().collect();
            if suffix.starts_with("ms") {
                out.push_str("__MS__");
                index = end + 2;
            } else if suffix.starts_with(" tok/s") {
                out.push_str("__TOKS__");
                index = end + 6;
            } else if suffix.starts_with('s')
                && !suffix[1..].chars().next().is_some_and(char::is_alphanumeric)
            {
                // A bare `Ns`/`N.Ns` duration token (`4.2s`), not part
                // of a longer word (`tokens`, `msgs`).
                out.push_str("__SECS__");
                index = end + 1;
            } else {
                for ch in &chars[index..end] {
                    out.push(*ch);
                }
                index = end;
            }
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// `Ask anything… "suggestion"` — the suggestion is Math.random-picked
/// on the ref side (channel 8 hit the same artifact and seeded it there;
/// here both texts are legal so both map to the token).
fn replace_placeholder(row: &str) -> String {
    match row.find("Ask anything… \"") {
        Some(at) => {
            let start = at + "Ask anything… \"".len();
            // The suggestion ends at the closing quote if present.
            let rest = &row[start..];
            let end = rest.find('"').unwrap_or(rest.len());
            let mut out = String::from(&row[..start]);
            out.push_str("__PROMPT__");
            out.push_str(&rest[end..]);
            out
        }
        None => row.to_owned(),
    }
}

/// A row that is ONLY a version/channel token (`local`, `2.0.24`,
/// `2.0.24-dev`) is the bottom-right pill; its text is a build artifact
/// of which end runs from source vs binary.
/// In-place collapse of runs of ≥2 spaces to a single space (applied
/// identically to both ends; only for rows whose root path was
/// substituted, where pre-replacement length differences shift padding).
fn collapse_spaces(row: &mut String) {
    let mut out = String::with_capacity(row.len());
    let mut in_run = false;
    for ch in row.chars() {
        if ch == ' ' {
            if !in_run {
                out.push(ch);
            }
            in_run = true;
        } else {
            in_run = false;
            out.push(ch);
        }
    }
    *row = out;
}

fn replace_version_pill(row: &str) -> String {
    let trimmed = row.trim();
    let looks_like_version = |text: &str| {
        text == "local"
            || (text
                .strip_prefix("2.0.24")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-')))
    };
    if !trimmed.is_empty() && looks_like_version(trimmed) {
        // Padding width is a build artifact too (right-aligned `local`
        // vs `2.0.24` differ by one column) — emit the bare token.
        "__VERSION__".to_owned()
    } else {
        // The gwave-1 mask retirement moved T9-7 to the full-grid
        // compare: the pill also rides INLINE as the sidebar's
        // right-edge trailing cell (`... /mcps      local` on the ref
        // source run vs `2.0.24` on the built binary) — normalize the
        // trailing token the same way (the build-source artifact, per
        // the registered normalization list; NOT a product gap).
        let head = row.trim_end();
        if let Some((before, token)) = head.rsplit_once(' ') {
            if !before.is_empty() && looks_like_version(token) {
                // Collapse the padding run too: the right-aligned pill
                // sits one column further left for `2.0.24` than for
                // `local` (same one-column artifact as above).
                return format!("{} __VERSION__", before.trim_end());
            }
        }
        row.to_owned()
    }
}

fn replace_id_shapes(row: &str) -> String {
    // The WIRE id prefixes of the v2 schema (oc-schema id.rs — the
    // `check "<prefix>"` registry): ses/msg/evt/wrk/per/sh/frm/psv/
    // cred/con. W4 Minor ③: narrowed to exactly this list — the old
    // set also carried `pty` (a local attachment id shape with no
    // schema prefix contract).
    const WIRE_PREFIXES: &[&str] =
        &["ses", "msg", "evt", "wrk", "per", "sh", "frm", "psv", "cred", "con"];
    let mut result = String::with_capacity(row.len());
    let mut rest = row;
    while let Some(index) = rest.find(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        let (prefix, tail) = rest.split_at(index);
        result.push_str(prefix);
        let end = tail.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(tail.len());
        let (token, tail_rest) = tail.split_at(end);
        if let Some((head, _)) = token.split_once('_') {
            if WIRE_PREFIXES.contains(&head) && token.len() > 8 {
                result.push_str("__ID__");
                rest = tail_rest;
                continue;
            }
        }
        result.push_str(token);
        rest = tail_rest;
    }
    result
}

fn replace_clocks(row: &str) -> String {
    let chars: Vec<char> = row.chars().collect();
    let is_digit =
        |chars: &[char], index: usize| chars.get(index).is_some_and(char::is_ascii_digit);
    let mut out = String::with_capacity(row.len());
    let mut index = 0;
    while index < chars.len() {
        if is_digit(&chars, index)
            && is_digit(&chars, index + 1)
            && chars.get(index + 2) == Some(&':')
            && is_digit(&chars, index + 3)
            && is_digit(&chars, index + 4)
            && !is_digit(&chars, index + 5)
        {
            out.push_str("__TIME__");
            index += 5;
            if chars.get(index) == Some(&':') && is_digit(&chars, index + 1) {
                index += 3;
            }
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// Dump a grid to `.test-logs` unconditionally (forensics before the
/// asserts run).
pub fn dump_grid(name: &str, grid: &str) {
    let dir = worktree_root().join(".test-logs");
    let _ = std::fs::create_dir_all(&dir);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let _ = std::fs::write(dir.join(format!("t9-pair-{name}-{stamp}.txt")), grid);
}

// ---------------------------------------------------------------------------
// W12 — the DIFF EXPORTER (user ruling 2026-11-18, register 2.2#12): the
// sandbox lane performs NO bug adjudication of its own. Every grid
// comparison (and every single-end content observation) becomes a
// structured artifact under `.test-logs/diff-reports/`, and the
// controller-side checker (`tools/diff/realchain/adjudicate.py` +
// `adjudications.json`) decides framework-diff vs real bug. The old
// assert_grids_match panic path and every mask/tripwire/closure pin
// lived in git history from here on.
// ---------------------------------------------------------------------------

/// The diff-report directory (consumed by adjudicate.py).
fn diff_report_dir() -> PathBuf {
    worktree_root().join(".test-logs/diff-reports")
}

fn tree_hash() -> String {
    // Absolute fallbacks: the test process may run under a scrubbed
    // PATH (dev-gate's systemd scope).
    for program in ["git", "/usr/bin/git", "/usr/local/bin/git"] {
        if let Ok(output) = std::process::Command::new(program)
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(worktree_root())
            .output()
        {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).trim().to_owned();
            }
        }
    }
    // M-2: leave a trace — a silent "unknown" binds the report to no
    // tree and voids baseline discipline (2.2#5).
    eprintln!(
        "realchain export: tree_hash unavailable (git rev-parse failed in {})",
        worktree_root().display()
    );
    "unknown".to_owned()
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The per-line divergences of two normalized grids (1-based line
/// numbers; a length mismatch materializes the missing counterpart as
/// an empty line).
fn grid_divergences(ref_grid: &str, rust_grid: &str) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let ref_lines: Vec<&str> = ref_grid.lines().collect();
    let rust_lines: Vec<&str> = rust_grid.lines().collect();
    let rows = ref_lines.len().max(rust_lines.len());
    for index in 0..rows {
        let left = ref_lines.get(index).copied().unwrap_or("");
        let right = rust_lines.get(index).copied().unwrap_or("");
        if left != right {
            out.push(serde_json::json!({
                "line_no": index + 1,
                "ref": left,
                "rust": right,
            }));
        }
    }
    out
}

/// Export one scenario-state grid pair as a structured diff report
/// (JSON + human-readable .txt). Divergences empty = EXACT. The
/// normalizations list makes every environmental artifact the
/// normalizer stripped VISIBLE to the adjudication side.
pub fn export_grid_diff(scenario: &str, state: &str, ref_grid: &str, rust_grid: &str) -> usize {
    let divergences = grid_divergences(ref_grid, rust_grid);
    let report = serde_json::json!({
        "scenario": scenario,
        "state": state,
        "tree_hash": tree_hash(),
        "ts": now_ts(),
        "divergences": divergences,
        "normalizations_applied": normalization_names(),
    });
    write_diff_report(scenario, state, &report, None);
    divergences.len()
}

/// Export a single-end (rust-only) content observation for adjudication
/// — `observations` = (label, expected, observed). Used where the old
/// tests asserted content presence/absence on the rust grid alone
/// (e.g. t9_8's boot-window "Connection lost" count). A divergence is
/// recorded whenever observed != expected. `screen` (I-3, review fix):
/// an optional full-screen capture attached verbatim to the .txt twin
/// — pass the violating frame so the red has its evidence in-band.
pub fn export_survey(
    scenario: &str,
    state: &str,
    observations: &[(&str, u64, u64)],
    screen: Option<&str>,
) -> usize {
    let divergences: Vec<serde_json::Value> = observations
        .iter()
        .filter(|&(_label, expected, observed)| expected != observed)
        .map(|(label, expected, observed)| {
            serde_json::json!({
                "line_no": 0,
                "ref": format!("{label}: expected {expected}"),
                "rust": format!("{label}: observed {observed}"),
            })
        })
        .collect();
    let report = serde_json::json!({
        "scenario": scenario,
        "state": state,
        "tree_hash": tree_hash(),
        "ts": now_ts(),
        "divergences": divergences,
        "normalizations_applied": normalization_names(),
    });
    write_diff_report(scenario, state, &report, screen);
    divergences.len()
}

fn write_diff_report(
    scenario: &str,
    state: &str,
    report: &serde_json::Value,
    attach: Option<&str>,
) {
    let dir = diff_report_dir();
    let _ = std::fs::create_dir_all(&dir);
    let base = format!("{scenario}-{state}");
    let _ = std::fs::write(
        dir.join(format!("{base}.json")),
        serde_json::to_string_pretty(report).unwrap_or_default(),
    );
    // Human-readable twin: the divergent lines plus the metadata.
    let mut text = String::new();
    let _ = writeln!(text, "scenario: {scenario}  state: {state}");
    let _ = writeln!(
        text,
        "tree_hash: {}  ts: {}",
        report["tree_hash"].as_str().unwrap_or("?"),
        report["ts"]
    );
    let _ = writeln!(text, "normalizations_applied: {}", report["normalizations_applied"]);
    let divergences = report["divergences"].as_array().cloned().unwrap_or_default();
    if divergences.is_empty() {
        let _ = writeln!(text, "divergences: EXACT (none)");
    } else {
        let _ = writeln!(text, "divergences: {}", divergences.len());
        for entry in &divergences {
            let _ = writeln!(
                text,
                "line {}: ref={:?}  rust={:?}",
                entry["line_no"],
                entry["ref"].as_str().unwrap_or("?"),
                entry["rust"].as_str().unwrap_or("?")
            );
        }
    }
    // I-3: an optional in-band evidence attachment (the violating
    // screen for survey violations).
    if let Some(screen) = attach {
        let _ = writeln!(text, "attached screen: see {base}.screen.txt");
        let _ = std::fs::write(dir.join(format!("{base}.screen.txt")), screen);
    }
    let _ = std::fs::write(dir.join(format!("{base}.txt")), text);
}

#[cfg(test)]
mod g461_ref_port_tests {
    use super::*;

    /// Mirror of the deterministic hashed base in `Sandbox::ref_service_port`.
    fn hashed_base(root: &Path) -> u16 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.hash(&mut hasher);
        50_000 + (hasher.finish() % 10_000) as u16
    }

    /// G-461 — an incumbent listener on a sandbox's hashed base port
    /// (the shape of a stray user-run `ref serve --service` DETACH
    /// child) must never be handed out: the probe walks past it, stays
    /// in the 50000-60000 band, and the pick is pinned so a re-call
    /// cannot silently re-point the sandbox at its own live daemon.
    #[test]
    fn ref_port_walks_past_incumbent_listener() {
        let sb = sandbox("g461incumbent", 0);
        let other = sandbox("g461other", 0);
        let base = hashed_base(&sb.root);
        // Distinct sandbox roots pin distinct bases (no cross-sandbox
        // fights inside one test process).
        assert_ne!(base, hashed_base(&other.root));
        let incumbent = std::net::TcpListener::bind(("127.0.0.1", base))
            .expect("dummy incumbent on the hashed base port (band free by construction)");
        let port = sb.ref_service_port();
        assert!((50_000..60_000).contains(&port), "port {port} outside the pinned band");
        assert_ne!(port, base, "must walk past the incumbent listener");
        drop(incumbent);
        // The pin is cached: even after the incumbent is gone, the
        // sandbox keeps its port (a live daemon owns it now).
        assert_eq!(sb.ref_service_port(), port);
        let _ = std::fs::remove_dir_all(&sb.root);
        let _ = std::fs::remove_dir_all(&other.root);
    }
}

#[cfg(test)]
mod w4_normalizer_tests {
    use super::*;

    #[test]
    fn seconds_durations_mask_but_words_survive() {
        assert_eq!(
            replace_timings("Build · claude-opus-4-7 · 3.6s · 5.1 tok/s"),
            "Build · claude-opus-4-7 · __SECS__ · __TOKS__"
        );
        assert_eq!(replace_timings("12ms later"), "__MS__ later");
        // `s` inside a word is not a duration.
        assert_eq!(replace_timings("38 tokens used"), "38 tokens used");
        assert_eq!(replace_timings("4 steps"), "4 steps");
    }

    #[test]
    fn id_shapes_use_the_schema_wire_prefix_list() {
        assert_eq!(replace_id_shapes("tab ses_0001abcdEFGhij"), "tab __ID__");
        assert_eq!(replace_id_shapes("row msg_1e64066090019u8zd"), "row __ID__");
        // `pty` is NOT a schema wire prefix (local attachment shape).
        assert_eq!(replace_id_shapes("attach pty_0001abcdEFGhij"), "attach pty_0001abcdEFGhij");
        // Short words that share a prefix stay intact.
        assert_eq!(replace_id_shapes("session message"), "session message");
    }
}
