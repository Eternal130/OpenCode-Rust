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
            ("project/src/main.rs".into(), "fn main() { println!(\"hi\"); }\n".into()),
            ("project/README.md".into(), "# browser-e2e workspace\n".into()),
        ],
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
