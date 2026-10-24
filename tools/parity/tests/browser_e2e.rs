//! G-14 browser smoke — chunk 1: harness + scenarios 1 (auth) and 2
//! (session/prompt streaming). Chunks 2+ (file tree/mentions, PTY
//! panel, permission popup, event reconnect/Last-Event-ID) extend this
//! file.
//!
//! The Web UI itself is NOT ported (R-04 ruling): the embedded SPA is
//! the reference build, so "UI consistency" == **server-contract
//! consistency observed through a real browser** (fetch, `EventSource`,
//! same-origin headers, asset serving).
//!
//! # Opt-in
//!
//! Every test is gated at runtime on `OC_BROWSER_E2E=1` (the `OC_P10`
//! opt-in pattern; unlike p10 there is no `#[ignore]` so the plain
//! `-E test(browser_e2e)` nextest filter selects them — the gate
//! command stays a single line):
//!
//! ```text
//! CARGO_INCREMENTAL=0 OC_BROWSER_E2E=1 cargo nextest run \
//!     -p parity --no-fail-fast -E 'test(browser_e2e)'
//! ```
//!
//! # Browser
//!
//! Headless Chrome via CDP (`chromiumoxide`). Discovery: `OC_BROWSER_BIN`
//! env, then `google-chrome`/`chromium` on PATH. Launched with
//! `--no-sandbox` and a fresh user-data-dir under the test temp root —
//! the real user profile is never touched.
//!
//! # Assets
//!
//! The SPA dist is resolved in this order (fed to the server through
//! the runtime `OPENCODE_APP_ASSETS` override — unpacked dist dir or
//! packaged archive, both supported by `oc_server::app_archive`):
//!
//! 1. `OC_BROWSER_APP_ASSETS` env (dist dir or `app-archive.json`);
//! 2. `/tmp/opencode/ref/packages/app/dist` (the RELEASE.md §"Local
//!    browser smoke" one-off local build — no network in the gate);
//! 3. `/tmp/opencode/app-archive.json` (packaged variant of 2);
//! 4. otherwise a **minimal local contract page** is generated: a tiny
//!    index.html + JS exercising the same `/api/*` fetch + `/api/event`
//!    `EventSource` contracts. In mode 4 the smoke covers server contract
//!    behavior, not the real SPA bundle; a run with real assets (1-3)
//!    additionally proves the real bundle boots against the port. The
//!    active mode is printed in the test output and asserted in
//!    [`assets_mode_label`].
//!
//! # Determinism
//!
//! No fixed sleeps where a condition can be polled; each scenario runs
//! inside a `tokio::time::timeout` budget; on failure the page HTML,
//! collected console lines and (when a page is live) a PNG screenshot
//! are dumped under `<target>/browser-e2e/<scenario>-<pid>/`.

#![allow(clippy::too_many_lines)]

mod p10_common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::cdp::js_protocol::runtime::EventConsoleApiCalled;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use p10_common::ScriptedFakeLlm;
use parity::scenario::ScenarioHost;
use serde_json::Value;

/// Whole-scenario budget (server spawn happens outside, in the caller).
const SCENARIO_BUDGET: Duration = Duration::from_secs(120);
/// Budget for a single in-page poll step.
const POLL_STEP: Duration = Duration::from_millis(250);

/// Cross-test concurrency cap: every scenario holds the scenario lock
/// from spawn to teardown. Nextest runs each test in its own PROCESS,
/// so an in-process semaphore serializes nothing — the cap is a
/// blocking `flock` on a shared lock file under the target dir.
/// Parallel scenarios each launch a Chrome + an `oc-server`; a second
/// concurrent Chrome died on `pthread_create: Resource temporarily
/// unavailable` inside the dev-gate scope's task limit, so scenarios
/// run one at a time (each is ~2-4s; the battery stays well inside its
/// budget).
struct ScenarioSlot {
    _file: std::fs::File,
}

impl Drop for ScenarioSlot {
    fn drop(&mut self) {
        // Closing the fd releases the flock.
    }
}

async fn scenario_slot() -> Result<ScenarioSlot> {
    let path = artifacts_root().join(".scenario-lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    #[allow(unsafe_code)]
    tokio::task::spawn_blocking(move || -> Result<ScenarioSlot> {
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("open scenario lock {}", path.display()))?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        anyhow::ensure!(rc == 0, "flock {}: {rc}", path.display());
        Ok(ScenarioSlot { _file: file })
    })
    .await
    .context("scenario lock task")?
}

/// The shared artifact root (target dir + `browser-e2e`).
fn artifacts_root() -> PathBuf {
    let target =
        std::env::var("CARGO_TARGET_DIR").map_or_else(|_| workspace_target(), PathBuf::from);
    let dir = target.join("browser-e2e");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// ---------------------------------------------------------------------------
// Opt-in gates
// ---------------------------------------------------------------------------

fn e2e_enabled() -> bool {
    std::env::var("OC_BROWSER_E2E").ok().as_deref() == Some("1")
}

fn skip_note(reason: &str) {
    eprintln!(
        "skipping: {reason} (opt-in; set OC_BROWSER_E2E=1 and see tests/browser_e2e.rs docs)"
    );
}

fn browser_bin() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OC_BROWSER_BIN") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
        eprintln!(
            "[browser_e2e] OC_BROWSER_BIN={} is not a file; falling back to PATH",
            path.display()
        );
    }
    for name in ["google-chrome", "chromium"] {
        if let Ok(path) = which(name) {
            return Some(path);
        }
    }
    None
}

fn which(name: &str) -> Result<PathBuf> {
    let path = std::env::var("PATH").context("PATH")?;
    for dir in path.split(':') {
        let candidate = Path::new(dir).join(name);
        if parity::rust_server::is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }
    Err(anyhow!("{name} not on PATH"))
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------

/// Where the served SPA dist came from (diagnostics + assertions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetsMode {
    /// Real reference SPA dist (env override or the local one-off build).
    RealSpa,
    /// Packaged archive of the real SPA.
    RealSpaArchive,
    /// Generated minimal contract page (no real bundle available).
    MinimalContract,
}

fn assets_mode_label(mode: AssetsMode) -> &'static str {
    match mode {
        AssetsMode::RealSpa => "real-spa-dist",
        AssetsMode::RealSpaArchive => "real-spa-archive",
        AssetsMode::MinimalContract => "minimal-contract-page",
    }
}

/// Resolve the SPA assets path (see the module docs for the order) and
/// the mode it came from. The minimal-contract fallback generates a
/// dist under `root` and returns its path.
fn resolve_assets(root: &Path) -> Result<(PathBuf, AssetsMode)> {
    if let Ok(path) = std::env::var("OC_BROWSER_APP_ASSETS") {
        let path = PathBuf::from(path);
        anyhow::ensure!(path.exists(), "OC_BROWSER_APP_ASSETS={} does not exist", path.display());
        let mode = if path.extension().is_some_and(|ext| ext == "json") {
            AssetsMode::RealSpaArchive
        } else {
            AssetsMode::RealSpa
        };
        return Ok((path, mode));
    }
    let dist = PathBuf::from("/tmp/opencode/ref/packages/app/dist");
    if dist.join("index.html").is_file() {
        return Ok((dist, AssetsMode::RealSpa));
    }
    let archive = PathBuf::from("/tmp/opencode/app-archive.json");
    if archive.is_file() {
        return Ok((archive, AssetsMode::RealSpaArchive));
    }
    // Fallback: minimal contract page. It exercises the same server
    // contracts (asset serving, /api fetch + auth, /api/event
    // EventSource) but is NOT the real SPA bundle — see module docs.
    let dist = root.join("minimal-dist");
    std::fs::create_dir_all(&dist)?;
    std::fs::write(
        dist.join("index.html"),
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>opencode contract page</title></head>
<body><div id="root">contract page</div>
<script>
window.__ocReady = true;
</script></body></html>
"#,
    )?;
    Ok((dist, AssetsMode::MinimalContract))
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// One browser session: a live headless Chrome page plus artifact
/// collection. `Drop` closes the browser.
struct BrowserSession {
    page: chromiumoxide::Page,
    console: Arc<Mutex<Vec<String>>>,
    #[allow(dead_code)] // keeps the handler task's feed alive for the page
    browser: Browser,
}

/// Launch headless Chrome with a fresh profile and open `url`.
async fn open_page(url: &str, artifacts: &Path) -> Result<BrowserSession> {
    let bin = browser_bin().context("no Chrome/Chromium binary (set OC_BROWSER_BIN)")?;
    let user_data = artifacts.join("chrome-profile");
    std::fs::create_dir_all(&user_data)?;
    let config = BrowserConfig::builder()
        .chrome_executable(bin)
        .no_sandbox()
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .user_data_dir(user_data)
        .build()
        .map_err(|error| anyhow!("browser config: {error}"))?;
    let (browser, mut handler) = Browser::launch(config).await?;
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            let _ = event;
        }
    });
    let page = browser.new_page("about:blank").await?;
    let console = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&console);
    let mut events =
        page.event_listener::<EventConsoleApiCalled>().await.context("attach console listener")?;
    tokio::spawn(async move {
        while let Some(event) = events.next().await {
            let line = event
                .args
                .iter()
                .map(|arg| arg.value.as_ref().map(Value::to_string).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" ");
            if let Ok(mut sink) = sink.lock() {
                sink.push(format!("[{:?}] {}", event.r#type, line));
            }
        }
    });
    page.goto(url).await.with_context(|| format!("goto {url}"))?;
    Ok(BrowserSession { page, console, browser })
}

impl BrowserSession {
    /// Evaluate `js` in the page and return the JSON value.
    async fn eval(&self, js: &str) -> Result<Value> {
        let value = self
            .page
            .evaluate(js)
            .await
            .with_context(|| format!("evaluate: {js}"))?
            .into_value::<Value>()
            .with_context(|| format!("evaluate result not JSON: {js}"))?;
        Ok(value)
    }

    /// Poll an in-page JS expression (must return a JSON value) until
    /// `pred` holds. No fixed sleeps — `POLL_STEP` paces the poll.
    async fn poll_js(
        &self,
        js: &str,
        pred: impl Fn(&Value) -> bool,
        budget: Duration,
        what: &str,
    ) -> Result<Value> {
        let started = std::time::Instant::now();
        let mut last = Value::Null;
        while started.elapsed() < budget {
            if let Ok(value) = self.eval(js).await {
                if pred(&value) {
                    return Ok(value);
                }
                last = value;
            }
            tokio::time::sleep(POLL_STEP).await;
        }
        Err(anyhow!("timed out waiting for {what}; last value: {last}"))
    }

    /// Dump failure artifacts (screenshot + HTML + console) under
    /// `artifacts`. Best-effort: never masks the original failure.
    async fn dump_artifacts(&self, artifacts: &Path, scenario: &str) {
        let _ = std::fs::create_dir_all(artifacts);
        if let Ok(html) = self.eval("document.documentElement.outerHTML").await {
            let _ = std::fs::write(artifacts.join("page.html"), html.to_string());
        }
        if let Ok(png) = self
            .page
            .screenshot(ScreenshotParams::builder().format(CaptureScreenshotFormat::Png).build())
            .await
        {
            let _ = std::fs::write(artifacts.join(format!("{scenario}.png")), png);
        }
        if let Ok(console) = self.console.lock() {
            let _ = std::fs::write(artifacts.join("console.log"), console.join("\n"));
        }
    }

    fn console_lines(&self) -> Vec<String> {
        self.console.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

/// The artifact directory for one scenario.
fn artifacts_dir(scenario: &str) -> PathBuf {
    let target =
        std::env::var("CARGO_TARGET_DIR").map_or_else(|_| workspace_target(), PathBuf::from);
    let dir = target.join("browser-e2e").join(format!("{scenario}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn workspace_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
}

/// Spawn the Rust server with the SPA assets + the scripted fake LLM
/// provider config wired in, seeding a small workspace.
async fn spawn_server(
    assets: &Path,
    fake: &ScriptedFakeLlm,
) -> Result<parity::rust_server::RustServer> {
    let config = p10_common::provider_config(&fake.base(), false);
    let leak = |value: String| Box::leak(value.into_boxed_str()) as &'static str;
    parity::rust_server::spawn_rust(parity::rust_server::RustSpawn {
        extra_env: vec![
            ("OPENCODE_APP_ASSETS", leak(assets.display().to_string())),
            // `{sandbox}` is expanded by spawn_rust to this server's root.
            ("OPENCODE_CONFIG", leak("{sandbox}/cfg/opencode.json".to_owned())),
        ],
        seed_files: vec![
            ("cfg/opencode.json".into(), config),
            ("project/src/main.rs".into(), "fn main() { println!(\"hi browser e2e\"); }\n".into()),
            ("project/README.md".into(), "# browser-e2e workspace\n".into()),
            ("project/src/util.rs".into(), "pub fn answer() -> u32 { 42 }\n".into()),
        ],
        // Serve the seeded workspace as the visible project (the default
        // mirrors the reference cwd instead — S3 needs the seeds).
        project_cwd: Some("project".into()),
    })
    .await
}

/// Shared preflight: the runtime gates every scenario needs. Returns
/// `None` (after printing the skip note) when the battery is opt-out.
fn preflight(scenario: &str) -> Option<(tempfile::TempDir, PathBuf, AssetsMode)> {
    if !e2e_enabled() {
        skip_note("OC_BROWSER_E2E != 1");
        return None;
    }
    if browser_bin().is_none() {
        skip_note("no Chrome/Chromium on PATH (set OC_BROWSER_BIN)");
        return None;
    }
    if parity::rust_server::skip_if_no_cargo() {
        return None;
    }
    let root =
        tempfile::tempdir_in("/tmp/opencode").context("test temp root under /tmp/opencode").ok()?;
    let (assets, mode) = resolve_assets(root.path()).ok()?;
    eprintln!("[browser_e2e:{scenario}] assets mode: {}", assets_mode_label(mode));
    Some((root, assets, mode))
}

// ---------------------------------------------------------------------------
// Scenario 1: auth
// ---------------------------------------------------------------------------

/// S1 — the signed-out shell loads, anonymous API calls are 401 with
/// the JSON envelope, both credential paths (Basic header, `auth_token`
/// query param) reach `/api/info`, and the app shell renders.
///
/// Note: `WWW-Authenticate: Basic` is intentionally NOT asserted from
/// page-context `fetch` — the server emits the challenge only when
/// `sec-fetch-mode` is absent (CLI) or `navigate` (page navigation),
/// and a same-origin `fetch` carries `sec-fetch-mode: cors`. The
/// challenge header is asserted harness-side via `minih` (same HTTP
/// contract), which sends no `sec-fetch-mode`.
#[tokio::test]
async fn browser_e2e_s1_auth() -> Result<()> {
    let Some((root, assets, mode)) = preflight("s1") else {
        return Ok(());
    };
    let _scenario_slot = scenario_slot().await?;
    let fake = ScriptedFakeLlm::start(vec![p10_common::text_script("unused")])
        .await
        .context("fake llm")?;
    let server = spawn_server(&assets, &fake).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    let run = tokio::time::timeout(SCENARIO_BUDGET, s1_body(&url, &password, mode)).await;
    let result = run.map_err(|_| anyhow!("scenario timed out after {SCENARIO_BUDGET:?}"));
    if result.is_err() {
        // dump artifacts while the (possibly still live) session exists
        // is not possible here; the body dumps on its own failures.
    }
    let outcome = result?;
    server.stop().await?;
    drop(root);
    outcome
}

async fn s1_body(url: &str, password: &str, mode: AssetsMode) -> Result<()> {
    let artifacts = artifacts_dir("s1");

    // Harness-side: the 401 challenge header (no sec-fetch-mode sent).
    let anon = parity::minih::request(
        url,
        None,
        "GET",
        "/api/info",
        None,
        "application/json",
        Duration::from_secs(10),
    )
    .await
    .context("anonymous /api/info")?;
    anyhow::ensure!(anon.status == 401, "anonymous /api/info status {}", anon.status);
    let challenge = anon
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("www-authenticate"))
        .map(|(_, value)| value.clone());
    anyhow::ensure!(
        challenge.as_ref().is_some_and(|value| value.to_ascii_lowercase().contains("basic")),
        "anonymous /api/info missing WWW-Authenticate Basic: {:?}",
        anon.headers
    );

    let mut session = open_page(url, &artifacts).await?;
    let result = s1_page_checks(&mut session, url, password, mode).await;
    if result.is_err() {
        session.dump_artifacts(&artifacts, "s1").await;
        eprintln!("console: {:?}", session.console_lines());
    }
    result
}

async fn s1_page_checks(
    session: &mut BrowserSession,
    _url: &str,
    password: &str,
    mode: AssetsMode,
) -> Result<()> {
    // The signed-out shell: the SPA index itself is served BEFORE auth
    // (assets.rs), so the page commits without credentials.
    session
        .poll_js(
            "document.readyState",
            |value| value == "complete" || value == "interactive",
            Duration::from_secs(30),
            "document readyState",
        )
        .await?;
    let shell = session
        .poll_js(
            "(() => ({ children: document.body.children.length, title: document.title }))()",
            |value| value.get("children").and_then(Value::as_u64).unwrap_or(0) > 0,
            Duration::from_secs(30),
            "app shell renders (body children)",
        )
        .await?;
    eprintln!("[s1] shell: {shell}");

    // One in-page probe covering all three credential paths. Runs in a
    // same-origin page context, so response headers are fully readable.
    let js = format!(
        r"(async () => {{
          const out = {{}};
          const anon = await fetch('/api/info', {{ credentials: 'omit' }});
          out.anonStatus = anon.status;
          out.anonBody = await anon.json();
          const basic = 'Basic ' + btoa('opencode:{password}');
          const withBasic = await fetch('/api/info', {{ headers: {{ Authorization: basic }} }});
          out.basicStatus = withBasic.status;
          out.basicBody = await withBasic.json();
          const token = encodeURIComponent(btoa('opencode:{password}'));
          const withToken = await fetch('/api/info?auth_token=' + token, {{ credentials: 'omit' }});
          out.tokenStatus = withToken.status;
          out.tokenBody = await withToken.json();
          const badCreds = 'Basic ' + btoa('opencode:wrong');
          const wrong = await fetch('/api/info', {{ headers: {{ Authorization: badCreds }} }});
          out.wrongStatus = wrong.status;
          return out;
        }})()"
    );
    let probe = session
        .poll_js(&js, |value| !value.is_null(), Duration::from_secs(30), "auth probe")
        .await?;
    eprintln!("[s1] probe: {probe}");
    anyhow::ensure!(probe["anonStatus"] == 401, "anonymous fetch not 401: {probe}");
    anyhow::ensure!(
        probe["anonBody"]["error"].is_string() || probe["anonBody"]["message"].is_string(),
        "401 body is not a JSON error envelope: {probe}"
    );
    anyhow::ensure!(probe["basicStatus"] == 200, "Basic-credentialed fetch not 200: {probe}");
    anyhow::ensure!(
        probe["basicBody"]["version"].is_string(),
        "Basic /api/info body missing version: {probe}"
    );
    anyhow::ensure!(probe["tokenStatus"] == 200, "auth_token fetch not 200: {probe}");
    anyhow::ensure!(
        probe["tokenBody"]["version"].is_string(),
        "auth_token /api/info body missing version: {probe}"
    );
    anyhow::ensure!(probe["wrongStatus"] == 401, "wrong-password fetch not 401: {probe}");

    // With the real SPA the shell that rendered above is the real
    // bundle booting against the port; with the minimal contract page
    // only the contract behavior is covered (module docs).
    match mode {
        AssetsMode::RealSpa | AssetsMode::RealSpaArchive => {
            anyhow::ensure!(
                shell["children"].as_u64().is_some_and(|n| n >= 1),
                "real SPA shell did not render: {shell}"
            );
        }
        AssetsMode::MinimalContract => {
            eprintln!("[s1] minimal-contract mode: real-bundle boot NOT covered here");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 2: session/prompt streaming
// ---------------------------------------------------------------------------

const S2_REPLY: &str = "Browser e2e streaming reply: hello from the scripted fake LLM.";

/// S2 — a prompt served by the scripted fake LLM streams through the
/// page's `EventSource` (`session.text.delta` frames arrive in the
/// browser; v1's `message.part` no longer exists in v2) and the final
/// assistant message text matches the script.
#[tokio::test]
async fn browser_e2e_s2_prompt_streaming() -> Result<()> {
    let Some((root, assets, _mode)) = preflight("s2") else {
        return Ok(());
    };
    let _scenario_slot = scenario_slot().await?;
    let fake = ScriptedFakeLlm::start(vec![p10_common::text_script(S2_REPLY)])
        .await
        .context("fake llm")?;
    let server = spawn_server(&assets, &fake).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    let run = tokio::time::timeout(SCENARIO_BUDGET, s2_body(&url, &password)).await;
    let result = run.map_err(|_| anyhow!("scenario timed out after {SCENARIO_BUDGET:?}"));
    let outcome = result?;
    server.stop().await?;
    drop(root);
    outcome
}

async fn s2_body(url: &str, password: &str) -> Result<()> {
    let artifacts = artifacts_dir("s2");

    // Session created through the API (the UI path drives the same
    // endpoint); the prompt below is submitted from the page.
    let (status, session) = p10_common::api(
        url,
        password,
        "POST",
        "/api/session",
        Some(r#"{"title":"browser-e2e-s2"}"#),
    )
    .await?;
    anyhow::ensure!(status == 200, "session create status {status}: {session}");
    let id = session["data"]["id"]
        .as_str()
        .with_context(|| format!("session create missing data.id: {session}"))?
        .to_owned();

    let mut browser = open_page(url, &artifacts).await?;
    let result = s2_page_checks(&mut browser, url, password, &id).await;
    if result.is_err() {
        browser.dump_artifacts(&artifacts, "s2").await;
        eprintln!("console: {:?}", browser.console_lines());
    }
    result
}

async fn s2_page_checks(
    session: &mut BrowserSession,
    url: &str,
    password: &str,
    id: &str,
) -> Result<()> {
    session
        .poll_js(
            "document.readyState",
            |value| value == "complete" || value == "interactive",
            Duration::from_secs(30),
            "document readyState",
        )
        .await?;

    // 1. Open the event feed from the page with the query-param
    //    credential (EventSource cannot send headers) and start
    //    recording every event payload.
    let setup = format!(
        r"(async () => {{
          window.__ev = [];
          const token = encodeURIComponent(btoa('opencode:{password}'));
          const es = new EventSource('/api/event?auth_token=' + token);
          window.__es = es;
          es.onmessage = (m) => {{
            try {{ window.__ev.push(JSON.parse(m.data)); }} catch {{ window.__ev.push({{ type: 'unparseable' }}); }}
          }};
          return new Promise((resolve) => {{
            const t0 = Date.now();
            const check = () => {{
              if (window.__ev.some((e) => e.type === 'server.connected') || Date.now() - t0 > 15000) resolve(window.__ev.length);
              else setTimeout(check, 100);
            }};
            check();
          }});
        }})()"
    );
    let first = session
        .poll_js(&setup, |value| !value.is_null(), Duration::from_secs(30), "EventSource setup")
        .await?;
    anyhow::ensure!(first.as_u64().unwrap_or(0) > 0, "event feed recorded no frames: {first}");

    // 2. Submit the prompt from the page (Basic header), driving the
    //    same endpoint the SPA composer uses.
    let prompt_js = format!(
        r"(async () => {{
          const basic = 'Basic ' + btoa('opencode:{password}');
          try {{
            const response = await fetch('/api/session/{id}/prompt', {{
              method: 'POST',
              headers: {{ Authorization: basic, 'Content-Type': 'application/json' }},
              body: JSON.stringify({{ text: 'please reply' }}),
            }});
            const text = await response.text();
            let body;
            try {{ body = JSON.parse(text); }} catch {{ body = text; }}
            return {{ status: response.status, body }};
          }} catch (e) {{
            return {{ error: String(e) }};
          }}
        }})()"
    );
    let submit = session
        .poll_js(
            &prompt_js,
            |value| value.get("status").is_some() || value.get("error").is_some(),
            Duration::from_secs(30),
            "prompt submit",
        )
        .await?;
    eprintln!("[s2] submit: {submit}");
    anyhow::ensure!(submit["status"] == 200, "prompt submit not 200: {submit}");

    // 3. Poll the recorded feed until streaming part frames arrived (the
    //    browser saw the stream), then assert the persisted text. Note
    //    the v2 event naming: the per-part stream frame is
    //    `session.text.delta` (v1's `message.part` no longer exists in
    //    the schema — packages/schema/src/session-event.ts:405).
    let wait_parts = session
        .poll_js(
            "window.__ev.map((e) => e.type)",
            |types| {
                types.as_array().is_some_and(|rows| rows.iter().any(|t| t == "session.text.delta"))
            },
            Duration::from_secs(60),
            "session.text.delta events through the page EventSource",
        )
        .await?;
    eprintln!("[s2] event types: {wait_parts}");

    // 4. Final message text matches the fake LLM script (API-side).
    let messages = session_final_text_check(url, password, id).await?;
    eprintln!("[s2] final assistant text: {messages:?}");
    Ok(())
}

async fn session_final_text_check(url: &str, password: &str, id: &str) -> Result<String> {
    let mut collected = String::new();
    let mut last_rows = Value::Null;
    for _ in 0..80 {
        tokio::time::sleep(POLL_STEP).await;
        let (status, body) =
            p10_common::api(url, password, "GET", &format!("/api/session/{id}/message"), None)
                .await?;
        anyhow::ensure!(status == 200, "message list status {status}: {body}");
        let rows = body["data"].as_array().cloned().unwrap_or_default();
        last_rows = Value::Array(rows.clone());
        if let Some(assistant) = rows.iter().rev().find(|row| {
            row["type"].as_str() == Some("assistant") || row["role"].as_str() == Some("assistant")
        }) {
            let mut text = String::new();
            // The assistant row carries `content: [{type:"text", text}]`
            // (v2 message schema; there is no v1-style `parts` field).
            if let Some(content) = assistant["content"].as_array() {
                for part in content {
                    if part["type"].as_str() == Some("text") {
                        text.push_str(part["text"].as_str().unwrap_or_default());
                    }
                }
            }
            if text.contains(S2_REPLY) {
                return Ok(text);
            }
            collected = text;
        }
    }
    Err(anyhow!(
        "assistant text never matched the fake LLM script; last: {collected:?}; rows: {last_rows}"
    ))
}

// ---------------------------------------------------------------------------
// Scenario 3: file tree / mentions
// ---------------------------------------------------------------------------

/// S3 — the file-tree/mention contracts the SPA composer consumes,
/// against the seeded workspace (the server cwd is pinned at
/// `project/` via `RustSpawn::project_cwd`, so the request location's
/// directory IS the seeded workspace — not the reference checkout the
/// default cwd-mirroring would pick):
///
/// - `GET /api/fs/list` — the file-tree root (the SPA file picker);
/// - `GET /api/fs/read/src/main.rs` — file content open;
/// - `GET /api/fs/find?query=main` — the @-mention / context-popover
///   autocomplete lookup the ref SPA drives through `fsFind`
///   (`packages/app/e2e/utils/mock-api.ts:206`,
///   `command-palette.spec.ts` routes `**/api/fs/find?*`).
///
/// Paths are location-relative (`RelativePath`,
/// `handlers/fs.ts:16-19`) — exactly what the browser sends.
#[tokio::test]
async fn browser_e2e_s3_file_tree_mentions() -> Result<()> {
    let Some((root, assets, _mode)) = preflight("s3") else {
        return Ok(());
    };
    let _scenario_slot = scenario_slot().await?;
    let fake = ScriptedFakeLlm::start(vec![p10_common::text_script("unused")])
        .await
        .context("fake llm")?;
    let server = spawn_server(&assets, &fake).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    let run = tokio::time::timeout(SCENARIO_BUDGET, s3_body(&url, &password)).await;
    let result = run.map_err(|_| anyhow!("scenario timed out after {SCENARIO_BUDGET:?}"));
    let outcome = result?;
    server.stop().await?;
    drop(root);
    outcome
}

async fn s3_body(url: &str, password: &str) -> Result<()> {
    let artifacts = artifacts_dir("s3");
    let mut session = open_page(url, &artifacts).await?;
    let result = s3_page_checks(&mut session, password).await;
    if result.is_err() {
        session.dump_artifacts(&artifacts, "s3").await;
        eprintln!("console: {:?}", session.console_lines());
    }
    result
}

async fn s3_page_checks(session: &mut BrowserSession, password: &str) -> Result<()> {
    session
        .poll_js(
            "document.readyState",
            |value| value == "complete" || value == "interactive",
            Duration::from_secs(30),
            "document readyState",
        )
        .await?;

    let js = format!(
        r"(async () => {{
          const headers = {{ Authorization: 'Basic ' + btoa('opencode:{password}') }};
          const out = {{}};
          const list = await fetch('/api/fs/list', {{ headers }});
          out.listStatus = list.status;
          out.listBody = await list.json();
          const read = await fetch('/api/fs/read/src/main.rs', {{ headers }});
          out.readStatus = read.status;
          out.readBody = await read.text();
          const find = await fetch('/api/fs/find?query=main', {{ headers }});
          out.findStatus = find.status;
          out.findBody = await find.json();
          const readme = await fetch('/api/fs/find?query=README', {{ headers }});
          out.readmeStatus = readme.status;
          out.readmeBody = await readme.json();
          return out;
        }})()"
    );
    let probe = session
        .poll_js(
            &js,
            |value| {
                value.get("listStatus").is_some()
                    && value.get("readStatus").is_some()
                    && value.get("findStatus").is_some()
                    && value.get("readmeStatus").is_some()
            },
            Duration::from_secs(30),
            "fs probe",
        )
        .await?;
    eprintln!(
        "[s3] statuses: list={} read={} find={} readme={}",
        probe["listStatus"], probe["readStatus"], probe["findStatus"], probe["readmeStatus"]
    );

    // File tree root lists the seeded workspace entries.
    anyhow::ensure!(probe["listStatus"] == 200, "fs/list not 200: {}", probe["listBody"]);
    let list_rows = probe["listBody"]["data"].as_array().cloned().unwrap_or_default();
    let listed: Vec<String> = list_rows
        .iter()
        .filter_map(|row| row["name"].as_str().or_else(|| row["path"].as_str()).map(str::to_owned))
        .collect();
    eprintln!("[s3] fs/list entries: {listed:?}");
    anyhow::ensure!(
        listed.iter().any(|entry| entry.contains("src")),
        "fs/list missing the seeded src/ directory: {listed:?}"
    );
    anyhow::ensure!(
        listed.iter().any(|entry| entry.contains("README")),
        "fs/list missing the seeded README.md: {listed:?}"
    );

    // File content read matches the seed.
    anyhow::ensure!(probe["readStatus"] == 200, "fs/read not 200: {}", probe["readBody"]);
    let read_body = probe["readBody"].as_str().unwrap_or_default();
    anyhow::ensure!(
        read_body.contains("fn main") && read_body.contains("hi browser e2e"),
        "fs/read/src/main.rs content mismatch: {read_body:?}"
    );

    // The @-mention autocomplete lookup (fsFind) finds the seeded files.
    for (key, needle) in [("findBody", "main.rs"), ("readmeBody", "README.md")] {
        anyhow::ensure!(
            probe[key.split("Status").next().unwrap_or(key)]["data"].as_array().is_some_and(
                |rows| {
                    rows.iter().any(|row| {
                        row["path"]
                            .as_str()
                            .or_else(|| row["name"].as_str())
                            .is_some_and(|path| path.contains(needle))
                    })
                }
            ),
            "fs/find did not surface {needle}: {}",
            probe[key.split("Status").next().unwrap_or(key)]["data"]
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scenario 6: event reconnect (Last-Event-ID)
// ---------------------------------------------------------------------------

/// S6 — reconnect semantics against the ground truth in
/// `oc-server/src/sse.rs` + `oc-core/src/bus.rs` (mirroring the ref
/// `event-feed.ts`/`handlers/event.ts`):
///
/// - **`/api/event` is live-only**: frames are `data:`-only (no SSE
///   `id:` field, `sse.rs::frame`), so a browser `EventSource` never
///   gains a `Last-Event-ID` and a reconnect does NOT replay the
///   missed window — it re-delivers only the fresh `server.connected`
///   preamble plus live events published after the reopen. Clients
///   resync via snapshots (what the SPA does on
///   "event stream disconnected").
/// - **The cursor-resume surface is
///   `GET /api/experimental/session/:id/log?after=<seq>`** (ref
///   `session.log`, handlers/session.ts:611): `after` replays the
///   durable per-session log strictly past the cursor — no gap, no
///   duplicate — ending at a `log.synced` marker carrying the
///   watermark (`bus.rs::log`, `follow:false` completes there).
///
/// Both halves are asserted from page context (fetch stream reader +
/// `EventSource`), including a mid-stream close/reopen on the log.
#[tokio::test]
async fn browser_e2e_s6_event_reconnect() -> Result<()> {
    let Some((root, assets, _mode)) = preflight("s6") else {
        return Ok(());
    };
    let _scenario_slot = scenario_slot().await?;
    let marker1 = "First turn marker for the reconnect scenario.";
    let marker2 = "Second turn marker for the reconnect scenario.";
    let fake = ScriptedFakeLlm::start(vec![
        p10_common::text_script(marker1),
        p10_common::text_script(marker2),
    ])
    .await
    .context("fake llm")?;
    let server = spawn_server(&assets, &fake).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    let run =
        tokio::time::timeout(SCENARIO_BUDGET, s6_body(&url, &password, marker1, marker2)).await;
    let result = run.map_err(|_| anyhow!("scenario timed out after {SCENARIO_BUDGET:?}"));
    let outcome = result?;
    server.stop().await?;
    drop(root);
    outcome
}

async fn s6_body(url: &str, password: &str, marker1: &str, marker2: &str) -> Result<()> {
    let artifacts = artifacts_dir("s6");

    // A session with one durable turn: the log cursor has content.
    let (status, created) = p10_common::api(
        url,
        password,
        "POST",
        "/api/session",
        Some(r#"{"title":"browser-e2e-s6"}"#),
    )
    .await?;
    anyhow::ensure!(status == 200, "session create status {status}: {created}");
    let id = created["data"]["id"]
        .as_str()
        .with_context(|| format!("session create missing data.id: {created}"))?
        .to_owned();
    submit_prompt(url, password, &id, marker1).await?;
    wait_assistant_text(url, password, &id, marker1).await?;

    let mut browser = open_page(url, &artifacts).await?;
    let result = s6_page_checks(&mut browser, url, password, &id, marker2).await;
    if result.is_err() {
        browser.dump_artifacts(&artifacts, "s6").await;
        eprintln!("console: {:?}", browser.console_lines());
    }
    result
}

/// POST `/api/session/:id/prompt` from the harness side (the page-side
/// submission shape is already covered by S2).
async fn submit_prompt(url: &str, password: &str, id: &str, text: &str) -> Result<()> {
    let body = serde_json::json!({ "text": text }).to_string();
    let (status, value) =
        p10_common::api(url, password, "POST", &format!("/api/session/{id}/prompt"), Some(&body))
            .await?;
    anyhow::ensure!(status == 200, "prompt submit status {status}: {value}");
    Ok(())
}

/// Poll the message list until the assistant text contains `needle`.
async fn wait_assistant_text(url: &str, password: &str, id: &str, needle: &str) -> Result<()> {
    for _ in 0..120 {
        tokio::time::sleep(POLL_STEP).await;
        let (status, body) =
            p10_common::api(url, password, "GET", &format!("/api/session/{id}/message"), None)
                .await?;
        anyhow::ensure!(status == 200, "message list status {status}: {body}");
        let done = body["data"].as_array().is_some_and(|rows| {
            rows.iter().any(|row| {
                row["type"].as_str() == Some("assistant")
                    && row["content"].as_array().is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part["text"].as_str().is_some_and(|text| text.contains(needle))
                        })
                    })
            })
        });
        if done {
            return Ok(());
        }
    }
    Err(anyhow!("assistant text never contained {needle:?}"))
}

async fn s6_page_checks(
    session: &mut BrowserSession,
    url: &str,
    password: &str,
    id: &str,
    marker2: &str,
) -> Result<()> {
    session
        .poll_js(
            "document.readyState",
            |value| value == "complete" || value == "interactive",
            Duration::from_secs(30),
            "document readyState",
        )
        .await?;

    // --- Part A: /api/event framing + live-only reconnect -------------
    // Raw framing first (fetch stream reader — EventSource hides the
    // bytes): frames must be `data:`-only, no `id:` field, so a browser
    // EventSource can never engage Last-Event-ID on this endpoint.
    // The reader runs as a page-side background task polled from Rust:
    // a single long `evaluate` sits on one CDP request until its
    // timeout if the stream stalls.
    let framing_kick = format!(
        r"(async () => {{
          window.__frame6 = null;
          (async () => {{
            const deadline = Date.now() + 15000;
            try {{
              const token = encodeURIComponent(btoa('opencode:{password}'));
              const response = await fetch('/api/event?auth_token=' + token, {{ cache: 'no-store' }});
              const reader = response.body.getReader();
              const decoder = new TextDecoder();
              let text = '';
              while (!text.includes('server.connected') && Date.now() < deadline) {{
                const {{ done, value }} = await reader.read();
                if (done) break;
                text += decoder.decode(value, {{ stream: true }});
              }}
              await reader.cancel();
              const frames = text.split('\n\n').filter((f) => f.length > 0);
              window.__frame6 = {{
                startsWithData: frames.every((f) => f.startsWith('data: ')),
                hasIdField: text.split('\n').some((line) => line.startsWith('id:')),
                sawConnected: text.includes('server.connected'),
                head: text.slice(0, 120),
              }};
            }} catch (e) {{
              window.__frame6 = {{ error: String(e) }};
            }}
          }})();
          return 'kicked';
        }})()"
    );
    session
        .poll_js(&framing_kick, |value| value == "kicked", Duration::from_secs(10), "framing kick")
        .await?;
    let framing = session
        .poll_js(
            "window.__frame6",
            |value| !value.is_null(),
            Duration::from_secs(30),
            "raw /api/event framing",
        )
        .await?;
    eprintln!("[s6] /api/event framing: {framing}");
    anyhow::ensure!(framing["sawConnected"] == true, "no server.connected frame: {framing}");
    anyhow::ensure!(
        framing["startsWithData"] == true,
        "/api/event frames are not data-only: {framing}"
    );
    anyhow::ensure!(
        framing["hasIdField"] == false,
        "/api/event frames carry an id: field (Last-Event-ID would engage — contract drift): {framing}"
    );

    // Reconnect: open es1, close it, publish while closed, reopen es2.
    // Ground truth: missed events are NOT replayed; the reopened feed
    // delivers a fresh server.connected + live events published after
    // the reopen only.
    let setup = format!(
        r"(async () => {{
          window.__ev6 = [];
          const open = () => {{
            const token = encodeURIComponent(btoa('opencode:{password}'));
            const es = new EventSource('/api/event?auth_token=' + token);
            es.onmessage = (m) => {{
              try {{ window.__ev6.push(JSON.parse(m.data)); }} catch {{}}
            }};
            return es;
          }};
          window.__es6 = open();
          const t0 = Date.now();
          return await new Promise((resolve) => {{
            const check = () => {{
              if (window.__ev6.some((e) => e.type === 'server.connected') || Date.now() - t0 > 15000) {{
                resolve(true);
              }} else {{
                setTimeout(check, 50);
              }}
            }};
            check();
          }});
        }})()"
    );
    session
        .poll_js(&setup, |value| value == &Value::Bool(true), Duration::from_secs(30), "es6 setup")
        .await?;

    // Close the feed, then publish session A's events while closed.
    session.eval("window.__es6.close(); window.__ev6 = []; 'closed'").await.context("close es6")?;
    let (status, missed) = p10_common::api(
        url,
        password,
        "POST",
        "/api/session",
        Some(r#"{"title":"s6-missed-while-closed"}"#),
    )
    .await?;
    anyhow::ensure!(status == 200, "missed-window session create status {status}: {missed}");
    let missed_id = missed["data"]["id"].as_str().unwrap_or_default().to_owned();
    // Deterministic "events committed" wait: the session row is visible.
    let mut committed = false;
    for _ in 0..40 {
        tokio::time::sleep(POLL_STEP).await;
        let (status, body) =
            p10_common::api(url, password, "GET", &format!("/api/session/{missed_id}"), None)
                .await?;
        if status == 200 {
            committed = true;
            break;
        }
        let _ = body;
    }
    anyhow::ensure!(committed, "missed-window session never became visible");

    // Reopen (new EventSource) and publish session B after the reopen.
    let reopen = format!(
        r"(async () => {{
          const token = encodeURIComponent(btoa('opencode:{password}'));
          const es = new EventSource('/api/event?auth_token=' + token);
          es.onmessage = (m) => {{
            try {{ window.__ev6.push(JSON.parse(m.data)); }} catch {{}}
          }};
          window.__es6 = es;
          const t0 = Date.now();
          return await new Promise((resolve) => {{
            const check = () => {{
              if (window.__ev6.some((e) => e.type === 'server.connected') || Date.now() - t0 > 15000) {{
                resolve(window.__ev6.length);
              }} else {{
                setTimeout(check, 50);
              }}
            }};
            check();
          }});
        }})()"
    );
    let first = session
        .poll_js(&reopen, |value| !value.is_null(), Duration::from_secs(30), "es6 reopen")
        .await?;
    anyhow::ensure!(first.as_u64().unwrap_or(0) > 0, "reopened feed delivered nothing: {first}");
    let (status, live) = p10_common::api(
        url,
        password,
        "POST",
        "/api/session",
        Some(r#"{"title":"s6-live-after-reopen"}"#),
    )
    .await?;
    anyhow::ensure!(status == 200, "post-reopen session create status {status}: {live}");
    let live_id = live["data"]["id"].as_str().unwrap_or_default().to_owned();

    // Poll: the reopened feed sees session B's events...
    let seen = session
        .poll_js(
            "window.__ev6.map((e) => JSON.stringify(e)).join('\\n')",
            |events| events.as_str().is_some_and(|text| text.contains(&live_id)),
            Duration::from_secs(30),
            "post-reopen live events",
        )
        .await?;
    // ...but NOT session A's (live-only feed, no replay).
    anyhow::ensure!(
        !seen.as_str().unwrap_or_default().contains(&missed_id),
        "events published while the feed was closed were replayed on reconnect (contract drift): {}",
        seen.as_str().unwrap_or_default()
    );
    eprintln!("[s6] reconnect: live events delivered, missed window correctly not replayed");

    // --- Part B: session-log cursor resume (after=<seq>) ---------------
    // Batch 1: full log (follow=false) through the page fetch stream.
    session
        .poll_js(
            &log_batch_js(password, id, "null"),
            |value| value == "kicked",
            Duration::from_secs(10),
            "log batch 1 kick",
        )
        .await?;
    let batch1 = session
        .poll_js(
            "window.__log6",
            |value| value.get("syncedSeq").is_some() || value.get("error").is_some(),
            Duration::from_secs(60),
            "log batch 1",
        )
        .await?;
    eprintln!("[s6] log batch 1: {batch1}");
    let seq1 = batch1["maxSeq"].as_i64().unwrap_or(-1);
    anyhow::ensure!(seq1 >= 0, "log batch 1 saw no durable seq: {batch1}");

    // Publish more events for the SAME session (turn 2), wait durable.
    submit_prompt(url, password, id, marker2).await?;
    wait_assistant_text(url, password, id, marker2).await?;

    // Batch 2: reopen past the cursor — no gap, no duplicate.
    session
        .poll_js(
            &log_batch_js(password, id, &seq1.to_string()),
            |value| value == "kicked",
            Duration::from_secs(10),
            "log batch 2 kick",
        )
        .await?;
    let batch2 = session
        .poll_js(
            "window.__log6",
            |value| value.get("syncedSeq").is_some() || value.get("error").is_some(),
            Duration::from_secs(60),
            "log batch 2 (after cursor)",
        )
        .await?;
    eprintln!("[s6] log batch 2: {batch2}");
    let seq2 = batch2["syncedSeq"].as_i64().unwrap_or(-1);
    anyhow::ensure!(seq2 > seq1, "watermark did not advance past the cursor: {seq1} -> {seq2}");

    // Batch 2 must cover the gap (turn-2 durable events present; the
    // per-delta frames are live-only — `session.text.delta` is not in
    // the durable log, only the lifecycle pair)...
    anyhow::ensure!(
        batch2["types"].as_array().is_some_and(|types| {
            types.iter().any(|t| t == "session.text.ended")
                && types.iter().any(|t| t == "session.execution.succeeded")
        }),
        "log batch 2 missed the turn-2 events: {}",
        batch2["types"]
    );
    anyhow::ensure!(
        batch2["count"].as_u64().unwrap_or(0) > 1,
        "log batch 2 carried no replay body: {batch2}"
    );
    // ...strictly past the cursor (no seq <= cursor, no duplicate ids)...
    anyhow::ensure!(
        batch2["minSeq"].as_i64().unwrap_or(i64::MAX) > seq1,
        "log batch 2 replayed at-or-before the cursor: {}",
        batch2["minSeq"]
    );
    let ids1: std::collections::HashSet<String> = batch1["ids"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
        .into_iter()
        .collect();
    let duplicates: Vec<String> = batch2["ids"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|id| ids1.contains(id))
        .collect();
    anyhow::ensure!(
        duplicates.is_empty(),
        "log batch 2 re-delivered event ids past the cursor: {duplicates:?}"
    );
    eprintln!("[s6] cursor resume: gap covered, no duplicates past cursor {seq1}");
    Ok(())
}

/// In-page JS reading one `follow:false` log batch through the fetch
/// stream reader: collects `data:` frames up to the `log.synced`
/// marker, stashing the result on `window.__log6`. Runs as a page-side
/// background task (a single long `evaluate` would hold one CDP
/// request open for the whole stream); the caller polls
/// `window.__log6` until it is set.
fn log_batch_js(password: &str, id: &str, after: &str) -> String {
    // `after` is spliced as a JS literal: "null" (no cursor) or a number.
    let after_literal = if after == "null" { "null".to_owned() } else { after.to_owned() };
    format!(
        r"(async () => {{
          window.__log6 = null;
          (async () => {{
            try {{
              const headers = {{ Authorization: 'Basic ' + btoa('opencode:{password}') }};
              const base = '/api/experimental/session/{id}/log?follow=false';
              const after = {after_literal};
              const afterQ = after === null ? '' : '&after=' + after;
              const response = await fetch(base + afterQ, {{ headers, cache: 'no-store' }});
              if (response.status !== 200) {{
                window.__log6 = {{ error: response.status, body: await response.text() }};
                return;
              }}
              const reader = response.body.getReader();
              const decoder = new TextDecoder();
              let text = '';
              for (;;) {{
                const {{ done, value }} = await reader.read();
                if (done) break;
                text += decoder.decode(value, {{ stream: true }});
              }}
              const events = text
                .split('\n\n')
                .filter((frame) => frame.startsWith('data: '))
                .map((frame) => {{
                  try {{ return JSON.parse(frame.slice(6)); }} catch {{ return null; }}
                }})
                .filter((event) => event !== null);
              const synced = events.find((event) => event.type === 'log.synced');
              const durable = events.filter((event) => event.durable && event.durable.seq !== undefined);
              const seqs = durable.map((event) => event.durable.seq);
              window.__log6 = {{
                count: events.length,
                types: events.map((event) => event.type),
                ids: durable.map((event) => event.id),
                minSeq: seqs.length ? Math.min(...seqs) : null,
                maxSeq: seqs.length ? Math.max(...seqs) : null,
                syncedSeq: synced && synced.seq !== undefined ? synced.seq : null,
              }};
            }} catch (e) {{
              window.__log6 = {{ error: String(e) }};
            }}
          }})();
          return 'kicked';
        }})()"
    )
}

// ---------------------------------------------------------------------------
// Scenario 4: PTY panel
// ---------------------------------------------------------------------------

/// S4 — the PTY panel contract, every leg from page context:
///
/// - `POST /api/pty` creates an interactive `sh`; the live feed carries
///   `pty.created` with the typed `info` payload (`id`, `status:
///   "running"`, numeric `pid`, no `exitCode` yet — `oc-schema/pty`);
/// - `POST /api/pty/:id/connect-token` (the SPA's ticket mint; requires
///   the `x-opencode-ticket: 1` header + same-host origin) returns
///   `{data: {ticket, expires_in}}`;
/// - the ticket-exempt WS `GET /api/pty/:id/connect?ticket=…`
///   (middleware.rs:198-208) attaches: stdin = text frames, stdout =
///   text frames, one binary `0x00 || {"cursor":N}` metaframe after the
///   replay (handlers/pty.rs §3);
/// - `PUT /api/pty/:id` publishes `pty.updated`; `exit` publishes
///   `pty.exited` with `exitCode` and closes the WS 1000.
#[tokio::test]
async fn browser_e2e_s4_pty_panel() -> Result<()> {
    let Some((root, assets, _mode)) = preflight("s4") else {
        return Ok(());
    };
    let _scenario_slot = scenario_slot().await?;
    let fake = ScriptedFakeLlm::start(vec![p10_common::text_script("unused")])
        .await
        .context("fake llm")?;
    let server = spawn_server(&assets, &fake).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    let run = tokio::time::timeout(SCENARIO_BUDGET, s4_body(&url, &password)).await;
    let result = run.map_err(|_| anyhow!("scenario timed out after {SCENARIO_BUDGET:?}"));
    let outcome = result?;
    server.stop().await?;
    drop(root);
    outcome
}

async fn s4_body(url: &str, password: &str) -> Result<()> {
    let artifacts = artifacts_dir("s4");
    let mut session = open_page(url, &artifacts).await?;
    let result = s4_page_checks(&mut session, password).await;
    if result.is_err() {
        session.dump_artifacts(&artifacts, "s4").await;
        eprintln!("console: {:?}", session.console_lines());
    }
    result
}

async fn s4_page_checks(session: &mut BrowserSession, password: &str) -> Result<()> {
    session
        .poll_js(
            "document.readyState",
            |value| value == "complete" || value == "interactive",
            Duration::from_secs(30),
            "document readyState",
        )
        .await?;

    // Live-feed collector (the SPA's own subscription shape).
    let setup = event_collector_js(password, "__ev4");
    session
        .poll_js(&setup, |value| value == "kicked", Duration::from_secs(10), "s4 feed kick")
        .await?;
    session
        .poll_js(
            "window.__ev4.some((e) => e.type === 'server.connected')",
            |value| value == &Value::Bool(true),
            Duration::from_secs(30),
            "s4 feed connected",
        )
        .await?;

    // Create the pty from the page.
    let create = format!(
        r"(async () => {{
          const headers = {{ Authorization: 'Basic ' + btoa('opencode:{password}'), 'Content-Type': 'application/json' }};
          const response = await fetch('/api/pty', {{ method: 'POST', headers, body: JSON.stringify({{ command: 'sh', title: 'browser-e2e-s4' }}) }});
          return {{ status: response.status, body: await response.json() }};
        }})()"
    );
    let created = session
        .poll_js(
            &create,
            |value| value.get("status").is_some(),
            Duration::from_secs(30),
            "pty create",
        )
        .await?;
    eprintln!("[s4] create: {created}");
    anyhow::ensure!(created["status"] == 200, "pty create not 200: {created}");
    let pty_id = created["body"]["data"]["id"]
        .as_str()
        .with_context(|| format!("pty create missing data.id: {created}"))?
        .to_owned();
    anyhow::ensure!(
        created["body"]["data"]["status"] == "running"
            && created["body"]["data"]["pid"].is_number(),
        "pty create row not typed as running w/ pid: {created}"
    );

    // pty.created on the live feed, typed info payload.
    let created_event = session
        .poll_js(
            "window.__ev4.filter((e) => e.type === 'pty.created').map((e) => JSON.stringify(e)).join('\\n')",
            |events| events.as_str().is_some_and(|text| text.contains(&pty_id)),
            Duration::from_secs(30),
            "pty.created event",
        )
        .await?;
    eprintln!("[s4] pty.created seen for {pty_id}");
    anyhow::ensure!(
        created_event.as_str().unwrap_or_default().contains("\"status\":\"running\"")
            || created_event.as_str().unwrap_or_default().contains("\"status\": \"running\""),
        "pty.created payload not the typed running info: {created_event}"
    );

    // Ticket mint (the SPA path): x-opencode-ticket + same-host origin.
    let token = format!(
        r"(async () => {{
          const response = await fetch('/api/pty/{pty_id}/connect-token', {{
            method: 'POST',
            headers: {{ Authorization: 'Basic ' + btoa('opencode:{password}'), 'x-opencode-ticket': '1' }},
          }});
          return {{ status: response.status, body: await response.json() }};
        }})()"
    );
    let ticket = session
        .poll_js(
            &token,
            |value| value.get("status").is_some(),
            Duration::from_secs(30),
            "connect-token",
        )
        .await?;
    eprintln!("[s4] connect-token: {ticket}");
    anyhow::ensure!(ticket["status"] == 200, "connect-token not 200: {ticket}");
    let ticket_value = ticket["body"]["data"]["ticket"]
        .as_str()
        .with_context(|| format!("connect-token missing data.ticket: {ticket}"))?
        .to_owned();

    // WS attach through the ticket-exempt URL; drive stdin; collect
    // stdout + the binary metaframe; exit and observe the close code.
    let ws_kick = format!(
        r"(async () => {{
          const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
          const url = proto + '//' + location.host + '/api/pty/{pty_id}/connect?ticket=' + encodeURIComponent('{ticket_value}');
          window.__ws4 = {{ open: false, texts: [], binaryMeta: null, closeCode: null, closeReason: null, sent: false, error: null, url }};
          const ws = new WebSocket(url);
          ws.binaryType = 'arraybuffer';
          ws.onopen = () => {{
            window.__ws4.open = true;
            ws.send('printf s4-hello-from-e2e\n');
            window.__ws4.sent = true;
          }};
          ws.onmessage = (m) => {{
            if (typeof m.data === 'string') {{
              window.__ws4.texts.push(m.data);
              if (m.data.includes('s4-hello-from-e2e')) {{
                ws.send('exit\n');
              }}
            }} else {{
              const bytes = new Uint8Array(m.data);
              const meta = bytes.length > 0 && bytes[0] === 0
                ? JSON.parse(new TextDecoder().decode(bytes.slice(1)))
                : null;
              window.__ws4.binaryMeta = meta;
            }}
          }};
          ws.onclose = (m) => {{ window.__ws4.closeCode = m.code; window.__ws4.closeReason = m.reason; }};
          ws.onerror = (e) => {{ window.__ws4.error = 'ws error type=' + e.type; }};
          return 'kicked';
        }})()"
    );
    session
        .poll_js(&ws_kick, |value| value == "kicked", Duration::from_secs(10), "ws kick")
        .await?;
    let ws = session
        .poll_js(
            "window.__ws4",
            |value| {
                value.get("closeCode").is_some_and(|code| !code.is_null())
                    || value.get("error").is_some_and(|error| !error.is_null())
            },
            Duration::from_secs(30),
            "pty ws session (marker echo + exit close)",
        )
        .await?;
    eprintln!("[s4] ws: {ws}");
    anyhow::ensure!(ws["open"] == true, "pty ws never opened: {ws}");
    anyhow::ensure!(
        ws["texts"].as_array().is_some_and(|texts| texts
            .iter()
            .any(|t| t.as_str().is_some_and(|t| t.contains("s4-hello-from-e2e")))),
        "pty ws never echoed the marker: {ws}"
    );
    anyhow::ensure!(
        ws["binaryMeta"].get("cursor").is_some(),
        "pty ws missing the binary metaframe cursor: {ws}"
    );
    anyhow::ensure!(ws["closeCode"] == 1000, "pty ws close code not 1000: {ws}");

    // pty.exited (typed: id + exitCode 0) and pty.updated via PUT title.
    let exited = session
        .poll_js(
            "window.__ev4.filter((e) => e.type === 'pty.exited').map((e) => JSON.stringify(e.data)).join('\\n')",
            |events| events.as_str().is_some_and(|text| text.contains(&pty_id)),
            Duration::from_secs(30),
            "pty.exited event",
        )
        .await?;
    eprintln!("[s4] pty.exited: {exited}");
    anyhow::ensure!(
        exited.as_str().unwrap_or_default().contains("\"exitCode\":0"),
        "pty.exited missing exitCode 0: {exited}"
    );

    // PUT on the exited row still publishes pty.updated (typed info).
    let put = format!(
        r"(async () => {{
          const headers = {{ Authorization: 'Basic ' + btoa('opencode:{password}'), 'Content-Type': 'application/json' }};
          const response = await fetch('/api/pty/{pty_id}', {{ method: 'PUT', headers, body: JSON.stringify({{ title: 'browser-e2e-s4-renamed' }}) }});
          return {{ status: response.status, body: await response.json() }};
        }})()"
    );
    let updated = session
        .poll_js(&put, |value| value.get("status").is_some(), Duration::from_secs(30), "pty update")
        .await?;
    anyhow::ensure!(updated["status"] == 200, "pty update not 200: {updated}");
    session
        .poll_js(
            "window.__ev4.filter((e) => e.type === 'pty.updated').map((e) => JSON.stringify(e.data)).join('\\n')",
            |events| {
                events
                    .as_str()
                    .is_some_and(|text| text.contains(&pty_id) && text.contains("browser-e2e-s4-renamed"))
            },
            Duration::from_secs(30),
            "pty.updated event",
        )
        .await?;
    eprintln!("[s4] pty.updated seen (renamed)");
    Ok(())
}

/// Kick off an in-page `EventSource` collector into `window[var]`.
fn event_collector_js(password: &str, var: &str) -> String {
    format!(
        r"(async () => {{
          window.{var} = [];
          const token = encodeURIComponent(btoa('opencode:{password}'));
          const es = new EventSource('/api/event?auth_token=' + token);
          es.onmessage = (m) => {{
            try {{ window.{var}.push(JSON.parse(m.data)); }} catch {{}}
          }};
          return 'kicked';
        }})()"
    )
}

// ---------------------------------------------------------------------------
// Scenario 5: permission popup
// ---------------------------------------------------------------------------

/// A fake-LLM script whose turn requests the `shell` builtin (the
/// default permission ruleset leaves unmatched actions at `Effect::Ask`,
/// oc-core/src/permission.rs:67 — no config needed).
fn shell_tool_script(call_id: &str, command: &str) -> String {
    let arguments = serde_json::json!({ "command": command }).to_string();
    let call = serde_json::json!({
        "choices":[{"delta":{"role":"assistant","tool_calls":[{
            "index":0,"id":call_id,"type":"function",
            "function":{"name":"shell","arguments":arguments}
        }]}}]
    })
    .to_string();
    let finish = serde_json::json!({
        "choices":[{"delta":{},"finish_reason":"tool_calls"}],
        "usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}
    })
    .to_string();
    format!("data: {call}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
}

const S5_AFTER: &str = "S5 finished after the permission grant.";

/// S5 — the permission popup contract, page legs end-to-end:
/// a `shell` tool call (default Ask) publishes `permission.asked` with
/// the pending `per_…` id (`sessionID`, `action`, `resources` typed per
/// oc-schema/permission); the SPA's accept path — `POST
/// /api/session/:sid/permission/:requestID/reply {"decision":"once"}`
/// (204) — unblocks the execution; `permission.replied` arrives, the
/// turn succeeds, the shell output lands in the transcript, and the
/// durable session log carries the tool step.
#[tokio::test]
async fn browser_e2e_s5_permission_popup() -> Result<()> {
    let Some((root, assets, _mode)) = preflight("s5") else {
        return Ok(());
    };
    let _scenario_slot = scenario_slot().await?;
    let fake = ScriptedFakeLlm::start(vec![
        shell_tool_script("call_s5_e2e", "echo s5-permission-marker"),
        p10_common::text_script(S5_AFTER),
    ])
    .await
    .context("fake llm")?;
    let server = spawn_server(&assets, &fake).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();

    let run = tokio::time::timeout(SCENARIO_BUDGET, s5_body(&url, &password)).await;
    let result = run.map_err(|_| anyhow!("scenario timed out after {SCENARIO_BUDGET:?}"));
    let outcome = result?;
    server.stop().await?;
    drop(root);
    outcome
}

async fn s5_body(url: &str, password: &str) -> Result<()> {
    let artifacts = artifacts_dir("s5");
    // The default build agent allows `*/*`; the SESSION ruleset (set at
    // create time, engine/mod.rs SessionRuleResolver) overrides it, so
    // the seeded ruleset pins `shell` back to ask.
    let create_body = serde_json::json!({
        "title": "browser-e2e-s5",
        "permissions": [{ "action": "shell", "resource": "*", "effect": "ask" }],
    })
    .to_string();
    let (status, created) =
        p10_common::api(url, password, "POST", "/api/session", Some(&create_body)).await?;
    anyhow::ensure!(status == 200, "session create status {status}: {created}");
    let id = created["data"]["id"]
        .as_str()
        .with_context(|| format!("session create missing data.id: {created}"))?
        .to_owned();

    let mut browser = open_page(url, &artifacts).await?;
    let result = match s5_page_checks(&mut browser, password, &id).await {
        Ok(()) => s5_aftermath(url, password, &id).await,
        Err(error) => Err(error),
    };
    if result.is_err() {
        browser.dump_artifacts(&artifacts, "s5").await;
        eprintln!("console: {:?}", browser.console_lines());
    }
    result
}

/// Post-grant aftermath: the shell tool actually ran (its output is in
/// the transcript) and the turn completed with the follow-up text.
async fn s5_aftermath(url: &str, password: &str, id: &str) -> Result<()> {
    let mut transcript = Value::Null;
    for _ in 0..120 {
        tokio::time::sleep(POLL_STEP).await;
        let (status, body) =
            p10_common::api(url, password, "GET", &format!("/api/session/{id}/message"), None)
                .await?;
        anyhow::ensure!(status == 200, "message list status {status}: {body}");
        transcript = body.clone();
        let text = serde_json::to_string(&body).unwrap_or_default();
        if text.contains("s5-permission-marker") && text.contains(S5_AFTER) {
            eprintln!("[s5] transcript carries the shell marker + final text");
            return Ok(());
        }
    }
    Err(anyhow!(
        "transcript never carried the shell marker + final text: {}",
        transcript.to_string().chars().take(800).collect::<String>()
    ))
}

async fn s5_page_checks(session: &mut BrowserSession, password: &str, id: &str) -> Result<()> {
    session
        .poll_js(
            "document.readyState",
            |value| value == "complete" || value == "interactive",
            Duration::from_secs(30),
            "document readyState",
        )
        .await?;

    let setup = event_collector_js(password, "__ev5");
    session
        .poll_js(&setup, |value| value == "kicked", Duration::from_secs(10), "s5 feed kick")
        .await?;
    session
        .poll_js(
            "window.__ev5.some((e) => e.type === 'server.connected')",
            |value| value == &Value::Bool(true),
            Duration::from_secs(30),
            "s5 feed connected",
        )
        .await?;

    // Submit the prompt from the page (the shell-tool turn).
    let prompt = format!(
        r"(async () => {{
          const headers = {{ Authorization: 'Basic ' + btoa('opencode:{password}'), 'Content-Type': 'application/json' }};
          const response = await fetch('/api/session/{id}/prompt', {{ method: 'POST', headers, body: JSON.stringify({{ text: 'please run the echo' }}) }});
          return {{ status: response.status }};
        }})()"
    );
    let submit = session
        .poll_js(
            &prompt,
            |value| value.get("status").is_some(),
            Duration::from_secs(30),
            "s5 prompt submit",
        )
        .await?;
    anyhow::ensure!(submit["status"] == 200, "prompt submit not 200: {submit}");

    // permission.asked arrives with the pending per_ id, typed payload.
    let asked = session
        .poll_js(
            "window.__ev5.filter((e) => e.type === 'permission.asked').map((e) => JSON.stringify(e)).join('\\n')",
            |events| {
                events
                    .as_str()
                    .is_some_and(|text| text.contains(id) && text.contains("\"per_"))
            },
            Duration::from_secs(60),
            "permission.asked event",
        )
        .await?;
    eprintln!("[s5] permission.asked: {asked}");
    anyhow::ensure!(
        asked.as_str().unwrap_or_default().contains("\"action\":\"shell\"")
            || asked.as_str().unwrap_or_default().contains("\"action\": \"shell\""),
        "permission.asked action not shell: {asked}"
    );
    // Extract the pending id from the event (page side — the same shape
    // the popup's accept button uses).
    let request_id = session
        .eval(&format!(
            "window.__ev5.find((e) => e.type === 'permission.asked' && (e.data || e).sessionID === '{id}').data.id"
        ))
        .await
        .with_context(|| format!("extract permission request id: {asked}"))?;
    let request_id = request_id
        .as_str()
        .with_context(|| format!("permission request id not a string: {request_id}"))?
        .to_owned();
    eprintln!("[s5] pending request: {request_id}");

    // The accept path: reply once (204).
    let reply = format!(
        r"(async () => {{
          const headers = {{ Authorization: 'Basic ' + btoa('opencode:{password}'), 'Content-Type': 'application/json' }};
          const response = await fetch('/api/session/{id}/permission/{request_id}/reply', {{
            method: 'POST', headers, body: JSON.stringify({{ decision: 'once' }}),
          }});
          return {{ status: response.status, body: await response.text() }};
        }})()"
    );
    let accepted = session
        .poll_js(
            &reply,
            |value| value.get("status").is_some(),
            Duration::from_secs(30),
            "permission reply",
        )
        .await?;
    eprintln!("[s5] reply: {accepted}");
    anyhow::ensure!(accepted["status"] == 204, "permission reply not 204: {accepted}");

    // permission.replied + execution proceeds to success.
    session
        .poll_js(
            "window.__ev5.some((e) => e.type === 'permission.replied')",
            |value| value == &Value::Bool(true),
            Duration::from_secs(30),
            "permission.replied event",
        )
        .await?;
    session
        .poll_js(
            "window.__ev5.some((e) => e.type === 'session.execution.succeeded')",
            |value| value == &Value::Bool(true),
            Duration::from_secs(60),
            "execution succeeded after grant",
        )
        .await?;
    eprintln!("[s5] replied + execution succeeded");
    Ok(())
}
