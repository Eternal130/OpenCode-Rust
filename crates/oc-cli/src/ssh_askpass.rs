//! Port of `cli/src/ssh-askpass.ts` — the hidden SSH askpass entry. When
//! `OPENCODE_SSH_ASKPASS_PORT` is set, OpenSSH has exec'd this binary as the
//! askpass helper: connect to the TUI's loopback broker, forward the prompt
//! (+ confirm flag + token), and print the answer (or exit 1 for cancel).

use std::io::Write as _;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub async fn run() -> i32 {
    askpass().await.unwrap_or(1)
}

async fn askpass() -> Result<i32, String> {
    let port: u16 = std::env::var("OPENCODE_SSH_ASKPASS_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|port| (0..=65535).contains(port))
        .ok_or("invalid askpass port")?;
    let token = std::env::var("OPENCODE_SSH_ASKPASS_TOKEN").ok();
    let text = std::env::args().skip(2).collect::<Vec<_>>().join(" ");
    let confirm = std::env::var("SSH_ASKPASS_PROMPT").as_deref() == Ok("confirm");

    let io = tokio::time::timeout(Duration::from_mins(5), async {
        let stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| e.to_string())?;
        let (reader, mut writer) = stream.into_split();
        let request = serde_json::json!({
            "token": token,
            "text": text,
            "confirm": confirm,
        });
        writer.write_all(format!("{request}\n").as_bytes()).await.map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(reader);
        let mut response = String::new();
        // The broker answers with one JSON line.
        reader.read_line(&mut response).await.map_err(|e| e.to_string())?;
        // Keep draining until the peer closes (v2 runs both directions
        // concurrently; a single answer + close is the observable contract).
        let mut sink = [0u8; 512];
        let _ = tokio::time::timeout(Duration::from_secs(2), reader.read(&mut sink)).await;
        Ok::<String, String>(response)
    })
    .await
    .map_err(|_| "askpass timed out".to_owned())??;

    let value: serde_json::Value =
        serde_json::from_str(io.trim_end()).map_err(|e| format!("bad broker response: {e}"))?;
    match value.get("value").cloned() {
        Some(serde_json::Value::String(answer)) => {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{answer}");
            Ok(0)
        }
        _ => Ok(1),
    }
}
