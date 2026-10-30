//! No-execution-plane spawner — port of
//! `packages/core/src/environment/unavailable.ts`.

use async_trait::async_trait;

use crate::environment::driver::{CommandSpec, ProcessOutput, SpawnError, Spawner};

/// A spawner for locations with no execution plane: no workspace is attached
/// and the host cannot spawn processes.
pub struct UnavailableSpawner;

#[async_trait]
impl Spawner for UnavailableSpawner {
    async fn spawn(&self, _command: CommandSpec) -> Result<ProcessOutput, SpawnError> {
        Err(
            "This location has no execution plane: no workspace is attached and the host cannot spawn processes"
                .to_string(),
        )
    }
}
