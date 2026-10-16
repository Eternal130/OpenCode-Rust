//! OAuth flow engines shared by the provider registrations: the device
//! flow (RFC 8628 — zen console, GitHub Copilot, xAI), PKCE helpers, and
//! the loopback callback listener (authorization-code + implicit flows —
//! `Poe`, `DigitalOcean`, and generic providers).

use base64::Engine;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::Duration;

/// A reqwest client with the app user-agent, cloned per flow.
pub type Http = reqwest::Client;

#[must_use]
pub fn http_client(useragent: &str) -> Http {
    reqwest::Client::builder().user_agent(useragent).build().expect("reqwest client")
}

// ---------------------------------------------------------------------------
// PKCE (RFC 7636, S256)
// ---------------------------------------------------------------------------

/// A generated PKCE pair.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

/// 32 random bytes → base64url verifier; challenge = BASE64URL(SHA256(verifier)).
#[must_use]
pub fn pkce_s256() -> Pkce {
    pkce_s256_with_bytes(32)
}

/// `n`-random-byte PKCE pair (snowflake uses 48, gitlab/poe 32).
#[must_use]
pub fn pkce_s256_with_bytes(n: usize) -> Pkce {
    let mut bytes = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    Pkce { verifier, challenge }
}

/// Decodes a JWT's payload claims without verification (claim extraction —
/// `openai.ts:415-430` account id, `chatgpt.ts` nonce checks).
#[must_use]
pub fn jwt_claims(token: &str) -> Option<BTreeMap<String, Value>> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// Whether an executable is on `PATH` (v2 `util/which`).
#[must_use]
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(program);
        candidate.is_file().then_some(candidate)
    })
}

/// `n` random bytes → base64url (OAuth `state` nonces).
#[must_use]
pub fn random_state(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Binds a pinned loopback port with v2's patience: on `AddrInUse`, probe
/// the incumbent's `/cancel` endpoint (openai.ts:131-138), retry
/// `attempts` × `retry_delay`, then try the fallback port the same way
/// before giving up with the v2 error message.
pub async fn listen_pinned(
    primary: u16,
    fallback: Option<u16>,
    mut make: impl FnMut(u16) -> LoopbackOptions,
    attempts: u32,
    retry_delay: std::time::Duration,
    describe_conflict: impl Fn(u16, u16) -> String,
) -> Result<LoopbackListener, String> {
    let mut ports = vec![primary];
    ports.extend(fallback);
    let last_port = *ports.last().expect("at least primary");
    for port in ports {
        for attempt in 0..attempts {
            match listen_loopback(make(port)).await {
                Ok(listener) => return Ok(listener),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    // Ask a stuck incumbent to exit, then wait out the race
                    // (openai.ts:131-138 — fetch /cancel with 2s timeout).
                    probe_cancel(port).await;
                    if attempt + 1 < attempts {
                        tokio::time::sleep(retry_delay).await;
                    }
                }
                Err(error) => return Err(format!("failed to bind loopback port {port}: {error}")),
            }
        }
    }
    Err(describe_conflict(primary, last_port))
}

/// Best-effort `GET /cancel` against a listener holding the pinned port.
async fn probe_cancel(port: u16) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Ok(Ok(mut stream)) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::net::TcpStream::connect(("localhost", port)),
    )
    .await
    else {
        return;
    };
    let _ = stream
        .write_all(
            format!("GET /cancel HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await;
    let mut sink = [0u8; 512];
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut sink)).await;
}

// ---------------------------------------------------------------------------
// Loopback callback listener
// ---------------------------------------------------------------------------

/// What the callback page should render (v2 `oauth/page.ts` roles):
/// success/close for code redirects, bootstrap (posts the token back) for
/// implicit flows.
#[derive(Debug, Clone)]
pub enum CallbackPage {
    /// Static HTML answering the redirect (closes itself).
    Html(String),
    /// The bootstrap page: javascript reads `location.hash`/`location.search`
    /// and POSTs it as JSON to `token_path` (implicit flow — digitalocean.ts:66-69).
    Bootstrap { token_path: String, provider: String },
}

/// Configuration for one loopback listener.
pub struct LoopbackOptions {
    /// `0` binds an ephemeral port; `DigitalOcean` pins 1456 (digitalocean.ts:22).
    pub port: u16,
    /// Bind host: `127.0.0.1` (poe) or `localhost` (digitalocean).
    pub host: String,
    /// Path the identity provider redirects to.
    pub callback_path: &'static str,
    pub page: CallbackPage,
}

impl Default for LoopbackOptions {
    fn default() -> Self {
        Self {
            port: 0,
            host: "127.0.0.1".to_owned(),
            callback_path: "/callback",
            page: CallbackPage::Html(String::new()),
        }
    }
}

/// A running loopback listener: resolves the first callback parameters
/// (query string for redirects, JSON body for the bootstrap POST-back).
pub struct LoopbackListener {
    pub port: u16,
    hit: tokio::sync::oneshot::Receiver<BTreeMap<String, String>>,
}

impl LoopbackListener {
    /// Waits for the callback and returns its parameters.
    pub async fn wait(self) -> Result<BTreeMap<String, String>, String> {
        self.hit.await.map_err(|_| "callback listener closed".to_owned())
    }
}

/// Parses `a=1&b=2` into a map.
fn parse_query(query: &str) -> BTreeMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((key, value)) => (
                urlencoding::decode(key)
                    .map_or_else(|_| key.to_owned(), std::borrow::Cow::into_owned),
                urlencoding::decode(value)
                    .map_or_else(|_| value.to_owned(), std::borrow::Cow::into_owned),
            ),
            None => (part.to_owned(), String::new()),
        })
        .collect()
}

/// The bootstrap page (a compact port of v2's `OauthCallbackPage.bootstrap`):
/// grabs the implicit-flow token out of the URL fragment/query and POSTs it
/// back to `token_path`.
#[must_use]
pub fn bootstrap_page(token_path: &str, provider: &str) -> String {
    format!(
        r#"<!doctype html><html><head><title>{provider}</title><style>body{{font-family:system-ui;display:flex;align-items:center;justify-content:center;height:100vh;margin:0}}main{{text-align:center}}</style></head><body><main><p>Completing {provider} authorization…</p><script>
(function() {{
  var params = new URLSearchParams(location.search);
  location.hash.replace(/^#/, '').split('&').forEach(function (part) {{
    var eq = part.indexOf('='); if (eq < 0) return;
    params.set(decodeURIComponent(part.slice(0, eq)), decodeURIComponent(part.slice(eq + 1)));
  }});
  var body = {{}};
  params.forEach(function (value, key) {{ body[key] = value; }});
  fetch("{token_path}", {{ method: "POST", headers: {{ "Content-Type": "application/json" }}, body: JSON.stringify(body) }})
    .then(function () {{ window.close(); setTimeout(function () {{ document.body.textContent = "You can close this window."; }}, 250); }})
    .catch(function () {{ document.body.textContent = "Failed to complete sign-in."; }});
}})();
</script></main></body></html>"#
    )
}

/// The success page (closes itself).
#[must_use]
pub fn success_page(provider: &str) -> String {
    format!(
        r"<!doctype html><html><head><title>{provider}</title></head><body><p>Connected to {provider}. You can close this window.</p><script>window.close()</script></body></html>"
    )
}

#[allow(clippy::too_many_lines)]
pub async fn listen_loopback(options: LoopbackOptions) -> std::io::Result<LoopbackListener> {
    let listener = tokio::net::TcpListener::bind((options.host.as_str(), options.port)).await?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = tokio::sync::oneshot::channel::<BTreeMap<String, String>>();
    let callback_path = options.callback_path;
    let page = options.page;
    tokio::spawn(async move {
        let mut tx = Some(tx);
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { break };
            let tx = tx.take();
            let page = page.clone();
            tokio::spawn(async move {
                let resolved = serve_callback_connection(&mut stream, callback_path, &page).await;
                if let (Some(tx), Some(params)) = (tx, resolved) {
                    let _ = tx.send(params);
                }
            });
        }
    });
    Ok(LoopbackListener { port, hit: rx })
}

/// Serves exactly one HTTP exchange on `stream`; returns the resolved
/// callback parameters when the exchange carried them.
async fn serve_callback_connection(
    stream: &mut tokio::net::TcpStream,
    callback_path: &str,
    page: &CallbackPage,
) -> Option<BTreeMap<String, String>> {
    use tokio::io::AsyncReadExt;
    let mut buffer = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    // Read until end of headers.
    let header_end = loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_subslice(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
        if buffer.len() > 64 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next()?.to_owned();
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut content_length = 0usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body_bytes = buffer[header_end..].to_vec();
    while body_bytes.len() < content_length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body_bytes.extend_from_slice(&chunk[..read]);
    }
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let query_params = parse_query(query);
    let token_path = match page {
        CallbackPage::Bootstrap { token_path, .. } => Some(token_path.as_str()),
        CallbackPage::Html(_) => None,
    };
    if method == "GET" && path == callback_path {
        let html = match page {
            CallbackPage::Html(html) => html.clone(),
            CallbackPage::Bootstrap { token_path, provider } => {
                bootstrap_page(token_path, provider)
            }
        };
        write_simple_response(stream, "200 OK", "text/html", html.as_bytes()).await;
        if let Some(_token_path) = token_path {
            // The bootstrap page will POST the token next; only the POST resolves.
            return None;
        }
        return Some(query_params);
    }
    if method == "POST" && token_path.is_some_and(|token_path| path == token_path) {
        let body = serde_json::from_slice::<serde_json::Value>(&body_bytes)
            .unwrap_or(serde_json::Value::Null);
        let mut params = BTreeMap::new();
        if let serde_json::Value::Object(map) = body {
            for (key, value) in map {
                if let Some(text) = value.as_str() {
                    params.insert(key, text.to_owned());
                }
            }
        }
        write_simple_response(stream, "200 OK", "application/json", br#"{"ok":true}"#).await;
        return Some(params);
    }
    write_simple_response(stream, "404 Not Found", "text/plain", b"Not found").await;
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

async fn write_simple_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) {
    use tokio::io::AsyncWriteExt;
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.flush().await;
}

// ---------------------------------------------------------------------------
// Device flow (RFC 8628)
// ---------------------------------------------------------------------------

/// The device-authorization response.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: Option<f64>,
    pub interval: Option<f64>,
}

/// POSTs `endpoint` with form-encoded `fields`; JSON response.
async fn post_form_json(
    http: &Http,
    endpoint: &str,
    fields: &[(&str, &str)],
) -> Result<(reqwest::StatusCode, Value), String> {
    let response = http
        .post(endpoint)
        .form(fields)
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = response.status();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    Ok((status, body))
}

/// Requests a device code (`POST endpoint` with `fields`).
pub async fn request_device_code(
    http: &Http,
    endpoint: &str,
    fields: Vec<(&str, String)>,
) -> Result<DeviceCode, String> {
    let fields: Vec<(&str, &str)> =
        fields.iter().map(|(key, value)| (*key, value.as_str())).collect();
    let (status, body) = post_form_json(http, endpoint, &fields).await?;
    if !status.is_success() {
        let error = body["error"].as_str().unwrap_or("unknown");
        let description = body["error_description"].as_str().unwrap_or("");
        return Err(format!("Device authorization failed: {error}{description}"));
    }
    let device_code = body["device_code"].as_str().ok_or("device_code missing")?.to_owned();
    let user_code = body["user_code"].as_str().ok_or("user_code missing")?.to_owned();
    let verification_uri = body["verification_uri"]
        .as_str()
        .or_else(|| body["verification_uri_complete"].as_str())
        .ok_or("verification_uri missing")?
        .to_owned();
    Ok(DeviceCode {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete: body["verification_uri_complete"].as_str().map(String::from),
        expires_in: body["expires_in"].as_f64(),
        interval: body["interval"].as_f64(),
    })
}

/// One device-flow token poll result.
pub enum DevicePoll {
    /// A token payload (provider-specific decoding happens in the caller).
    Token(Value),
    /// Keep waiting (`authorization_pending`).
    Pending,
    /// Back off to `interval` seconds (`slow_down`).
    SlowDown(f64),
}

/// Polls the token endpoint once.
pub async fn poll_device_token(
    http: &Http,
    endpoint: &str,
    client_id: &str,
    device_code: &str,
) -> Result<DevicePoll, String> {
    let grant = "urn:ietf:params:oauth:grant-type:device_code";
    let (_status, body) = post_form_json(
        http,
        endpoint,
        &[("client_id", client_id), ("device_code", device_code), ("grant_type", grant)],
    )
    .await?;
    if body["access_token"].as_str().is_some_and(|token| !token.is_empty()) {
        return Ok(DevicePoll::Token(body));
    }
    match body["error"].as_str().unwrap_or_default() {
        "authorization_pending" => Ok(DevicePoll::Pending),
        "slow_down" => Ok(DevicePoll::SlowDown(body["interval"].as_f64().unwrap_or(0.0))),
        other => Err(format!("Device authorization failed: {other}")),
    }
}

/// The shared poll loop: poll every `interval` seconds (+3s safety margin,
/// github-copilot.ts:15 / xai.ts:13) until token / hard error. Decode via
/// `decode` (each provider maps its token body to a credential).
pub async fn run_device_poll<F, Fut>(
    http: Http,
    endpoint: String,
    client_id: String,
    device_code: String,
    interval_seconds: f64,
    decode: F,
) -> Result<super::types::CredentialValue, String>
where
    F: Fn(Value) -> Fut,
    Fut: std::future::Future<Output = Result<super::types::CredentialValue, String>>,
{
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let interval = (interval_seconds.max(1.0) * 1000.0) as u64;
    let margin = 3000;
    loop {
        match poll_device_token(&http, &endpoint, &client_id, &device_code).await? {
            DevicePoll::Token(body) => return decode(body).await,
            DevicePoll::Pending => {
                tokio::time::sleep(Duration::from_millis(interval + margin)).await;
            }
            DevicePoll::SlowDown(next) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let next_ms = if next > 0.0 { (next * 1000.0) as u64 } else { interval + 5000 };
                tokio::time::sleep(Duration::from_millis(next_ms + margin)).await;
            }
        }
    }
}
