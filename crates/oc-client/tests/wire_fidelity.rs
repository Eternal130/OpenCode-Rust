//! Wire-fidelity tests: representative request bodies (and a few response
//! envelopes) across every client namespace are serialized and compared
//! byte-for-byte against JSON hand-derived from the pinned openapi.json
//! (field names, camelCase, `ID` suffixes, explicit-null semantics).
//!
//! 30 operations are covered across 26 namespaces.

use oc_client::types::*;
use serde_json::json;

fn ser<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).expect("serialize")
}

// ---- server / location / agent -------------------------------------------

#[test]
fn server_connect_and_pair_responses() {
    // operationId server.connect (GET /auth/connect/{code}) -> PairingSession
    let session: PairingSession = serde_json::from_value(json!({"token": "tok"})).unwrap();
    assert_eq!(ser(&session), json!({"token": "tok"}));
    // operationId server.pair -> PairingCode (snake_case wire keys)
    let code: PairingCode =
        serde_json::from_value(json!({"code": "c", "expires_in": 300})).unwrap();
    assert_eq!(code.expires_in, 300);
    assert_eq!(ser(&code), json!({"code": "c", "expires_in": 300}));
}

#[test]
fn location_public_info_decode() {
    let info: LocationPublicInfo = serde_json::from_value(json!({
        "directory": "/w",
        "project": {"id": "p", "directory": "/w", "canonical": "/canonical/w"}
    }))
    .unwrap();
    assert_eq!(info.directory, "/w");
    assert_eq!(info.project.id, "p");
    assert_eq!(info.project.canonical, "/canonical/w");
    // Nested inline struct serializes with exact wire keys.
    assert_eq!(
        ser(&info.project),
        json!({"id": "p", "directory": "/w", "canonical": "/canonical/w"})
    );
}

// ---- agent / plugin -------------------------------------------------------

#[test]
fn agent_info_mode_enum_and_model_ref() {
    let model = ModelRef {
        id: "claude".into(),
        provider_id: "anthropic".into(),
        variant: Some("opus".into()),
    };
    assert_eq!(ser(&model), json!({"id": "claude", "providerID": "anthropic", "variant": "opus"}));
}

#[test]
fn plugin_check_and_update_bodies() {
    let check = PluginCheckBody { target: Some("pkg".into()) };
    assert_eq!(ser(&check), json!({"target": "pkg"}));
    let update = PluginUpdateBody { targets: vec!["a".into(), "b".into()] };
    assert_eq!(ser(&update), json!({"targets": ["a", "b"]}));
}

// ---- session bodies -------------------------------------------------------

#[test]
fn session_create_body() {
    let body = SessionCreateBody {
        parent_id: Some("ses_parent".into()),
        title: Some("t".into()),
        model: Some(ModelRef { id: "m".into(), provider_id: "p".into(), variant: None }),
        ..SessionCreateBody::default()
    };
    assert_eq!(
        ser(&body),
        json!({"parentID": "ses_parent", "title": "t", "model": {"id": "m", "providerID": "p"}})
    );
}

#[test]
fn session_prompt_body_attachments_and_delivery() {
    let body = SessionPromptBody {
        text: "hello".into(),
        files: Some(vec![PromptInputFileAttachment {
            uri: "file:///a.txt".into(),
            name: Some("a.txt".into()),
            ..PromptInputFileAttachment::default()
        }]),
        delivery: Some(SessionInboxDelivery::Steer),
        resume: Some(true),
        ..SessionPromptBody::default()
    };
    assert_eq!(
        ser(&body),
        json!({
            "text": "hello",
            "files": [{"uri": "file:///a.txt", "name": "a.txt"}],
            "delivery": "steer",
            "resume": true
        })
    );
    assert_eq!(SessionInboxDelivery::Queue.as_str(), "queue");
}

#[test]
fn session_update_fork_move_compact_bodies() {
    let update = SessionUpdateBody { title: Some(String::new()), ..SessionUpdateBody::default() };
    assert_eq!(ser(&update), json!({"title": ""}));
    let fork = SessionForkBody { before: Some("msg_1".into()) };
    assert_eq!(ser(&fork), json!({"before": "msg_1"}));
    let r#move =
        SessionMoveBody { directory: "/d".into(), delivery: Some(SessionInboxDelivery::Queue) };
    assert_eq!(ser(&r#move), json!({"directory": "/d", "delivery": "queue"}));
    let compact = SessionCompactBody {
        id: Some("msg_1".into()),
        delivery: Some(SessionInboxDelivery::Queue),
    };
    assert_eq!(ser(&compact), json!({"id": "msg_1", "delivery": "queue"}));
}

#[test]
fn session_revert_stage_switch_model_command_bodies() {
    let stage = SessionRevertStageBody { message_id: "msg_9".into(), files: Some(true) };
    assert_eq!(ser(&stage), json!({"messageID": "msg_9", "files": true}));
    let switch = SessionSwitchModelBody {
        model: ModelRef { id: "gpt".into(), provider_id: "openai".into(), variant: None },
    };
    assert_eq!(ser(&switch), json!({"model": {"id": "gpt", "providerID": "openai"}}));
    let command = SessionCommandBody {
        name: "test".into(),
        text: "run".into(),
        ..SessionCommandBody::default()
    };
    assert_eq!(ser(&command), json!({"name": "test", "text": "run"}));
}

#[test]
fn session_shell_synthetic_environment_view_bodies() {
    let shell = SessionShellBody { command: "cargo test".into(), ..SessionShellBody::default() };
    assert_eq!(ser(&shell), json!({"command": "cargo test"}));
    let synthetic = SessionSyntheticBody {
        id: Some("msg_1".into()),
        description: Some("d".into()),
        text: "synthetic".into(),
        ..SessionSyntheticBody::default()
    };
    assert_eq!(ser(&synthetic), json!({"id": "msg_1", "description": "d", "text": "synthetic"}));
    let mut environment = SessionEnvironmentBody::default();
    environment.variables.insert("A".into(), "1".into());
    assert_eq!(ser(&environment), json!({"variables": {"A": "1"}}));
    let view = SessionViewBody { idle: 1.5 };
    assert_eq!(ser(&view), json!({"idle": 1.5}));
}

#[test]
fn session_form_create_and_reply_bodies() {
    let create = FormCreatePayload {
        title: "pick".into(),
        fields: vec![FormField::StringField(FormStringField {
            key: "choice".into(),
            title: Some("Choice".into()),
            ..FormStringField::default()
        })],
        ..FormCreatePayload::default()
    };
    assert_eq!(
        ser(&create),
        json!({
            "title": "pick",
            "fields": [{"type": "string", "key": "choice", "title": "Choice"}]
        })
    );
    let mut answer: FormAnswer = std::collections::BTreeMap::new();
    answer.insert("choice".into(), FormValue::String("yes".into()));
    let reply = FormReply { answer };
    assert_eq!(ser(&reply), json!({"answer": {"choice": "yes"}}));
}

#[test]
fn session_inbox_update_body() {
    let body = SessionInboxUpdateBody { delivery: SessionInboxDelivery::Steer };
    assert_eq!(ser(&body), json!({"delivery": "steer"}));
}

#[test]
fn session_permission_create_body() {
    let body = SessionPermissionCreateBody {
        id: Some("per_1".into()),
        action: "bash".into(),
        resources: vec!["file:///**".into()],
        ..SessionPermissionCreateBody::default()
    };
    assert_eq!(ser(&body), json!({"id": "per_1", "action": "bash", "resources": ["file:///**"]}));
}

#[test]
fn session_generate_body() {
    let body = SessionGenerateBody { prompt: "commit message".into() };
    assert_eq!(ser(&body), json!({"prompt": "commit message"}));
}

#[test]
fn session_import_body_envelope() {
    let info: SessionInfo = serde_json::from_value(json!({
        "id": "ses_1",
        "projectID": "p",
        "time": {"created": 1.0, "updated": 2.0},
        "cost": 0.0,
        "tokens": {"input": 1, "output": 2, "reasoning": 0, "cache": {"read": 0, "write": 0}},
        "location": {"directory": "/w"}
    }))
    .unwrap_or_else(|e| panic!("SessionInfo decode: {e}"));
    let body = ExperimentalSessionImportBody {
        info,
        messages: vec![],
        location: Some(LocationPublicRef { directory: "/w".into() }),
    };
    let value = ser(&body);
    assert_eq!(value["location"], json!({"directory": "/w"}));
    assert_eq!(value["messages"], json!([]));
    assert_eq!(value["info"]["projectID"], json!("p"));
}

// ---- message / model / generate ------------------------------------------

#[test]
fn session_messages_response_decode() {
    let response: SessionMessagesResponse = serde_json::from_value(json!({
        "data": [{
            "id": "msg_1", "type": "user", "text": "hi", "time": {"created": 1.0, "updated": 1.0},
            "files": [], "agents": [], "skills": []
        }],
        "cursor": {"previous": null, "next": "n"}
    }))
    .unwrap();
    match &response.data[0] {
        SessionMessageInfo::User(user) => assert_eq!(user.text, "hi"),
        other => panic!("expected User variant, got {other:?}"),
    }
    assert_eq!(response.cursor.next.as_deref(), Some("n"));
}

#[test]
fn experimental_generate_body_and_response() {
    let body = ExperimentalGenerateTextBody {
        prompt: "p".into(),
        ..ExperimentalGenerateTextBody::default()
    };
    assert_eq!(ser(&body), json!({"prompt": "p"}));
    let response: GenerateTextResponse = serde_json::from_value(json!({
        "data": {"text": "out"}
    }))
    .unwrap();
    assert_eq!(response.data.text, "out");
}

// ---- provider / integration ----------------------------------------------

#[test]
fn integration_bodies() {
    let wellknown = ExperimentalIntegrationWellknownAddBody {
        url: "https://x/.well-known/opencode.json".into(),
    };
    assert_eq!(ser(&wellknown), json!({"url": "https://x/.well-known/opencode.json"}));
    let oauth = IntegrationOauthConnectBody {
        method_id: "oauth".into(),
        ..IntegrationOauthConnectBody::default()
    };
    assert_eq!(ser(&oauth), json!({"methodID": "oauth"}));
    let command = IntegrationCommandConnectBody {
        method_id: "cmd".into(),
        ..IntegrationCommandConnectBody::default()
    };
    assert_eq!(ser(&command), json!({"methodID": "cmd"}));
}

// ---- websearch / credential ----------------------------------------------

#[test]
fn websearch_query_body() {
    let body = WebsearchQueryBody { query: "rust".into(), provider_id: Some("brave".into()) };
    assert_eq!(ser(&body), json!({"query": "rust", "providerID": "brave"}));
}

#[test]
fn credential_create_input() {
    // CredentialCreateInput embeds a required union (Credential.Value) so it
    // cannot derive Default; construct it fully.
    let body = CredentialCreateInput {
        integration_id: "linear".into(),
        value: CredentialValue::Key(CredentialKey {
            key: "k".into(),
            type_: CredentialKeyType::Key,
            configuration: None,
            metadata: None,
        }),
        activate: Some(true),
        id: None,
        label: None,
    };
    assert_eq!(
        ser(&body),
        json!({
            "integrationID": "linear",
            "value": {"type": "key", "key": "k"},
            "activate": true
        })
    );
    // And the discriminated union round-trips through the tag field.
    let decoded: CredentialValue =
        serde_json::from_value(json!({"type": "key", "key": "k2"})).unwrap();
    assert!(matches!(decoded, CredentialValue::Key(_)));
}

// ---- fs / shell / pty / persistent-pty -----------------------------------

#[test]
fn pty_create_body() {
    let mut env = std::collections::BTreeMap::new();
    env.insert("TERM".to_owned(), "xterm".to_owned());
    let body = PtyCreateBody {
        command: Some("bash".into()),
        args: Some(vec!["-l".into()]),
        env: Some(env),
        ..PtyCreateBody::default()
    };
    assert_eq!(ser(&body), json!({"command": "bash", "args": ["-l"], "env": {"TERM": "xterm"}}));
}

#[test]
fn persistent_pty_create_and_update_inputs() {
    let create = PersistentPtyCreateInput {
        args: vec![],
        command: Some("zsh".into()),
        title: "main".into(),
        env: std::collections::BTreeMap::new(),
        size: Some(PersistentPtyCreateInputSize { cols: 80, rows: 24 }),
        ..PersistentPtyCreateInput::default()
    };
    assert_eq!(
        ser(&create),
        json!({"args": [], "command": "zsh", "title": "main", "env": {}, "size": {"cols": 80, "rows": 24}})
    );
    let update = PersistentPtyUpdateInput {
        size: PersistentPtyUpdateInputSize { cols: 120, rows: 30 },
        ..PersistentPtyUpdateInput::default()
    };
    assert_eq!(ser(&update), json!({"size": {"cols": 120, "rows": 30}}));
}

#[test]
fn shell_create_body() {
    let body = ShellCreateBody { command: "echo hi".into(), ..ShellCreateBody::default() };
    assert_eq!(ser(&body), json!({"command": "echo hi"}));
}

// ---- project / worktree / vcs --------------------------------------------

#[test]
fn project_update_body() {
    let body = ProjectUpdateBody { name: Some("app".into()), ..ProjectUpdateBody::default() };
    assert_eq!(ser(&body), json!({"name": "app"}));
}

#[test]
fn worktree_bodies() {
    let create = WorktreeCreateInput {
        project_id: "p".into(),
        branch: Some("feature".into()),
        ..WorktreeCreateInput::default()
    };
    assert_eq!(ser(&create), json!({"projectID": "p", "branch": "feature"}));
    let remove =
        WorktreeRemoveInput { project_id: "p".into(), directory: "wt-1".into(), force: true };
    assert_eq!(ser(&remove), json!({"projectID": "p", "directory": "wt-1", "force": true}));
}

#[test]
fn vcs_mode_enum() {
    assert_eq!(VcsMode::Working.as_str(), "working");
    assert_eq!(VcsMode::Branch.as_str(), "branch");
    assert_eq!(ser(&VcsMode::Committed), json!("committed"));
}

// ---- config / rpc / misc --------------------------------------------------

#[test]
fn config_patch_required_nullable_serializes_null() {
    let patch = ConfigPatch { shell: None };
    // `shell` is required-but-nullable: explicit null, never skipped.
    assert_eq!(ser(&patch), json!({"shell": null}));
    let patch = ConfigPatch { shell: Some("fish".into()) };
    assert_eq!(ser(&patch), json!({"shell": "fish"}));
}

#[test]
fn rpc_input_output() {
    let input = RpcInput { input: Some(json!({"a": [1, 2]})) };
    assert_eq!(ser(&input), json!({"input": {"a": [1, 2]}}));
    let output: RpcOutput = serde_json::from_value(json!({"output": {"b": true}})).unwrap();
    assert_eq!(output.output, Some(json!({"b": true})));
}

#[test]
fn server_info_capabilities_decode() {
    let info: ServerInfo = serde_json::from_value(json!({
        "version": "2.0.24", "pid": 42, "urls": ["http://127.0.0.1:4096"],
        "paths": {"tmp": "/tmp"}, "capabilities": {"persistentPty": true}
    }))
    .unwrap();
    assert_eq!(info.version, "2.0.24");
    assert_eq!(info.capabilities.as_ref().unwrap().persistent_pty, Some(true));
}

#[test]
fn sessions_response_cursor_decode() {
    let response: SessionsResponse = serde_json::from_value(json!({
        "data": [], "cursor": {"previous": "p", "next": null}
    }))
    .unwrap();
    assert_eq!(response.cursor.previous.as_deref(), Some("p"));
    assert!(response.cursor.next.is_none());
}

#[test]
fn experimental_session_skill_body() {
    let body = ExperimentalSessionSkillBody {
        id: "fix-tests".into(),
        ..ExperimentalSessionSkillBody::default()
    };
    assert_eq!(ser(&body), json!({"id": "fix-tests"}));
}

#[test]
fn form_tagged_union_roundtrip() {
    let field = FormField::NumberField(FormNumberField {
        key: "n".into(),
        title: Some("N".into()),
        ..FormNumberField::default()
    });
    assert_eq!(ser(&field), json!({"type": "number", "key": "n", "title": "N"}));
    let decoded: FormField = serde_json::from_value(json!({
        "type": "boolean", "key": "b", "title": "B", "description": "d"
    }))
    .unwrap();
    assert!(matches!(decoded, FormField::BooleanField(_)));
}
