//! Phase-10 cross-replacement (b): the **generated Rust client**
//! (`oc-client`, exercised in-process) as a client of the **booted
//! REFERENCE v2.0.24 server**, asserting the port's client decodes every
//! response of a scripted session flow:
//!
//! 1. `session.create` (title + location + the config-seeded `FakeLlm`
//!    model) → `SessionInfo` decoded;
//! 2. `session.prompt` → `SessionInboxUser` decoded;
//! 3. the fake's `read` tool call on an external directory parks a
//!    permission ask → `session.permission_list` decodes the
//!    `PermissionRequest` (action `external_directory`);
//! 4. `session.permission_reply` (`once`) → the tool proceeds;
//! 5. `session.message.list` → **every** message (user, tool part,
//!    assistant reply) decodes into the wire-faithful types.
//!
//! The provider config is seeded into the reference sandbox project
//! (`spawn_reference.seed_files`) — no LLM keys, fully offline.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_cross_client)'
//! ```

mod p10_common;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{ensure, Context, Result};

use oc_client::types::{
    LocationPublicRef, ModelRef, PermissionReply, SessionCreateBody, SessionMessageInfo,
    SessionMessageListQuery, SessionPermissionReplyBody, SessionPromptBody,
};
use oc_client::Client;

use p10_common::{enabled, read_tool_script, skip, text_script, ScriptedFakeLlm};

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_rust_client_against_reference_server() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if !p10_common::reference_runnable() {
        skip("bun or the reference checkout is unavailable");
        return Ok(());
    }

    // -- scratch layout + the scripted fake ---------------------------------
    let root = PathBuf::from(format!("/tmp/opencode/p10-client-{}", std::process::id()));
    let project = root.join("project");
    let outside = root.join("outside");
    std::fs::create_dir_all(&project)?;
    std::fs::create_dir_all(&outside)?;
    let secret = outside.join("secret.txt");
    std::fs::write(&secret, "p10-client-secret\n")?;

    let fake = ScriptedFakeLlm::start(vec![
        // first MAIN request: a read tool call on the OUTSIDE file → ask
        read_tool_script("call_p10c_read_out", &secret.display().to_string()),
        // after the tool result: the final text reply
        text_script("The outside file says p10-client-secret."),
    ])
    .await?;

    // -- boot the reference with the provider config seeded -----------------
    let mut spawn = parity::reference::ReferenceSpawn::default();
    spawn.seed_files.push((
        PathBuf::from("project/opencode.json"),
        p10_common::provider_config(&fake.base(), false),
    ));
    let reference = parity::reference::spawn_reference(spawn).await?;
    eprintln!("[p10-client] reference ready at {}", reference.url());
    let project_dir = reference.sandbox().join("project").display().to_string();

    let result = run(&reference, &project_dir).await;
    let _ = std::fs::write(
        p10_common::artifacts().join("client-session.txt"),
        format!("base={}\nproject={project_dir}\n", reference.url()),
    );
    reference.stop().await?;
    result
}

#[allow(clippy::too_many_lines)]
async fn run(reference: &parity::reference::ReferenceServer, project_dir: &str) -> Result<()> {
    let client = Client::new(reference.url())
        .with_context(|| format!("client for {}", reference.url()))?
        .with_basic_auth("opencode", Some(reference.password().to_owned()));

    // 1. session.create — decodes SessionInfo.
    let session = client
        .session()
        .create(&SessionCreateBody {
            title: Some("p10 rust client".to_owned()),
            location: Some(LocationPublicRef { directory: project_dir.to_owned() }),
            model: Some(ModelRef {
                provider_id: "parityfake".to_owned(),
                id: "fake-model".to_owned(),
                variant: None,
            }),
            ..SessionCreateBody::default()
        })
        .await
        .context("session.create (decode SessionInfo)")?;
    let session_id = session.id.clone();
    ensure!(!session_id.is_empty(), "empty session id");
    ensure!(
        session.title.as_deref() == Some("p10 rust client"),
        "session title not decoded: {:?}",
        session.title
    );
    ensure!(
        session.model.as_ref().is_some_and(|m| m.id == "fake-model"),
        "session model not decoded: {:?}",
        session.model
    );
    eprintln!("[p10-client] 1. session.create decoded (id {session_id})");

    // 2. session.prompt — decodes SessionInboxUser.
    let inbox = client
        .session()
        .prompt(
            &session_id,
            &SessionPromptBody {
                text: "Read the outside file.".to_owned(),
                ..SessionPromptBody::default()
            },
        )
        .await
        .context("session.prompt (decode SessionInboxUser)")?;
    ensure!(!inbox.id.is_empty(), "inbox user message id not decoded");
    eprintln!("[p10-client] 2. session.prompt decoded (inbox id {})", inbox.id);

    // 3. poll session.permission_list until the external read parks an ask.
    let mut ask = None;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let requests = client
            .session()
            .permission_list(&session_id)
            .await
            .context("session.permission_list (decode PermissionRequest[])")?;
        if let Some(first) = requests.first() {
            ask = Some(first.clone());
            break;
        }
    }
    let ask = ask.context("no permission ask appeared on the reference server")?;
    ensure!(
        ask.action == "external_directory",
        "expected an external_directory ask, got {:?}",
        ask.action
    );
    ensure!(!ask.resources.is_empty(), "ask without resources: {ask:?}");
    eprintln!("[p10-client] 3. permission ask decoded ({} / {})", ask.id, ask.action);

    // 4. session.permission_reply(once) — empty 204 must decode.
    client
        .session()
        .permission_reply(
            &session_id,
            &ask.id,
            &SessionPermissionReplyBody { decision: PermissionReply::Once, message: None },
        )
        .await
        .context("session.permission_reply (decode 204)")?;
    eprintln!("[p10-client] 4. permission once replied; tool proceeding");

    // 5. poll session.message.list until the final reply; EVERY message
    //    must decode (the untagged SessionMessageInfo union).
    let query = SessionMessageListQuery::default();
    let mut messages = Vec::new();
    let mut reply_found = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let response = client
            .message()
            .list(&session_id, &query)
            .await
            .context("session.message.list (decode SessionMessagesResponse)")?;
        messages = response.data;
        reply_found = messages.iter().any(|message| {
            assistant_text(message)
                .is_some_and(|text| text.contains("The outside file says p10-client-secret."))
        });
        if reply_found {
            break;
        }
    }
    ensure!(reply_found, "the final assistant reply never appeared/decoded");
    let has_tool_part = messages.iter().any(|message| matches!(
        message,
        SessionMessageInfo::Assistant(assistant) if assistant
            .content
            .iter()
            .any(|item| matches!(item, oc_client::types::SessionMessageAssistantContentItem::Tool(_)))
    ));
    ensure!(has_tool_part, "no decoded tool part in {} messages", messages.len());
    ensure!(messages.len() >= 2, "expected user + assistant at minimum");
    eprintln!(
        "[p10-client] 5. message.list decoded {} messages (user + tool + reply)",
        messages.len()
    );

    let evidence = serde_json::json!({
        "session_id": session_id,
        "messages": messages.len(),
        "ask": {"id": ask.id, "action": ask.action},
    });
    std::fs::write(
        p10_common::artifacts().join("client-messages.json"),
        format!("{}\n", serde_json::to_string_pretty(&evidence)?),
    )?;
    eprintln!("[p10-client] PASS — the oc-client decoded every response of the flow");
    Ok(())
}

/// The concatenated text parts of an assistant message (None for others).
fn assistant_text(message: &SessionMessageInfo) -> Option<String> {
    let SessionMessageInfo::Assistant(assistant) = message else {
        return None;
    };
    let mut text = String::new();
    for item in &assistant.content {
        if let oc_client::types::SessionMessageAssistantContentItem::Text(part) = item {
            text.push_str(&part.text);
        }
    }
    Some(text)
}
