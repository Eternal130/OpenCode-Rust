//! Environment execution layer — port of
//! `packages/core/src/environment/` (+ the per-location bind from
//! `environment.ts`).

pub mod driver;
pub mod exec_defaults;
pub mod files;
pub mod local;
pub mod memory;
pub mod stdio;
pub mod unavailable;
pub mod workspace;

#[cfg(test)]
mod tests;

pub use driver::{make_files, CommandSpec, Driver, ProcessOutput, SpawnError, Spawner};
pub use files::{
    type_following, DirEntry, FileInfo, FileType, Files, FilesError, Range, ReadResult,
};
pub use local::{make_local_driver, make_local_driver_default, HostSpawner, LocalFiles};
pub use memory::MemoryDriver;
pub use stdio::{
    BoxedReader, BoxedWriter, FailingStdioSpawner, LocalStdioSpawner, PipedProcess, StdioSpawner,
    StdioSpec, StopChild,
};
pub use unavailable::UnavailableSpawner;
pub use workspace::{
    Binding, DestroyResult, LocalWorkspaceDriver, Registry, WorkspaceDriver, WorkspaceError,
    WorkspaceInfo, WorkspaceService,
};

use std::sync::Arc;

use crate::location::LocationRef;

/// `Environment.Service` equivalent: resolves the per-location execution
/// plane. A location with a `workspaceID` binds to that workspace's driver;
/// otherwise the host filesystem driver is used.
pub struct Environment {
    workspace: Option<Arc<WorkspaceService>>,
}

impl Default for Environment {
    fn default() -> Self {
        Environment { workspace: None }
    }
}

impl Environment {
    /// Host-only environment (no workspace service attached).
    pub fn local() -> Self {
        Environment::default()
    }

    /// Environment with workspace connectivity.
    pub fn with_workspace(workspace: Arc<WorkspaceService>) -> Self {
        Environment { workspace: Some(workspace) }
    }

    /// Per-location driver bind. An unknown or destroyed workspace placement
    /// is a configuration defect by design (environment.ts `orDie`).
    pub async fn bind(&self, location: &LocationRef) -> Result<Driver, String> {
        match (&location.workspace_id, &self.workspace) {
            (Some(workspace_id), Some(workspace)) => workspace
                .connect(workspace_id)
                .await
                .map_err(|cause| format!("Failed to bind Environment to workspace {workspace_id}: {cause}")),
            (Some(workspace_id), None) => Err(format!(
                "Failed to bind Environment to workspace {workspace_id}: no workspace service attached"
            )),
            (None, _) => Ok(make_local_driver_default()),
        }
    }

    /// Resolved `Files` for a location.
    pub async fn files(&self, location: &LocationRef) -> Result<Arc<dyn Files>, String> {
        let driver = self.bind(location).await?;
        Ok(make_files(&driver))
    }

    /// Resolved streaming stdio spawner for a location — the seam the
    /// MCP stdio transport spawns its servers through (ref
    /// `mcp/stdio.ts` uses `Environment.Service`'s spawner).
    pub async fn stdio(
        &self,
        location: &LocationRef,
    ) -> Result<Arc<dyn crate::environment::stdio::StdioSpawner>, String> {
        let driver = self.bind(location).await?;
        Ok(driver.stdio)
    }
}
