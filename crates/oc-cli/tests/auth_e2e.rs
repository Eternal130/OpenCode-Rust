//! BE-3 e2e: `oc auth` against a real spawned `opencode serve` — the CLI
//! login/list/logout battery over the integration host routes, with the
//! zen (`OpenCode Console`) device flow driven by a local mock `IdP` via the
//! hidden `server` form answer.

#[allow(unused_imports)]
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

/// Points the battery at an arbitrary binary (the parity harness).
fn opencode_binary() -> PathBuf {
    std::env::var_os("OPENCODE_SMOKE_BIN")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CARGO_BIN_EXE_opencode-rust").map(PathBuf::from))
        .expect("test binary (run via cargo test)")
}

struct Sandbox {
    project: PathBuf,
    #[allow(dead_code)]
    root: PathBuf,
}

fn sandbox(name: &str) -> Sandbox {
    let root =
        std::env::temp_dir().join(format!("opencode/cli-auth-e2e-{}-{name}", std::process::id()));
    let project = root.join("project");
    let home = root.join("home");
    for dir in [
        project.join(".opencode"),
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        root.join("tmp"),
    ] {
        std::fs::create_dir_all(&dir).expect("sandbox dir");
    }
    Sandbox { project, root }
}

fn run_command(binary: &Path, project: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(project)
        .env("OPENCODE_TEST_HOME", project.parent().unwrap().join("home"))
        .env("XDG_DATA_HOME", project.parent().unwrap().join("home/.local/share"))
        .env("XDG_STATE_HOME", project.parent().unwrap().join("home/.local/state"))
        .env("XDG_CONFIG_HOME", project.parent().unwrap().join("home/.config"))
        .env("XDG_CACHE_HOME", project.parent().unwrap().join("home/.cache"))
        .env("TMPDIR", project.parent().unwrap().join("tmp"))
        .env("OPENCODE_PASSWORD", "be3-e2e-password")
        .env_remove("OPENCODE_SERVER_PASSWORD")
        .env_remove("OPENCODE_CHANNEL")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// One free TCP port (bind :0, read it, drop the listener).
async fn free_port() -> u16 {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    listener.local_addr().expect("addr").port()
}

/// The mock `IdP`: zen device endpoints answering pending-then-token.
struct MockIdp {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

async fn mock_idp() -> MockIdp {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = tokio::spawn(async move {
        let mut polls = 0;
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .split('?')
                .next()
                .unwrap_or_default()
                .to_owned();
            let body = if path == "/auth/device/code" {
                serde_json::json!({
                    "device_code": "dev-e2e",
                    "user_code": "E2E-CODE",
                    "verification_uri_complete": "/verify?code=E2E-CODE",
                    "expires_in": 900,
                    "interval": 0
                })
            } else if path == "/auth/device/token" {
                polls += 1;
                if polls < 2 {
                    serde_json::json!({"error": "authorization_pending"})
                } else {
                    serde_json::json!({
                        "access_token": "e2e-access",
                        "refresh_token": "e2e-refresh",
                        "expires_in": 3600,
                        "org_id": "org_e2e"
                    })
                }
            } else {
                serde_json::json!({"error": "not_found"})
            };
            let body_text = body.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_text}",
                body_text.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    MockIdp { base: format!("http://127.0.0.1:{port}"), handle }
}

impl Drop for MockIdp {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[tokio::test]
async fn oc_auth_login_list_logout_end_to_end() {
    let sandbox = sandbox("zen");
    let binary = opencode_binary();
    let idp = mock_idp().await;
    let port = free_port().await;

    // Spawn the real server.
    let mut server =
        run_command(&binary, &sandbox.project, &["serve", "--port", &port.to_string()]);
    server.kill_on_drop(true);
    let mut child = server.spawn().expect("spawn serve");
    let mut server_stderr = child.stderr.take().expect("stderr");
    let _ = child;
    // Drain serve's stderr so the pipe never fills.
    tokio::spawn(async move {
        let mut sink = Vec::new();
        let _ = server_stderr.read_to_end(&mut sink).await;
    });

    // Wait for readiness.
    let server_url = format!("http://127.0.0.1:{port}");
    let mut ready = false;
    let credentials = reqwest::get("http://127.0.0.1:1") // placeholder for basic-auth builder
        .await;
    drop(credentials);
    let client = reqwest::Client::new();
    for _ in 0..100 {
        let request = client
            .get(format!("{server_url}/api/info"))
            .basic_auth("opencode", Some("be3-e2e-password"));
        if request.send().await.is_ok_and(|response| response.status().is_success()) {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready, "server did not come up at {server_url}");

    // auth list before any login — v2 lists only connected integrations.
    let output = run_command(&binary, &sandbox.project, &["auth", "list", "--server", &server_url])
        .output()
        .await
        .expect("auth list");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "auth list failed: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("No authenticated integrations"), "expected empty list, got: {stdout}");

    // auth login — the zen device flow against the mock IdP (the hidden
    // `server` form answer points the flow at the local endpoint).
    let server_answer = format!("server={}", idp.base);
    let output = run_command(
        &binary,
        &sandbox.project,
        &[
            "auth",
            "login",
            "opencode",
            "--method",
            "device",
            "--answer",
            &server_answer,
            "--server",
            &server_url,
        ],
    )
    .output()
    .await
    .expect("auth login");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "auth login failed:\n{stdout}\n{stderr}");
    assert!(stdout.contains("E2E-CODE"), "user code not shown: {stdout}");
    assert!(stdout.contains("Connected to OpenCode Console"), "not connected: {stdout}");

    // auth list now shows the stored connection (the full pipeline:
    // routes → host → credential store → connection projection).
    let output = run_command(&binary, &sandbox.project, &["auth", "list", "--server", &server_url])
        .output()
        .await
        .expect("auth list 2");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(stdout.contains("org_e2e"), "credential label missing: {stdout}");

    // auth logout — removes the credential (label as the picker arg).
    let output = run_command(
        &binary,
        &sandbox.project,
        &["auth", "logout", "opencode", "org_e2e", "--server", &server_url],
    )
    .output()
    .await
    .expect("auth logout");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "auth logout failed:\n{stdout}\n{stderr}");

    let output = run_command(&binary, &sandbox.project, &["auth", "list", "--server", &server_url])
        .output()
        .await
        .expect("auth list 3");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(!stdout.contains("org_e2e"), "credential should be gone: {stdout}");
}
