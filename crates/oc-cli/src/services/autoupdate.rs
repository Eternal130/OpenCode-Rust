//! Policy-driven automatic update check (lane C).
//!
//! HARD REQUIREMENT — network failures must never affect program
//! operation. Everything here is shaped around it:
//!
//! * The checker runs on ONE detached background thread spawned from
//!   the CLI entry for the long-running paths only (default/TUI and
//!   `serve`); boot never waits on it — zero added latency.
//! * Every network call is bounded (5 s connect / 15 s total per feed
//!   request, one retry) and every failure — DNS, connect, HTTP error,
//!   parse garbage, GitHub rate limit — is a silent no-op: no stderr
//!   noise, no exit-code impact. The thread body is wrapped in
//!   `catch_unwind` so even a panic cannot escape or kill the process.
//! * The loop re-checks every 10 minutes (a natural retry for
//!   transient DNS blips) on the same thread with fixed resources;
//!   downloads stage to PID-unique temp files under the cache dir,
//!   cleaned on failure and swept on the next run.
//! * `update: auto` installs are all-or-nothing (sha256 verify →
//!   atomic rename); a failed install keeps the current binary.
//! * The manual `opencode-rust upgrade` command is the exception: it
//!   reports network errors clearly and exits non-zero (see
//!   [`crate::services::updater::upgrade`]).
//!
//! Policy resolution: the global config's `update` field
//! (`disable|notify|auto`, v2 semantics, unset = notify), the legacy
//! v1 `autoupdate` key mapping (false→disable, true→auto, "notify"→
//! notify — the same mapping as oc-core's normalize), and the
//! `OPENCODE_DISABLE_AUTOUPDATE` env kill-switch (highest priority).
//! A `dev` build (no release tag stamped) skips the checker entirely —
//! no thread, no network.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::{LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::services::update_feed::{self, Candidate, ReleaseVer};
use crate::services::updater;

/// One installation notice for the in-process server's event bus — the
/// port of v2's server-process updater callbacks (`process.ts:115-117`):
/// the CLI-side updater publishes `installation.update-available` /
/// `installation.updated` `{version}` when the process hosting the server
/// learns of one. The detached checker thread cannot hold the engine, so
/// notices flow through this rendezvous channel; `serve` (the one mode
/// whose process owns a server) drains it onto the bus
/// (`handlers/serve.rs`). With no server in process (TUI against a remote
/// daemon, short-lived commands) sends are a silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallationNotice {
    /// `installation.update-available` `{version}`.
    Available(String),
    /// `installation.updated` `{version}`.
    Installed(String),
}

type InstallationChannel = (
    tokio::sync::mpsc::UnboundedSender<InstallationNotice>,
    Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<InstallationNotice>>>,
);

static INSTALLATION_CHANNEL: LazyLock<InstallationChannel> = LazyLock::new(|| {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (tx, Mutex::new(Some(rx)))
});

/// Sends one installation notice (no-op when nobody drains).
fn notify_installation(notice: InstallationNotice) {
    let _ = INSTALLATION_CHANNEL.0.send(notice);
}

/// Takes the receiver the server-hosting process drains (`serve`); only
/// the first caller gets one.
pub fn take_installation_receiver(
) -> Option<tokio::sync::mpsc::UnboundedReceiver<InstallationNotice>> {
    INSTALLATION_CHANNEL.1.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
}

/// How often the background checker re-runs.
pub const CHECK_INTERVAL: Duration = Duration::from_mins(10);

/// The notice-file name under the state dir (TUI mode writes here
/// instead of stderr — printing over a TUI screen corrupts it).
pub const NOTICE_FILE: &str = "update-notice.log";

/// Env kill-switch: any truthy value forces [`Policy::Disable`].
pub const DISABLE_ENV: &str = "OPENCODE_DISABLE_AUTOUPDATE";

/// The update policy (`Config.Info.update`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Never check or update.
    Disable,
    /// Check and announce (stderr when not in the TUI, notice file
    /// when in it). Default.
    Notify,
    /// Check and install (verified, atomic), announce the result.
    Auto,
}

impl Policy {
    /// Parse the config string form.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "disable" => Some(Self::Disable),
            "notify" => Some(Self::Notify),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// Resolve the effective policy: env kill-switch, then the global
/// config files (`opencode.jsonc` > `opencode.json` > legacy
/// `config.json`, first file that states a policy wins), else Notify.
#[must_use]
pub fn resolve_policy() -> Policy {
    if env_truthy(DISABLE_ENV) {
        return Policy::Disable;
    }
    let config = oc_util::global_roots::resolve().config;
    policy_from_config_dir(&config).unwrap_or(Policy::Notify)
}

/// Extract a policy from config-file text (JSONC tolerated via json5).
/// A present-but-invalid `update` value falls through to the legacy
/// `autoupdate` key; a document stating neither is `None`.
#[must_use]
pub fn policy_from_text(text: &str) -> Option<Policy> {
    let value: serde_json::Value = json5::from_str(text).ok()?;
    let object = value.as_object()?;
    if let Some(update) = object.get("update") {
        if let Some(policy) = update.as_str().and_then(Policy::parse) {
            return Some(policy);
        }
    }
    // Legacy v1 `autoupdate` (mirrors oc-core normalize's mapping).
    match object.get("autoupdate") {
        Some(serde_json::Value::Bool(false)) => Some(Policy::Disable),
        Some(serde_json::Value::Bool(true)) => Some(Policy::Auto),
        Some(serde_json::Value::String(value)) if value == "notify" => Some(Policy::Notify),
        _ => None,
    }
}

fn policy_from_config_dir(dir: &Path) -> Option<Policy> {
    // Same relative order as the config loader: jsonc beats json;
    // the v1 config.json is the legacy fallback.
    for name in ["opencode.jsonc", "opencode.json", "config.json"] {
        if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
            if let Some(policy) = policy_from_text(&text) {
                return Some(policy);
            }
        }
    }
    None
}

/// Truthy env value: set and not one of the falsy spellings.
fn env_truthy(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !value.is_empty() && value != "0" && value != "false"
        }
        Err(_) => false,
    }
}

/// The notice-file path (`<state>/update-notice.log`).
#[must_use]
pub fn notice_file() -> PathBuf {
    oc_util::global_roots::resolve().state.join(NOTICE_FILE)
}

/// Append one line to the notice file (TUI mode's notify channel).
fn append_notice(line: &str) {
    let file = notice_file();
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        use std::io::Write as _;
        let _ = writeln!(file, "{line}");
    }
}

/// Entry point for the CLI: spawn the detached checker for the
/// long-running paths ONLY (`tui` selects the notify channel — stderr
/// when false, the notice file when true). Returns the handle (never
/// joined in production — the thread dies with the process); `None`
/// when the checker must not run at all (`dev` build or policy
/// disabled — no thread, no network).
pub fn spawn_background_update_check(tui: bool) -> Option<JoinHandle<()>> {
    spawn_with(updater::RELEASE_TAG, update_feed::FEED_URL, tui, None, None)
}

/// Stop-flag source for the test harness: production passes `None`
/// (loop forever), tests pass a flag they flip so the thread joins
/// promptly.
type StopFlag = Arc<AtomicBool>;

/// Test- and embedder-facing core of the entry point (the public
/// wrapper just fixes the release tag and feed URL). The whole body is
/// wrapped in `catch_unwind`: a panic anywhere inside is swallowed —
/// the hard requirement forbids ANY escape.
fn spawn_with(
    tag: &str,
    feed: &str,
    tui: bool,
    stop: Option<StopFlag>,
    checks_done: Option<Arc<AtomicU32>>,
) -> Option<JoinHandle<()>> {
    // Dev builds (and anything the policy disables) never touch the
    // network at all.
    if tag == "dev" || resolve_policy() == Policy::Disable {
        return None;
    }
    let tag = tag.to_owned();
    let feed = feed.to_owned();
    std::thread::Builder::new()
        .name("opencode-rust-update-check".to_owned())
        .spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_checks(&tag, &feed, tui, stop.as_deref(), checks_done.as_ref());
            }));
            // A panicking checker thread ends silently; the process
            // keeps running with the current binary. Nothing to clean
            // up beyond the PID-unique cache temps (swept next run).
        })
        .ok()
}

/// The loop: policy re-read (cheap), one bounded check, then sleep the
/// interval in small chunks so a test's stop flag is honored promptly.
fn run_checks(
    tag: &str,
    feed: &str,
    tui: bool,
    stop: Option<&AtomicBool>,
    checks_done: Option<&Arc<AtomicU32>>,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        return;
    };
    // One notice/install per detected version, ever, per process.
    let mut handled: Option<String> = None;
    loop {
        let policy = resolve_policy();
        if policy != Policy::Disable {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.block_on(run_one(tag, feed, tui, policy, &mut handled));
            }));
        }
        if let Some(counter) = checks_done {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        let deadline = Instant::now() + CHECK_INTERVAL;
        while Instant::now() < deadline {
            if stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

/// One check cycle. Every failure path is a plain return — the silent
/// no-op of the hard requirement.
async fn run_one(tag: &str, feed: &str, tui: bool, policy: Policy, handled: &mut Option<String>) {
    let Some(current) = ReleaseVer::parse(tag) else { return };
    // ANY feed failure (DNS, connect, TLS garbage, HTTP error, parse,
    // rate limit) is Ok(None)/Err → silent return.
    let Some(candidate) = fetch_quiet(feed, &current).await else { return };
    if handled.as_deref() == Some(candidate.tag.as_str()) {
        return; // already announced/installed this version
    }
    match policy {
        Policy::Disable => {}
        Policy::Notify => {
            *handled = Some(candidate.tag.clone());
            announce_available(&candidate, &current, tui);
        }
        Policy::Auto => {
            if let Ok(()) = install_candidate_quiet(&candidate).await {
                *handled = Some(candidate.tag.clone());
                announce_installed(&candidate, tui);
            }
            // Failed install: current binary untouched, temps cleaned
            // inside install_candidate — retry naturally next cycle.
        }
    }
}

/// Feed query that maps every error to no-update (the auto-check
/// contract; the manual `upgrade` command is where errors surface).
async fn fetch_quiet(feed: &str, current: &ReleaseVer) -> Option<Candidate> {
    update_feed::fetch_latest(feed, current).await.ok().flatten()
}

/// Auto-path install wrapper: resolve the running binary, delegate to
/// the verified installer. The install's own failure cleanup keeps the
/// current binary running.
async fn install_candidate_quiet(candidate: &Candidate) -> Result<(), String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("opencode-rust"));
    updater::install_candidate(candidate, &exe).await
}

fn announce_available(candidate: &Candidate, current: &ReleaseVer, tui: bool) {
    let line = format!(
        "opencode-rust: update available: v{} (current v{current}); run 'opencode-rust upgrade'",
        candidate.version
    );
    if tui {
        append_notice(&line);
    } else {
        eprintln!("{line}");
    }
    // v2 process.ts:116 — `installation.update-available` `{version}`.
    notify_installation(InstallationNotice::Available(candidate.version.to_string()));
}

fn announce_installed(candidate: &Candidate, tui: bool) {
    let line =
        format!("opencode-rust: update installed (v{}), restart to apply", candidate.version);
    if tui {
        append_notice(&line);
    } else {
        eprintln!("{line}");
    }
    // v2 process.ts:117 — `installation.updated` `{version}`.
    notify_installation(InstallationNotice::Installed(candidate.version.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Isolated env for the policy tests (nextest runs each test in
    /// its own process, so mutation is safe).
    struct Env {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl Env {
        fn lock(vars: &[&'static str]) -> Self {
            let saved =
                vars.iter().map(|&name| (name, std::env::var(name).ok())).collect::<Vec<_>>();
            for &(name, _) in &saved {
                std::env::remove_var(name);
            }
            Self { saved }
        }
        #[allow(clippy::unused_self)] // keeps call sites reading like a harness
        fn set(&self, name: &str, value: &str) {
            std::env::set_var(name, value);
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    fn sandbox_home() -> (Env, tempfile::TempDir) {
        let env = Env::lock(&[
            "OPENCODE_DISABLE_AUTOUPDATE",
            "OPENCODE_TEST_HOME",
            "XDG_CONFIG_HOME",
            "XDG_STATE_HOME",
            "XDG_CACHE_HOME",
        ]);
        let home = tempfile::tempdir().expect("home");
        env.set("OPENCODE_TEST_HOME", home.path().to_str().expect("utf8"));
        (env, home)
    }

    #[test]
    fn policy_text_parsing_native_update_wins() {
        assert_eq!(policy_from_text(r#"{"update":"disable"}"#), Some(Policy::Disable));
        assert_eq!(policy_from_text(r#"{"update":"notify"}"#), Some(Policy::Notify));
        assert_eq!(policy_from_text(r#"{"update":"auto"}"#), Some(Policy::Auto));
        assert_eq!(policy_from_text(r"{}"), None, "unset");
        assert_eq!(policy_from_text("not json at all"), None);
        assert_eq!(policy_from_text(r#"{"update":true}"#), None, "invalid native value");
        // jsonc comments tolerated.
        assert_eq!(
            policy_from_text("{\n  // notify me\n  \"update\": \"notify\",\n}"),
            Some(Policy::Notify)
        );
        // Native beats legacy in the same document.
        assert_eq!(
            policy_from_text(r#"{"update":"disable","autoupdate":true}"#),
            Some(Policy::Disable)
        );
    }

    #[test]
    fn policy_text_legacy_autoupdate_mapping() {
        assert_eq!(policy_from_text(r#"{"autoupdate":false}"#), Some(Policy::Disable));
        assert_eq!(policy_from_text(r#"{"autoupdate":true}"#), Some(Policy::Auto));
        assert_eq!(policy_from_text(r#"{"autoupdate":"notify"}"#), Some(Policy::Notify));
        assert_eq!(policy_from_text(r#"{"autoupdate":"weird"}"#), None);
    }

    #[test]
    fn resolve_policy_defaults_to_notify_and_reads_config_files() {
        let (env, home) = sandbox_home();
        assert_eq!(resolve_policy(), Policy::Notify, "unset → notify");

        let config = home.path().join(".config/opencode-rust");
        std::fs::create_dir_all(&config).expect("mkdir");
        std::fs::write(config.join("config.json"), r#"{"autoupdate":false}"#).expect("write");
        assert_eq!(resolve_policy(), Policy::Disable, "legacy config.json honored");

        std::fs::write(config.join("opencode.json"), r#"{"update":"auto"}"#).expect("write");
        assert_eq!(resolve_policy(), Policy::Auto, "opencode.json beats config.json");

        std::fs::write(config.join("opencode.jsonc"), "// c\n{\"update\":\"notify\"}").expect("w");
        assert_eq!(resolve_policy(), Policy::Notify, "jsonc beats json");
        drop(env);
    }

    #[test]
    fn resolve_policy_env_kill_switch_wins() {
        let (env, home) = sandbox_home();
        let config = home.path().join(".config/opencode-rust");
        std::fs::create_dir_all(&config).expect("mkdir");
        std::fs::write(config.join("opencode.json"), r#"{"update":"auto"}"#).expect("write");
        for truthy in ["1", "true", "yes", "TRUE"] {
            env.set(DISABLE_ENV, truthy);
            assert_eq!(resolve_policy(), Policy::Disable, "{truthy} must disable");
        }
        env.set(DISABLE_ENV, "0");
        assert_eq!(resolve_policy(), Policy::Auto, "\"0\" is falsy");
        env.set(DISABLE_ENV, "");
        assert_eq!(resolve_policy(), Policy::Auto, "empty is falsy");
        drop(env);
    }

    #[test]
    fn tui_notice_goes_to_the_state_log_not_stderr() {
        let (_env, _home) = sandbox_home();
        let candidate = Candidate {
            tag: "v2.0.25.beta.1".to_owned(),
            version: ReleaseVer::parse("v2.0.25.beta.1").expect("ver"),
            tarball_url: "http://127.0.0.1:1/t.tar.gz".to_owned(),
            sha256_url: "http://127.0.0.1:1/t.tar.gz.sha256".to_owned(),
        };
        announce_available(&candidate, &ReleaseVer::parse("v2.0.24.beta.1").expect("cur"), true);
        let file = notice_file();
        let text = std::fs::read_to_string(&file).expect("notice written");
        assert!(
            text.contains("update available: v2.0.25.beta.1 (current v2.0.24.beta.1)"),
            "{text}"
        );
        assert!(text.contains("opencode-rust upgrade"));
        announce_installed(&candidate, true);
        let text = std::fs::read_to_string(&file).expect("notice appended");
        assert!(text.contains("update installed (v2.0.25.beta.1), restart to apply"), "{text}");
    }

    #[tokio::test]
    async fn run_one_with_a_dead_feed_is_a_silent_no_op() {
        let (_env, _home) = sandbox_home();
        let mut handled = None;
        // Connection-refused feed + TLS-garbage flavor: neither may
        // panic, print, or mark anything handled.
        for feed in ["http://127.0.0.1:1", "http://127.0.0.1:1/feed"] {
            run_one("v2.0.24.beta.1", feed, true, Policy::Notify, &mut handled).await;
        }
        assert_eq!(handled, None, "dead feed handles nothing");
        assert!(!notice_file().exists(), "no notice for a dead feed");
    }

    #[test]
    fn spawn_skips_dev_builds_and_disabled_policies() {
        assert!(spawn_with("dev", "http://127.0.0.1:1", false, None, None).is_none());
        // Test builds ARE dev builds (no OC_RELEASE_TAG stamped).
        assert_eq!(updater::RELEASE_TAG, "dev");
        assert!(spawn_background_update_check(false).is_none());
        let (env, _home) = sandbox_home();
        env.set(DISABLE_ENV, "1");
        assert!(spawn_with("v2.0.24.beta.1", "http://127.0.0.1:1", false, None, None).is_none());
    }

    #[test]
    fn checker_thread_spawns_and_joins_cleanly_with_a_dead_network() {
        let (_env, _home) = sandbox_home();
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU32::new(0));
        let handle = spawn_with(
            "v2.0.24.beta.1",
            "http://127.0.0.1:1", // dead endpoint; bounded by the short timeouts
            false,
            Some(Arc::clone(&stop)),
            Some(Arc::clone(&done)),
        )
        .expect("checker spawns for a release tag");
        // Wait for the first completed cycle (feed errors are fast;
        // the retry backoff keeps this well under the 15 s budget).
        let deadline = Instant::now() + Duration::from_secs(30);
        while done.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(done.load(Ordering::SeqCst) >= 1, "one cycle completed against a dead feed");
        stop.store(true, Ordering::SeqCst);
        let joined = handle.join();
        assert!(joined.is_ok(), "thread must join without panicking: {joined:?}");
    }
}
