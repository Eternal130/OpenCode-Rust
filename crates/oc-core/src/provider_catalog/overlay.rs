//! Per-location provider overlay (core-deep-map §5 `Provider`/`Model`/
//! `ModelResolver` nodes): the config-defined custom providers + model
//! default for ONE location directory, discovered against that
//! directory's config (the config lane's [`Loader`]) and consulted
//! BEFORE the global catalog/boot bindings.
//!
//! v2 resolves providers per location (`provider.ts` reads the layer's
//! `Config`): a daemon started in project A serves project B with B's
//! custom providers. The Rust engine's boot-time
//! [`EngineOptions`](crate::engine::EngineOptions) seeding binds only the
//! spawning cwd's config, so this module re-runs the same mapping over
//! each location's loaded config — the overlay half of
//! [`crate::engine::resolver::LocationModelResolver`].

use oc_schema::config::ModelSelection;

use crate::config::Loaded;
use crate::provider_catalog::facade::{CustomModel, CustomProvider};

/// The config-derived provider bindings for one loaded document set
/// (boot seeding and the per-location overlay share this shape).
#[derive(Debug, Default, Clone)]
pub struct ConfigProviders {
    /// `providers.<id>` with a `settings.baseURL` (openai-compatible).
    pub custom: Vec<CustomProvider>,
    /// The `model` default selection.
    pub default_model: Option<ModelSelection>,
}

/// Maps a loaded config onto provider bindings: every
/// `providers.<id>.settings.baseURL` entry becomes an
/// openai-compatible [`CustomProvider`] (models from the entry's
/// `models` map; `apiKey` explicit or the first `env` variable that is
/// set), and the last `model` selection wins. Later documents override
/// earlier ones for the same provider id (the loader orders nearest
/// config last).
#[must_use]
pub fn providers_from_config(loaded: &Loaded) -> ConfigProviders {
    let mut out = ConfigProviders::default();
    // Later entries win (priority order: nearest config last).
    for entry in &loaded.entries {
        let oc_schema::config::Entry::Document(document) = entry else { continue };
        if let Some(providers) = &document.info.providers {
            for (id, provider) in providers {
                let Some(settings) = &provider.settings else { continue };
                let base_url = settings
                    .rest
                    .get("baseURL")
                    .or_else(|| settings.rest.get("base_url"))
                    .and_then(|value| value.as_ref().and_then(|value| value.as_str()))
                    .map(str::to_owned);
                let Some(base_url) = base_url else { continue };
                let api_key = settings
                    .rest
                    .get("apiKey")
                    .or_else(|| settings.rest.get("api_key"))
                    .and_then(|value| value.as_ref().and_then(|value| value.as_str()))
                    .map(str::to_owned)
                    .or_else(|| {
                        provider
                            .env
                            .as_ref()
                            .and_then(|names| names.first())
                            .and_then(|name| std::env::var(name).ok())
                    });
                let models = provider
                    .models
                    .as_ref()
                    .map(|models| {
                        models
                            .iter()
                            .map(|(id, model)| CustomModel {
                                id: id.clone(),
                                // The display name (`models.<id>.name` — the
                                // listing surfaces show it, v2
                                // config/plugin/provider.ts:112).
                                name: model.name.clone(),
                                // Per-model overlays (`providers.<id>.models.<mid>
                                // .headers|body`) — v2 merges them onto the
                                // effective model (core/src/model.ts:204-209);
                                // the request-path pipeline forwards them to
                                // the wire (ModelOverlays.body → provider
                                // request body).
                                headers: model.headers.clone(),
                                body: model.body.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                out.custom.push(CustomProvider {
                    id: id.clone(),
                    name: provider.name.clone().unwrap_or_else(|| id.clone()),
                    base_url,
                    api_key,
                    // Provider-level overlays (`providers.<id>.headers|body`)
                    // — merged under every model's own overlays.
                    headers: provider.headers.clone(),
                    body: provider.body.clone(),
                    models,
                });
            }
        }
        if let Some(selection) = &document.info.model {
            out.default_model = Some(selection.clone());
        }
    }
    // Later documents override earlier ones for the same provider id.
    let mut deduped: Vec<CustomProvider> = Vec::new();
    for provider in out.custom {
        match deduped.iter().position(|existing| existing.id == provider.id) {
            Some(index) => deduped[index] = provider,
            None => deduped.push(provider),
        }
    }
    out.custom = deduped;
    out
}

/// Whether this document set contributes any provider binding (an empty
/// overlay needs no wrapper — the engine-level resolver already covers
/// the location).
#[must_use]
pub fn is_empty(providers: &ConfigProviders) -> bool {
    providers.custom.is_empty() && providers.default_model.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalDirs, Loader};

    fn loader(dir: &std::path::Path, home: &std::path::Path) -> Loader {
        Loader::new(
            dir.to_path_buf(),
            GlobalDirs { config: home.join(".config").join("opencode"), home: home.to_path_buf() },
        )
    }

    #[test]
    fn overlay_maps_custom_providers_and_default_model() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let project = root.path().join("proj");
        let home = root.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            project.join("opencode.json"),
            r#"{
                "model": { "providerID": "acme", "model": "acme-large" },
                "providers": {
                    "acme": {
                        "name": "Acme",
                        "settings": { "baseURL": "https://acme.test/v1", "apiKey": "s3cret" },
                        "models": { "acme-large": { "name": "Large" }, "acme-small": {} }
                    }
                }
            }"#,
        )
        .unwrap();

        let providers = providers_from_config(&loader(&project, &home).load());
        assert_eq!(providers.custom.len(), 1);
        let acme = &providers.custom[0];
        assert_eq!(acme.id, "acme");
        assert_eq!(acme.name, "Acme");
        assert_eq!(acme.base_url, "https://acme.test/v1");
        assert_eq!(acme.api_key.as_deref(), Some("s3cret"));
        assert_eq!(acme.models.len(), 2);
        // `models.<id>.name` rides the entry (provider.ts:112).
        let large = acme.models.iter().find(|model| model.id == "acme-large").expect("large");
        assert_eq!(large.name.as_deref(), Some("Large"));
        let small = acme.models.iter().find(|model| model.id == "acme-small").expect("small");
        assert_eq!(small.name, None, "unnamed models fall back to the id at the listing");
        let default = providers.default_model.expect("default selection");
        assert_eq!(default.provider_id.as_str(), "acme");
        assert_eq!(default.model.as_str(), "acme-large");
    }

    #[test]
    fn overlay_keeps_model_and_provider_overlays() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let project = root.path().join("proj");
        let home = root.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        // The request-path rig shape: a model-level `body` overlay forcing
        // a wire field (v2 ModelOverlays.body — the sanctioned way to pin
        // e.g. `temperature`), plus provider- and model-level headers.
        std::fs::write(
            project.join("opencode.json"),
            r#"{
                "model": { "providerID": "acme", "model": "acme-large" },
                "providers": {
                    "acme": {
                        "settings": { "baseURL": "https://acme.test/v1" },
                        "headers": { "X-Org": "org" },
                        "body": { "top_k": 7 },
                        "models": {
                            "acme-large": {
                                "body": { "temperature": 0 },
                                "headers": { "X-Tier": "gold" }
                            },
                            "acme-small": {}
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let providers = providers_from_config(&loader(&project, &home).load());
        assert_eq!(providers.custom.len(), 1);
        let acme = &providers.custom[0];
        assert_eq!(
            acme.body.as_ref().and_then(|b| b.get("top_k")),
            Some(&serde_json::json!(7)),
            "provider-level body overlay is captured"
        );
        assert_eq!(acme.headers.as_ref().and_then(|h| h.get("X-Org")), Some(&"org".to_string()));
        let large = acme.models.iter().find(|m| m.id == "acme-large").expect("large present");
        assert_eq!(
            large.body.as_ref().and_then(|b| b.get("temperature")),
            Some(&serde_json::json!(0)),
            "model-level body overlay is captured"
        );
        assert_eq!(large.headers.as_ref().and_then(|h| h.get("X-Tier")), Some(&"gold".to_string()));
        let small = acme.models.iter().find(|m| m.id == "acme-small").expect("small present");
        assert!(small.body.is_none() && small.headers.is_none());
    }

    #[test]
    fn overlay_nearest_document_overrides_same_provider_id() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let project = root.path().join("proj");
        let home = root.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(home.join(".config").join("opencode")).unwrap();
        std::fs::write(
            home.join(".config").join("opencode").join("opencode.json"),
            r#"{
                "providers": {
                    "acme": { "settings": { "baseURL": "https://global.test/v1" },
                              "models": { "global-model": {} } },
                    "other": { "settings": { "baseURL": "https://other.test/v1" },
                               "models": { "other-model": {} } }
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            project.join("opencode.json"),
            r#"{
                "providers": {
                    "acme": { "settings": { "baseURL": "https://project.test/v1" },
                              "models": { "project-model": {} } }
                }
            }"#,
        )
        .unwrap();

        let providers = providers_from_config(&loader(&project, &home).load());
        // project overrides global for `acme`, keeps `other`.
        assert_eq!(providers.custom.len(), 2);
        let acme = providers.custom.iter().find(|p| p.id == "acme").unwrap();
        assert_eq!(acme.base_url, "https://project.test/v1");
        assert_eq!(
            acme.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["project-model"]
        );
        assert!(providers.custom.iter().any(|p| p.id == "other"));
        assert!(is_empty(&ConfigProviders::default()));
        assert!(!is_empty(&providers));
    }

    #[test]
    fn overlay_entries_without_base_url_are_skipped() {
        let root = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let project = root.path().join("proj");
        let home = root.path().join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            project.join("opencode.json"),
            r#"{ "providers": { "conn": { "settings": { "apiKey": "k" } } } }"#,
        )
        .unwrap();
        let providers = providers_from_config(&loader(&project, &home).load());
        assert!(providers.custom.is_empty());
    }
}
