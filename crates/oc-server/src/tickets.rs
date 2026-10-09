//! PTY connect tickets (`core/src/pty/ticket.ts`).
//!
//! Issue/consume primitives for the `x-opencode-ticket: 1` connect-token
//! routes: randomUUID tickets, 60s TTL, 10k capacity, scoped to
//! `{ptyID, directory, workspaceID}` with exact-match single-use consume.
//!
//! The routes themselves (header + origin policy, WS upgrade) live in the
//! handler lane; this module is the shared store over
//! [`crate::state::AppState::tickets`].

use std::path::PathBuf;

use serde_json::json;

use crate::auth;
use crate::state::{AppState, TicketEntry};

/// Ticket TTL (`Duration.seconds(60)`).
pub const TTL_SECONDS: u64 = 60;

/// Cache capacity.
pub const CAPACITY: usize = 10_000;

/// The scope a ticket is bound to (`PtyTicket.Scope`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub pty_id: String,
    pub directory: PathBuf,
    pub workspace_id: Option<String>,
}

/// Issues a ticket for `scope`: `{ticket, expires_in}` JSON shape.
pub fn issue(state: &AppState, scope: Scope) -> serde_json::Value {
    let ticket = uuid::Uuid::new_v4().to_string();
    evict_if_full(&state.tickets, CAPACITY);
    let expires_at = u64::try_from(auth::unix_now_ms()).unwrap_or(0) + TTL_SECONDS * 1000;
    state.tickets.insert(
        ticket.clone(),
        TicketEntry {
            expires_at,
            pty_id: scope.pty_id,
            directory: scope.directory,
            workspace_id: scope.workspace_id,
        },
    );
    json!({ "ticket": ticket, "expires_in": TTL_SECONDS })
}

/// Consumes a ticket (single-use): the stored scope must match the request
/// scope exactly and the ticket must still be live. A scope mismatch does
/// NOT consume the ticket (`invalidateWhen` guards on `matches`).
pub fn consume(state: &AppState, scope: &Scope, ticket: &str, now_ms: i64) -> bool {
    let now = u64::try_from(now_ms).unwrap_or(0);
    state
        .tickets
        .remove_if(ticket, |_, entry| {
            let live = now < entry.expires_at;
            let matches = entry.pty_id == scope.pty_id
                && entry.directory == scope.directory
                && entry.workspace_id == scope.workspace_id;
            live && matches
        })
        .is_some()
}

/// Evicts the oldest (nearest-expiry) entry at capacity.
fn evict_if_full(map: &dashmap::DashMap<String, TicketEntry>, capacity: usize) {
    if map.len() < capacity {
        return;
    }
    let oldest =
        map.iter().min_by_key(|entry| entry.value().expires_at).map(|entry| entry.key().clone());
    if let Some(oldest) = oldest {
        map.remove(&oldest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AuthConfig;
    use oc_core::engine::{Engine, EngineOptions};
    use oc_util::determinism::{SystemClock, UuidV4IdGen};

    async fn state() -> std::sync::Arc<AppState> {
        let tmp = tempfile::tempdir().expect("tempdir");
        let engine =
            Engine::boot(EngineOptions::new(tmp.path().to_owned(), tmp.path().to_owned())).await;
        std::sync::Arc::new(AppState::new(
            engine,
            AuthConfig { password: Some("secret".to_owned()) },
            "test".to_owned(),
            std::sync::Arc::new(UuidV4IdGen),
            std::sync::Arc::new(SystemClock),
        ))
    }

    fn scope(pty_id: &str) -> Scope {
        Scope { pty_id: pty_id.to_owned(), directory: PathBuf::from("/tmp"), workspace_id: None }
    }

    #[tokio::test]
    async fn server_tickets_issue_consume_single_use() {
        let state = state().await;
        let issued = issue(&state, scope("pty_1"));
        let ticket = issued["ticket"].as_str().unwrap().to_owned();
        assert_eq!(issued["expires_in"], json!(60));
        assert!(uuid::Uuid::parse_str(&ticket).is_ok(), "randomUUID shape");

        let now = auth::unix_now_ms();
        assert!(consume(&state, &scope("pty_1"), &ticket, now), "matching scope consumes");
        assert!(!consume(&state, &scope("pty_1"), &ticket, now), "single use");
        assert!(!consume(&state, &scope("pty_1"), "unknown", now), "unknown ticket");
    }

    #[tokio::test]
    async fn server_tickets_scope_must_match_exactly() {
        let state = state().await;
        let ticket = issue(&state, scope("pty_1"))["ticket"].as_str().unwrap().to_owned();
        let now = auth::unix_now_ms();
        assert!(!consume(&state, &scope("pty_2"), &ticket, now), "wrong pty");
        assert!(
            !consume(
                &state,
                &Scope {
                    pty_id: "pty_1".into(),
                    directory: PathBuf::from("/elsewhere"),
                    workspace_id: None
                },
                &ticket,
                now
            ),
            "wrong directory"
        );
        assert!(
            !consume(
                &state,
                &Scope {
                    pty_id: "pty_1".into(),
                    directory: PathBuf::from("/tmp"),
                    workspace_id: Some("ws_1".into())
                },
                &ticket,
                now
            ),
            "wrong workspace"
        );
        // Still consumable by the exact scope after failed attempts.
        assert!(consume(&state, &scope("pty_1"), &ticket, now));
    }

    #[tokio::test]
    async fn server_tickets_expire() {
        let state = state().await;
        let now = auth::unix_now_ms();
        let ticket = issue(&state, scope("pty_1"))["ticket"].as_str().unwrap().to_owned();
        assert!(!consume(
            &state,
            &scope("pty_1"),
            &ticket,
            now + i64::try_from(TTL_SECONDS * 1000).unwrap() + 1
        ));
    }

    #[tokio::test]
    async fn server_tickets_capacity_bound() {
        use crate::state::TicketEntry;
        let state = state().await;
        // The oldest expiry is evicted at capacity: seed one with a plainly
        // older expiry (same-ms issuance would make "oldest" ambiguous).
        let oldest = "ticket_manual_oldest".to_owned();
        state.tickets.insert(
            oldest.clone(),
            TicketEntry {
                expires_at: 1,
                pty_id: "pty_seed".to_owned(),
                directory: PathBuf::from("/tmp"),
                workspace_id: None,
            },
        );
        for index in 0..CAPACITY - 1 {
            issue(
                &state,
                Scope {
                    pty_id: format!("pty_{index}"),
                    directory: PathBuf::from("/tmp"),
                    workspace_id: None,
                },
            );
        }
        assert_eq!(state.tickets.len(), CAPACITY);
        issue(&state, scope("pty_overflow"));
        assert_eq!(state.tickets.len(), CAPACITY);
        assert!(!state.tickets.contains_key(&oldest), "oldest evicted");
    }
}
