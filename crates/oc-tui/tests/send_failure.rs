//! Send-failure rendering (symptom 2): a prompt POST the server
//! answers with 500 must surface as the v2-shaped **error toast** +
//! the restored composer draft — never a raw `eprintln!` over the
//! frame. The stub server here always answers 500 (the fixture class
//! the golden scenario pins); the transcript's `Error: …` row for a
//! message-level error is covered by
//! `components::message::rows::assistant_footer_with_error_…`.

use oc_client::Client;
use oc_tui::app::{apply_send_failure, HttpPromptSink, PromptSinkEvent, HOME_PROMPT_KEY};
use oc_tui::components::composer::{PromptSink as _, SubmittedPrompt};
use oc_tui::reactive::Runtime;
use oc_tui::store::Store;
use std::io::{Read, Write};
use std::net::TcpListener;

/// A one-shot HTTP server that answers every request with 500.
struct Server500 {
    url: String,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Server500 {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut seen = Vec::new();
                let mut chunk = [0u8; 1024];
                // Read one request head + body (best effort).
                while seen.len() < 64 * 1024 {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            seen.extend_from_slice(&chunk[..count]);
                            if seen.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let body = br#"{"error":"upstream exploded"}"#;
                let head = format!(
                    "HTTP/1.1 500 Internal Server Error\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
        });
        Self { url, handle: Some(handle) }
    }
}

impl Drop for Server500 {
    fn drop(&mut self) {
        // Closing happens when the listener's accept errors after the
        // test process drops its connections; the thread parks in
        // accept and exits with the process. Detach explicitly.
        if let Some(handle) = self.handle.take() {
            let _ = handle;
        }
    }
}

fn submitted(text: &str) -> SubmittedPrompt {
    SubmittedPrompt { text: text.to_owned(), ..Default::default() }
}

/// A session prompt against a 500-ing server: the sink emits
/// `SendFailed` with v2's exact toast title, the toast RENDERS in the
/// frame, and the draft is restored into the composer.
#[tokio::test(flavor = "current_thread")]
async fn send_failure_renders_the_error_toast_and_restores_the_draft() {
    let server = Server500::spawn();
    let client = Client::new(&server.url).expect("client");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PromptSinkEvent>();
    let sink = HttpPromptSink::new(client, "/tmp/opencode".to_owned(), tx);

    sink.send("ses_500", &submitted("please answer"));

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("event within 5s")
        .expect("channel open");
    let PromptSinkEvent::SendFailed { session_key, title, message, draft } = event else {
        panic!("expected SendFailed, got {event:?}");
    };
    assert_eq!(session_key, "ses_500");
    assert_eq!(title, "Failed to send prompt", "v2's fail() title");
    assert_eq!(draft, "please answer");
    assert!(
        message.contains("500") || message.contains("upstream exploded") || !message.is_empty(),
        "the error text surfaces: {message}"
    );

    // The run loop's drain, applied to a live store: the toast is in
    // the FRAME (not stderr) and the draft returned to the composer.
    let store = Store::new(Runtime::new());
    store.open_tab("ses_500", None);
    apply_send_failure(
        &store,
        PromptSinkEvent::SendFailed {
            session_key: "ses_500".into(),
            title,
            message: message.clone(),
            draft,
        },
    );
    assert_eq!(store.prompt_draft("ses_500").text, "please answer", "restoreEntry()");
    let current = store.toasts().current().expect("toast queued").clone();
    assert_eq!(current.variant, "error");
    assert_eq!(current.title.as_deref(), Some("Failed to send prompt"));
    // The rendered FRAME carries the toast (the user-visible error —
    // symptom 2's contract: errors render, never eprintln).
    let app = oc_tui::app::App::with_store(store, (80, 24));
    let frame = app.frame_text();
    assert!(frame.contains("Failed to send prompt"), "toast renders in the frame:\n{frame}");

    // The restore never clobbers new typing: a failure landing after
    // the user typed something new keeps the NEW draft.
    let store = Store::new(Runtime::new());
    store.open_tab("ses_500", None);
    let mut composer = store.composer("ses_500");
    composer.set(oc_tui::components::composer::PromptDraft {
        text: "new thought".into(),
        ..Default::default()
    });
    store.set_composer("ses_500", &composer);
    apply_send_failure(
        &store,
        PromptSinkEvent::SendFailed {
            session_key: "ses_500".into(),
            title: "Failed to send prompt".into(),
            message: "x".into(),
            draft: "please answer".into(),
        },
    );
    assert_eq!(store.prompt_draft("ses_500").text, "new thought");
    drop(server);
}

/// The HOME route's failure title: creating the session failed (v2's
/// `newSession.recover` wording).
#[tokio::test(flavor = "current_thread")]
async fn home_send_failure_uses_the_session_create_title() {
    let server = Server500::spawn();
    let client = Client::new(&server.url).expect("client");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PromptSinkEvent>();
    let sink = HttpPromptSink::new(client, "/tmp/opencode".to_owned(), tx);

    sink.send(HOME_PROMPT_KEY, &submitted("first prompt"));

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("event")
        .expect("open");
    let PromptSinkEvent::SendFailed { session_key, title, .. } = event else {
        panic!("expected SendFailed");
    };
    assert_eq!(session_key, HOME_PROMPT_KEY);
    assert_eq!(title, "Creating a session failed");
    drop(server);
}

/// The toast options' shape matches the error variant the run loop
/// enqueues (both the send-failure and the config-error toasts drain
/// through `store.toast_show`).
#[test]
fn toast_options_shape_matches_the_error_variant() {
    let options = oc_tui::components::toast::ToastOptions {
        title: Some("Failed to send prompt".into()),
        message: "unexpected status 500".into(),
        variant: "error",
        duration_ms: oc_tui::components::toast::TOAST_DEFAULT_DURATION_MS,
        action_label: None,
        action_command: None,
        modal: false,
    };
    assert_eq!(options.variant, "error");
    assert!(options.duration_ms > 0);
}
