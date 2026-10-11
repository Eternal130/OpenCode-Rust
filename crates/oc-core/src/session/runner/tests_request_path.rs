//! Request-path parity regressions (corpus2 findings #26/#27):
//! title-generation cadence — one title request per session (the rename
//! lands after turn 1, so later turns never re-fire), and the wire body
//! carries the config model `body` overlay end-to-end.

use crate::session::runner::tests_harness::harness_with;
use oc_ai::testing::TestLlm;
use oc_schema::session_inbox::{Delivery, Item, UserPayload};

/// #27: an UNTITLED session over two turns issues exactly 2 chat + 1
/// title request (ref cadence: title generation fires on the first
/// promotion only — llm.ts:174-175 — because the rename lands; a lost
/// rename re-fires it every turn).
#[tokio::test]
async fn runner_title_generates_once_across_turns() {
    let llm = TestLlm::new();
    llm.always(oc_ai::testing::text("Probe title line"));
    let harness = harness_with(llm.clone()).await;

    let dir = harness.dir.to_string_lossy().into_owned();
    let session_id = harness.session_id();
    use crate::session::store::{CreateInput, CreateOutcome};
    let row = match harness
        .store
        .create(
            &harness.bus,
            CreateInput {
                id: Some(session_id.clone()),
                location: Some(oc_schema::location::Ref {
                    directory: oc_schema::common::AbsolutePath::parse(dir),
                    workspace_id: None,
                }),
                title: None,
                agent: None,
                model: Some(oc_schema::model::Ref {
                    provider_id: oc_schema::id::ProviderId::parse("parityfake"),
                    id: oc_schema::id::ModelId::parse("fake-model"),
                    variant: None,
                }),
                permissions: None,
                metadata: None,
                parent_id: None,
                project: None,
                version: "unknown".into(),
            },
        )
        .await
        .expect("create session")
    {
        CreateOutcome::Created(row) | CreateOutcome::Existing(row) => row,
    };
    assert!(
        crate::session::runner::title::is_untitled(&row),
        "fresh session is untitled (title=None is the root fallback)"
    );

    for turn in 1..=2u32 {
        harness
            .inbox
            .admit(
                &format!("msg_title_turn_{turn}"),
                session_id.as_str(),
                Item::User {
                    payload: UserPayload {
                        text: format!("Turn {turn}"),
                        files: None,
                        agents: None,
                        skills: None,
                        metadata: None,
                    },
                    delivery: Delivery::Steer,
                },
            )
            .await
            .expect("admit prompt");
        harness.execution.resume(session_id.as_str()).await.expect("execution");

        // The forked title task settles with the drain; poll until the
        // rename lands (or the request count is stable well past the
        // drain, proving it never will).
        let mut stable = 0;
        let mut last = usize::MAX;
        for _ in 0..250 {
            let titled = harness
                .db
                .transaction({
                    let id = session_id.as_str().to_string();
                    move |tx| {
                        Ok(crate::database::schema::SessionRow::get(tx, id)?
                            .and_then(|row| row.title))
                    }
                })
                .await
                .expect("row read");
            if titled.is_some() {
                break;
            }
            let now = llm.requests().len();
            stable = if now == last { stable + 1 } else { 0 };
            if stable >= 20 {
                break;
            }
            last = now;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let row = harness
            .db
            .transaction({
                let id = session_id.as_str().to_string();
                move |tx| crate::database::schema::SessionRow::get(tx, id)
            })
            .await
            .expect("row read")
            .expect("session row");
        assert_eq!(
            row.title.as_deref(),
            Some("Probe title line"),
            "turn {turn}: the generated title renamed the session"
        );
    }

    // Cadence: 2 chat requests + exactly ONE title request.
    let requests = llm.requests();
    let title_requests = requests
        .iter()
        .filter(|request| request.generation.as_ref().is_none_or(|g| g.max_tokens.is_none()))
        .count();
    assert_eq!(requests.len(), 3, "2 chat + 1 title requests");
    assert_eq!(title_requests, 1, "title path fires once per session");
}
