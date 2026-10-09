//! Command handlers (ports of `cli/src/commands/handlers/*`).

pub mod api;
pub mod auth;
pub mod mcp;
pub mod misc;
pub mod plugin;
pub mod serve;
pub mod service_cmds;
pub mod session;
pub mod stats;

use crate::cli::ServerParams;
use crate::services::server_connection::{self, Resolved};

/// `ServerConnection.resolve` + the common error path (stderr + exit 1).
pub async fn connect(
    server: &ServerParams,
    mismatch: Option<server_connection::Mismatch>,
) -> Result<Resolved, i32> {
    match server_connection::resolve(server_connection::Args {
        server: server.server.clone(),
        standalone: server.standalone,
        mismatch: mismatch.unwrap_or(server_connection::Mismatch::Ignore),
    })
    .await
    {
        Ok(resolved) => Ok(resolved),
        Err(message) => {
            eprintln!("{message}");
            Err(1)
        }
    }
}
