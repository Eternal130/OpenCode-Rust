//! Phase-10 cross-replacement (a): the **real v2.0.24 TUI** (bun, tmux —
//! the m1-smoke pattern) as a client of the **Rust server**, driving THREE
//! interactions end-to-end:
//!
//! 1. **prompt + reply** — type a prompt, the config-pinned `FakeLlm` model
//!    answers with deterministic text; the reply must render in the
//!    transcript (the TS client's prompt/inbox/message pipeline against the
//!    Rust server's wire contract).
//! 2. **session rename** — `ctrl+r` (the v2 `session.rename` binding) →
//!    the "Rename session" dialog → type a title → Enter; the TUI sends the
//!    `session.update`, and the Rust server's session row must show the
//!    new title (asserted through the API, not the pane).
//! 3. **permission once** — a second prompt makes the fake emit a `read`
//!    tool call on a file OUTSIDE the project → the Rust server parks it
//!    (`permission.asked`) → the TUI renders the permission footer
//!    ("Allow once") → Enter → the TUI POSTs the `once` reply itself →
//!    the parked tool proceeds and the final reply renders.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference
//! checkout + tmux):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_cross_tui)'
//! ```
//!
//! Artifacts: pane captures in `/tmp/opencode/p10-artifacts/` + evidence
//! copies in `tools/parity/goldens/p10/tui/`. Never binds 49374.

mod p10_common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use p10_common::{enabled, read_tool_script, skip, text_script, ScriptedFakeLlm};
use parity::scenario::ScenarioHost;

const SESSION: &str = "p10tui";
const W: &str = "160";
const H: &str = "40";

fn tmux(args: &[&str]) -> String {
    let output = Command::new("tmux").args(args).output().expect("run tmux");
    if !output.status.success() {
        eprintln!(
            "[p10-tui] tmux {:?} failed: {}{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn key(name: &str) {
    tmux(&["send-keys", "-t", SESSION, name]);
}

fn type_text(text: &str) {
    tmux(&["send-keys", "-t", SESSION, "-l", text]);
}

fn capture() -> String {
    tmux(&["capture-pane", "-p", "-t", SESSION])
}

async fn sleep(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
}

/// Poll captures until one contains `needle`; saves `artifact` on success
/// or the tail capture into the error.
async fn wait_capture(needle: &str, tries: usize, secs: u64, artifact: &str) -> Result<String> {
    let mut last = String::new();
    for _ in 0..tries {
        sleep(secs).await;
        last = capture();
        if last.contains(needle) {
            std::fs::write(p10_common::artifacts().join(artifact), &last)
                .context("write artifact")?;
            return Ok(last);
        }
    }
    Err(anyhow!("timed out waiting for {needle:?} in the pane:\n{last}"))
}

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs tmux + reference checkout)"]
async fn p10_ts_tui_against_rust_server() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if Command::new("tmux").arg("-V").output().is_err() {
        skip("tmux not installed");
        return Ok(());
    }
    let tui_root = std::path::Path::new(
        &std::env::var("PARITY_REF_ROOT").unwrap_or_else(|_| "/tmp/opencode/ref".to_owned()),
    )
    .join("packages/tui");
    if !tui_root.is_dir()
        && parity::gate::skip_or_fail(&format!(
            "reference TUI checkout missing at {} (set PARITY_REF_ROOT or rebuild \
             /tmp/opencode/ref)",
            tui_root.display()
        ))
    {
        return Ok(());
    }
    if p10_common::bun_path().is_err()
        && parity::gate::skip_or_fail("bun not found (set PARITY_BUN)")
    {
        return Ok(());
    }
    let bun = p10_common::bun_path().expect("bun path checked above");
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }

    // Known-red wrapper (B0-2, §8.5): the rename interaction is a
    // registered red (GATE-TUI-RENAME, expiry Batch 1B) — see
    // docs/parity-baseline.md §2.1. Categorized, never skipped.
    if let Err(error) = run_against_rust_server(&bun).await {
        return Err(p10_common::known_red("GATE-TUI-RENAME", "Batch 1B").categorize(error));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn run_against_rust_server(bun: &std::path::Path) -> Result<()> {
    let tui_root = std::path::PathBuf::from(
        std::env::var("PARITY_REF_ROOT").unwrap_or_else(|_| "/tmp/opencode/ref".to_owned()),
    )
    .join("packages/tui");

    // -- scratch layout -----------------------------------------------------
    let root = PathBuf::from(format!("/tmp/opencode/p10-tui-{}", std::process::id()));
    let project = root.join("project");
    let outside = root.join("outside");
    let tui_home = root.join("tui-home");
    for dir in [
        project.clone(),
        outside.clone(),
        tui_home.join(".local/share"),
        tui_home.join(".local/state"),
        tui_home.join(".config"),
        tui_home.join(".cache"),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    let secret = outside.join("secret.txt");
    std::fs::write(&secret, "p10-secret\n")?;
    std::fs::write(project.join("notes.md"), "p10 tui notes\n")?;

    // -- the scripted fake + the Rust server --------------------------------
    let fake = ScriptedFakeLlm::start(vec![
        // interaction 1: plain reply
        text_script("Parity fake says hello."),
        // interaction 3: read tool on the OUTSIDE file -> permission ask
        read_tool_script("call_p10_read_out", &secret.display().to_string()),
        // interaction 3 continued: final reply after the tool result
        text_script("The outside file says p10-secret."),
    ])
    .await?;
    let config = p10_common::provider_config(&fake.base(), false);
    let mut options = parity::rust_server::RustSpawn::default();
    options.seed_files.push((PathBuf::from("project/opencode.json"), config));
    options.extra_env.push(("OPENCODE_CONFIG", "{sandbox}/project/opencode.json"));
    let server = parity::rust_server::spawn_rust(options).await?;
    eprintln!("[p10-tui] rust server ready at {}", server.url());
    let url = server.url().to_owned();
    let password = server.password().to_owned();
    let config_path = server.sandbox().join("project/opencode.json").display().to_string();

    // -- boot the official v2.0.24 TUI against the Rust server -------------
    let _ = Command::new("tmux").args(["kill-session", "-t", SESSION]).output();
    let launch = format!(
        "cd {cwd} && env -i PATH={bunbin}:/usr/local/bin:/usr/bin:/bin TERM=xterm-256color \
         HOME={home} OPENCODE_TEST_HOME={home} XDG_DATA_HOME={data} XDG_STATE_HOME={state} \
         XDG_CONFIG_HOME={conf} XDG_CACHE_HOME={cache} TMPDIR={tmp} \
         OPENCODE_PASSWORD={password} OPENCODE_CONFIG={config} \
         {bun} run --cwd {cli} --define OPENCODE_VERSION=\"2.0.24\" \
         --define OPENCODE_CHANNEL=\"stable\" --define OPENCODE_ARTIFACT=\"cli\" \
         src/index.ts --server {url}",
        cwd = project.display(),
        bunbin = bun.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        bun = bun.display(),
        home = tui_home.display(),
        data = tui_home.join(".local/share").display(),
        state = tui_home.join(".local/state").display(),
        conf = tui_home.join(".config").display(),
        cache = tui_home.join(".cache").display(),
        tmp = tui_home.join(".cache").display(),
        password = password,
        config = config_path,
        cli = tui_root.join("../cli").display(),
        url = url,
    );
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", SESSION, "-x", W, "-y", H, &launch])
        .output()
        .context("tmux new-session")?;
    anyhow::ensure!(status.status.success(), "tmux new-session failed: {status:?}");

    // Connected home screen (the m1 marker).
    let boot = wait_capture("Server", 30, 2, "tui-01-boot.txt").await?;
    anyhow::ensure!(
        !boot.to_lowercase().contains("reconnect"),
        "the TUI shows a reconnect banner (not connected):\n{boot}"
    );
    // Dismiss any first-run modal.
    key("Escape");
    sleep(1).await;

    // -- interaction 1: prompt + reply --------------------------------------
    type_text("Say hello.");
    sleep(1).await;
    key("Enter");
    let reply = wait_capture("Parity fake says hello.", 30, 3, "tui-02-reply.txt").await?;
    anyhow::ensure!(
        reply.contains("Say hello."),
        "the typed prompt is missing from the transcript:\n{reply}"
    );
    eprintln!("[p10-tui] interaction 1 PASS — prompt + fake reply in transcript");

    let session_id =
        p10_common::new_session_id(&url, &password, &[]).await.context("find the TUI session")?;
    eprintln!("[p10-tui] session = {session_id}");

    // -- interaction 2: session rename (ctrl+r) ------------------------------
    key("C-r");
    let dialog = wait_capture("Rename session", 10, 1, "tui-03-rename-dialog.txt").await?;
    anyhow::ensure!(
        dialog.contains("Session title") || dialog.contains("Rename session"),
        "the rename dialog did not render:\n{dialog}"
    );
    type_text("p10 renamed from tui");
    sleep(1).await;
    key("Enter");
    let renamed = p10_common::wait_api(
        &url,
        &password,
        "GET",
        &format!("/api/session/{session_id}"),
        None,
        |value| {
            value
                .pointer("/data/title")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|title| title == "p10 renamed from tui")
        },
        10,
        1,
    )
    .await
    .context("the rename never reached the server's session row")?;
    std::fs::write(
        p10_common::artifacts().join("tui-04-rename-server-row.json"),
        serde_json::to_string_pretty(&renamed)?,
    )?;
    eprintln!("[p10-tui] interaction 2 PASS — rename PATCH reached the Rust server row");

    // -- interaction 3: permission once --------------------------------------
    type_text("Now read the outside file please.");
    sleep(1).await;
    key("Enter");
    // The Rust server parks the external read; the TUI renders the footer.
    let ask = wait_capture("Allow once", 30, 3, "tui-05-permission-ask.txt").await?;
    anyhow::ensure!(
        ask.contains("Allow once"),
        "the permission footer did not render the ask:\n{ask}"
    );
    // Also verify the ask is real on the server side.
    let asks = p10_common::wait_api(
        &url,
        &password,
        "GET",
        &format!("/api/session/{session_id}/permission"),
        None,
        |value| {
            value.get("data").and_then(serde_json::Value::as_array).is_some_and(|a| !a.is_empty())
        },
        10,
        1,
    )
    .await
    .context("no permission ask appeared on the server")?;
    std::fs::write(
        p10_common::artifacts().join("tui-05-permission-asks.json"),
        serde_json::to_string_pretty(&asks)?,
    )?;
    // Enter = "Allow once" (the default option) — the TUI POSTs the reply.
    key("Enter");
    let reply2 = wait_capture("The outside file says p10-secret.", 30, 3, "tui-06-tool-reply.txt")
        .await
        .context("the parked tool never proceeded after the TUI's once reply")?;
    anyhow::ensure!(
        !reply2.contains("Allow once"),
        "the permission footer did not resolve:\n{reply2}"
    );
    eprintln!(
        "[p10-tui] interaction 3 PASS — permission once replied through the TUI, tool proceeded"
    );

    // -- evidence copies + shutdown ------------------------------------------
    let _ = Command::new("tmux").args(["kill-session", "-t", SESSION]).output();
    let goldens = p10_common::golden_dir().join("tui");
    std::fs::create_dir_all(&goldens)?;
    for entry in std::fs::read_dir(p10_common::artifacts())? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("tui-") && entry.path().is_file() {
            let _ = std::fs::copy(entry.path(), goldens.join(entry.file_name()));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    server.stop().await?;
    eprintln!(
        "[p10-tui] PASS — captures in {} + goldens/p10/tui/",
        p10_common::artifacts().display()
    );
    Ok(())
}
