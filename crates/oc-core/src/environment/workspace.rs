//! Workspace service — port of `packages/core/src/workspace.ts` +
//! `workspace/driver.ts` (bounded: the idle-suspension janitor loop is a
//! later follow-up; `suspend_for_idle` is exposed on the driver trait).
//! Also deferred, pending a non-local `WorkspaceDriver`: the ref's shared
//! provision attempt (workspace.ts:113-146 — concurrent `provision` calls
//! here invoke `driver.create` twice, safe because create is idempotent by
//! contract), connection pooling / wake-per-spawn spawner wrapper
//! (workspace.ts:254-290), and destroy-vs-racing-provision `NotFound`
//! settling. These become load-bearing only once a remote provider
//! registers — the local provider needs none of them.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::environment::driver::Driver;
use crate::environment::local::make_local_driver_default;
use crate::project::SqliteProjectStore;

/// Smallest provider-owned JSON value required to reconnect to the same
/// provider resource. Core stores it opaquely and hands it back; only the
/// owning driver reads inside.
pub type Binding = serde_json::Map<String, Value>;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("workspace driver error: {0}")]
    Driver(String),
    #[error("workspace provider not found: {0}")]
    ProviderNotFound(String),
    #[error("workspace {workspace_id} already exists with provider {existing_provider}, requested {provider}")]
    CreateConflict { workspace_id: String, provider: String, existing_provider: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceInfo {
    pub id: String,
    pub provider: String,
    pub binding: Option<Value>,
    pub created_at: i64,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DestroyResult {
    pub destroyed: bool,
}

/// `WorkspaceDriver.Interface`: provider-owned resource lifecycle.
#[async_trait]
pub trait WorkspaceDriver: Send + Sync {
    /// Get-or-create the provider resource backing this logical workspace.
    /// MUST be idempotent per `workspaceID`.
    async fn create(&self, workspace_id: &str) -> Result<Binding, WorkspaceError>;
    async fn connect(
        &self,
        workspace_id: &str,
        binding: &Binding,
    ) -> Result<Driver, WorkspaceError>;
    async fn suspend_for_idle(
        &self,
        workspace_id: &str,
        binding: &Binding,
    ) -> Result<(), WorkspaceError>;
    /// Release the provider resource. `binding` is `None` when none was
    /// persisted (never provisioned or interrupted mid-create); treat absence
    /// as success.
    async fn destroy(
        &self,
        workspace_id: &str,
        binding: Option<&Binding>,
    ) -> Result<(), WorkspaceError>;
}

/// `WorkspaceDriver.Registry`.
#[derive(Clone, Default)]
pub struct Registry {
    drivers: HashMap<String, Arc<dyn WorkspaceDriver>>,
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Registry::default()
    }

    pub fn register(&mut self, provider: &str, driver: Arc<dyn WorkspaceDriver>) {
        self.drivers.insert(provider.to_string(), driver);
    }

    pub fn get(&self, provider: &str) -> Result<Arc<dyn WorkspaceDriver>, WorkspaceError> {
        self.drivers
            .get(provider)
            .cloned()
            .ok_or_else(|| WorkspaceError::ProviderNotFound(provider.to_string()))
    }
}

/// The default provider: the host itself (local driver, empty binding).
pub struct LocalWorkspaceDriver;

#[async_trait]
impl WorkspaceDriver for LocalWorkspaceDriver {
    async fn create(&self, _workspace_id: &str) -> Result<Binding, WorkspaceError> {
        Ok(Binding::new())
    }

    async fn connect(
        &self,
        _workspace_id: &str,
        _binding: &Binding,
    ) -> Result<Driver, WorkspaceError> {
        Ok(make_local_driver_default())
    }

    async fn suspend_for_idle(
        &self,
        _workspace_id: &str,
        _binding: &Binding,
    ) -> Result<(), WorkspaceError> {
        Ok(())
    }

    async fn destroy(
        &self,
        _workspace_id: &str,
        binding: Option<&Binding>,
    ) -> Result<(), WorkspaceError> {
        let _ = binding;
        Ok(())
    }
}

/// `Workspace.Service`: create / provision / connect / destroy over the
/// `workspace` sqlite table.
pub struct WorkspaceService {
    store: Arc<SqliteProjectStore>,
    registry: Registry,
}

impl WorkspaceService {
    pub fn new(store: Arc<SqliteProjectStore>) -> Self {
        let mut registry = Registry::new();
        registry.register("local", Arc::new(LocalWorkspaceDriver));
        WorkspaceService { store, registry }
    }

    pub fn with_registry(store: Arc<SqliteProjectStore>, registry: Registry) -> Self {
        WorkspaceService { store, registry }
    }

    /// Instantly commits a logical workspace ID. No provider work happens
    /// here.
    pub async fn create(&self, id: Option<&str>, provider: &str) -> Result<String, WorkspaceError> {
        let workspace_id =
            id.map_or_else(|| format!("wrk_{:032x}", rand::random::<u128>()), str::to_string);
        if let Some(row) = self.store.workspace_row(&workspace_id).await {
            if row.1 == provider {
                return Ok(workspace_id);
            }
            return Err(WorkspaceError::CreateConflict {
                workspace_id: workspace_id.clone(),
                provider: provider.to_string(),
                existing_provider: row.1,
            });
        }
        self.registry.get(provider)?;
        self.store.insert_workspace(&workspace_id, provider, None).await;
        // INSERT OR IGNORE raced a concurrent create: re-check the provider.
        if let Some(row) = self.store.workspace_row(&workspace_id).await {
            if row.1 != provider {
                return Err(WorkspaceError::CreateConflict {
                    workspace_id,
                    provider: provider.to_string(),
                    existing_provider: row.1,
                });
            }
        }
        Ok(workspace_id)
    }

    async fn load(&self, workspace_id: &str) -> Result<WorkspaceInfo, WorkspaceError> {
        let row = self
            .store
            .workspace_row(workspace_id)
            .await
            .ok_or_else(|| WorkspaceError::NotFound(workspace_id.to_string()))?;
        Ok(WorkspaceInfo {
            id: row.0,
            provider: row.1,
            binding: row.2.as_deref().and_then(|text| serde_json::from_str(text).ok()),
            created_at: row.3,
            last_used_at: row.4,
        })
    }

    /// Starts or joins the shared attempt that makes the backing resource
    /// real, then returns it.
    pub async fn provision(&self, workspace_id: &str) -> Result<WorkspaceInfo, WorkspaceError> {
        let mut info = self.load(workspace_id).await?;
        if let Some(binding) = &info.binding {
            let _ = binding;
            return Ok(info);
        }
        let driver = self.registry.get(&info.provider)?;
        let binding = driver.create(workspace_id).await?;
        let text = Value::Object(binding).to_string();
        self.store.save_workspace_binding(workspace_id, &text).await;
        info.binding = serde_json::from_str(&text).ok();
        Ok(info)
    }

    /// Provision (if needed), then hand back the environment driver and
    /// record activity.
    pub async fn connect(&self, workspace_id: &str) -> Result<Driver, WorkspaceError> {
        let info = self.provision(workspace_id).await?;
        let binding =
            info.binding.as_ref().and_then(|value| value.as_object().cloned()).ok_or_else(
                || {
                    WorkspaceError::Driver(format!(
                        "workspace {workspace_id} has no binding after provision"
                    ))
                },
            )?;
        let driver = self.registry.get(&info.provider)?;
        let connected = driver.connect(workspace_id, &binding).await?;
        self.store.touch_workspace(workspace_id).await;
        Ok(connected)
    }

    /// Makes the workspace absent; reports whether this call destroyed an
    /// existing workspace.
    pub async fn destroy(&self, workspace_id: &str) -> Result<DestroyResult, WorkspaceError> {
        let Some(row) = self.store.workspace_row(workspace_id).await else {
            return Ok(DestroyResult { destroyed: false });
        };
        // Null binding still reaches the driver: an interrupted or crashed
        // provision may have created a resource that was never persisted. A
        // provider missing from the registry cannot block deleting a
        // never-provisioned row.
        let binding: Option<Binding> =
            row.2.as_deref().and_then(|text| serde_json::from_str(text).ok());
        match self.registry.get(&row.1) {
            Ok(driver) => driver.destroy(workspace_id, binding.as_ref()).await?,
            Err(WorkspaceError::ProviderNotFound(_)) if binding.is_none() => {}
            Err(other) => return Err(other),
        }
        let destroyed = self.store.delete_workspace(workspace_id).await;
        Ok(DestroyResult { destroyed })
    }
}
