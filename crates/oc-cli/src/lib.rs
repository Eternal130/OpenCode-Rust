//! Library shim for the `oc-cli` crate.
//!
//! The crate is a binary (`src/main.rs` keeps its own `mod acp`); this
//! lib target exists only so integration tests under `tests/` can link
//! the pure ACP fold pipeline (notably [`acp::translate`]) for the
//! env-gated differential driver (`tests/acp_translate_diff_driver.rs`,
//! channel 2 of the diff layer). Every acp submodule is the *real*
//! file, re-mounted via `#[path]`; the only non-file pieces are the
//! small `App`/`Conn` definitions (mirrored verbatim from
//! `src/acp/mod.rs:48-107` — the bin's `run()`/standalone-server entry
//! is deliberately not duplicated) and the `util::VERSION` mirror.
//! The binary remains the production path and is untouched.

pub mod acp {
    #[path = "agent.rs"]
    pub mod agent;
    #[path = "capabilities.rs"]
    pub mod capabilities;
    #[path = "catalog.rs"]
    pub mod catalog;
    #[path = "child.rs"]
    pub mod child;
    #[path = "compaction.rs"]
    pub mod compaction;
    #[path = "config_option.rs"]
    pub mod config_option;
    #[path = "content.rs"]
    pub mod content;
    #[path = "directories.rs"]
    pub mod directories;
    #[path = "elicitation.rs"]
    pub mod elicitation;
    #[path = "error.rs"]
    pub mod error;
    #[path = "permission.rs"]
    pub mod permission;
    #[path = "prompt.rs"]
    pub mod prompt;
    #[path = "replay.rs"]
    pub mod replay;
    #[path = "service.rs"]
    pub mod service;
    #[path = "sessions.rs"]
    pub mod sessions;
    #[path = "tool.rs"]
    pub mod tool;
    #[path = "translate.rs"]
    pub mod translate;
    #[path = "transport.rs"]
    pub mod transport;
    #[path = "turn.rs"]
    pub mod turn;

    // --- mirror of src/acp/mod.rs:48-107 (App/Conn) -------------------
    use std::sync::Arc;

    use agent_client_protocol::Client as ClientRole;
    use agent_client_protocol::ConnectionTo;
    use tokio::sync::RwLock;

    use crate::acp::capabilities::Capabilities;
    use crate::acp::catalog::Catalogs;
    use crate::acp::sessions::Sessions;
    use crate::acp::turn::Turns;

    /// The agent-side handle to the ACP client on the other side of stdio.
    pub type Conn = ConnectionTo<ClientRole>;

    /// Shared agent state (the closure environment of every handler).
    pub struct App {
        /// The opencode HTTP/SSE client (handlers/acp.ts:18-26).
        pub client: oc_client::Client,
        /// Per-cwd model/mode/command catalogs (acp/agent.ts:28).
        pub catalogs: Arc<Catalogs>,
        /// Attached sessions (acp/agent.ts:122).
        pub sessions: Sessions,
        /// The parsed client capabilities, set by `initialize`
        /// (acp/agent.ts:123, service.ts:107).
        pub capabilities: RwLock<Capabilities>,
        /// Active prompt turns (acp/agent.ts:124).
        pub turns: Turns,
        /// The connection, set once the ACP wire is live.
        conn: tokio::sync::OnceCell<Conn>,
    }

    impl App {
        /// Build the shared state (before the connection exists).
        #[must_use]
        pub fn new(client: oc_client::Client) -> Arc<Self> {
            Arc::new(Self {
                catalogs: Arc::new(Catalogs::new(client.clone())),
                sessions: Sessions::new(client.clone()),
                client,
                capabilities: RwLock::new(Capabilities::default()),
                turns: Turns::default(),
                conn: tokio::sync::OnceCell::new(),
            })
        }

        /// Bind the live connection (agent boot, agent.ts:120-129).
        pub fn bind_connection(&self, conn: Conn) {
            let _ = self.conn.set(conn);
        }

        /// The live connection (handlers gate on the boot task having bound
        /// it — the `ready` deferred of agent.ts:30-33).
        pub async fn conn(&self) -> Conn {
            loop {
                if let Some(conn) = self.conn.get() {
                    return conn.clone();
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }
}

pub mod util {
    /// Mirror of the bin-only `src/util.rs` VERSION (same crate version).
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
}
