//! The model/mode/command catalog (port of `acp/catalog.ts`): loads per
//! cwd, retries while providers discover models, reloads on
//! `model.updated`/`agent.updated`/`command.updated` events.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::watch;
use tokio::sync::{Mutex, RwLock};

use crate::acp::error::AcpError;

/// `builtinCommands` (catalog.ts:9-11).
pub const BUILTIN_COMPACT: &str = "compact";

/// One catalog entry mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// One catalog entry command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
}

/// A model as the catalog tracks it (`Model.Info`).
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub provider_id: String,
    pub id: String,
    pub name: String,
    pub variants: Vec<String>,
    pub context_limit: u64,
}

/// A model reference (`Model.Ref`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelRef {
    pub provider_id: String,
    pub id: String,
    pub variant: Option<String>,
}

/// `Catalog` (catalog.ts:13-19).
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    pub models: Vec<Model>,
    pub default_model: ModelRef,
    pub modes: Vec<Mode>,
    pub default_mode_id: String,
    pub commands: Vec<Command>,
}

impl Catalog {
    /// `findModel` (catalog.ts:21-23).
    #[must_use]
    pub fn find_model(&self, reference: &ModelRef) -> Option<&Model> {
        self.models
            .iter()
            .find(|model| model.provider_id == reference.provider_id && model.id == reference.id)
    }
}

/// One registry entry: the loaded catalog plus a change feed.
struct Entry {
    catalog: RwLock<Arc<Catalog>>,
    changes: watch::Sender<u64>,
}

/// The per-process catalog registry with a shared reload feed.
pub struct Catalogs {
    client: oc_client::Client,
    entries: Mutex<HashMap<String, Arc<Entry>>>,
}

impl Catalogs {
    pub fn new(client: oc_client::Client) -> Self {
        Self { client, entries: Mutex::new(HashMap::new()) }
    }

    fn key(cwd: &str) -> String {
        std::fs::canonicalize(cwd)
            .map_or_else(|_| cwd.to_owned(), |path| path.to_string_lossy().into_owned())
    }

    async fn entry(&self, cwd: &str) -> Result<Arc<Entry>, AcpError> {
        let key = Self::key(cwd);
        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.get(&key) {
            return Ok(Arc::clone(entry));
        }
        // load (catalog.ts:127-135): retry not-ready for up to 5s.
        let catalog = load_with_retry(&self.client, cwd).await?;
        let (changes, _) = watch::channel(0);
        let entry = Arc::new(Entry { catalog: RwLock::new(Arc::new(catalog)), changes });
        entries.insert(key, entry.clone());
        Ok(entry)
    }

    /// `get` (catalog.ts:116-119).
    pub async fn get(&self, cwd: &str) -> Result<Arc<Catalog>, AcpError> {
        let entry = self.entry(cwd).await?;
        let catalog = entry.catalog.read().await;
        Ok(Arc::clone(&catalog))
    }

    /// `reload` (catalog.ts:120-122): re-read and publish to watchers.
    pub async fn reload(&self, cwd: &str) -> Result<(), AcpError> {
        let entry = self.entry(cwd).await?;
        let catalog = load_with_retry(&self.client, cwd).await?;
        *entry.catalog.write().await = Arc::new(catalog);
        entry.changes.send_modify(|generation| *generation += 1);
        Ok(())
    }

    /// The `reloadOn` fan-out (catalog.ts:68-79): a located event
    /// reloads that directory's catalog; an unlocated one reloads every
    /// loaded catalog.
    pub async fn reload_affected(&self, directory: Option<&str>) -> Result<(), AcpError> {
        let targets: Vec<String> = match directory {
            Some(directory) => vec![directory.to_owned()],
            None => self.entries.lock().await.keys().cloned().collect(),
        };
        let known: Vec<String> = self.entries.lock().await.keys().cloned().collect();
        for target in targets {
            if known.iter().any(|known| *known == Self::key(&target)) {
                self.reload(&target).await?;
            }
        }
        Ok(())
    }

    /// `changes` (catalog.ts:123): a feed that ticks on every reload of
    /// this cwd's catalog.
    pub async fn changes(&self, cwd: &str) -> Result<watch::Receiver<u64>, AcpError> {
        let entry = self.entry(cwd).await?;
        Ok(entry.changes.subscribe())
    }
}

/// `read` (catalog.ts:137-168).
async fn load(client: &oc_client::Client, cwd: &str) -> Result<Catalog, AcpError> {
    let location = oc_client::types::QueryLocation { directory: Some(cwd.to_owned()) };
    let models = client
        .model()
        .list(&oc_client::types::ModelListQuery { location: Some(location.clone()) })
        .await
        .map_err(|error| {
            AcpError::CatalogLoad(format!("{}", crate::acp::error::classify(&error)))
        })?;
    let preferred = client
        .model()
        .default(&oc_client::types::ModelDefaultQuery { location: Some(location.clone()) })
        .await
        .map_err(|error| {
            AcpError::CatalogLoad(format!("{}", crate::acp::error::classify(&error)))
        })?;
    let agents = client
        .agent()
        .list(&oc_client::types::AgentListQuery { location: Some(location.clone()) })
        .await
        .map_err(|error| {
            AcpError::CatalogLoad(format!("{}", crate::acp::error::classify(&error)))
        })?;
    let commands = client
        .command()
        .list(&oc_client::types::CommandListQuery { location: Some(location) })
        .await
        .map_err(|error| {
            AcpError::CatalogLoad(format!("{}", crate::acp::error::classify(&error)))
        })?;

    let models: Vec<Model> = models
        .data
        .iter()
        .filter(|model| model.enabled)
        .map(|model| Model {
            provider_id: model.provider_id.clone(),
            id: model.id.clone(),
            name: model.name.clone(),
            variants: model.variants.iter().map(|variant| variant.id.clone()).collect(),
            context_limit: model.limit.context.max(0).cast_unsigned(),
        })
        .collect();
    // The parallel default read can name a model missing from this list
    // (catalog.ts:150-152).
    let default_model = preferred
        .data
        .as_ref()
        .and_then(|preferred| {
            models.iter().find(|model| {
                model.provider_id == preferred.provider_id && model.id == preferred.id
            })
        })
        .or_else(|| models.first())
        .ok_or(AcpError::CatalogNotReady { reason: "models" })?;
    let default_model = ModelRef {
        provider_id: default_model.provider_id.clone(),
        id: default_model.id.clone(),
        variant: default_model.variants.iter().find(|v| v == &"default").cloned(),
    };
    // Core lists its resolved default agent first, the same one a new
    // session runs (catalog.ts:153-156).
    let primary_modes: Vec<Mode> = agents
        .data
        .iter()
        .filter(|agent| agent.mode != oc_client::types::AgentInfoMode::Subagent && !agent.hidden)
        .map(|agent| Mode {
            id: agent.id.clone(),
            name: agent.name.clone(),
            description: agent.description.clone(),
        })
        .collect();
    let default_mode_id =
        primary_modes.first().ok_or(AcpError::CatalogNotReady { reason: "agents" })?.id.clone();
    let commands: Vec<Command> = commands
        .data
        .iter()
        .filter(|command| command.name != BUILTIN_COMPACT)
        .map(|command| Command {
            name: command.name.clone(),
            description: command.description.clone(),
        })
        .collect();
    Ok(Catalog { models, default_model, modes: primary_modes, default_mode_id, commands })
}

async fn load_with_retry(client: &oc_client::Client, cwd: &str) -> Result<Catalog, AcpError> {
    // Providers may still be discovering models after startup
    // (catalog.ts:129-133): 25ms-spaced retries for up to 5 seconds.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match load(client, cwd).await {
            Ok(catalog) => return Ok(catalog),
            Err(error @ AcpError::CatalogNotReady { .. }) => {
                if std::time::Instant::now() >= deadline {
                    return Err(error);
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// `reloadOn` (catalog.ts:39): the event types that invalidate a
/// catalog.
pub fn reload_on(event_type: &str) -> bool {
    matches!(event_type, "model.updated" | "agent.updated" | "command.updated")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_on_covers_catalog_sources() {
        assert!(reload_on("model.updated"));
        assert!(reload_on("agent.updated"));
        assert!(reload_on("command.updated"));
        assert!(!reload_on("session.text.delta"));
    }
}
