//! The model-variant preference repository — port of
//! `packages/tui/src/model-preference.ts` backed by
//! `<state>/model.json` (`mini-host.ts:18-30` wires it into the host's
//! `preferences`). The document keeps `recent`/`favorite` model lists
//! and a `variant` map keyed `provider/model`; `default` is preserved
//! so it can override an agent's configured variant
//! (`model-preference.ts:63-65`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `ModelPreferenceModel`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModelPreferenceModel {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

/// `normalizeModelVariant` (`model-preference.ts:56`).
#[must_use]
pub fn normalize_model_variant(value: Option<&str>) -> Option<String> {
    match value {
        None | Some("default") => None,
        Some(value) => Some(value.to_owned()),
    }
}

/// `modelPreferenceKey` (`model-preference.ts:58`).
#[must_use]
pub fn model_preference_key(model: &ModelPreferenceModel) -> String {
    format!("{}/{}", model.provider_id, model.model_id)
}

/// `cycleModelVariant` (`model-preference.ts:101-109`): none → first
/// named; last/unknown → none (`default`).
#[must_use]
pub fn cycle_model_variant(current: Option<&str>, variants: &[String]) -> Option<String> {
    let named: Vec<&String> = variants.iter().filter(|variant| *variant != "default").collect();
    if named.is_empty() {
        return None;
    }
    let value = normalize_model_variant(current);
    let Some(value) = value else { return Some((*named[0]).clone()) };
    match named.iter().position(|variant| **variant == value) {
        None => None,
        Some(index) if index + 1 >= named.len() => None,
        Some(index) => Some((*named[index + 1]).clone()),
    }
}

/// `recentModels` (`model-preference.ts:67-78`): dedup, front-insert,
/// cap at 10.
#[must_use]
pub fn recent_models(
    model: &ModelPreferenceModel,
    recent: &[ModelPreferenceModel],
) -> Vec<ModelPreferenceModel> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for item in std::iter::once(model).chain(recent.iter()) {
        let key = model_preference_key(item);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        out.push(item.clone());
        if out.len() == 10 {
            break;
        }
    }
    out
}

/// `decodeModelPreference` (`model-preference.ts:82-90`): tolerant
/// decode — malformed fields decode to empty.
#[must_use]
pub fn decode_model_preference(value: &serde_json::Value) -> ModelPreferenceDocument {
    let root = value.as_object().cloned().unwrap_or_default();
    let models = |field: &str| -> Vec<ModelPreferenceModel> {
        root.get(field)
            .and_then(|item| item.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let provider = item.get("providerID")?.as_str()?;
                        let model = item.get("modelID")?.as_str()?;
                        (!provider.is_empty() && !model.is_empty()).then_some(
                            ModelPreferenceModel {
                                provider_id: provider.to_owned(),
                                model_id: model.to_owned(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let variant = root
        .get("variant")
        .and_then(|item| item.as_object())
        .map(|map| {
            map.iter()
                .filter(|(key, _)| !key.is_empty())
                .filter_map(|(key, value)| {
                    // Empty strings drop; "default" is preserved.
                    let value = value.as_str()?;
                    (!value.is_empty()).then(|| (key.clone(), value.to_owned()))
                })
                .collect::<BTreeMap<String, String>>()
        })
        .unwrap_or_default();
    ModelPreferenceDocument { recent: models("recent"), favorite: models("favorite"), variant }
}

/// `ModelPreferenceDocument` (`model-preference.ts:20`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelPreferenceDocument {
    #[serde(default)]
    pub recent: Vec<ModelPreferenceModel>,
    #[serde(default)]
    pub favorite: Vec<ModelPreferenceModel>,
    #[serde(default)]
    pub variant: BTreeMap<String, String>,
}

/// `createModelPreferenceRepository` (`model-preference.ts:110`):
/// read/patch/write over one JSON file. The flock + fs watcher stay
/// JS-side concerns; the Rust port keeps the same read-merge-write
/// atomicity (write-to-temp + rename).
pub struct ModelPreferenceRepository {
    path: PathBuf,
}

impl ModelPreferenceRepository {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Read the document (best-effort; the boot path shares it with
    /// the main TUI's model ring).
    #[must_use]
    pub fn read(&self) -> ModelPreferenceDocument {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .map(|value| decode_model_preference(&value))
            .unwrap_or_default()
    }

    pub fn write(&self, document: &ModelPreferenceDocument) {
        let Some(parent) = self.path.parent() else { return };
        let _ = std::fs::create_dir_all(parent);
        let Ok(text) = serde_json::to_string_pretty(document) else { return };
        let temp = self.path.with_extension(format!("tmp-{}", std::process::id()));
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &self.path);
        }
    }

    /// `resolveVariant` (`model-preference.ts:170-172`).
    #[must_use]
    pub fn resolve_variant(&self, model: &ModelPreferenceModel) -> Option<String> {
        let key = model_preference_key(model);
        let document = self.read();
        normalize_model_variant(document.variant.get(&key).map(String::as_str))
    }

    /// `saveVariant` (`model-preference.ts:173-177`): `undefined`
    /// persists as the literal `"default"`.
    pub fn save_variant(&self, model: &ModelPreferenceModel, value: Option<&str>) {
        let key = model_preference_key(model);
        let mut document = self.read();
        document
            .variant
            .insert(key, normalize_model_variant(value).unwrap_or_else(|| "default".to_owned()));
        self.write(&document);
    }

    /// `addRecent` (`model-preference.ts:150-152`).
    pub fn add_recent(&self, model: &ModelPreferenceModel) {
        let mut document = self.read();
        document.recent = recent_models(model, &document.recent);
        self.write(&document);
    }
}

/// The host preference seam (`MiniHost["preferences"]`,
/// `mini-host.ts:18-30`): a directory-backed variant store.
#[must_use]
pub fn host_preferences(state_dir: &Path) -> ModelPreferenceRepository {
    ModelPreferenceRepository::new(state_dir.join("model.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(provider: &str, model: &str) -> ModelPreferenceModel {
        ModelPreferenceModel { provider_id: provider.into(), model_id: model.into() }
    }

    #[test]
    fn variant_cycles_through_named_entries() {
        let variants: Vec<String> = vec!["default".into(), "fast".into(), "think".into()];
        assert_eq!(cycle_model_variant(None, &variants).as_deref(), Some("fast"));
        assert_eq!(cycle_model_variant(Some("fast"), &variants).as_deref(), Some("think"));
        assert_eq!(cycle_model_variant(Some("think"), &variants), None);
        assert_eq!(cycle_model_variant(Some("default"), &variants).as_deref(), Some("fast"));
        assert_eq!(cycle_model_variant(None, &[]), None);
    }

    #[test]
    fn recent_models_dedups_front_inserts_and_caps() {
        let a = model("anthropic", "claude");
        let b = model("openai", "gpt");
        let recent = recent_models(&a, &[b.clone(), a.clone()]);
        assert_eq!(recent, vec![a.clone(), b.clone()], "dedup keeps newest first");
        let many: Vec<ModelPreferenceModel> =
            (0..12).map(|index| model("p", &format!("m{index}"))).collect();
        assert_eq!(recent_models(&a, &many).len(), 10);
    }

    #[test]
    fn decode_is_tolerant() {
        let value = serde_json::json!({
            "recent": [{ "providerID": "", "modelID": "x" }, { "providerID": "p", "modelID": "m" }, "junk"],
            "variant": { "p/m": "fast", "": "x", "p/n": "" }
        });
        let document = decode_model_preference(&value);
        assert_eq!(document.recent, vec![model("p", "m")]);
        assert_eq!(document.variant.get("p/m").map(String::as_str), Some("fast"));
        assert!(!document.variant.contains_key(""));
        assert!(!document.variant.contains_key("p/n"));
        assert_eq!(
            decode_model_preference(&serde_json::Value::Null),
            ModelPreferenceDocument::default()
        );
    }

    #[test]
    fn repository_round_trips_variants_with_default_preserved() {
        let dir = std::env::temp_dir().join(format!("oc-mini-pref-{}", std::process::id()));
        let repo = ModelPreferenceRepository::new(dir.join("nested/model.json"));
        let target = model("anthropic", "claude");
        repo.save_variant(&target, Some("think"));
        assert_eq!(repo.resolve_variant(&target).as_deref(), Some("think"));
        repo.save_variant(&target, None);
        // Explicit "default" survives (`model-preference.ts:63-65`).
        let raw = std::fs::read_to_string(dir.join("nested/model.json")).expect("written");
        assert!(raw.contains("\"default\""), "{raw}");
        assert_eq!(repo.resolve_variant(&target), None);
        repo.add_recent(&target);
        let document = repo.read();
        assert_eq!(document.recent.first(), Some(&target));
        let _ = std::fs::remove_dir_all(dir);
    }
}
