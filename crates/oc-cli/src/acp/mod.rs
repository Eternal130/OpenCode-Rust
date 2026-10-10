//! `opencode acp` — the Agent Client Protocol agent (R-03): a v1 ACP
//! agent over stdio backed by the local opencode server. Port of
//! `packages/cli/src/acp/*` + `commands/handlers/acp.ts`.
//!
//! Layout mirrors the reference one file per module:
//!
//! | module        | reference        |
//! |---------------|------------------|
//! | [`agent`]     | acp/agent.ts     |
//! | [`service`]   | acp/service.ts   |
//! | [`turn`]      | acp/turn.ts      |
//! | [`translate`] | acp/translate.ts |
//! | [`permission`]| acp/permission.ts|
//! | [`elicitation`]| acp/elicitation.ts |
//! | [`sessions`]  | acp/sessions.ts  |
//! | [`catalog`]   | acp/catalog.ts   |
//! | [`config_option`]| acp/config-option.ts |
//! | [`directories`]| acp/directories.ts |
//! | [`content`]   | acp/content.ts   |
//! | [`prompt`]    | acp/prompt.ts    |
//! | [`replay`]    | acp/replay.ts    |
//! | [`tool`]      | acp/tool.ts      |
//! | [`child`]     | acp/child.ts     |
//! | [`compaction`]| acp/compaction.ts|
//! | [`capabilities`]| acp/capabilities.ts |
//! | [`error`]     | acp/error.ts     |

pub mod agent;
pub mod capabilities;
pub mod catalog;
pub mod child;
pub mod compaction;
pub mod config_option;
pub mod content;
pub mod directories;
pub mod elicitation;
pub mod error;
pub mod permission;
pub mod prompt;
pub mod replay;
pub mod service;
pub mod sessions;
pub mod tool;
pub mod translate;
pub mod transport;
pub mod turn;

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

/// `opencode acp` (commands/handlers/acp.ts:13-51): mark the client env,
/// start the standalone server, build the client, serve ACP on fd 0/1,
/// and race the server's exit.
pub async fn run() -> i32 {
    // process.env.OPENCODE_CLIENT = "acp" (acp.ts:16).
    std::env::set_var("OPENCODE_CLIENT", "acp");
    // Standalone.start() (acp.ts:17).
    let server = match crate::services::standalone::start().await {
        Ok(server) => server,
        Err(message) => {
            eprintln!("Error: {message}");
            return 1;
        }
    };
    let endpoint = server.endpoint();
    let app = App::new(endpoint.client());
    let server_exited = server.exited();

    // ndJsonStream(stdout, stdin) + ACP.connect (acp.ts:27): the capped
    // ndjson transport over the process's own stdio.
    let transport = transport::CappedNdJson::std_io();
    let serve = agent::serve(app, transport);
    // Race the connection close against the server exit (acp.ts:28-44).
    let failure = tokio::select! {
        outcome = serve => match outcome {
            Ok(()) => None,
            Err(error) => Some(format!("acp connection failed: {error}")),
        },
        () = server_exited => Some("server exited unexpectedly".to_owned()),
    };
    // Exit directly: dropping the standalone guard would wait on the
    // server's graceful shutdown (acp.ts:45-49).
    match failure {
        Some(failure) => {
            eprintln!("opencode acp: {failure}");
            std::process::exit(1);
        }
        None => std::process::exit(0),
    }
}
