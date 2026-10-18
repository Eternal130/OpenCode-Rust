//! Integration tests for the handler lane: real HTTP over an ephemeral
//! listener, dogfooded through oc-client (every response must decode —
//! the generated client is the wire oracle).
//!
//! Coverage: session create/get/remove + 404 envelope, list cursor
//! round-trip + `InvalidCursor`, prompt (resume:false) → inbox, permission
//! ask/reply + 404, form lifecycle (ownership 404 / invalid answer 400 /
//! settled 409), export/import round-trip + duplicate 409, shell/fs/vcs
//! surfaces, PTY REST + connect-token, catalog envelopes, server info/pair.

use std::sync::Arc;

use oc_client::error::ClientError;
use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

struct TestServer {
    client: Client,
    _workspace: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    boot_with(None).await
}

async fn boot_with(password: Option<String>) -> TestServer {
    let dir = tempfile::tempdir().expect("tempdir");
    let data = tempfile::tempdir().expect("data tempdir");
    std::env::set_current_dir(dir.path()).expect("chdir to workspace");
    // Keep engine state OUT of the workspace so vcs status stays clean.
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("server");
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, _workspace: dir, _data: data }
}

fn create_body(title: &str) -> SessionCreateBody {
    SessionCreateBody { title: Some(title.to_owned()), ..Default::default() }
}

async fn create_session(client: &Client, title: &str) -> SessionInfo {
    client.session().create(&create_body(title)).await.expect("create")
}

fn assert_api(error: &ClientError, status: u16, tag: &str) {
    match &error {
        ClientError::Api { status: actual, name, .. } => {
            assert_eq!(*actual, status, "{tag}: status {actual} != {status}");
            assert_eq!(name, tag, "tag mismatch: {name} != {tag}");
        }
        other => panic!("expected Api({status}, {tag}), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Session lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_session_create_get_remove_round_trip() {
    let server = boot().await;
    let client = &server.client;

    let created = create_session(client, "alpha").await;
    assert!(created.id.starts_with("ses_"), "id prefix: {}", created.id);
    assert_eq!(created.title.as_deref(), Some("alpha"));

    let fetched = client.session().get(&created.id).await.expect("get");
    assert_eq!(fetched.id, created.id);
    // The {data} envelope decode through the client IS the byte check: the
    // generated SessionInfo must match field-for-field.
    assert_eq!(fetched.location.directory, created.location.directory);

    client.session().remove(&created.id).await.expect("remove");
    let error = client.session().get(&created.id).await.expect_err("gone");
    match &error {
        ClientError::Api { status, name, fields, .. } => {
            assert_eq!((*status, name.as_str()), (404, "SessionNotFoundError"));
            assert_eq!(fields["sessionID"], serde_json::json!(created.id));
            assert!(fields["message"].as_str().unwrap().contains("Session not found"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn handlers_session_list_cursor_round_trip() {
    let server = boot().await;
    let client = &server.client;

    for index in 0..3 {
        create_session(client, &format!("s{index}")).await;
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }

    let page = client
        .session()
        .list(&SessionListQuery { limit: Some("2".to_owned()), ..Default::default() })
        .await
        .expect("list");
    assert_eq!(page.data.len(), 2);
    // v2 session.ts:79-96 — every non-empty page carries BOTH cursors
    // (first/last row anchors, no has-more gate).
    assert!(page.cursor.next.is_some(), "non-empty pages carry a next cursor");
    assert!(page.cursor.previous.is_some(), "page 1 carries a previous cursor");

    let next = client
        .session()
        .list(&SessionListQuery {
            cursor: page.cursor.next.clone(),
            limit: Some("2".to_owned()),
            ..Default::default()
        })
        .await
        .expect("list next");
    assert_eq!(next.data.len(), 1);
    assert!(next.cursor.previous.is_some(), "cursor'd pages carry a previous cursor");
    assert!(next.cursor.next.is_some(), "final non-empty page still carries next");
    let page_one_last = page.data.last().unwrap().id.clone();
    assert_ne!(next.data[0].id, page_one_last, "descending default order");

    // Following the last page's cursor yields the empty terminal page.
    let terminal = client
        .session()
        .list(&SessionListQuery {
            cursor: next.cursor.next.clone(),
            limit: Some("2".to_owned()),
            ..Default::default()
        })
        .await
        .expect("terminal page");
    assert!(terminal.data.is_empty(), "pagination terminates: {:?}", terminal.data);

    let error = client
        .session()
        .list(&SessionListQuery {
            cursor: Some("!!!not-base64!!!".to_owned()),
            ..Default::default()
        })
        .await
        .expect_err("bad cursor");
    assert_api(&error, 400, "InvalidCursorError");
}

#[tokio::test]
async fn handlers_prompt_resume_false_and_inbox() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "prompt").await.id;

    // resume:false skips awaiting the (unconfigured) LLM execution.
    let inbox_user = client
        .session()
        .prompt(
            &session_id,
            &SessionPromptBody {
                text: "hello world".to_owned(),
                resume: Some(false),
                id: Some("msg_custom1".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect("prompt");
    assert_eq!(inbox_user.id, "msg_custom1");
    assert_eq!(inbox_user.payload.text, "hello world");
    assert_eq!(inbox_user.session_id, session_id);

    let pending = client.session().inbox().list(&session_id).await.expect("inbox");
    assert!(pending
        .iter()
        .any(|item| matches!(item, SessionInboxInfo::User(user) if user.id == "msg_custom1")));

    let error = client
        .session()
        .prompt(
            "ses_missing",
            &SessionPromptBody { text: "x".to_owned(), resume: Some(false), ..Default::default() },
        )
        .await
        .expect_err("404");
    assert_api(&error, 404, "SessionNotFoundError");
}

/// R-07: an image prompt's `files` (`PromptInput` URI shape) round-trip
/// through the handler as resolved attachments (data + mime + source)
/// — the inbox stores what the runner's media lowering consumes.
#[tokio::test]
async fn handlers_prompt_image_files_materialize_into_the_inbox() {
    use base64::Engine as _;

    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "prompt-image").await.id;

    let png = include_bytes!("fixtures/r07-tiny-4x2.png");
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    let inbox_user = client
        .session()
        .prompt(
            &session_id,
            &SessionPromptBody {
                text: "describe this".to_owned(),
                resume: Some(false),
                files: Some(vec![PromptInputFileAttachment {
                    uri: format!("data:image/png;base64,{encoded}"),
                    name: Some("clipboard.png".to_owned()),
                    description: None,
                    mention: None,
                }]),
                ..Default::default()
            },
        )
        .await
        .expect("prompt with image");

    let files = inbox_user.payload.files.expect("files stored resolved");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].mime, "image/png");
    assert_eq!(files[0].data, encoded, "base64 payload round-trips verbatim");
    assert!(matches!(files[0].source, oc_client::types::PromptFileSource::Inline(_)));
    assert_eq!(files[0].name.as_deref(), Some("clipboard.png"));

    // A non-data/file scheme is rejected with the v2 message, not
    // silently dropped.
    let error = client
        .session()
        .prompt(
            &session_id,
            &SessionPromptBody {
                text: "bad".to_owned(),
                resume: Some(false),
                files: Some(vec![PromptInputFileAttachment {
                    uri: "https://example.test/a.png".to_owned(),
                    name: None,
                    description: None,
                    mention: None,
                }]),
                ..Default::default()
            },
        )
        .await
        .expect_err("unsupported scheme");
    assert!(error.to_string().contains("Unsupported attachment URI"));
}

#[tokio::test]
async fn handlers_messages_cursor_order_rules() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "msgs").await.id;

    // v2 message.ts:35-36 — cursor + order → 400 "Cursor cannot be
    // combined with order".
    let asc_cursor = handlers::common::encode_message_cursor(&handlers::common::MessageCursor {
        id: "msg_1".to_owned(),
        order: handlers::common::ListOrderWire::Asc,
        direction: handlers::common::CursorDirection::Next,
    })
    .unwrap();
    let error = client
        .message()
        .list(
            &session_id,
            &SessionMessageListQuery {
                cursor: Some(asc_cursor.clone()),
                order: Some(SessionMessageListQueryOrder::Asc),
                ..Default::default()
            },
        )
        .await
        .expect_err("cursor+order rejected");
    assert_api(&error, 400, "InvalidCursorError");

    // Cursor-only: the order comes from the token (message.ts:40) — an
    // asc cursor pages ascending even though the request says nothing.
    let page = client
        .message()
        .list(
            &session_id,
            &SessionMessageListQuery { cursor: Some(asc_cursor), ..Default::default() },
        )
        .await;
    assert!(page.is_ok(), "cursor-only pages use the token's order: {:?}", page.err());

    let error = client
        .message()
        .list("ses_missing", &SessionMessageListQuery::default())
        .await
        .expect_err("404");
    assert_api(&error, 404, "SessionNotFoundError");
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Session model/agent switch (the M2 D3 defect: both endpoints 500'd for
// every fresh session — the projector's previous-model lookup read the
// NULL `session_v2.model`/`agent` columns as non-nullable and the durable
// commit rolled back).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_session_switch_model_persists() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "switch-model").await.id;

    // A fresh session carries no model — the switch must still commit.
    client
        .session()
        .switch_model(
            &session_id,
            &SessionSwitchModelBody {
                model: ModelRef {
                    id: "fake-model-a".to_owned(),
                    provider_id: "parityfake".to_owned(),
                    variant: None,
                },
            },
        )
        .await
        .expect("switch model");

    // Persistence read-back: the session row's model column updated.
    let fetched = client.session().get(&session_id).await.expect("get");
    let model = fetched.model.expect("model on the session row");
    assert_eq!(model.id, "fake-model-a");
    assert_eq!(model.provider_id, "parityfake");

    // The durable event committed: the model-switched context message is
    // part of the transcript (message.list read-back).
    let messages = client
        .message()
        .list(&session_id, &SessionMessageListQuery::default())
        .await
        .expect("messages");
    assert!(
        messages.data.iter().any(|message| matches!(message, SessionMessageInfo::ModelSelected(_))),
        "a model-switched row projected: {:?}",
        messages.data
    );

    // Switching again over a NON-null column (the previous-model path)
    // commits too, and the row follows.
    client
        .session()
        .switch_model(
            &session_id,
            &SessionSwitchModelBody {
                model: ModelRef {
                    id: "fake-model".to_owned(),
                    provider_id: "parityfake".to_owned(),
                    variant: None,
                },
            },
        )
        .await
        .expect("second switch");
    let fetched = client.session().get(&session_id).await.expect("get again");
    assert_eq!(fetched.model.expect("model").id, "fake-model");
}

#[tokio::test]
async fn handlers_session_switch_agent_persists() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "switch-agent").await.id;

    let agents = client.agent().list(&AgentListQuery::default()).await.expect("agents");
    let agent = agents
        .data
        .iter()
        .find(|agent| !agent.hidden)
        .map(|agent| agent.id.clone())
        .expect("a visible built-in agent");

    client
        .session()
        .switch_agent(&session_id, &SessionSwitchAgentBody { agent: agent.clone() })
        .await
        .expect("switch agent");

    // Persistence read-back: the session row's agent column updated and
    // the agent-switched context message projected.
    let fetched = client.session().get(&session_id).await.expect("get");
    assert_eq!(fetched.agent.as_deref(), Some(agent.as_str()));
    let messages = client
        .message()
        .list(&session_id, &SessionMessageListQuery::default())
        .await
        .expect("messages");
    assert!(
        messages.data.iter().any(|message| matches!(message, SessionMessageInfo::AgentSelected(_))),
        "an agent-switched row projected: {:?}",
        messages.data
    );

    // An unknown agent id is a 404, not a 500.
    let error = client
        .session()
        .switch_agent(&session_id, &SessionSwitchAgentBody { agent: "nope".to_owned() })
        .await
        .expect_err("unknown agent");
    assert_eq!(
        match &error {
            ClientError::Api { status, .. } => *status,
            other => panic!("expected Api error, got {other:?}"),
        },
        404
    );
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_permission_ask_reply_round_trip() {
    let server = boot().await;
    let client = &server.client;
    let session_id = client
        .session()
        .create(&SessionCreateBody {
            title: Some("perm".to_owned()),
            permissions: Some(vec![PermissionRule {
                action: "test.action".to_owned(),
                resource: "resource-1".to_owned(),
                effect: PermissionEffect::Ask,
            }]),
            ..Default::default()
        })
        .await
        .expect("create")
        .id;

    let ask = client
        .session()
        .permission_create(
            &session_id,
            &SessionPermissionCreateBody {
                action: "test.action".to_owned(),
                resources: vec!["resource-1".to_owned()],
                ..Default::default()
            },
        )
        .await
        .expect("ask");
    assert!(ask.id.starts_with("per_"));
    assert_eq!(ask.effect, PermissionEffect::Ask);

    let pending = client.session().permission_list(&session_id).await.expect("pending");
    assert!(pending.iter().any(|request| request.id == ask.id));

    let got = client.session().permission_get(&session_id, &ask.id).await.expect("get");
    assert_eq!(got.id, ask.id);

    client
        .session()
        .permission_reply(
            &session_id,
            &ask.id,
            &SessionPermissionReplyBody { decision: PermissionReply::Once, message: None },
        )
        .await
        .expect("reply");

    // Settled request leaves the pending map → re-reply 404.
    let error = client
        .session()
        .permission_reply(
            &session_id,
            &ask.id,
            &SessionPermissionReplyBody { decision: PermissionReply::Once, message: None },
        )
        .await
        .expect_err("settled");
    assert_api(&error, 404, "PermissionNotFoundError");

    let error =
        client.session().permission_get(&session_id, "per_missing").await.expect_err("missing");
    match &error {
        ClientError::Api { fields, .. } => {
            assert_eq!(fields["requestID"], serde_json::json!("per_missing"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert_api(&error, 404, "PermissionNotFoundError");
}

// ---------------------------------------------------------------------------
// Forms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_form_lifecycle_and_errors() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "form").await.id;

    let form = client
        .session()
        .form()
        .create(
            &session_id,
            &FormCreatePayload {
                title: "Pick one".to_owned(),
                fields: vec![FormField::StringField(FormStringField {
                    key: "name".to_owned(),
                    title: Some("Name".to_owned()),
                    required: Some(true),
                    ..Default::default()
                })],
                ..Default::default()
            },
        )
        .await
        .expect("form create");
    assert!(form.id.starts_with("frm_"));

    let detail = client.session().form().get(&session_id, &form.id).await.expect("form get");
    assert!(matches!(detail.state, FormState::Pending(_)));

    // Wrong session ownership → 404 with the id field (the session itself
    // must exist — ownership is a form-level check).
    let other = create_session(client, "form-other").await;
    let error = client.session().form().get(&other.id, &form.id).await.expect_err("ownership");
    match &error {
        ClientError::Api { fields, .. } => {
            assert_eq!(fields["id"], serde_json::json!(form.id));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert_api(&error, 404, "FormNotFoundError");

    // Missing required key → 400 FormInvalidAnswerError.
    let error = client
        .session()
        .form()
        .reply(&session_id, &form.id, &FormReply::default())
        .await
        .expect_err("invalid answer");
    assert_api(&error, 400, "FormInvalidAnswerError");

    // Valid reply → 204; re-reply → 409 FormAlreadySettledError.
    let mut answer = FormAnswer::default();
    answer.insert("name".to_owned(), FormValue::String("x".to_owned()));
    client
        .session()
        .form()
        .reply(&session_id, &form.id, &FormReply { answer })
        .await
        .expect("reply");
    let mut other_answer = FormAnswer::default();
    other_answer.insert("name".to_owned(), FormValue::String("y".to_owned()));
    let error = client
        .session()
        .form()
        .reply(&session_id, &form.id, &FormReply { answer: other_answer })
        .await
        .expect_err("settled");
    assert_api(&error, 409, "FormAlreadySettledError");

    // Global list decodes with the {location, data} envelope (settled
    // forms leave the pending listing).
    let global = client.form().list(&FormListQuery::default()).await.expect("global list");
    assert_eq!(global.data, [] as [oc_client::types::FormInfo; 0]);
}

#[tokio::test]
async fn handlers_session_update_view_and_active() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "before").await.id;

    client
        .session()
        .update(
            &session_id,
            &SessionUpdateBody {
                title: Some("after".to_owned()),
                metadata: Some({
                    let mut map = SessionMetadata::default();
                    map.insert("k".to_owned(), serde_json::json!("v"));
                    map
                }),
                ..Default::default()
            },
        )
        .await
        .expect("update");
    let fetched = client.session().get(&session_id).await.expect("get after update");
    assert_eq!(fetched.title.as_deref(), Some("after"));
    assert_eq!(
        fetched.metadata.as_ref().and_then(|map| map.get("k")),
        Some(&serde_json::json!("v"))
    );

    client.session().view(&session_id, &SessionViewBody { idle: 123.0 }).await.expect("view");
    let viewed = client.session().get(&session_id).await.expect("get after view");
    assert_eq!(viewed.time.viewed, Some(123.0));

    // active: nothing running → empty map decode.
    let active = client.session().active().await.expect("active");
    assert!(active.is_empty());

    // inbox steer/queue update + cancel round out the family.
    let inbox_user = client
        .session()
        .prompt(
            &session_id,
            &SessionPromptBody {
                text: "queued".to_owned(),
                resume: Some(false),
                delivery: Some(SessionInboxDelivery::Queue),
                ..Default::default()
            },
        )
        .await
        .expect("prompt queue");
    client
        .session()
        .inbox()
        .update(
            &session_id,
            &inbox_user.id,
            &SessionInboxUpdateBody { delivery: SessionInboxDelivery::Steer },
        )
        .await
        .expect("inbox steer");
    client.session().inbox().cancel(&session_id, &inbox_user.id).await.expect("inbox cancel");
}

// ---------------------------------------------------------------------------
// Import / export
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_export_import_round_trip_and_duplicate() {
    let server = boot().await;
    let client = &server.client;
    let created = create_session(client, "transfer").await;

    let exported = client
        .session()
        .export(&created.id, &ExperimentalSessionExportQuery::default())
        .await
        .expect("export");
    assert_eq!(exported.info.id, created.id);
    assert_eq!(exported.info.title.as_deref(), Some("transfer"));

    let info = SessionInfo {
        id: "ses_imported1".to_owned(),
        title: Some("imported".to_owned()),
        project_id: exported.info.project_id.clone(),
        location: exported.info.location.clone(),
        ..Default::default()
    };
    let payload = ExperimentalSessionImportBody {
        info,
        messages: Vec::new(),
        location: Some(exported.info.location.clone()),
    };
    let imported = client.session().import(&payload).await.expect("import");
    assert_eq!(imported.id, "ses_imported1");

    // Duplicate → 409 with the exact message (§3).
    let error = client.session().import(&payload).await.expect_err("duplicate");
    match &error {
        ClientError::Api { status, name, message, .. } => {
            assert_eq!((*status, name.as_str()), (409, "ConflictError"));
            assert_eq!(message, "Session already exists: ses_imported1");
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn handlers_revert_commit_without_stage_conflicts() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "revert").await.id;

    let error = client.session().revert().commit(&session_id).await.expect_err("no stage");
    assert_api(&error, 409, "ConflictError");
}

// ---------------------------------------------------------------------------
// Shell / fs / vcs / worktree
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_shell_round_trip() {
    let server = boot().await;
    let client = &server.client;

    let shell = client
        .shell()
        .create(
            &ShellCreateQuery::default(),
            &ShellCreateBody { command: "echo handler-lane".to_owned(), ..Default::default() },
        )
        .await
        .expect("shell create")
        .data;
    assert!(shell.id.starts_with("sh_"), "shell id: {}", shell.id);

    // Wait for completion, then read the capture.
    for _ in 0..50 {
        let current = client.shell().get(&shell.id, &ShellGetQuery::default()).await.expect("get");
        if current.data.status != ShellInfoStatus::Running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    let output =
        client.shell().output(&shell.id, &ShellOutputQuery::default()).await.expect("output").data;
    assert!(output.output.contains("handler-lane"));

    let error =
        client.shell().get("shl_missing", &ShellGetQuery::default()).await.expect_err("404");
    assert_api(&error, 404, "ShellNotFoundError");

    client.shell().remove(&shell.id, &ShellRemoveQuery::default()).await.expect("remove");
}

#[tokio::test]
async fn handlers_fs_read_write_list() {
    let server = boot().await;
    let client = &server.client;

    let written = client
        .fs()
        .write(
            &ExperimentalFsWriteQuery { path: "notes/x.txt".to_owned(), ..Default::default() },
            b"handler lane payload".as_slice(),
        )
        .await
        .expect("fs write");
    assert!(written.data.path.ends_with("notes/x.txt"));

    let bytes = client.fs().read("notes/x.txt", &FsReadQuery::default()).await.expect("fs read");
    assert_eq!(bytes, b"handler lane payload".to_vec());

    let listed = client
        .fs()
        .list(&FsListQuery { path: Some("notes".to_owned()), ..Default::default() })
        .await
        .expect("fs list")
        .data;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].type_, FileSystemEntryType::File);

    let error = client.fs().read("missing.txt", &FsReadQuery::default()).await.expect_err("404");
    assert_api(&error, 404, "FileNotFoundError");
}

#[tokio::test]
async fn handlers_vcs_init_and_status() {
    let server = boot().await;
    let client = &server.client;

    client.vcs().init(&VcsInitQuery::default()).await.expect("vcs init");
    let status = client.vcs().status(&VcsStatusQuery::default()).await.expect("vcs status");
    assert!(status.data.is_empty(), "clean tree, got {:?}", status.data);

    let error = client.vcs().init(&VcsInitQuery::default()).await.expect_err("re-init");
    assert_api(&error, 409, "ConflictError");
}

#[tokio::test]
async fn handlers_worktree_list_requires_project() {
    let server = boot().await;
    let client = &server.client;

    let error = client
        .worktree()
        .list(&WorktreeListQuery { project_id: "prj_missing".to_owned() })
        .await
        .expect_err("missing project");
    assert_api(&error, 404, "ProjectNotFoundError");
}

// ---------------------------------------------------------------------------
// Catalog surfaces + envelopes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_catalog_surfaces_decode() {
    let server = boot().await;
    let client = &server.client;

    let agents = client.agent().list(&AgentListQuery::default()).await.expect("agent list");
    assert!(!agents.data.is_empty(), "built-in agents are seeded");

    let models = client.model().list(&ModelListQuery::default()).await.expect("model list");
    assert!(!models.data.is_empty(), "catalog models decode with location+data envelope");
    // D-05: the wire order is `time.released` DESCENDING (core
    // model.ts:218-220 — `all` is sorted, `available()` only filters).
    let mut prev = f64::INFINITY;
    for model in &models.data {
        assert!(
            model.time.released <= prev,
            "model list is released-descending ({} after {prev})",
            model.time.released
        );
        prev = model.time.released;
    }

    let providers =
        client.provider().list(&ProviderListQuery::default()).await.expect("provider list");
    assert_ne!(providers.data, [] as [oc_client::types::ProviderInfo; 0]);

    let error = client.provider().get("nope", &ProviderGetQuery::default()).await.expect_err("404");
    match &error {
        ClientError::Api { fields, .. } => {
            assert_eq!(fields["providerID"], serde_json::json!("nope"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert_api(&error, 404, "ProviderNotFoundError");

    let skills = client.skill().list(&SkillListQuery::default()).await.expect("skill list");
    let _ = skills;
    let commands = client.command().list(&CommandListQuery::default()).await.expect("command list");
    assert!(commands.data.is_empty(), "plugin-registered commands: documented empty");

    let _config = client.config().get(&ConfigGetQuery::default()).await.expect("config get");
    let shells = client.config().shells().await.expect("shells");
    assert!(!shells.is_empty(), "at least one shell resolves on linux");

    let location = client.location().get(&LocationGetQuery::default()).await.expect("location get");
    assert_ne!(location.directory, "");
    assert_ne!(location.project.id, "");

    let debug = client.debug().location().list().await.expect("debug list");
    assert_ne!(debug, [] as [oc_client::types::LocationPublicRef; 0]);
}

#[tokio::test]
async fn handlers_mcp_list_and_connect_404() {
    let server = boot().await;
    let client = &server.client;

    let list = client.mcp().list(&McpListQuery::default()).await.expect("mcp list");
    assert_eq!(list.data, [] as [oc_client::types::McpServer; 0]);

    let error = client
        .mcp()
        .connect("ghost", &ExperimentalMcpConnectQuery::default())
        .await
        .expect_err("404");
    assert_api(&error, 404, "McpServerNotFoundError");
}

// ---------------------------------------------------------------------------
// Server info / pair
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_server_info_and_pair() {
    let server = boot().await;
    let client = &server.client;

    let info = client.server().info().await.expect("info");
    assert_eq!(info.version, "test-version");
    assert_ne!(info.urls, [] as [std::string::String; 0]);
    assert!(info.pid > 0);

    // Pairing mints session tokens, which only exist on password-protected
    // servers (passwordless auth is a pass-through).
    let authed = boot_with(Some("pair-password".to_owned())).await;
    let authed_client = Client::new(authed.client.base_url())
        .expect("client")
        .with_basic_auth("opencode", Some("pair-password".to_owned()));

    let pair = authed_client.server().pair().await.expect("pair");
    assert_eq!(pair.expires_in, 300);
    assert_ne!(pair.code, "");

    // Single-use: the second connect fails 401.
    let session = authed_client.server().connect(&pair.code).await.expect("connect");
    assert_ne!(session.token, "");
    let error = authed_client.server().connect(&pair.code).await.expect_err("replayed");
    assert_api(&error, 401, "UnauthorizedError");
}

// ---------------------------------------------------------------------------
// PTY REST + connect-token
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_pty_rest_and_connect_token() {
    let server = boot().await;
    let client = &server.client;

    let pty = client
        .pty()
        .create(
            &PtyCreateQuery::default(),
            &PtyCreateBody { command: Some("cat".to_owned()), ..Default::default() },
        )
        .await
        .expect("pty create")
        .data;
    assert!(pty.id.starts_with("pty_"));

    let _updated = client
        .pty()
        .update(
            &pty.id,
            &PtyUpdateQuery::default(),
            &PtyUpdateBody {
                size: Some(PtyUpdateBodySize { cols: 120, rows: 40 }),
                ..Default::default()
            },
        )
        .await
        .expect("pty update (resize)");

    // connect-token requires x-opencode-ticket: 1.
    let token = client
        .pty()
        .connect_token(
            &pty.id,
            &PtyConnectTokenQuery { x_opencode_ticket: Some("1".to_owned()), ..Default::default() },
        )
        .await
        .expect("connect token")
        .data;
    assert_eq!(token.expires_in, 60);
    assert_ne!(token.ticket, "");

    let error = client
        .pty()
        .connect_token(&pty.id, &PtyConnectTokenQuery::default())
        .await
        .expect_err("missing ticket header");
    assert_eq!(error.status(), Some(403));

    let error = client.pty().get("pty_missing", &PtyGetQuery::default()).await.expect_err("404");
    match &error {
        ClientError::Api { fields, .. } => {
            assert_eq!(fields["ptyID"], serde_json::json!("pty_missing"));
        }
        other => panic!("expected Api error, got {other:?}"),
    }
    assert_api(&error, 404, "PtyNotFoundError");

    client.pty().remove(&pty.id, &PtyRemoveQuery::default()).await.expect("pty remove");
}

// ---------------------------------------------------------------------------
// Misc documented shapes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handlers_misc_documented_shapes() {
    let server = boot().await;
    let client = &server.client;
    let session_id = create_session(client, "misc").await.id;

    // session.generate: declared 503 until the LLM surface lands.
    let error = client
        .session()
        .generate(&session_id, &SessionGenerateBody { prompt: "hi".to_owned() })
        .await
        .expect_err("503");
    assert_eq!(error.status(), Some(503));

    // session.command: plugin-registered commands are phase-9 → 404.
    let error = client
        .session()
        .command(
            &session_id,
            &SessionCommandBody { name: "commit".to_owned(), ..Default::default() },
        )
        .await
        .expect_err("404");
    assert_api(&error, 404, "CommandNotFoundError");

    // rpc without a host → v2's empty registry: `rpc.unavailable`
    // (RpcError 400 — handlers/rpc.ts maps the failure channel; the old
    // 500 stub was PLUG-D14's defect).
    let error = client
        .rpc()
        .call(
            "rpcID",
            "method",
            &RpcCallQuery::default(),
            &RpcInput { input: Some(serde_json::json!({})) },
        )
        .await
        .expect_err("400");
    assert_api(&error, 400, "RpcError");

    let _ = client.migration().status().await.expect("migration status");
}
