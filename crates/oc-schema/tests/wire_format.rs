//! Wire-format snapshot tests against v2.0.24 shapes.
//!
//! Fixtures are taken from `packages/protocol/openapi.json` component schemas
//! and `packages/app/e2e/utils/timeline.ts` (the canonical event/message
//! framing used by the real server).

use oc_schema::config;
use oc_schema::event::{DurableEnvelope, DurableInfo, EphemeralEnvelope};
use oc_schema::events::{ServerEvent, SessionStatusEvent, StatusInfo};
use oc_schema::id::*;
use oc_schema::session_event::{self, Event as SessionEvent};
use oc_schema::session_message::{
    AssistantContent, CompactionReason, Info as MessageInfo, ToolState,
};
use oc_schema::{location, model, session_message, shell, tool};

#[allow(clippy::needless_pass_by_value)]
fn assert_round_trip<T>(value: T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_value(&value).expect("serialize");
    let back: T = serde_json::from_value(json.clone()).expect("deserialize");
    assert_eq!(back, value, "round trip through {json}");
}

#[test]
fn user_message_matches_v2_fixture() {
    let message: MessageInfo = serde_json::from_value(serde_json::json!({
        "id": "msg_user",
        "type": "user",
        "text": "Run it",
        "time": { "created": 1 }
    }))
    .unwrap();
    let serialized = serde_json::to_value(&message).unwrap();
    assert_eq!(
        serialized,
        serde_json::json!({
            "id": "msg_user",
            "type": "user",
            "text": "Run it",
            "time": {"created": 1}
        })
    );
    assert_round_trip(message);
}

#[test]
fn completed_assistant_matches_v2_fixture() {
    let message: MessageInfo = serde_json::from_value(serde_json::json!({
        "id": "msg_assistant",
        "type": "assistant",
        "agent": "build",
        "model": { "id": "model", "providerID": "provider" },
        "content": [{ "type": "text", "text": "Working" }],
        "time": { "created": 2, "completed": 3 }
    }))
    .unwrap();
    let serialized = serde_json::to_value(&message).unwrap();
    assert_eq!(serialized["type"], "assistant");
    assert_eq!(serialized["model"]["providerID"], "provider");
    assert_eq!(serialized["content"][0]["type"], "text");
    assert_eq!(serialized["time"]["completed"], 3);
    assert_round_trip(message);
}

#[test]
fn synthetic_and_skill_notices() {
    let synthetic: MessageInfo = serde_json::from_value(serde_json::json!({
        "id": "msg_subagent",
        "type": "synthetic",
        "text": "done",
        "description": "Search code",
        "metadata": { "source": "subagent", "agent": "explore", "state": "completed" },
        "time": { "created": 4 }
    }))
    .unwrap();
    let serialized = serde_json::to_value(&synthetic).unwrap();
    assert_eq!(serialized["metadata"]["agent"], "explore");
    let skill: MessageInfo = serde_json::from_value(serde_json::json!({
        "id": "msg_skill",
        "type": "skill",
        "skill": "review",
        "name": "Review",
        "text": "instructions",
        "time": { "created": 6 }
    }))
    .unwrap();
    assert_eq!(serde_json::to_value(&skill).unwrap()["type"], "skill");
    assert_round_trip(synthetic);
    assert_round_trip(skill);
}

#[test]
fn tool_part_state_machine_all_states() {
    let streaming: ToolState = serde_json::from_value(serde_json::json!({
        "status": "streaming",
        "input": "{\"command\":"
    }))
    .unwrap();
    let running: ToolState = serde_json::from_value(serde_json::json!({
        "status": "running",
        "input": { "command": "ls" },
        "metadata": { "title": "Listing" }
    }))
    .unwrap();
    let completed: ToolState = serde_json::from_value(serde_json::json!({
        "status": "completed",
        "input": { "command": "ls" },
        "content": [{ "type": "text", "text": "a\nb" }],
        "metadata": { "exit": 0 }
    }))
    .unwrap();
    let error: ToolState = serde_json::from_value(serde_json::json!({
        "status": "error",
        "input": { "command": "ls" },
        "error": { "type": "ProcessError", "message": "not found" }
    }))
    .unwrap();
    let states = [&streaming, &running, &completed, &error];
    let expected = ["streaming", "running", "completed", "error"];
    for (state, tag) in states.iter().zip(expected) {
        let json = serde_json::to_value(state).unwrap();
        assert_eq!(json["status"], tag);
    }
    for state in states {
        let json = serde_json::to_value(state).unwrap();
        let back: ToolState = serde_json::from_value(json).unwrap();
        assert_eq!(&back, state);
    }
}

#[test]
fn assistant_with_tool_part_and_costs() {
    let message = MessageInfo::Assistant {
        id: MessageId::parse("msg_1").unwrap(),
        metadata: None,
        time: session_message::AssistantTime { created: 1, streamed: Some(2), completed: Some(3) },
        agent: AgentId::parse("build"),
        model: model::Ref::parse("anthropic/claude-sonnet-4-6").unwrap(),
        content: vec![
            AssistantContent::Reasoning {
                text: "thinking".into(),
                state: None,
                time: Some(session_message::ReasoningTime { created: 1, completed: Some(2) }),
            },
            AssistantContent::Tool {
                id: "toolu_01".into(),
                name: "bash".into(),
                executed: Some(true),
                provider_state: None,
                provider_result_state: None,
                state: ToolState::Completed {
                    input: [("command".to_string(), serde_json::json!("ls -la"))]
                        .into_iter()
                        .collect(),
                    content: vec![tool::Content::Text { text: "total 0".into() }],
                    metadata: None,
                },
                time: session_message::ToolTime { created: 2, ran: Some(3), completed: Some(4) },
            },
        ],
        snapshot: None,
        finish: Some(oc_schema::FinishReason::Stop),
        raw_finish: Some("end_turn".into()),
        provider_state: None,
        cost: Some(oc_schema::Usd(oc_schema::Num(0.0123))),
        tokens: Some(oc_schema::TokenUsage {
            input: oc_schema::Num(1200.0),
            output: oc_schema::Num(80.0),
            reasoning: oc_schema::Num(0.0),
            cache: oc_schema::TokenUsageCache {
                read: oc_schema::Num(1000.0),
                write: oc_schema::Num(0.0),
            },
        }),
        error: None,
        retry: None,
    };
    let json = serde_json::to_value(&message).unwrap();
    // Whole-dollar/fractional cost keeps decimals; whole token counts emit as
    // integers, exactly like JS.
    assert_eq!(json["cost"], serde_json::json!(0.0123));
    assert_eq!(json["tokens"]["input"], serde_json::json!(1200));
    assert_eq!(json["finish"], "stop");
    assert_eq!(json["content"][0]["type"], "reasoning");
    assert_eq!(json["content"][1]["state"]["content"][0]["text"], "total 0");
    assert_round_trip(message);
}

#[test]
fn compaction_marker_full_union() {
    for status in ["running", "completed", "failed"] {
        let payload = match status {
            "running" => serde_json::json!({
                "id": "msg_c1", "type": "compaction", "status": "running",
                "reason": "auto", "summary": "s", "recent": "r", "time": {"created": 1}
            }),
            "completed" => serde_json::json!({
                "id": "msg_c2", "type": "compaction", "status": "completed",
                "reason": "manual", "summary": "s", "recent": "r",
                "model": { "id": "m", "providerID": "p" },
                "time": {"created": 1}
            }),
            _ => serde_json::json!({
                "id": "msg_c3", "type": "compaction", "status": "failed",
                "reason": "auto", "error": {"type": "APIError", "message": "boom"},
                "time": {"created": 1}
            }),
        };
        let message: MessageInfo = serde_json::from_value(payload.clone()).unwrap();
        let serialized = serde_json::to_value(&message).unwrap();
        assert_eq!(serialized["type"], "compaction");
        assert_eq!(serialized["status"], status);
        let back: MessageInfo = serde_json::from_value(serialized).unwrap();
        assert_eq!(back, message);
    }
    assert_eq!(serde_json::to_value(CompactionReason::Auto).unwrap(), "auto");
}

#[test]
fn durable_event_envelope_shape() {
    let event = SessionEvent::Created(DurableEnvelope {
        id: EventId::parse("evt_timeline_0001").unwrap(),
        created: 1_700_000_002_001,
        metadata: None,
        durable: DurableInfo { aggregate_id: "ses_timeline".into(), seq: 1, version: 1 },
        location: Some(location::Ref {
            directory: oc_schema::AbsolutePath::parse("C:/OpenCode/TimelineStability"),
            workspace_id: None,
        }),
        data: session_event::Created {
            session_id: SessionId::parse("ses_timeline").unwrap(),
            project_id: ProjectId::parse("proj_timeline_stability"),
            location: location::Ref {
                directory: oc_schema::AbsolutePath::parse("C:/OpenCode/TimelineStability"),
                workspace_id: None,
            },
            subpath: None,
            parent_id: None,
            slug: "timeline-stability".into(),
            title: Some("Timeline visual stability".into()),
            agent: None,
            model: None,
            metadata: None,
            permissions: None,
            version: "2.0.24".into(),
        },
    });
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "id": "evt_timeline_0001",
            "created": 1_700_000_002_001u64,
            "type": "session.created",
            "durable": { "aggregateID": "ses_timeline", "seq": 1, "version": 1 },
            "location": { "directory": "C:/OpenCode/TimelineStability" },
            "data": {
                "sessionID": "ses_timeline",
                "projectID": "proj_timeline_stability",
                "location": { "directory": "C:/OpenCode/TimelineStability" },
                "slug": "timeline-stability",
                "title": "Timeline visual stability",
                "version": "2.0.24"
            }
        })
    );
    assert_round_trip(event);
}

#[test]
fn ephemeral_event_omits_durable_key() {
    let event = SessionEvent::ToolProgress(EphemeralEnvelope {
        id: EventId::parse("evt_5").unwrap(),
        created: 7,
        metadata: None,
        location: None,
        data: session_event::ToolProgress {
            session_id: SessionId::parse("ses_1").unwrap(),
            assistant_message_id: MessageId::parse("msg_1").unwrap(),
            id: "call_1".into(),
            metadata: [("progress".to_string(), serde_json::json!(50))].into_iter().collect(),
        },
    });
    let json = serde_json::to_value(&event).unwrap();
    let object = json.as_object().unwrap();
    assert!(!object.contains_key("durable"));
    assert_eq!(object["type"], "session.tool.progress");
    assert_eq!(object["data"]["metadata"]["progress"], 50);
    assert_round_trip(event);
}

#[test]
fn server_and_status_events() {
    let connected = ServerEvent::Connected(EphemeralEnvelope {
        id: EventId::parse("evt_1").unwrap(),
        created: 1,
        metadata: None,
        location: None,
        data: oc_schema::events::ServerConnected {},
    });
    assert_eq!(
        serde_json::to_value(&connected).unwrap(),
        serde_json::json!({
            "id": "evt_1", "created": 1, "type": "server.connected", "data": {}
        })
    );
    let busy = StatusInfo::Busy;
    assert_eq!(serde_json::to_value(&busy).unwrap(), serde_json::json!({"type": "busy"}));
    let retry = StatusInfo::Retry {
        attempt: 2,
        message: "rate limited".into(),
        action: None,
        next: 1_700_000_005_000,
    };
    assert_eq!(retry, serde_json::from_value(serde_json::to_value(&retry).unwrap()).unwrap());
    let status = SessionStatusEvent::Status(EphemeralEnvelope {
        id: EventId::parse("evt_2").unwrap(),
        created: 2,
        metadata: None,
        location: None,
        data: oc_schema::events::StatusUpdated {
            session_id: SessionId::parse("ses_1").unwrap(),
            status: StatusInfo::Idle,
        },
    });
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["type"], "session.status");
    assert_eq!(json["data"]["status"]["type"], "idle");
    assert_round_trip(status);
}

#[test]
fn config_document_end_to_end() {
    let document = serde_json::json!({
        "type": "document",
        "path": "/repo/opencode.json",
        "info": {
            "$schema": "https://opencode.ai/config.json",
            "theme_note": "theme lives in tui.json",
            "model": "anthropic/claude-sonnet-4-6",
            "default_agent": "build",
            "permissions": [
                { "action": "bash", "resource": "rm *", "effect": "ask" }
            ],
            "plugins": [
                "github:opencode-org/opencode-plugin-example",
                "-legacy-package",
                { "package": "@opencode/local", "options": { "verbose": true } }
            ],
            "mcp": {
                "servers": {
                    "everything": {
                        "type": "local",
                        "command": ["npx", "-y", "@modelcontextprotocol/server-everything"]
                    }
                }
            },
            "providers": {
                "openai-compatible": {
                    "name": "Local",
                    "options_note": "settings keys pass through",
                    "settings": { "timeout": 30000 }
                }
            }
        }
    });
    let entry: config::Entry = serde_json::from_value(document.clone()).unwrap();
    let config::Entry::Document(document) = &entry else {
        panic!("expected document");
    };
    let info = &document.info;
    assert_eq!(info.model.as_ref().unwrap().provider_id.as_str(), "anthropic");
    assert_eq!(info.default_agent.as_deref(), Some("build"));
    assert_eq!(info.permissions.as_ref().unwrap()[0].effect, oc_schema::PermissionEffect::Ask);
    let plugins = info.plugins.as_ref().unwrap();
    assert_eq!(plugins.len(), 3);
    assert_eq!(
        plugins[1].operation().unwrap(),
        oc_schema::config::plugin::Operation::Remove { target: "legacy-package".into() }
    );
    let mcp = info.mcp.as_ref().unwrap();
    assert!(mcp.servers.as_ref().unwrap().contains_key("everything"));
    let providers = info.providers.as_ref().unwrap();
    assert_eq!(
        providers.get("openai-compatible").unwrap().settings.as_ref().unwrap().timeout,
        Some(oc_schema::provider::Timeout::Millis(oc_schema::Num(30000.0)))
    );
    let serialized = serde_json::to_value(&entry).unwrap();
    assert_eq!(serialized["type"], "document");
    assert_eq!(
        serialized["info"]["model"],
        serde_json::json!({"providerID": "anthropic", "model": "claude-sonnet-4-6"})
    );
    assert_round_trip(entry);
}

#[test]
fn shell_message_and_info() {
    let message: MessageInfo = serde_json::from_value(serde_json::json!({
        "id": "msg_sh",
        "type": "shell",
        "shellID": "sh_1",
        "command": "cargo test",
        "status": "exited",
        "exit": 0,
        "output": { "output": "ok", "cursor": 2, "size": 2, "truncated": false },
        "time": { "created": 1, "completed": 2 }
    }))
    .unwrap();
    let json = serde_json::to_value(&message).unwrap();
    assert_eq!(json["shellID"], "sh_1");
    assert_eq!(json["exit"], 0);
    assert_round_trip(message);

    let info = shell::Info {
        id: ShellId::parse("sh_2").unwrap(),
        status: shell::Status::Running,
        command: "sleep 1".into(),
        cwd: "/repo".into(),
        shell: "bash".into(),
        file: "/tmp/opencode/sh-2.log".into(),
        pid: Some(99),
        exit: None,
        signal: None,
        metadata: [("ses".to_string(), serde_json::json!("ses_1"))].into_iter().collect(),
        time: shell::Time { started: 10, completed: None },
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["pid"], 99);
    assert_eq!(json["metadata"]["ses"], "ses_1");
    assert_round_trip(info);
}

#[test]
fn id_formats_match_v2() {
    // Prefix validation from v2 brands.
    assert!(SessionId::parse("ses_abc123").is_ok());
    assert!(SessionId::parse("sesabc").is_err());
    assert!(MessageId::parse("msg_1").is_ok());
    assert!(MessageId::parse("evt_1").is_err());
    assert!(EventId::parse("evt_1").is_ok());
    assert!(WorkspaceId::parse("wrk_1").is_ok());
    assert!(WorkspaceId::parse("wrk1").is_ok()); // v2 checks the bare "wrk" prefix
    assert!(ShellId::parse("sh_1").is_ok());
    assert!(FormId::parse("frm_1").is_ok());
    // Generated ids carry the right prefixes and shape.
    let session = SessionId::new();
    assert!(session.as_str().starts_with("ses_"));
    assert_eq!(session.as_str().len(), 4 + 26);
    let event = EventId::new();
    assert_eq!(MessageId::from_event(&event).as_str(), format!("msg_{}", &event.as_str()[4..]));
    // Model refs round-trip through string form.
    let reference = model::Ref::parse("openrouter/deepseek/deepseek-chat#free").unwrap();
    assert_eq!(reference.to_string(), "openrouter/deepseek/deepseek-chat#free");
}

#[test]
fn schemars_generation_smoke() {
    // JSON Schema output is secondary to wire format, but generation must not
    // panic for the main public types.
    let _ = schemars::schema_for!(MessageInfo);
    let _ = schemars::schema_for!(SessionEvent);
    let _ = schemars::schema_for!(config::Info);
    let _ = schemars::schema_for!(oc_schema::AgentInfo);
    let _ = schemars::schema_for!(model::Ref);
    let _ = schemars::schema_for!(shell::Info);
    let _ = schemars::schema_for!(oc_schema::events::ServerEvent);
    let _ = schemars::schema_for!(oc_schema::mcp::ServerConfig);
}
