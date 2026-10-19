//! Config normalization + v1→v2 migration — port of `config/normalize.ts`
//! (808 LOC) and `v1/config/migrate.ts`.
//!
//! Every recognized value is decoded, canonicalized, and merged into the
//! encoded document with per-entry recovery: malformed entries produce
//! [`DiagnosticKind::Invalid`] diagnostics and are skipped, legacy/native
//! collisions produce [`DiagnosticKind::Conflict`] (native wins), and removed
//! legacy settings produce [`DiagnosticKind::Unsupported`]. Structurally
//! invalid documents (non-object root) are rejected outright.

use std::collections::BTreeMap;

use oc_schema::common::{Num, UsdPerMillionTokens};
use oc_schema::config::agent::Mode as AgentMode;
use oc_schema::config::formatter::{Entry as FormatterEntry, Formatter};
use oc_schema::config::lsp::{Entry as LspEntry, Lsp};
use oc_schema::config::media::Media;
use oc_schema::config::model::ModelSelection;
use oc_schema::config::provider::{
    Capabilities, Cost, CostCache, CostList, CostTier, CostTierType, Limit, Model, ModelSettings,
    ModelVariant, Provider, Settings,
};
use oc_schema::config::tool_output::ToolOutput;
use oc_schema::config::warming::Warming;
use oc_schema::config::websearch::WebSearchSelection;
use oc_schema::config::worktree::Worktree;
use oc_schema::config::{Enterprise, Share, Update};
use oc_schema::mcp::{
    LocalConfig, OAuthConfig, OAuthValue, RemoteConfig, ServerConfig, TimeoutConfig,
};
use oc_schema::permission::Rule as PermissionRule;

use crate::config::Json;

/// `ConfigNormalize.Diagnostic.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    Conflict,
    Invalid,
    Unsupported,
}

/// `ConfigNormalize.Diagnostic`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub path: Vec<String>,
    pub message: String,
}

/// `ConfigNormalize.Result`.
#[derive(Debug, Clone, PartialEq)]
pub enum NormalizeResult {
    Normalized { encoded: Json, diagnostics: Vec<Diagnostic> },
    Rejected { diagnostics: Vec<Diagnostic> },
}

// v2's policy action literal is "provider.use" (config/policy.ts:8);
// oc-schema's `config::policy::Action` serializes that literal and
// accepts the historical kebab-case "provider-use" spelling as an
// alias, so encoded documents decode into `Info` either way.
pub const POLICY_ACTION_PROVIDER_USE: &str = "provider.use";
pub const POLICY_ACTION_PERMISSION: &str = "permission";

const UNSUPPORTED_TOP_LEVEL: [&str; 4] = ["logLevel", "server", "subagent_depth", "layout"];
const UNSUPPORTED_EXPERIMENTAL: [&str; 5] = [
    "disable_paste_summary",
    "batch_tool",
    "openTelemetry",
    "primary_tools",
    "continue_loop_on_deny",
];
const UNSUPPORTED_PROVIDER: [&str; 3] = ["id", "whitelist", "blacklist"];
const UNSUPPORTED_MODEL: [&str; 5] =
    ["release_date", "attachment", "reasoning", "temperature", "experimental"];

/// Renames retired provider IDs (v1 fields and the shared top-level model).
fn migrate_provider_id(input: &str) -> String {
    match input {
        "azure-cognitive-services" => "azure".to_string(),
        "google-vertex-anthropic" => "google-vertex".to_string(),
        other => other.to_string(),
    }
}

/// Maps v1 permission/tool keys onto renamed v2 tool actions.
fn normalize_action(action: &str) -> String {
    match action {
        "write" | "patch" => "edit".to_string(),
        "task" => "subagent".to_string(),
        "bash" => "shell".to_string(),
        other => other.to_string(),
    }
}

fn aisdk(value: &str) -> String {
    if value.starts_with("aisdk:") {
        value.to_string()
    } else {
        format!("aisdk:{value}")
    }
}

// ---------------------------------------------------------------------------
// Small Json helpers
// ---------------------------------------------------------------------------

fn own(value: &Json, key: &str) -> bool {
    value.get(key).is_some()
}

fn is_record(value: Option<&Json>) -> bool {
    value.is_some_and(Json::is_object)
}

fn invalid(path: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        kind: DiagnosticKind::Invalid,
        path: path.iter().map(std::string::ToString::to_string).collect(),
        message: "skipped malformed recognized value".to_string(),
    });
}

fn conflict(path: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        kind: DiagnosticKind::Conflict,
        path: path.iter().map(std::string::ToString::to_string).collect(),
        message: "retained native value over legacy value".to_string(),
    });
}

/// `Vec<String>` path → `&[&str]` view for the diagnostic helpers.
fn path_refs(parts: &[String]) -> Vec<&str> {
    parts.iter().map(String::as_str).collect()
}

/// `[...base, extra]` for diagnostic paths.
/// `[...base, extra]` for owned diagnostic paths.
fn join_path_owned(base: &[String], extra: &str) -> Vec<String> {
    let mut out = base.to_vec();
    out.push(extra.to_string());
    out
}

fn join_path(base: &[&str], extra: &str) -> Vec<String> {
    base.iter().map(|item| (*item).to_string()).chain([extra.to_string()]).collect()
}

/// `[...base, extra]` viewed as `&str`s in one step.
fn join_path_refs<'a>(base: &[&'a str], extra: &'a str) -> Vec<&'a str> {
    base.iter().copied().chain([extra]).collect()
}

fn unsupported(path: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    diagnostics.push(Diagnostic {
        kind: DiagnosticKind::Unsupported,
        path: path.iter().map(std::string::ToString::to_string).collect(),
        message: "omitted unsupported legacy setting".to_string(),
    });
}

fn unsupported_if_present(
    value: &Json,
    key: &str,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if own(value, key) {
        unsupported(path, diagnostics);
    }
}

fn deep_equal(a: &Json, b: &Json) -> bool {
    a == b
}

/// `decodeValue`: decode without canonicalizing.
fn decode_value<T: serde::de::DeserializeOwned>(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<T> {
    serde_json::from_value(serde_json::Value::from(value.clone())).map_err(|_| ()).ok().or_else(
        || {
            invalid(path, diagnostics);
            None
        },
    )
}

/// `decodeEncoded`: decode then re-encode canonically.
fn decode_encoded<T: serde::de::DeserializeOwned + serde::Serialize>(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    let decoded: T = decode_value(value, path, diagnostics)?;
    serde_json::to_value(&decoded).map(Json::from).map_err(|_| ()).ok().or_else(|| {
        invalid(path, diagnostics);
        None
    })
}

fn obj(entries: Vec<(String, Json)>) -> Json {
    Json::Object(entries)
}

fn rule(action: &str, resource: &str, effect: &str) -> Json {
    obj(vec![
        ("action".to_string(), Json::String(action.to_string())),
        ("resource".to_string(), Json::String(resource.to_string())),
        ("effect".to_string(), Json::String(effect.to_string())),
    ])
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `ConfigNormalize.normalize`.
// v2 keeps this as one ordered function over the v1→v2 key table; splitting
// it would obscure the migration order the diagnostics rely on.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn normalize(input: &Json) -> NormalizeResult {
    let mut diagnostics = Vec::new();
    if !input.is_object() {
        return NormalizeResult::Rejected {
            diagnostics: vec![Diagnostic {
                kind: DiagnosticKind::Invalid,
                path: vec!["$".to_string()],
                message: "rejected configuration because its root is not an object".to_string(),
            }],
        };
    }

    let mut encoded: Vec<(String, Json)> = Vec::new();
    let set = |key: &str, value: Json, encoded: &mut Vec<(String, Json)>| {
        encoded.retain(|(name, _)| name != key);
        encoded.push((key.to_string(), value));
    };
    for key in UNSUPPORTED_TOP_LEVEL {
        unsupported_if_present(input, key, &[key], &mut diagnostics);
    }

    // --- snapshots / update / share / media -------------------------------
    let legacy_snapshots = own(input, "snapshot")
        .then(|| {
            decode_encoded::<bool>(input.get("snapshot").unwrap(), &["snapshot"], &mut diagnostics)
        })
        .flatten();
    let legacy_update = if own(input, "autoupdate") {
        decode_value::<LegacyAutoupdate>(
            input.get("autoupdate").unwrap(),
            &["autoupdate"],
            &mut diagnostics,
        )
    } else {
        None
    };
    let native_update = if own(input, "update") {
        decode_encoded::<Update>(input.get("update").unwrap(), &["update"], &mut diagnostics)
    } else {
        None
    };
    let legacy_share = if own(input, "autoshare") {
        decode_value::<bool>(input.get("autoshare").unwrap(), &["autoshare"], &mut diagnostics)
            .filter(|value| *value)
            .map(|_| Json::String("auto".to_string()))
    } else {
        None
    };
    let legacy_media = if own(input, "attachment") {
        decode_encoded::<Media>(input.get("attachment").unwrap(), &["attachment"], &mut diagnostics)
    } else {
        None
    };
    if let Some(media) = legacy_media {
        set("media", media, &mut encoded);
    }
    if let Some(snapshots) = legacy_snapshots {
        set("snapshots", snapshots, &mut encoded);
    }
    let migrated_update = match legacy_update {
        Some(LegacyAutoupdate::Bool(false)) => Some(Json::String("disable".to_string())),
        Some(LegacyAutoupdate::Notify) => Some(Json::String("notify".to_string())),
        Some(LegacyAutoupdate::Bool(true)) => Some(Json::String("auto".to_string())),
        None => None,
    };
    let update = prefer(migrated_update, native_update, &["update"], &mut diagnostics);
    if let Some(update) = update {
        set("update", update, &mut encoded);
    }
    if let Some(share) = legacy_share {
        set("share", share, &mut encoded);
    }

    // --- references --------------------------------------------------------
    let legacy_references = decode_map(
        input.get("reference"),
        &["reference"],
        &mut diagnostics,
        decode_reference_entry,
    );
    let native_references = decode_map(
        input.get("references"),
        &["references"],
        &mut diagnostics,
        decode_reference_entry,
    );
    merge_map(
        &mut encoded,
        "references",
        legacy_references,
        native_references,
        is_record(input.get("reference")) || is_record(input.get("references")),
        &mut diagnostics,
    );

    // --- commands ----------------------------------------------------------
    let legacy_commands =
        decode_map(input.get("command"), &["command"], &mut diagnostics, decode_v1_command);
    diagnose_selection_map(input.get("command"), &["command"], &mut diagnostics);
    let migrated_commands: Vec<(String, Json)> = legacy_commands
        .into_iter()
        .map(|(name, command)| {
            (name, Json::from(serde_json::to_value(&command).unwrap_or_default()))
        })
        .collect();
    let native_commands =
        decode_map(input.get("commands"), &["commands"], &mut diagnostics, decode_command_entry);
    merge_map(
        &mut encoded,
        "commands",
        migrated_commands,
        native_commands,
        is_record(input.get("command")) || is_record(input.get("commands")),
        &mut diagnostics,
    );

    // --- agents (agent + mode + small_model + agents) ----------------------
    let mut legacy_agents: Vec<(String, Json)> =
        decode_map(input.get("agent"), &["agent"], &mut diagnostics, |value, path, diagnostics| {
            decode_v1_agent(value, path, diagnostics).map(|v1| migrate_agent_document(&v1))
        });
    let legacy_small_model = if own(input, "small_model") {
        decode_value::<String>(
            input.get("small_model").unwrap(),
            &["small_model"],
            &mut diagnostics,
        )
    } else {
        None
    };
    let migrated_small_model = legacy_small_model.as_deref().and_then(migrate_model_selection);
    if legacy_small_model.is_some() && migrated_small_model.is_none() {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Unsupported,
            path: vec!["small_model".to_string()],
            message: "omitted unsupported legacy model reference".to_string(),
        });
    }
    if let Some(small) = &migrated_small_model {
        // `{ model: small, ...title }` — the title entry keeps its own model.
        match legacy_agents.iter_mut().find(|(name, _)| name == "title") {
            Some((_, Json::Object(fields))) if !fields.iter().any(|(name, _)| name == "model") => {
                fields.insert(0, ("model".to_string(), small.clone()));
            }
            Some(_) => {}
            None => legacy_agents
                .push(("title".to_string(), obj(vec![("model".to_string(), small.clone())]))),
        }
    }
    let mode_agents: Vec<(String, Json)> =
        decode_map(input.get("mode"), &["mode"], &mut diagnostics, |value, path, diagnostics| {
            decode_v1_agent(value, path, diagnostics).map(|mut v1| {
                if let Json::Object(fields) = &mut v1 {
                    fields.retain(|(name, _)| name != "mode");
                    fields.push(("mode".to_string(), Json::String("primary".to_string())));
                }
                migrate_agent_document(&v1)
            })
        });
    let migrated_agents = merge_maps(legacy_agents, mode_agents, &["agents"], &mut diagnostics);
    let native_agents =
        decode_map(input.get("agents"), &["agents"], &mut diagnostics, decode_agent_entry);
    diagnose_agent_unsupported(input.get("agent"), &["agent"], &mut diagnostics);
    diagnose_agent_unsupported(input.get("mode"), &["mode"], &mut diagnostics);
    merge_map(
        &mut encoded,
        "agents",
        migrated_agents,
        native_agents,
        migrated_small_model.is_some()
            || is_record(input.get("agent"))
            || is_record(input.get("mode"))
            || is_record(input.get("agents")),
        &mut diagnostics,
    );

    // --- providers ---------------------------------------------------------
    let legacy_providers = migrate_providers(input.get("provider"), &mut diagnostics);
    let native_providers =
        decode_map(input.get("providers"), &["providers"], &mut diagnostics, decode_provider_entry);
    merge_map(
        &mut encoded,
        "providers",
        legacy_providers,
        native_providers,
        is_record(input.get("provider")) || is_record(input.get("providers")),
        &mut diagnostics,
    );

    // --- permissions -------------------------------------------------------
    let tool_rules = migrate_tools(input.get("tools"), &mut diagnostics);
    let permission_rules = migrate_permissions(input.get("permission"), &mut diagnostics);
    let native_permissions: Vec<Json> = decode_list(
        input.get("permissions"),
        &["permissions"],
        &mut diagnostics,
        |value, path, diagnostics| {
            decode_value::<PermissionRule>(value, path, diagnostics)
                .map(|rule| Json::from(serde_json::to_value(&rule).unwrap_or_default()))
        },
    );
    let permissions: Vec<Json> =
        tool_rules.into_iter().chain(permission_rules).chain(native_permissions).collect();
    if !permissions.is_empty()
        || input.get("permissions").is_some_and(|value| value.as_array().is_some())
    {
        set("permissions", Json::Array(permissions), &mut encoded);
    }

    // --- plugins -----------------------------------------------------------
    let legacy_plugins: Vec<Json> =
        decode_list(input.get("plugin"), &["plugin"], &mut diagnostics, decode_v1_plugin);
    let native_plugins: Vec<Json> = decode_list(
        input.get("plugins"),
        &["plugins"],
        &mut diagnostics,
        |value, path, diagnostics| {
            decode_value::<oc_schema::config::Plugin>(value, path, diagnostics)
                .map(|plugin| Json::from(serde_json::to_value(&plugin).unwrap_or_default()))
        },
    );
    if !legacy_plugins.is_empty()
        || !native_plugins.is_empty()
        || input.get("plugin").is_some_and(|value| value.as_array().is_some())
        || input.get("plugins").is_some_and(|value| value.as_array().is_some())
    {
        set(
            "plugins",
            Json::Array(legacy_plugins.into_iter().chain(native_plugins).collect()),
            &mut encoded,
        );
    }

    normalize_skills(input, &mut encoded, &mut diagnostics);
    normalize_mcp(input, &mut encoded, &mut diagnostics);
    normalize_compaction(input, &mut encoded, &mut diagnostics);
    normalize_experimental(input, &mut encoded, &mut diagnostics);
    normalize_watcher(input, &mut encoded, &mut diagnostics);
    normalize_formatter(input, &mut encoded, &mut diagnostics);
    normalize_lsp(input, &mut encoded, &mut diagnostics);

    // --- model (with provider id renames) ----------------------------------
    if own(input, "model") {
        if let Some(model) = decode_value::<ModelSelection>(
            input.get("model").unwrap(),
            &["model"],
            &mut diagnostics,
        ) {
            let mut value = serde_json::to_value(&model).unwrap_or_default();
            if let Some(provider) =
                value.get_mut("providerID").and_then(|id| id.as_str().map(str::to_string))
            {
                value["providerID"] = serde_json::Value::String(migrate_provider_id(&provider));
            }
            set("model", Json::from(value), &mut encoded);
        }
    }

    // --- native atomic fields ----------------------------------------------
    let schema_ref = decode_atomic::<String>(input, "$schema", &mut diagnostics)
        .map(|value| ("$schema".to_string(), value));
    if let Some((key, value)) = schema_ref {
        let path = [key.as_str()];
        overlay(&mut encoded, key.clone(), value, &path, &mut diagnostics);
    }
    for key in ["shell", "default_agent"] {
        if !own(input, key) {
            continue;
        }
        let value = decode_value::<String>(input.get(key).unwrap(), &[key], &mut diagnostics);
        if let Some(value) = value {
            overlay(&mut encoded, key.to_string(), Json::String(value), &[key], &mut diagnostics);
        }
    }
    if own(input, "share") {
        if let Some(value) =
            decode_encoded::<Share>(input.get("share").unwrap(), &["share"], &mut diagnostics)
        {
            overlay(&mut encoded, "share".to_string(), value, &["share"], &mut diagnostics);
        }
    }
    if own(input, "enterprise") {
        if let Some(value) = decode_encoded::<Enterprise>(
            input.get("enterprise").unwrap(),
            &["enterprise"],
            &mut diagnostics,
        ) {
            overlay(
                &mut encoded,
                "enterprise".to_string(),
                value,
                &["enterprise"],
                &mut diagnostics,
            );
        }
    }
    if own(input, "username") {
        if let Some(value) =
            decode_value::<String>(input.get("username").unwrap(), &["username"], &mut diagnostics)
        {
            overlay(
                &mut encoded,
                "username".to_string(),
                Json::String(value),
                &["username"],
                &mut diagnostics,
            );
        }
    }
    if own(input, "snapshots") {
        if let Some(value) = decode_encoded::<bool>(
            input.get("snapshots").unwrap(),
            &["snapshots"],
            &mut diagnostics,
        ) {
            overlay(&mut encoded, "snapshots".to_string(), value, &["snapshots"], &mut diagnostics);
        }
    }
    if own(input, "media") {
        if let Some(value) =
            decode_encoded::<Media>(input.get("media").unwrap(), &["media"], &mut diagnostics)
        {
            overlay(&mut encoded, "media".to_string(), value, &["media"], &mut diagnostics);
        }
    }
    if own(input, "tool_output") {
        if let Some(value) = decode_encoded::<ToolOutput>(
            input.get("tool_output").unwrap(),
            &["tool_output"],
            &mut diagnostics,
        ) {
            overlay(
                &mut encoded,
                "tool_output".to_string(),
                value,
                &["tool_output"],
                &mut diagnostics,
            );
        }
    }
    if own(input, "websearch") {
        if let Some(value) = decode_encoded::<WebSearchSelection>(
            input.get("websearch").unwrap(),
            &["websearch"],
            &mut diagnostics,
        ) {
            overlay(&mut encoded, "websearch".to_string(), value, &["websearch"], &mut diagnostics);
        }
    }
    if own(input, "worktree") {
        if let Some(value) = decode_encoded::<Worktree>(
            input.get("worktree").unwrap(),
            &["worktree"],
            &mut diagnostics,
        ) {
            overlay(&mut encoded, "worktree".to_string(), value, &["worktree"], &mut diagnostics);
        }
    }
    if own(input, "warming") {
        if let Some(value) =
            decode_encoded::<Warming>(input.get("warming").unwrap(), &["warming"], &mut diagnostics)
        {
            overlay(&mut encoded, "warming".to_string(), value, &["warming"], &mut diagnostics);
        }
    }

    // --- instructions ------------------------------------------------------
    let instructions: Vec<Json> = decode_list(
        input.get("instructions"),
        &["instructions"],
        &mut diagnostics,
        |value, path, diagnostics| {
            decode_value::<String>(value, path, diagnostics).map(Json::String)
        },
    );
    if !instructions.is_empty()
        || input.get("instructions").is_some_and(|value| value.as_array().is_some())
    {
        set("instructions", Json::Array(instructions), &mut encoded);
    }

    NormalizeResult::Normalized { encoded: obj(encoded), diagnostics }
}

fn decode_atomic<T: serde::de::DeserializeOwned + serde::Serialize>(
    input: &Json,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    if !own(input, key) {
        return None;
    }
    decode_encoded::<T>(input.get(key).unwrap(), &[key], diagnostics)
}

#[derive(Debug, Clone, Copy)]
enum LegacyAutoupdate {
    Bool(bool),
    Notify,
}

impl<'de> serde::Deserialize<'de> for LegacyAutoupdate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Bool(value) => Ok(LegacyAutoupdate::Bool(value)),
            serde_json::Value::String(value) if value == "notify" => Ok(LegacyAutoupdate::Notify),
            _ => Err(serde::de::Error::custom("expected boolean or \"notify\"")),
        }
    }
}

// ---------------------------------------------------------------------------
// Map / list decode plumbing
// ---------------------------------------------------------------------------

fn decode_map(
    value: Option<&Json>,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
    decode: impl Fn(&Json, &[&str], &mut Vec<Diagnostic>) -> Option<Json>,
) -> Vec<(String, Json)> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(entries) = value.as_object() else {
        invalid(path, diagnostics);
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(name, raw)| {
            let entry_path = join_path(path, name);
            let refs: Vec<&str> = entry_path.iter().map(String::as_str).collect();
            decode(raw, &refs, diagnostics).map(|decoded| (name.clone(), decoded))
        })
        .collect()
}

fn decode_list(
    value: Option<&Json>,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
    decode: impl Fn(&Json, &[&str], &mut Vec<Diagnostic>) -> Option<Json>,
) -> Vec<Json> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        invalid(path, diagnostics);
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let entry_path = join_path(path, &index.to_string());
            let refs: Vec<&str> = entry_path.iter().map(String::as_str).collect();
            decode(item, &refs, diagnostics)
        })
        .collect()
}

fn merge_map(
    encoded: &mut Vec<(String, Json)>,
    key: &str,
    legacy: Vec<(String, Json)>,
    native: Vec<(String, Json)>,
    present: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let merged = merge_maps(legacy, native, &[key], diagnostics);
    if present {
        encoded.retain(|(name, _)| name != key);
        encoded.push((key.to_string(), obj(merged)));
    }
}

fn merge_maps(
    legacy: Vec<(String, Json)>,
    native: Vec<(String, Json)>,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(String, Json)> {
    let mut result = legacy;
    for (name, value) in native {
        if let Some((_, existing)) = result.iter_mut().find(|(existing, _)| *existing == name) {
            if !deep_equal(existing, &value) {
                let full = join_path(path, &name);
                conflict(&path_refs(&full), diagnostics);
            }
        }
        result.retain(|(existing, _)| *existing != name);
        result.push((name, value));
    }
    result
}

fn overlay(
    encoded: &mut Vec<(String, Json)>,
    key: String,
    value: Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some((_, existing)) = encoded.iter_mut().find(|(name, _)| *name == key) {
        if !deep_equal(existing, &value) {
            conflict(path, diagnostics);
        }
    }
    encoded.retain(|(name, _)| *name != key);
    encoded.push((key, value));
}

fn prefer(
    legacy: Option<Json>,
    native: Option<Json>,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    match native {
        None => legacy,
        Some(native) => {
            if let Some(legacy) = legacy.filter(|legacy| !deep_equal(legacy, &native)) {
                let _ = legacy;
                conflict(path, diagnostics);
            }
            Some(native)
        }
    }
}

// ---------------------------------------------------------------------------
// Field sections
// ---------------------------------------------------------------------------

fn normalize_skills(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(skills) = input.get("skills") else {
        return;
    };
    if skills.as_array().is_some() {
        let decoded =
            decode_list(Some(skills), &["skills"], diagnostics, |value, path, diagnostics| {
                decode_value::<String>(value, path, diagnostics).map(Json::String)
            });
        encoded.retain(|(name, _)| name != "skills");
        encoded.push(("skills".to_string(), Json::Array(decoded)));
        return;
    }
    if !skills.is_object() {
        invalid(&["skills"], diagnostics);
        return;
    }
    let paths = decode_list(
        skills.get("paths"),
        &["skills", "paths"],
        diagnostics,
        |value, path, diagnostics| {
            decode_value::<String>(value, path, diagnostics).map(Json::String)
        },
    );
    let urls = decode_list(
        skills.get("urls"),
        &["skills", "urls"],
        diagnostics,
        |value, path, diagnostics| {
            decode_value::<String>(value, path, diagnostics).map(Json::String)
        },
    );
    encoded.retain(|(name, _)| name != "skills");
    encoded.push(("skills".to_string(), Json::Array(paths.into_iter().chain(urls).collect())));
}

fn normalize_mcp(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut legacy_servers: Vec<(String, Json)> = Vec::new();
    let mut native_servers: Vec<(String, Json)> = Vec::new();
    let mut timeout: Vec<(String, Json)> = Vec::new();

    if is_record(input.get("experimental"))
        && own(input.get("experimental").unwrap(), "mcp_timeout")
    {
        let value = input.get("experimental").unwrap().get("mcp_timeout").unwrap();
        if let Some(value) =
            decode_value::<u64>(value, &["experimental", "mcp_timeout"], diagnostics)
        {
            let number = Json::Number(serde_json::Number::from(value));
            timeout.push(("catalog".to_string(), number.clone()));
            timeout.push(("execution".to_string(), number));
        }
    }

    if own(input, "mcp") {
        let mcp = input.get("mcp").unwrap();
        if !is_record(Some(mcp)) {
            invalid(&["mcp"], diagnostics);
        }
        if let Some(entries) = mcp.as_object() {
            for (name, value) in entries {
                let path = vec!["mcp".to_string(), name.clone()];
                let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                if is_enabled_only_mcp(value) {
                    diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::Unsupported,
                        path,
                        message: "omitted enabled-only legacy MCP entry".to_string(),
                    });
                    continue;
                }
                if name == "servers" && !is_direct_legacy_mcp(value) {
                    for (key, server) in
                        decode_map(Some(value), &refs, diagnostics, decode_mcp_server)
                    {
                        set_map_entry(&mut native_servers, key, server);
                    }
                    continue;
                }
                if name == "timeout" && !is_direct_legacy_mcp(value) {
                    normalize_mcp_timeout(value, &mut timeout, &refs, diagnostics);
                    continue;
                }
                if let Some(server) = decode_v1_mcp(value, &refs, diagnostics) {
                    set_map_entry(&mut legacy_servers, name.clone(), server);
                }
            }
        }
    }

    let servers = merge_maps(legacy_servers, native_servers, &["mcp", "servers"], diagnostics);
    if servers.is_empty() && timeout.is_empty() {
        if is_record(input.get("mcp"))
            && input.get("mcp").unwrap().as_object().is_some_and(<[(String, Json)]>::is_empty)
        {
            encoded.retain(|(name, _)| name != "mcp");
            encoded.push(("mcp".to_string(), obj(Vec::new())));
        }
        return;
    }
    let mut value: Vec<(String, Json)> = Vec::new();
    if !timeout.is_empty() {
        value.push(("timeout".to_string(), obj(timeout)));
    }
    if !servers.is_empty() {
        value.push(("servers".to_string(), obj(servers)));
    }
    encoded.retain(|(name, _)| name != "mcp");
    encoded.push(("mcp".to_string(), obj(value)));
}

fn set_map_entry(map: &mut Vec<(String, Json)>, key: String, value: Json) {
    map.retain(|(name, _)| *name != key);
    map.push((key, value));
}

fn normalize_mcp_timeout(
    value: &Json,
    timeout: &mut Vec<(String, Json)>,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(entries) = value.as_object() else {
        invalid(path, diagnostics);
        return;
    };
    let recognized: Vec<(&String, &Json)> = entries
        .iter()
        .filter(|(key, _)| matches!(key.as_str(), "startup" | "catalog" | "execution"))
        .map(|(key, value)| (key, value))
        .collect();
    if !entries.is_empty() && recognized.is_empty() {
        invalid(path, diagnostics);
        return;
    }
    for (key, leaf) in recognized {
        let leaf_path = join_path(path, key);
        let refs: Vec<&str> = leaf_path.iter().map(String::as_str).collect();
        if let Some(decoded) = decode_value::<u64>(leaf, &refs, diagnostics) {
            let new = Json::Number(serde_json::Number::from(decoded));
            // overlay: replacing an existing leaf (e.g. experimental
            // mcp_timeout) with a different value is a conflict.
            if let Some((_, existing)) = timeout.iter_mut().find(|(name, _)| name == key) {
                if existing != &new {
                    conflict(&refs, diagnostics);
                }
            }
            set_map_entry(timeout, key.clone(), new);
        }
    }
    // Schema field order: startup, catalog, execution.
    timeout.sort_by_key(|(key, _)| match key.as_str() {
        "startup" => 0,
        "catalog" => 1,
        _ => 2,
    });
}

fn is_direct_legacy_mcp(value: &Json) -> bool {
    value.get("type").and_then(Json::as_str).is_some_and(|kind| kind == "local" || kind == "remote")
}

fn is_enabled_only_mcp(value: &Json) -> bool {
    value.is_object()
        && value.get("type").is_none()
        && value.get("enabled").and_then(Json::as_bool).is_some()
}

fn normalize_compaction(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(compaction) = input.get("compaction") else {
        return;
    };
    let Some(entries) = compaction.as_object() else {
        invalid(&["compaction"], diagnostics);
        return;
    };
    unsupported_if_present(compaction, "tail_turns", &["compaction", "tail_turns"], diagnostics);
    unsupported_if_present(compaction, "prune", &["compaction", "prune"], diagnostics);
    let mut result: Vec<(String, Json)> = Vec::new();

    if own(compaction, "auto") {
        if let Some(value) = decode_value::<bool>(
            compaction.get("auto").unwrap(),
            &["compaction", "auto"],
            diagnostics,
        ) {
            result.push(("auto".to_string(), Json::Bool(value)));
        }
    }
    let legacy_tokens = if own(compaction, "preserve_recent_tokens") {
        decode_value::<u64>(
            compaction.get("preserve_recent_tokens").unwrap(),
            &["compaction", "preserve_recent_tokens"],
            diagnostics,
        )
    } else {
        None
    };
    let native_keep = compaction.get("keep").filter(|keep| keep.is_object());
    if own(compaction, "keep") && native_keep.is_none() {
        invalid(&["compaction", "keep"], diagnostics);
    }
    let native_tokens = native_keep.and_then(|keep| keep.get("tokens")).and_then(|tokens| {
        decode_value::<u64>(tokens, &["compaction", "keep", "tokens"], diagnostics)
    });
    let tokens = prefer(
        legacy_tokens.map(|value| Json::Number(serde_json::Number::from(value))),
        native_tokens.map(|value| Json::Number(serde_json::Number::from(value))),
        &["compaction", "keep", "tokens"],
        diagnostics,
    );
    if let Some(tokens) = tokens {
        result.push(("keep".to_string(), obj(vec![("tokens".to_string(), tokens)])));
    }
    let legacy_buffer = if own(compaction, "reserved") {
        decode_value::<u64>(
            compaction.get("reserved").unwrap(),
            &["compaction", "reserved"],
            diagnostics,
        )
    } else {
        None
    };
    let native_buffer = if own(compaction, "buffer") {
        decode_value::<u64>(
            compaction.get("buffer").unwrap(),
            &["compaction", "buffer"],
            diagnostics,
        )
    } else {
        None
    };
    let buffer = prefer(
        legacy_buffer.map(|value| Json::Number(serde_json::Number::from(value))),
        native_buffer.map(|value| Json::Number(serde_json::Number::from(value))),
        &["compaction", "buffer"],
        diagnostics,
    );
    if let Some(buffer) = buffer {
        result.push(("buffer".to_string(), buffer));
    }
    if !result.is_empty() || entries.is_empty() {
        encoded.retain(|(name, _)| name != "compaction");
        encoded.push(("compaction".to_string(), obj(result)));
    }
}

fn normalize_experimental(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut result: Vec<(String, Json)> = Vec::new();
    let mut generated: Vec<Json> = Vec::new();

    let enabled = decode_provider_list(input, "enabled_providers", diagnostics);
    if enabled.present && (!enabled.non_empty || !enabled.values.is_empty()) {
        generated.push(rule(POLICY_ACTION_PROVIDER_USE, "*", "deny"));
        generated.extend(enabled.values.iter().map(|resource| {
            rule(POLICY_ACTION_PROVIDER_USE, &migrate_provider_id(resource), "allow")
        }));
    }
    let disabled = decode_provider_list(input, "disabled_providers", diagnostics);
    generated.extend(
        disabled.values.iter().map(|resource| {
            rule(POLICY_ACTION_PROVIDER_USE, &migrate_provider_id(resource), "deny")
        }),
    );

    let mut native: Vec<Json> = Vec::new();
    if own(input, "experimental") {
        let experimental = input.get("experimental").unwrap();
        if !is_record(Some(experimental)) {
            invalid(&["experimental"], diagnostics);
        }
        if let Some(entries) = experimental.as_object() {
            for key in UNSUPPORTED_EXPERIMENTAL {
                unsupported_if_present(experimental, key, &["experimental", key], diagnostics);
            }
            let _ = entries;
            if own(experimental, "portable_shell_scanner") {
                if let Some(value) = decode_value::<bool>(
                    experimental.get("portable_shell_scanner").unwrap(),
                    &["experimental", "portable_shell_scanner"],
                    diagnostics,
                ) {
                    result.push(("portable_shell_scanner".to_string(), Json::Bool(value)));
                }
            }
            if let Some(value) = experimental.get("subagent_depth") {
                if let Some(value) =
                    decode_value::<u64>(value, &["experimental", "subagent_depth"], diagnostics)
                {
                    result.push((
                        "subagent_depth".to_string(),
                        Json::Number(serde_json::Number::from(value)),
                    ));
                }
            }
            native.extend(decode_list(
                experimental.get("policies"),
                &["experimental", "policies"],
                diagnostics,
                |value, path, diagnostics| {
                    decode_value::<LenientPolicy>(value, path, diagnostics)
                        .map(|policy| Json::from(serde_json::to_value(&policy).unwrap_or_default()))
                },
            ));
        }
    }
    if !generated.is_empty()
        || !native.is_empty()
        || (is_record(input.get("experimental"))
            && input
                .get("experimental")
                .unwrap()
                .get("policies")
                .is_some_and(|value| value.as_array().is_some()))
    {
        result.push((
            "policies".to_string(),
            Json::Array(generated.into_iter().chain(native).collect()),
        ));
    }
    if !result.is_empty()
        || (is_record(input.get("experimental"))
            && input
                .get("experimental")
                .unwrap()
                .as_object()
                .is_some_and(<[(String, Json)]>::is_empty))
    {
        encoded.retain(|(name, _)| name != "experimental");
        encoded.push(("experimental".to_string(), obj(result)));
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LenientPolicy {
    action: String,
    resource: String,
    effect: String,
}

fn decode_provider_list(
    input: &Json,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> ProviderList {
    if !own(input, key) {
        return ProviderList { present: false, non_empty: false, values: Vec::new() };
    }
    let value = input.get(key).unwrap();
    if !value.is_array() {
        invalid(&[key], diagnostics);
        return ProviderList { present: true, non_empty: true, values: Vec::new() };
    }
    let non_empty = !value.as_array().is_some_and(<[Json]>::is_empty);
    let values = decode_list(Some(value), &[key], diagnostics, |item, path, diagnostics| {
        decode_value::<String>(item, path, diagnostics).map(Json::String)
    })
    .into_iter()
    .map(|item| item.as_str().unwrap_or_default().to_string())
    .collect();
    ProviderList { present: true, non_empty, values }
}

struct ProviderList {
    present: bool,
    non_empty: bool,
    values: Vec<String>,
}

fn normalize_watcher(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(watcher) = input.get("watcher") else {
        return;
    };
    let Some(entries) = watcher.as_object() else {
        invalid(&["watcher"], diagnostics);
        return;
    };
    let ignore = decode_list(
        watcher.get("ignore"),
        &["watcher", "ignore"],
        diagnostics,
        |value, path, diagnostics| {
            decode_value::<String>(value, path, diagnostics).map(Json::String)
        },
    );
    let value = if !ignore.is_empty()
        || watcher.get("ignore").is_some_and(|value| value.as_array().is_some())
    {
        obj(vec![("ignore".to_string(), Json::Array(ignore))])
    } else {
        obj(Vec::new())
    };
    let _ = entries;
    encoded.retain(|(name, _)| name != "watcher");
    encoded.push(("watcher".to_string(), value));
}

fn normalize_formatter(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(formatter) = input.get("formatter") else {
        return;
    };
    if formatter.as_bool().is_some() {
        if let Some(value) = decode_encoded::<Formatter>(formatter, &["formatter"], diagnostics) {
            encoded.retain(|(name, _)| name != "formatter");
            encoded.push(("formatter".to_string(), value));
        }
        return;
    }
    let entries =
        decode_map(Some(formatter), &["formatter"], diagnostics, |value, path, diagnostics| {
            decode_value::<FormatterEntry>(value, path, diagnostics)
                .map(|entry| Json::from(serde_json::to_value(&entry).unwrap_or_default()))
        });
    if formatter.is_object()
        && (formatter.as_object().is_some_and(<[(String, Json)]>::is_empty) || !entries.is_empty())
    {
        encoded.retain(|(name, _)| name != "formatter");
        encoded.push(("formatter".to_string(), obj(entries)));
    }
}

fn normalize_lsp(
    input: &Json,
    encoded: &mut Vec<(String, Json)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(lsp) = input.get("lsp") else {
        return;
    };
    if lsp.as_bool().is_some() {
        if let Some(value) = decode_encoded::<Lsp>(lsp, &["lsp"], diagnostics) {
            encoded.retain(|(name, _)| name != "lsp");
            encoded.push(("lsp".to_string(), value));
        }
        return;
    }
    let entries = decode_map(Some(lsp), &["lsp"], diagnostics, |value, path, diagnostics| {
        decode_value::<LspEntry>(value, path, diagnostics)
            .map(|entry| Json::from(serde_json::to_value(&entry).unwrap_or_default()))
    });
    if lsp.is_object()
        && (lsp.as_object().is_some_and(<[(String, Json)]>::is_empty) || !entries.is_empty())
    {
        encoded.retain(|(name, _)| name != "lsp");
        encoded.push(("lsp".to_string(), obj(entries)));
    }
}

// ---------------------------------------------------------------------------
// v1 migrations
// ---------------------------------------------------------------------------

fn decode_reference_entry(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    decode_encoded::<oc_schema::config::reference::Entry>(value, path, diagnostics)
}

fn decode_command_entry(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    decode_value::<oc_schema::config::Command>(value, path, diagnostics)
        .map(|command| Json::from(serde_json::to_value(&command).unwrap_or_default()))
}

fn decode_agent_entry(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    decode_value::<oc_schema::config::Agent>(value, path, diagnostics)
        .map(|agent| Json::from(serde_json::to_value(&agent).unwrap_or_default()))
}

fn decode_provider_entry(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    decode_value::<Provider>(value, path, diagnostics)
        .map(|provider| Json::from(serde_json::to_value(&provider).unwrap_or_default()))
}

fn decode_mcp_server(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    decode_value::<ServerConfig>(value, path, diagnostics)
        .map(|server| Json::from(serde_json::to_value(&server).unwrap_or_default()))
}

/// `ConfigMigrateV1.modelSelection`: `provider/model` (+ variant) → explicit
/// selection (`^[^/#]+/[^#]+$`); invalid references become `None`.
fn migrate_model_selection(input: &str) -> Option<Json> {
    // `provider/model[#variant]`; the base must satisfy ^[^/#]+/[^#]+$.
    if let Some((base, variant)) = input.split_once('#') {
        if variant.is_empty() || variant.contains('#') {
            return None;
        }
        let mut selection = migrate_model_selection(base)?;
        selection.set("variant", Json::String(variant.to_string()));
        return Some(selection);
    }
    let (provider, model) = input.split_once('/')?;
    if provider.is_empty() || model.is_empty() || provider.contains('#') || model.contains('#') {
        return None;
    }
    Some(obj(vec![
        ("providerID".to_string(), Json::String(migrate_provider_id(provider))),
        ("model".to_string(), Json::String(model.to_string())),
    ]))
}

fn diagnose_selection(value: &Json, path: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    let model = value.get("model").and_then(Json::as_str);
    // v2 selection regex: ^[^/#]+/[^#]+$
    let model_valid = model.is_some_and(|model| match model.split_once('/') {
        Some((provider, rest)) => {
            !provider.is_empty()
                && !rest.is_empty()
                && !provider.contains('#')
                && !rest.contains('#')
        }
        None => false,
    });
    if model.is_some() && !model_valid {
        diagnostics.push(Diagnostic {
            kind: DiagnosticKind::Unsupported,
            path: join_path(path, "model"),
            message: "omitted unsupported legacy model reference".to_string(),
        });
    }
    let variant = value.get("variant").and_then(Json::as_str);
    if let Some(variant) = variant {
        if !model_valid || variant.is_empty() || variant.contains('#') {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::Unsupported,
                path: join_path(path, "variant"),
                message: "omitted unsupported legacy model variant".to_string(),
            });
        }
    }
}

fn diagnose_selection_map(value: Option<&Json>, path: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    if let Some(entries) = value.and_then(Json::as_object) {
        for (name, entry) in entries {
            if entry.is_object() {
                let entry_path = join_path(path, name);
                let refs: Vec<&str> = entry_path.iter().map(String::as_str).collect();
                diagnose_selection(entry, &refs, diagnostics);
            }
        }
    }
}

fn diagnose_agent_unsupported(
    value: Option<&Json>,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(entries) = value.and_then(Json::as_object) else {
        return;
    };
    for (name, agent) in entries {
        let Some(fields) = agent.as_object() else {
            continue;
        };
        let agent_path = join_path(path, name);
        let agent_refs: Vec<&str> = agent_path.iter().map(String::as_str).collect();
        if agent.get("name").is_some() {
            unsupported(&path_refs(&join_path(&agent_refs, "name")), diagnostics);
        }
        let _ = fields;
        diagnose_selection(agent, &agent_refs, diagnostics);
    }
}

fn migrate_tools(value: Option<&Json>, diagnostics: &mut Vec<Diagnostic>) -> Vec<Json> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(entries) = value.as_object() else {
        invalid(&["tools"], diagnostics);
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|(action, raw)| {
            let path = join_path(&["tools"], action);
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            let enabled = decode_value::<bool>(raw, &refs, diagnostics)?;
            Some(rule(&normalize_action(action), "*", if enabled { "allow" } else { "deny" }))
        })
        .collect()
}

fn migrate_permissions(value: Option<&Json>, diagnostics: &mut Vec<Diagnostic>) -> Vec<Json> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Some(action) = value.as_str() {
        return match decode_v1_action(action, &["permission"], diagnostics) {
            Some(effect) => vec![rule("*", "*", &effect)],
            None => Vec::new(),
        };
    }
    let Some(entries) = value.as_object() else {
        invalid(&["permission"], diagnostics);
        return Vec::new();
    };
    let mut out = Vec::new();
    for (action, raw) in entries {
        let action_path = join_path(&["permission"], action);
        let refs: Vec<&str> = action_path.iter().map(String::as_str).collect();
        if let Some(effect) = raw.as_str() {
            if let Some(effect) = decode_v1_action(effect, &refs, diagnostics) {
                out.push(rule(&normalize_action(action), "*", &effect));
            }
            continue;
        }
        let Some(resources) = raw.as_object() else {
            invalid(&refs, diagnostics);
            continue;
        };
        for (index, (resource, effect)) in resources.iter().enumerate() {
            let index_path = join_path(&refs, &index.to_string());
            let index_refs: Vec<&str> = index_path.iter().map(String::as_str).collect();
            if let Some(effect) = effect
                .as_str()
                .and_then(|effect| decode_v1_action(effect, &index_refs, diagnostics))
            {
                out.push(rule(&normalize_action(action), resource, &effect));
            } else if effect.as_str().is_some() {
                // decode_v1_action already logged the invalid diagnostic.
            } else {
                invalid(&index_refs, diagnostics);
            }
        }
    }
    out
}

fn decode_v1_action(
    value: &str,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    if matches!(value, "ask" | "allow" | "deny") {
        Some(value.to_string())
    } else {
        invalid(path, diagnostics);
        None
    }
}

fn decode_v1_plugin(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    if value.as_str().is_some() {
        return decode_value::<String>(value, path, diagnostics).map(Json::String);
    }
    if let Some(items) = value.as_array() {
        if items.len() == 2 {
            let package = decode_value::<String>(&items[0], path, diagnostics)?;
            let options = items[1].clone();
            return Some(obj(vec![
                ("package".to_string(), Json::String(package)),
                ("options".to_string(), options.plain()),
            ]));
        }
    }
    invalid(path, diagnostics);
    None
}

// --- v1 agents --------------------------------------------------------------

const V1_AGENT_KNOWN_KEYS: [&str; 13] = [
    "model",
    "variant",
    "temperature",
    "top_p",
    "prompt",
    "tools",
    "disable",
    "description",
    "mode",
    "hidden",
    "options",
    "color",
    "steps",
];

/// Decodes a v1 agent applying the v1 `normalize` transform: unknown keys move
/// into `options`, `tools` booleans fold into `permission` (edit/write/patch
/// collapse), and `maxSteps` falls back into `steps`.
// The v1 normalize transform in one function, mirroring `agent.ts`.
#[allow(clippy::too_many_lines)]
fn decode_v1_agent(value: &Json, path: &[&str], diagnostics: &mut Vec<Diagnostic>) -> Option<Json> {
    let Some(entries) = value.as_object() else {
        invalid(path, diagnostics);
        return None;
    };
    let mut options: Vec<(String, Json)> = Vec::new();
    let mut fields: Vec<(String, Json)> = Vec::new();
    for (key, field) in entries {
        let known = key == "name"
            || V1_AGENT_KNOWN_KEYS.contains(&key.as_str())
            || key == "maxSteps"
            || key == "permission";
        if !known {
            options.push((key.clone(), field.clone()));
        }
    }
    for (key, field) in entries {
        if V1_AGENT_KNOWN_KEYS.contains(&key.as_str())
            || key == "maxSteps"
            || key == "permission"
            || key == "name"
        {
            fields.push((key.clone(), field.clone()));
        }
    }
    if let Some(existing) =
        fields.iter().find(|(key, _)| key == "options").and_then(|(_, value)| value.as_object())
    {
        for (key, field) in existing {
            options.retain(|(name, _)| name != key);
            options.push((key.clone(), field.clone()));
        }
    }
    // Validate scalar fields, dropping malformed ones (decodeValue semantics).
    for (key, expected) in [
        ("model", FieldType::String),
        ("variant", FieldType::String),
        ("temperature", FieldType::Finite),
        ("top_p", FieldType::Finite),
        ("prompt", FieldType::String),
        ("description", FieldType::String),
        ("mode", FieldType::Mode),
        ("hidden", FieldType::Bool),
        ("color", FieldType::String),
        ("steps", FieldType::Positive),
        ("maxSteps", FieldType::Positive),
        ("disable", FieldType::Bool),
    ] {
        if let Some(field) = fields.iter().find(|(name, _)| name == key).map(|(_, value)| value) {
            #[allow(clippy::match_same_arms)]
            let valid = match (expected, field) {
                (FieldType::String, Json::String(_)) => true,
                (FieldType::Finite, Json::Number(_)) => true,
                (FieldType::Bool, Json::Bool(_)) => true,
                (FieldType::Positive, Json::Number(n)) => n.as_u64().is_some_and(|value| value > 0),
                (FieldType::Mode, Json::String(mode)) => {
                    matches!(mode.as_str(), "subagent" | "primary" | "all")
                }
                (_, Json::Null) => true,
                _ => false,
            };
            if !valid {
                let field_path = join_path(path, key);
                let refs: Vec<&str> = field_path.iter().map(String::as_str).collect();
                invalid(&refs, diagnostics);
                fields.retain(|(name, _)| name != key);
            }
        }
    }
    let tools: Vec<(String, bool)> = fields
        .iter()
        .find(|(key, _)| key == "tools")
        .and_then(|(_, value)| value.as_object())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(tool, enabled)| {
                    enabled.as_bool().map(|enabled| (tool.clone(), enabled))
                })
                .collect()
        })
        .unwrap_or_default();
    if fields.iter().any(|(key, _)| key == "tools") {
        let tools_valid =
            fields.iter().find(|(key, _)| key == "tools").is_some_and(|(_, value)| {
                value.as_object().is_some_and(|entries| {
                    entries.iter().all(|(_, enabled)| enabled.as_bool().is_some())
                })
            });
        if !tools_valid {
            let tools_path = join_path(path, "tools");
            let refs: Vec<&str> = tools_path.iter().map(String::as_str).collect();
            invalid(&refs, diagnostics);
        }
        fields.retain(|(key, _)| key != "tools");
    }

    // permission: tools first, explicit permission wins per key.
    let mut permission: Vec<(String, Json)> = Vec::new();
    for (tool, enabled) in tools {
        let action = if enabled { "allow" } else { "deny" };
        let key = if tool == "write" || tool == "edit" || tool == "patch" {
            "edit"
        } else {
            tool.as_str()
        };
        permission.retain(|(name, _)| name != key);
        permission.push((key.to_string(), Json::String(action.to_string())));
    }
    let permission_path = join_path(path, "permission");
    let permission_refs: Vec<&str> = permission_path.iter().map(String::as_str).collect();
    if let Some(explicit) =
        fields.iter().find(|(key, _)| key == "permission").map(|(_, value)| value)
    {
        // Non-record permission: string form already handled inside.
        if let Some(entries) = decode_v1_permission(explicit, &permission_refs, diagnostics) {
            for (key, value) in entries {
                permission.retain(|(name, _)| name != &key);
                permission.push((key, value));
            }
        }
        fields.retain(|(key, _)| key != "permission");
    }

    // steps fallback.
    let steps = fields
        .iter()
        .find(|(key, _)| key == "steps")
        .cloned()
        .or_else(|| fields.iter().find(|(key, _)| key == "maxSteps").cloned());
    fields.retain(|(key, _)| key != "steps" && key != "maxSteps");
    if let Some((_, steps)) = steps {
        fields.push(("steps".to_string(), steps));
    }

    fields.retain(|(name, _)| name != "options");
    fields.push(("options".to_string(), obj(options)));
    fields.push(("permission".to_string(), obj(permission)));
    Some(obj(fields))
}

enum FieldType {
    String,
    Finite,
    Bool,
    Positive,
    Mode,
}

/// v1 permission: action string or map of action → action-string | resource map.
fn decode_v1_permission(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<(String, Json)>> {
    if value.as_str().is_some() {
        let action = decode_v1_action(value.as_str()?, path, diagnostics)?;
        return Some(vec![("*".to_string(), Json::String(action))]);
    }
    let entries = value.as_object()?;
    let mut out = Vec::new();
    for (action, rule) in entries {
        let rule_path = join_path(path, action);
        let refs: Vec<&str> = rule_path.iter().map(String::as_str).collect();
        if let Some(effect) = rule.as_str() {
            let effect = decode_v1_action(effect, &refs, diagnostics)?;
            out.push((action.clone(), Json::String(effect)));
            continue;
        }
        let Some(resources) = rule.as_object() else {
            invalid(&refs, diagnostics);
            continue;
        };
        let mut resource_rules = Vec::new();
        for (resource, effect) in resources {
            let Some(effect) =
                effect.as_str().and_then(|effect| decode_v1_action(effect, &refs, diagnostics))
            else {
                continue;
            };
            resource_rules.push((resource.clone(), Json::String(effect)));
        }
        out.push((action.clone(), obj(resource_rules)));
    }
    Some(out)
}

/// `ConfigMigrateV1.migrateAgent` — v1 normalized agent fields → v2 agent JSON.
fn migrate_agent_document(agent: &Json) -> Json {
    let get = |key: &str| agent.get(key).filter(|value| !matches!(value, Json::Null));
    let model = get("model").and_then(Json::as_str).and_then(|model| {
        match get("variant").and_then(Json::as_str) {
            Some(variant)
                if !variant.is_empty() && !variant.contains('#') && !model.contains('#') =>
            {
                migrate_model_selection(&format!("{model}#{variant}"))
            }
            _ => migrate_model_selection(model),
        }
    });
    let mut body: Vec<(String, Json)> = Vec::new();
    if let Some(options) = get("options").and_then(Json::as_object) {
        for (key, value) in options {
            body.push((key.clone(), value.clone()));
        }
    }
    if let Some(temperature) = get("temperature") {
        body.push(("temperature".to_string(), temperature.clone()));
    }
    if let Some(top_p) = get("top_p") {
        body.push(("top_p".to_string(), top_p.clone()));
    }
    let mut fields: Vec<(String, Json)> = Vec::new();
    if let Some(model) = model {
        fields.push(("model".to_string(), model));
    }
    if !body.is_empty() {
        fields.push(("request".to_string(), obj(vec![("body".to_string(), obj(body))])));
    }
    if let Some(prompt) = get("prompt") {
        fields.push(("system".to_string(), prompt.clone()));
    }
    if let Some(description) = get("description") {
        fields.push(("description".to_string(), description.clone()));
    }
    if let Some(mode) = get("mode").and_then(Json::as_str) {
        if let Some(mode) = match mode {
            "subagent" => Some(AgentMode::Subagent),
            "primary" => Some(AgentMode::Primary),
            "all" => Some(AgentMode::All),
            _ => None,
        } {
            fields.push((
                "mode".to_string(),
                Json::String(
                    serde_json::to_value(mode)
                        .unwrap_or_default()
                        .as_str()
                        .unwrap_or("subagent")
                        .to_string(),
                ),
            ));
        }
    }
    if let Some(hidden) = get("hidden").and_then(Json::as_bool) {
        fields.push(("hidden".to_string(), Json::Bool(hidden)));
    }
    if let Some(color) = get("color").and_then(Json::as_str) {
        let migrated =
            if color.starts_with('#') { color.to_string() } else { "#aaaaaa".to_string() };
        fields.push(("color".to_string(), Json::String(migrated)));
    }
    if let Some(steps) = get("steps") {
        fields.push(("steps".to_string(), steps.clone()));
    }
    if let Some(disable) = get("disable").and_then(Json::as_bool) {
        fields.push(("disabled".to_string(), Json::Bool(disable)));
    }
    if let Some(permission) = get("permission").and_then(Json::as_object) {
        let rules = migrate_v1_permission_rules(permission);
        if !rules.is_empty() {
            fields.push(("permissions".to_string(), Json::Array(rules)));
        }
    }
    obj(fields)
}

/// Joins the frontmatter `model` + `variant` pair when both are plain
/// strings (`model` without `#`, `variant` non-empty without `#`); returns
/// the updated data map, or `None` to keep the original.
pub(crate) fn join_model_variant_markdown(data: &Json) -> Option<Json> {
    let model = data.get("model")?;
    let variant = data.get("variant")?;
    let model = model.as_str()?;
    if model.contains('#') {
        return None;
    }
    let variant = variant.as_str()?;
    if variant.is_empty() || variant.contains('#') {
        return None;
    }
    let joined = migrate_model_selection(&format!("{model}#{variant}"))?;
    let mut out = data.clone();
    out.set("model", joined);
    Some(out)
}

/// Markdown legacy decode: v1 agent fields without diagnostics plumbing.
pub(crate) fn decode_v1_agent_lenient(value: &Json) -> Json {
    let mut diagnostics = Vec::new();
    decode_v1_agent(value, &[], &mut diagnostics).unwrap_or_else(|| obj(Vec::new()))
}

/// Markdown legacy migrate: v1 normalized fields → v2 agent JSON.
pub(crate) fn migrate_agent_document_pub(agent: &Json) -> Json {
    migrate_agent_document(agent)
}

fn migrate_v1_permission_rules(permission: &[(String, Json)]) -> Vec<Json> {
    let mut rules = Vec::new();
    for (key, rule_value) in permission {
        let action = normalize_action(key);
        if let Some(effect) = rule_value.as_str() {
            rules.push(rule(&action, "*", effect));
            continue;
        }
        if let Some(resources) = rule_value.as_object() {
            for (resource, effect) in resources {
                if let Some(effect) = effect.as_str() {
                    rules.push(rule(&action, resource, effect));
                }
            }
        }
    }
    rules
}

/// Decodes + migrates a v1 command entry into the v2 `Command` shape.
fn decode_v1_command(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Json> {
    #[derive(serde::Deserialize)]
    struct V1Command {
        template: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        variant: Option<String>,
        #[serde(default)]
        subtask: Option<bool>,
    }
    let command: V1Command = decode_value(value, path, diagnostics)?;
    // modelSelection(model, variant): the model must be valid on its own; a
    // clean variant joins into `model#variant`.
    let model = command.model.as_deref().and_then(|model| {
        let selection = match &command.variant {
            Some(variant)
                if !variant.is_empty() && !variant.contains('#') && !model.contains('#') =>
            {
                migrate_model_selection(&format!("{model}#{variant}"))
            }
            _ => migrate_model_selection(model),
        };
        selection
    });
    let mut out = obj(vec![("template".to_string(), Json::String(command.template.clone()))]);
    if let Some(description) = command.description {
        out.set("description", Json::String(description));
    }
    if let Some(agent) = command.agent {
        out.set("agent", Json::String(agent));
    }
    if let Some(model) = model {
        out.set("model", model);
    }
    if let Some(subtask) = command.subtask {
        out.set("subagent", Json::Bool(subtask));
    }
    Some(out)
}

// --- v1 providers -----------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1ProviderModel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    tool_call: Option<bool>,
    #[serde(default)]
    interleaved: Option<Json>,
    #[serde(default)]
    cost: Option<V1Cost>,
    #[serde(default)]
    limit: Option<V1Limit>,
    #[serde(default)]
    modalities: Option<V1Modalities>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, rename = "provider")]
    provider_overlay: Option<V1ModelProvider>,
    #[serde(default)]
    options: Option<Json>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    variants: Option<Json>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1Cost {
    input: f64,
    output: f64,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
    #[serde(default, rename = "context_over_200k")]
    context_over_200k: Option<Box<V1Cost>>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1Limit {
    context: f64,
    #[serde(default)]
    input: Option<f64>,
    output: f64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1Modalities {
    #[serde(default)]
    input: Option<Vec<String>>,
    #[serde(default)]
    output: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1ModelProvider {
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1Provider {
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    env: Option<Vec<String>>,
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    options: Option<Json>,
    #[serde(default)]
    models: Option<Json>,
}

fn migrate_providers(
    value: Option<&Json>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(String, Json)> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(entries) = value.as_object() else {
        invalid(&["provider"], diagnostics);
        return Vec::new();
    };
    let mut candidates: Vec<(String, String, Json)> = Vec::new();
    for (name, raw) in entries {
        let path = join_path(&["provider"], name);
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        diagnose_provider_unsupported(raw, &refs, diagnostics);
        if invalid_provider_overlays(raw, &refs, diagnostics) {
            continue;
        }
        let Some(provider): Option<V1Provider> = decode_value(raw, &refs, diagnostics) else {
            continue;
        };
        let destination = migrate_provider_id(name);
        let migrated = migrate_provider(name, &provider);
        candidates.push((
            name.clone(),
            destination,
            Json::from(serde_json::to_value(&migrated).unwrap_or_default()),
        ));
    }
    let current: Vec<String> = candidates
        .iter()
        .filter(|(name, destination, _)| name == destination)
        .map(|(_, destination, _)| destination.clone())
        .collect();
    let mut result: Vec<(String, Json)> = Vec::new();
    for (name, destination, provider) in candidates {
        if name != destination && current.contains(&destination) {
            continue;
        }
        set_map_entry(&mut result, destination, provider);
    }
    result
}

fn diagnose_provider_unsupported(value: &Json, path: &[&str], diagnostics: &mut Vec<Diagnostic>) {
    let Some(entries) = value.as_object() else {
        return;
    };
    for key in UNSUPPORTED_PROVIDER {
        unsupported_if_present(value, key, &join_path_refs(path, key), diagnostics);
    }
    let _ = entries;
    let Some(models) = value.get("models").and_then(Json::as_object) else {
        return;
    };
    for (name, model) in models {
        let Some(fields) = model.as_object() else {
            continue;
        };
        let model_path = join_path_owned(&join_path(path, "models"), name);
        for key in UNSUPPORTED_MODEL {
            let key_path = join_path_owned(&model_path, key);
            let refs: Vec<&str> = key_path.iter().map(String::as_str).collect();
            unsupported_if_present(model, key, &refs, diagnostics);
        }
        if let Some(status) = model.get("status").and_then(Json::as_str) {
            if status != "deprecated" {
                let status_path = join_path_owned(&model_path, "status");
                let refs: Vec<&str> = status_path.iter().map(String::as_str).collect();
                unsupported(&refs, diagnostics);
            }
        }
        if let Some(interleaved) = model.get("interleaved") {
            if interleaved.as_bool().is_some() {
                let interleaved_path = join_path_owned(&model_path, "interleaved");
                let refs: Vec<&str> = interleaved_path.iter().map(String::as_str).collect();
                unsupported(&refs, diagnostics);
            }
        }
        let _ = fields;
    }
}

fn invalid_provider_overlays(
    value: &Json,
    path: &[&str],
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(options) = value.get("options") else {
        return false;
    };
    if !options.is_object() {
        return false;
    }
    let headers_invalid = options.get("headers").is_some_and(|headers| {
        !headers.is_object()
            || headers.as_object().is_some_and(|entries| {
                entries.iter().any(|(_, value)| !matches!(value, Json::String(_)))
            })
    });
    let body_invalid = options.get("body").is_some_and(|body| !body.is_object());
    if headers_invalid {
        let full = join_path_owned(&join_path(path, "options"), "headers");
        invalid(&path_refs(&full), diagnostics);
    }
    if body_invalid {
        let full = join_path_owned(&join_path(path, "options"), "body");
        invalid(&path_refs(&full), diagnostics);
    }
    headers_invalid || body_invalid
}

/// Splits raw provider options into `{settings, headers, body}`.
/// Split provider-options triple: settings / headers / body overlays.
type ProviderOptions = (
    BTreeMap<String, Option<serde_json::Value>>,
    Option<BTreeMap<String, String>>,
    Option<BTreeMap<String, serde_json::Value>>,
);

fn provider_options(options: &Json) -> ProviderOptions {
    let entries = options.as_object().unwrap_or(&[]);
    let mut settings = BTreeMap::new();
    let mut headers: Option<BTreeMap<String, String>> = None;
    let mut body: Option<BTreeMap<String, serde_json::Value>> = None;
    for (key, value) in entries {
        match key.as_str() {
            "headers" => {
                if let Some(map) = value.as_object() {
                    headers = Some(
                        map.iter()
                            .filter_map(|(name, value)| {
                                value.as_str().map(|value| (name.clone(), value.to_string()))
                            })
                            .collect(),
                    );
                }
            }
            "body" => {
                if value.is_object() {
                    body = Some(
                        value
                            .as_object()
                            .unwrap_or(&[])
                            .iter()
                            .map(|(name, value)| {
                                (name.clone(), serde_json::Value::from(value.clone()))
                            })
                            .collect(),
                    );
                }
            }
            other => {
                settings.insert(other.to_string(), Some(serde_json::Value::from(value.clone())));
            }
        }
    }
    (settings, headers, body)
}

fn migrate_provider(source_id: &str, info: &V1Provider) -> Provider {
    if source_id == "azure-cognitive-services" {
        return migrate_azure_provider(info);
    }
    if source_id == "google-vertex-anthropic" {
        return migrate_google_vertex_anthropic_provider(info);
    }
    migrate_standard_provider(info)
}

fn migrate_standard_provider(info: &V1Provider) -> Provider {
    let (settings, headers, body) = info.options.as_ref().map(provider_options).unwrap_or_default();
    let mut settings = settings;
    if let Some(api) = &info.api {
        settings.insert("baseURL".to_string(), Some(serde_json::Value::String(api.clone())));
    }
    Provider {
        name: info.name.clone(),
        env: info.env.clone(),
        package: info.npm.as_deref().map(aisdk),
        settings: (info.api.is_some() || info.options.is_some()).then(|| {
            let mut rest = settings.clone();
            if let Some(api) = &info.api {
                rest.insert("baseURL".to_string(), Some(serde_json::Value::String(api.clone())));
            }
            Settings { rest, ..Default::default() }
        }),
        headers: info.options.as_ref().and(headers),
        body: if info.options.is_some() { body } else { None },
        models: info.models.as_ref().and_then(|models| models.as_object()).map(|models| {
            models
                .iter()
                .map(|(name, model)| {
                    let decoded: V1ProviderModel =
                        serde_json::from_value(serde_json::Value::from(model.clone()))
                            .unwrap_or_default();
                    (name.clone(), migrate_model(&decoded))
                })
                .collect()
        }),
        canonical: None,
    }
}

fn migrate_azure_provider(info: &V1Provider) -> Provider {
    let mut standard = migrate_standard_provider(info);
    standard.env = standard.env.map(|env| {
        env.into_iter().filter(|name| name != "AZURE_COGNITIVE_SERVICES_RESOURCE_NAME").collect()
    });
    if info.npm.as_deref() != Some("@ai-sdk/openai-compatible") || info.api.is_some() {
        return standard;
    }
    let mut settings = standard.settings.clone().unwrap_or_default();
    settings.rest.insert(
        "baseURL".to_string(),
        Some(serde_json::Value::String(
            "https://${AZURE_COGNITIVE_SERVICES_RESOURCE_NAME}.cognitiveservices.azure.com/openai"
                .to_string(),
        )),
    );
    standard.settings = Some(settings);
    standard
}

fn migrate_google_vertex_anthropic_provider(info: &V1Provider) -> Provider {
    let mut migrated = migrate_standard_provider(info);
    let package = aisdk("@ai-sdk/google-vertex/anthropic");
    migrated.package = None;
    if let Some(models) = migrated.models.as_mut() {
        for model in models.values_mut() {
            if model.package.is_none() {
                model.package = Some(package.clone());
            }
        }
    }
    migrated
}

/// `Model.compatibility(interleaved)`: string / `{field}` → reasoningField.
fn model_compatibility(interleaved: Option<&Json>) -> Option<oc_schema::model::Compatibility> {
    let value = interleaved?;
    if let Some(field) = value.as_str() {
        return Some(oc_schema::model::Compatibility {
            reasoning_field: Some(field.to_string()),
            ..Default::default()
        });
    }
    if let Some(field) = value.get("field").and_then(Json::as_str) {
        return Some(oc_schema::model::Compatibility {
            reasoning_field: Some(field.to_string()),
            ..Default::default()
        });
    }
    None
}

// v2 truncates limit numbers to integers (Math.trunc parity).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn migrate_cost(cost: &V1Cost) -> Vec<Cost> {
    let mut costs = vec![Cost {
        tier: None,
        input: UsdPerMillionTokens(Num(cost.input)),
        output: UsdPerMillionTokens(Num(cost.output)),
        cache: Some(CostCache {
            read: cost.cache_read.map(|value| UsdPerMillionTokens(Num(value))),
            write: cost.cache_write.map(|value| UsdPerMillionTokens(Num(value))),
        }),
    }];
    if let Some(over) = &cost.context_over_200k {
        costs.push(Cost {
            tier: Some(CostTier { tier_type: CostTierType::Context, size: 200_000 }),
            input: UsdPerMillionTokens(Num(over.input)),
            output: UsdPerMillionTokens(Num(over.output)),
            cache: Some(CostCache {
                read: over.cache_read.map(|value| UsdPerMillionTokens(Num(value))),
                write: over.cache_write.map(|value| UsdPerMillionTokens(Num(value))),
            }),
        });
    }
    costs
}

fn migrate_model(info: &V1ProviderModel) -> Model {
    let settings = info.options.as_ref().map(|options| {
        let mut map: BTreeMap<String, Option<serde_json::Value>> = options
            .as_object()
            .unwrap_or(&[])
            .iter()
            .map(|(key, value)| (key.clone(), Some(serde_json::Value::from(value.clone()))))
            .collect();
        if let Some(api) = info.provider_overlay.as_ref().and_then(|provider| provider.api.clone())
        {
            map.insert("baseURL".to_string(), Some(serde_json::Value::String(api)));
        }
        ModelSettings { rest: map, ..Default::default() }
    });
    let defaults = oc_schema::model::Capabilities::default();
    let capabilities = if info.tool_call.is_some()
        || info.modalities.as_ref().is_some_and(|m| m.input.is_some() || m.output.is_some())
    {
        Some(Capabilities {
            tools: Some(info.tool_call.unwrap_or(defaults.tools)),
            input: Some(
                info.modalities
                    .as_ref()
                    .and_then(|m| m.input.clone())
                    .unwrap_or(defaults.input.clone()),
            ),
            output: Some(
                info.modalities
                    .as_ref()
                    .and_then(|m| m.output.clone())
                    .unwrap_or(defaults.output.clone()),
            ),
        })
    } else {
        None
    };
    let variants: Option<Vec<ModelVariant>> =
        info.variants.as_ref().and_then(Json::as_object).map(|variants| {
            variants
                .iter()
                .map(|(id, options)| ModelVariant {
                    id: oc_schema::id::ModelVariantId::parse(id.clone()),
                    settings: Some(ModelSettings {
                        rest: options
                            .as_object()
                            .unwrap_or(&[])
                            .iter()
                            .map(|(key, value)| {
                                (key.clone(), Some(serde_json::Value::from(value.clone())))
                            })
                            .collect(),
                        ..Default::default()
                    }),
                    headers: None,
                    body: None,
                })
                .collect()
        });
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let limit = info.limit.as_ref().map(|limit| Limit {
        context: Some(limit.context.trunc() as i64),
        input: limit.input.map(|value| value.trunc() as i64),
        output: Some(limit.output.trunc() as i64),
    });
    Model {
        model_id: info.id.clone().map(oc_schema::id::ModelId::parse),
        family: info.family.clone(),
        name: info.name.clone(),
        compatibility: model_compatibility(info.interleaved.as_ref()),
        package: info
            .provider_overlay
            .as_ref()
            .and_then(|provider| provider.npm.as_deref())
            .map(aisdk),
        settings,
        headers: info.headers.clone(),
        body: None,
        capabilities,
        variants,
        cost: info.cost.as_ref().map(|cost| CostList::Many(migrate_cost(cost))),
        disabled: (info.status.as_deref() == Some("deprecated")).then_some(true),
        limit,
    }
}

// --- v1 MCP -------------------------------------------------------------------

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn decode_v1_mcp(value: &Json, path: &[&str], diagnostics: &mut Vec<Diagnostic>) -> Option<Json> {
    let server: V1Mcp = decode_value(value, path, diagnostics)?;
    let disabled = server.enabled.map(|enabled| !enabled);
    let timeout = server.timeout.map(|timeout| TimeoutConfig {
        startup: None,
        catalog: Some(timeout),
        execution: Some(timeout),
    });
    let config = match server.kind.as_deref() {
        Some("local") => ServerConfig::Local(Box::new(LocalConfig {
            command: server.command.unwrap_or_default(),
            cwd: server.cwd.clone(),
            environment: server.environment.clone(),
            disabled,
            codemode: None,
            timeout: timeout.clone(),
            protocol: None,
        })),
        Some("remote") => ServerConfig::Remote(Box::new(RemoteConfig {
            url: server.url.clone().unwrap_or_default(),
            headers: server.headers.clone(),
            oauth: server.oauth.map(|oauth| {
                OAuthValue::Config(OAuthConfig {
                    client_id: oauth.client_id,
                    client_secret: oauth.client_secret,
                    scope: oauth.scope,
                    callback_port: oauth.callback_port.map(|port| port as u16),
                    redirect_uri: oauth.redirect_uri,
                    auth_server_metadata_url: None,
                })
            }),
            disabled,
            codemode: None,
            timeout,
            protocol: None,
        })),
        _ => {
            invalid(path, diagnostics);
            return None;
        }
    };
    Some(Json::from(serde_json::to_value(&config).unwrap_or_default()))
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1Mcp {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    command: Option<Vec<String>>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    environment: Option<BTreeMap<String, String>>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    oauth: Option<V1Oauth>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct V1Oauth {
    #[serde(default, rename = "clientId")]
    client_id: Option<String>,
    #[serde(default, rename = "clientSecret")]
    client_secret: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default, rename = "callbackPort")]
    callback_port: Option<i64>,
    #[serde(default, rename = "redirectUri")]
    redirect_uri: Option<String>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions
    use super::*;
    use crate::config::Json;

    fn parse(text: &str) -> Json {
        json5::from_str(text).unwrap()
    }

    fn normalized(input: &str) -> (Json, Vec<Diagnostic>) {
        match normalize(&parse(input)) {
            NormalizeResult::Normalized { encoded, diagnostics } => (encoded, diagnostics),
            NormalizeResult::Rejected { .. } => panic!("expected normalized config for {input}"),
        }
    }

    fn kinds(diagnostics: &[Diagnostic]) -> Vec<(DiagnosticKind, String)> {
        diagnostics.iter().map(|diagnostic| (diagnostic.kind, diagnostic.path.join("."))).collect()
    }

    fn get<'a>(value: &'a Json, key: &str) -> &'a Json {
        value.get(key).unwrap_or(&Json::Null)
    }

    fn to_value(value: &Json) -> serde_json::Value {
        serde_json::to_value(value).unwrap()
    }

    #[test]
    fn config_normalize_rejects_non_object_roots() {
        for input in ["null", "[]", "\"config\"", "true", "1"] {
            match normalize(&parse(input)) {
                NormalizeResult::Rejected { diagnostics } => {
                    assert_eq!(diagnostics.len(), 1);
                    assert_eq!(diagnostics[0].kind, DiagnosticKind::Invalid);
                    assert_eq!(diagnostics[0].path, vec!["$".to_string()]);
                }
                NormalizeResult::Normalized { .. } => panic!("expected rejection for {input}"),
            }
        }
    }

    /// dogfood2 bug 3 — the sanitized REAL-WORLD provider shape (a live
    /// config probe, synthetic values) survives normalize untouched:
    /// `providers.<id>` with camelCase keys, `settings`{apiKey,baseURL},
    /// per-model overlays whose `cost` is an ARRAY of {input,output}
    /// entries, a mixed-case top-level `model` selection ("Own/glm-5.3"
    /// — the providerID keeps its case exactly like v2's
    /// `Model.Ref.parse`, schema model.ts:27-42), and `small_model`
    /// migrating into the title agent's model (normalize.ts:137-150).
    /// A silent drop here hides every config-defined model from the
    /// picker — the dogfood2 boot-model bug.
    #[test]
    fn config_normalize_keeps_the_real_shape_custom_provider() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "providers": {
                    "own": {
                        "name": "Own",
                        "package": "@opencode/ai/providers/anthropic",
                        "settings": { "apiKey": "sk-SYNTHETIC", "baseURL": "https://synth.invalid/v1" },
                        "models": {
                            "glm-5.3": {
                                "name": "GLM-5.3",
                                "family": "glm",
                                "capabilities": { "tools": true, "input": ["text"], "output": ["text"] },
                                "cost": [ { "input": 0, "output": 0 } ],
                                "limit": { "context": 600000, "output": 131072 }
                            }
                        }
                    }
                },
                "model": "Own/glm-5.3",
                "small_model": "Own/glm-5.3-flash"
            }"#,
        );
        assert!(diagnostics.is_empty(), "no diagnostics for the real shape: {diagnostics:?}");
        let own = get(&encoded, "providers").get("own").expect("the provider survives");
        assert_eq!(own.get("name").and_then(Json::as_str), Some("Own"));
        assert_eq!(
            own.get("package").and_then(Json::as_str),
            Some("@opencode/ai/providers/anthropic")
        );
        assert_eq!(
            own.get("settings").and_then(|s| s.get("baseURL")).and_then(Json::as_str),
            Some("https://synth.invalid/v1")
        );
        let model = own.get("models").and_then(|m| m.get("glm-5.3")).expect("the model survives");
        assert!(
            model.get("cost").and_then(Json::as_array).is_some_and(|cost| cost.len() == 1),
            "the cost ARRAY decodes one-or-many and re-encodes as the list"
        );
        assert!(
            model.get("limit").and_then(|l| l.get("context")).is_some_and(|v| {
                matches!(v, Json::Number(number) if number.as_i64() == Some(600_000))
            }),
            "the limit context survives"
        );
        // The selection keeps its case (v2 is case-preserving here).
        let selection = get(&encoded, "model");
        assert_eq!(selection.get("providerID").and_then(Json::as_str), Some("Own"));
        assert_eq!(selection.get("model").and_then(Json::as_str), Some("glm-5.3"));
        // small_model migrates into the title agent's model.
        let title = get(get(&encoded, "agents"), "title");
        assert_eq!(
            title.get("model").and_then(|m| m.get("providerID")).and_then(Json::as_str),
            Some("Own")
        );
        assert_eq!(
            title.get("model").and_then(|m| m.get("model")).and_then(Json::as_str),
            Some("glm-5.3-flash")
        );
    }

    #[test]
    fn config_normalize_keeps_native_fields_alongside_legacy() {
        let (encoded, _) = normalized(
            r#"{ "snapshot": false, "agents": { "reviewer": { "system": "Use V2" } } }"#,
        );
        assert_eq!(get(&encoded, "snapshots"), &Json::Bool(false));
        assert_eq!(
            get(get(&encoded, "agents"), "reviewer").get("system").and_then(Json::as_str),
            Some("Use V2")
        );
    }

    #[test]
    fn config_normalize_merges_named_maps_native_wins_with_conflicts() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "reference": { "legacy": { "path": "../legacy" }, "duplicate": { "path": "../old" } },
                "references": { "native": { "path": "../native" }, "duplicate": { "path": "../new" } },
                "command": { "legacy": { "template": "legacy" }, "duplicate": { "template": "old" } },
                "commands": { "native": { "template": "native" }, "duplicate": { "template": "new" } }
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "references")),
            serde_json::json!({
                "legacy": { "path": "../legacy" },
                "native": { "path": "../native" },
                "duplicate": { "path": "../new" }
            })
        );
        assert_eq!(
            to_value(get(&encoded, "commands")),
            serde_json::json!({
                "legacy": { "template": "legacy" },
                "native": { "template": "native" },
                "duplicate": { "template": "new" }
            })
        );
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (DiagnosticKind::Conflict, "references.duplicate".to_string()),
                (DiagnosticKind::Conflict, "commands.duplicate".to_string()),
            ]
        );
    }

    #[test]
    fn config_normalize_canonical_equal_duplicates_are_not_conflicts() {
        let (_, diagnostics) = normalized(
            r#"{
                "snapshot": false,
                "snapshots": false,
                "reference": { "docs": { "path": "../docs" } },
                "references": { "docs": { "path": "../docs" } },
                "agent": { "reviewer": { "prompt": "same" } },
                "agents": { "reviewer": { "system": "same" } },
                "provider": { "custom": { "name": "same" } },
                "providers": { "custom": { "name": "same" } },
                "compaction": { "preserve_recent_tokens": 1000, "keep": { "tokens": 1000 } }
            }"#,
        );
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.kind != DiagnosticKind::Conflict));
    }

    #[test]
    fn config_normalize_agent_mode_native_precedence() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "agent": { "reviewer": { "prompt": "agent" }, "agentOnly": { "prompt": "agent-only" } },
                "mode": { "reviewer": { "prompt": "mode" }, "modeOnly": { "prompt": "mode-only" } },
                "agents": { "reviewer": { "system": "native" }, "nativeOnly": { "system": "native-only" } }
            }"#,
        );
        let agents = to_value(get(&encoded, "agents"));
        assert_eq!(
            agents,
            serde_json::json!({
                "reviewer": { "system": "native" },
                "agentOnly": { "system": "agent-only" },
                "modeOnly": { "system": "mode-only", "mode": "primary" },
                "nativeOnly": { "system": "native-only" }
            })
        );
        assert_eq!(
            kinds(&diagnostics)
                .iter()
                .filter(|(kind, _)| *kind == DiagnosticKind::Conflict)
                .count(),
            2,
            "agent+mode and native conflicts"
        );
        assert!(serde_json::from_value::<oc_schema::config::Info>(to_value(&encoded)).is_ok());
    }

    #[test]
    fn config_normalize_renames_legacy_provider_ids_in_model() {
        let (encoded, _) = normalized(r#"{ "model": "google-vertex-anthropic/claude#high" }"#);
        assert_eq!(
            to_value(get(&encoded, "model")),
            serde_json::json!({ "providerID": "google-vertex", "model": "claude", "variant": "high" })
        );
        let (encoded, _) = normalized(r#"{ "model": "anthropic/claude" }"#);
        assert_eq!(
            to_value(get(&encoded, "model")),
            serde_json::json!({ "providerID": "anthropic", "model": "claude" })
        );
    }

    #[test]
    fn config_normalize_migrates_small_model_to_title_agent() {
        let (encoded, diagnostics) =
            normalized(r#"{ "small_model": "anthropic/claude-haiku-4-5" }"#);
        assert_eq!(
            to_value(get(&encoded, "agents")),
            serde_json::json!({ "title": { "model": { "providerID": "anthropic", "model": "claude-haiku-4-5" } } })
        );
        assert!(diagnostics.is_empty());

        let (encoded, diagnostics) = normalized(
            r#"{ "small_model": "anthropic/claude-haiku-4-5", "agent": { "title": { "prompt": "Custom" } } }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "agents")),
            serde_json::json!({ "title": { "model": { "providerID": "anthropic", "model": "claude-haiku-4-5" }, "system": "Custom" } })
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn config_normalize_omits_invalid_small_model_without_exposing_value() {
        let secret = "do-not-log-this-value";
        let (encoded, diagnostics) = normalized(&format!(r#"{{ "small_model": "{secret}" }}"#));
        assert_eq!(get(&encoded, "agents"), &Json::Null);
        assert_eq!(
            kinds(&diagnostics),
            vec![(DiagnosticKind::Unsupported, "small_model".to_string())]
        );
        assert!(!format!("{diagnostics:?}").contains(secret));
    }

    #[test]
    fn config_normalize_recovers_malformed_map_entries() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "command": { "fallback": { "template": "legacy" } },
                "commands": { "fallback": { "template": 1 }, "valid": { "template": "native" }, "invalid": { "template": false } },
                "providers": { "valid": { "name": "Valid" }, "invalid": { "env": [1] } }
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "commands")),
            serde_json::json!({ "fallback": { "template": "legacy" }, "valid": { "template": "native" } })
        );
        assert_eq!(
            to_value(get(&encoded, "providers")),
            serde_json::json!({ "valid": { "name": "Valid" } })
        );
        assert_eq!(
            kinds(&diagnostics).iter().filter(|(kind, _)| *kind == DiagnosticKind::Invalid).count(),
            3
        );
    }

    #[test]
    fn config_normalize_preserves_permission_source_order() {
        let (encoded, _) = normalized(
            r#"{
                "tools": { "bash": true, "write": false },
                "permission": { "read": "allow", "custom": { "first": "deny", "second": "ask" }, "task": "allow" },
                "permissions": [{ "action": "native", "resource": "*", "effect": "deny" }]
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "permissions")),
            serde_json::json!([
                { "action": "shell", "resource": "*", "effect": "allow" },
                { "action": "edit", "resource": "*", "effect": "deny" },
                { "action": "read", "resource": "*", "effect": "allow" },
                { "action": "custom", "resource": "first", "effect": "deny" },
                { "action": "custom", "resource": "second", "effect": "ask" },
                { "action": "subagent", "resource": "*", "effect": "allow" },
                { "action": "native", "resource": "*", "effect": "deny" }
            ])
        );
    }

    #[test]
    fn config_normalize_redacts_invalid_permission_resources() {
        let (encoded, diagnostics) = normalized(
            r#"{ "permission": { "bash": { "curl -H Authorization:Bearer TOPSECRET *": "bogus" } } }"#,
        );
        assert_eq!(get(&encoded, "permissions"), &Json::Null);
        assert_eq!(
            kinds(&diagnostics),
            vec![(DiagnosticKind::Invalid, "permission.bash.0".to_string())]
        );
        assert!(!format!("{diagnostics:?}").contains("TOPSECRET"));
    }

    #[test]
    fn config_normalize_recovers_list_items() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "skills": { "paths": ["./skills", 1], "urls": [false, "https://example.com/skills"] },
                "instructions": ["one", 2, "three"],
                "permissions": [
                    { "action": "read", "resource": "*", "effect": "allow" },
                    { "action": "read", "resource": "*", "effect": "invalid" }
                ]
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "skills")),
            serde_json::json!(["./skills", "https://example.com/skills"])
        );
        assert_eq!(to_value(get(&encoded, "instructions")), serde_json::json!(["one", "three"]));
        assert_eq!(
            to_value(get(&encoded, "permissions")),
            serde_json::json!([{ "action": "read", "resource": "*", "effect": "allow" }])
        );
        assert_eq!(diagnostics.iter().filter(|d| d.kind == DiagnosticKind::Invalid).count(), 4);
    }

    #[test]
    fn config_normalize_omits_malformed_collection_roots() {
        let (encoded, diagnostics) = normalized(
            r#"{ "commands": [], "providers": "invalid", "references": false, "agents": 1, "permissions": {}, "instructions": {} }"#,
        );
        assert_eq!(encoded, obj(Vec::new()));
        assert_eq!(
            kinds(&diagnostics)
                .iter()
                .filter(|(kind, _)| *kind == DiagnosticKind::Invalid)
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>(),
            vec![
                "references".to_string(),
                "commands".to_string(),
                "agents".to_string(),
                "providers".to_string(),
                "permissions".to_string(),
                "instructions".to_string(),
            ]
        );
    }

    #[test]
    fn config_normalize_formatter_and_lsp_maps() {
        let (invalid, diagnostics) = normalized(
            r#"{ "formatter": { "prettier": { "command": [1] } }, "lsp": { "typescript": { "command": [1] } } }"#,
        );
        assert_eq!(get(&invalid, "formatter"), &Json::Null);
        assert_eq!(get(&invalid, "lsp"), &Json::Null);
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (DiagnosticKind::Invalid, "formatter.prettier".to_string()),
                (DiagnosticKind::Invalid, "lsp.typescript".to_string()),
            ]
        );
        let (empty, _) = normalized(r#"{ "formatter": {}, "lsp": {} }"#);
        assert_eq!(get(&empty, "formatter"), &obj(Vec::new()));
        assert_eq!(get(&empty, "lsp"), &obj(Vec::new()));
    }

    #[test]
    fn config_normalize_combines_mcp_servers_and_timeout() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "experimental": { "mcp_timeout": 5000 },
                "mcp": {
                    "legacy": { "type": "local", "command": ["legacy"] },
                    "duplicate": { "type": "remote", "url": "https://legacy.example.com" },
                    "servers": {
                        "native": { "type": "local", "command": ["native"] },
                        "duplicate": { "type": "remote", "url": "https://native.example.com" },
                        "invalid": { "type": "local", "command": [1] }
                    },
                    "timeout": { "startup": 1000, "catalog": 6000 }
                }
            }"#,
        );
        let mcp = to_value(get(&encoded, "mcp"));
        assert_eq!(
            mcp["timeout"],
            serde_json::json!({ "catalog": 6000, "execution": 5000, "startup": 1000 })
        );
        assert_eq!(
            mcp["servers"]["legacy"],
            serde_json::json!({ "type": "local", "command": ["legacy"] })
        );
        assert_eq!(
            mcp["servers"]["duplicate"]["url"],
            serde_json::json!("https://native.example.com")
        );
        assert_eq!(
            mcp["servers"]["native"],
            serde_json::json!({ "type": "local", "command": ["native"] })
        );
        assert!(diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Conflict
                && d.path.join(".") == "mcp.servers.duplicate"));
        assert!(diagnostics.iter().any(
            |d| d.kind == DiagnosticKind::Conflict && d.path.join(".") == "mcp.timeout.catalog"
        ));
        assert!(diagnostics.iter().any(
            |d| d.kind == DiagnosticKind::Invalid && d.path.join(".") == "mcp.servers.invalid"
        ));
    }

    #[test]
    fn config_normalize_raw_mcp_discriminators_for_reserved_names() {
        let (encoded, _) = normalized(
            r#"{ "mcp": { "servers": { "type": "local", "command": ["reserved-servers"] }, "timeout": { "type": "remote", "url": "https://reserved.example.com" } } }"#,
        );
        let servers = to_value(get(get(&encoded, "mcp"), "servers"));
        assert_eq!(
            servers,
            serde_json::json!({
                "servers": { "type": "local", "command": ["reserved-servers"] },
                "timeout": { "type": "remote", "url": "https://reserved.example.com" }
            })
        );
        let (enabled_only, diagnostics) = normalized(
            r#"{ "mcp": { "servers": { "enabled": true }, "timeout": { "enabled": false } } }"#,
        );
        assert_eq!(get(&enabled_only, "mcp"), &Json::Null);
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (DiagnosticKind::Unsupported, "mcp.servers".to_string()),
                (DiagnosticKind::Unsupported, "mcp.timeout".to_string()),
            ]
        );
    }

    #[test]
    fn config_normalize_mcp_timeout_leaf_recovery() {
        let (encoded, diagnostics) = normalized(
            r#"{ "mcp": { "timeout": { "execution": 3000, "startup": "invalid", "catalog": 2000 } } }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "mcp")),
            serde_json::json!({ "timeout": { "catalog": 2000, "execution": 3000 } })
        );
        assert_eq!(
            kinds(&diagnostics),
            vec![(DiagnosticKind::Invalid, "mcp.timeout.startup".to_string())]
        );

        let (empty, _) = normalized(r#"{ "mcp": { "timeout": {} } }"#);
        assert_eq!(get(&empty, "mcp"), &Json::Null);
        let (unknown, diagnostics) = normalized(r#"{ "mcp": { "timeout": { "unknown": 1000 } } }"#);
        assert_eq!(get(&unknown, "mcp"), &Json::Null);
        assert_eq!(kinds(&diagnostics), vec![(DiagnosticKind::Invalid, "mcp.timeout".to_string())]);
    }

    #[test]
    fn config_normalize_compaction_leaves() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "compaction": {
                    "auto": false,
                    "preserve_recent_tokens": 1000,
                    "keep": { "tokens": 2000 },
                    "reserved": 3000,
                    "buffer": 4000,
                    "tail_turns": 2,
                    "prune": true
                }
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "compaction")),
            serde_json::json!({ "auto": false, "keep": { "tokens": 2000 }, "buffer": 4000 })
        );
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (DiagnosticKind::Unsupported, "compaction.tail_turns".to_string()),
                (DiagnosticKind::Unsupported, "compaction.prune".to_string()),
                (DiagnosticKind::Conflict, "compaction.keep.tokens".to_string()),
                (DiagnosticKind::Conflict, "compaction.buffer".to_string()),
            ]
        );
    }

    #[test]
    fn config_normalize_provider_lists_to_policies() {
        let (encoded, _) = normalized(
            r#"{ "enabled_providers": ["anthropic", "openai"], "disabled_providers": ["openai"] }"#,
        );
        let policies = to_value(get(get(&encoded, "experimental"), "policies"));
        assert_eq!(
            policies,
            serde_json::json!([
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "*", "effect": "deny" },
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "anthropic", "effect": "allow" },
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "openai", "effect": "allow" },
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "openai", "effect": "deny" }
            ])
        );
        let (deny_only, _) = normalized(r#"{ "enabled_providers": [] }"#);
        assert_eq!(
            to_value(get(get(&deny_only, "experimental"), "policies")),
            serde_json::json!([{ "action": POLICY_ACTION_PROVIDER_USE, "resource": "*", "effect": "deny" }])
        );
    }

    #[test]
    fn config_normalize_provider_list_edge_cases() {
        let (mixed, _) = normalized(r#"{ "enabled_providers": [1, "anthropic", false] }"#);
        assert_eq!(
            to_value(get(get(&mixed, "experimental"), "policies")),
            serde_json::json!([
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "*", "effect": "deny" },
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "anthropic", "effect": "allow" }
            ])
        );
        let (wholly_invalid, _) = normalized(r#"{ "enabled_providers": [1, false] }"#);
        assert_eq!(get(&wholly_invalid, "experimental"), &Json::Null);
        let (not_a_list, _) = normalized(r#"{ "enabled_providers": "anthropic" }"#);
        assert_eq!(get(&not_a_list, "experimental"), &Json::Null);
    }

    #[test]
    fn config_normalize_appends_native_policies_after_generated() {
        let (encoded, _) = normalized(
            r#"{
                "enabled_providers": ["anthropic"],
                "disabled_providers": ["openai"],
                "experimental": {
                    "portable_shell_scanner": true,
                    "subagent_depth": 0,
                    "policies": [{ "action": "permission", "resource": "custom", "effect": "allow" }]
                }
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "experimental")),
            serde_json::json!({
                "portable_shell_scanner": true,
                "subagent_depth": 0,
                "policies": [
                    { "action": POLICY_ACTION_PROVIDER_USE, "resource": "*", "effect": "deny" },
                    { "action": POLICY_ACTION_PROVIDER_USE, "resource": "anthropic", "effect": "allow" },
                    { "action": POLICY_ACTION_PROVIDER_USE, "resource": "openai", "effect": "deny" },
                    { "action": "permission", "resource": "custom", "effect": "allow" }
                ]
            })
        );
    }

    #[test]
    fn config_normalize_reports_unsupported_settings_without_values() {
        let secret = "do-not-log-this-value";
        let (_, diagnostics) = normalized(&format!(
            r#"{{
                "logLevel": "DEBUG",
                "agent": {{ "reviewer": {{ "name": "{secret}", "prompt": "review" }} }},
                "provider": {{
                    "custom": {{
                        "id": "{secret}",
                        "whitelist": ["model"],
                        "models": {{ "model": {{ "release_date": "{secret}", "status": "active", "interleaved": true }} }}
                    }}
                }},
                "experimental": {{ "openTelemetry": true }}
            }}"#
        ));
        assert_eq!(
            kinds(&diagnostics)
                .iter()
                .filter(|(kind, _)| *kind == DiagnosticKind::Unsupported)
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>(),
            vec![
                "logLevel".to_string(),
                "agent.reviewer.name".to_string(),
                "provider.custom.id".to_string(),
                "provider.custom.whitelist".to_string(),
                "provider.custom.models.model.release_date".to_string(),
                "provider.custom.models.model.status".to_string(),
                "provider.custom.models.model.interleaved".to_string(),
                "experimental.openTelemetry".to_string(),
            ]
        );
        assert!(!format!("{diagnostics:?}").contains(secret));
    }

    #[test]
    fn config_normalize_diagnoses_unsupported_legacy_model_selections() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "command": {
                    "invalidModel": { "template": "one", "model": "invalid" },
                    "invalidVariant": { "template": "two", "model": "anthropic/model", "variant": "bad#variant" },
                    "missingModel": { "template": "three", "variant": "high" }
                },
                "agent": { "invalid": { "prompt": "agent", "model": "invalid", "variant": "" } }
            }"#,
        );
        let commands = get(&encoded, "commands").as_object().unwrap();
        let names: Vec<&str> = commands.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["invalidModel", "invalidVariant", "missingModel"]);
        let agents = get(&encoded, "agents").as_object().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(
            kinds(&diagnostics)
                .iter()
                .filter(|(kind, _)| *kind == DiagnosticKind::Unsupported)
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>(),
            vec![
                "command.invalidModel.model".to_string(),
                "command.invalidVariant.variant".to_string(),
                "command.missingModel.variant".to_string(),
                "agent.invalid.model".to_string(),
                "agent.invalid.variant".to_string(),
            ]
        );
    }

    #[test]
    fn config_normalize_invalid_provider_overlays_skip_only_that_provider() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "provider": {
                    "headers": { "options": { "headers": { "valid": "yes", "invalid": 1 } } },
                    "body": { "options": { "body": "not-an-object" } },
                    "valid": { "options": { "headers": { "valid": "yes" }, "body": { "trace": true } } }
                }
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "providers")),
            serde_json::json!({ "valid": { "settings": {}, "headers": { "valid": "yes" }, "body": { "trace": true } } })
        );
        assert_eq!(
            kinds(&diagnostics),
            vec![
                (DiagnosticKind::Invalid, "provider.headers.options.headers".to_string()),
                (DiagnosticKind::Invalid, "provider.body.options.body".to_string()),
            ]
        );
    }

    #[test]
    fn config_normalize_preserves_explicit_empty_values() {
        let (encoded, _) = normalized(
            r#"{
                "snapshot": false,
                "autoshare": false,
                "references": {},
                "commands": {},
                "agents": {},
                "providers": {},
                "instructions": [],
                "experimental": { "subagent_depth": 0 }
            }"#,
        );
        assert_eq!(get(&encoded, "snapshots"), &Json::Bool(false));
        assert_eq!(get(&encoded, "references"), &obj(Vec::new()));
        assert_eq!(get(&encoded, "commands"), &obj(Vec::new()));
        assert_eq!(get(&encoded, "agents"), &obj(Vec::new()));
        assert_eq!(get(&encoded, "providers"), &obj(Vec::new()));
        assert_eq!(get(&encoded, "instructions"), &Json::Array(Vec::new()));
        assert_eq!(get(&encoded, "share"), &Json::Null);
    }

    #[test]
    fn config_normalize_migrates_update_policy() {
        let (encoded, _) = normalized(r#"{ "autoupdate": false }"#);
        assert_eq!(get(&encoded, "update").as_str(), Some("disable"));
        let (encoded, _) = normalized(r#"{ "autoupdate": "notify" }"#);
        assert_eq!(get(&encoded, "update").as_str(), Some("notify"));
        let (encoded, _) = normalized(r#"{ "autoupdate": true }"#);
        assert_eq!(get(&encoded, "update").as_str(), Some("auto"));
        let (encoded, _) = normalized(r#"{ "autoupdate": false, "update": "notify" }"#);
        assert_eq!(get(&encoded, "update").as_str(), Some("notify"));
        let (encoded, _) = normalized(r"{}");
        assert_eq!(get(&encoded, "update"), &Json::Null);
    }

    #[test]
    fn config_normalize_native_update_passthrough() {
        let (encoded, diagnostics) = normalized(r#"{ "update": "auto" }"#);
        assert_eq!(get(&encoded, "update").as_str(), Some("auto"));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn config_normalize_migrates_v1_command_with_subtask() {
        for subtask in [true, false] {
            let (encoded, _) = normalized(&format!(
                r#"{{ "command": {{ "review": {{ "template": "Review changes", "description": "Review code", "agent": "reviewer", "model": "anthropic/claude", "variant": "high", "subtask": {subtask} }} }} }}"#
            ));
            assert_eq!(
                to_value(get(&encoded, "commands")),
                serde_json::json!({
                    "review": {
                        "template": "Review changes",
                        "description": "Review code",
                        "agent": "reviewer",
                        "model": { "providerID": "anthropic", "model": "claude", "variant": "high" },
                        "subagent": subtask
                    }
                })
            );
        }
    }

    #[test]
    fn config_normalize_migrates_v1_agent_fields() {
        let (encoded, _) = normalized(
            r##"{
                "agent": {
                    "reviewer": {
                        "model": "test/model",
                        "variant": "high",
                        "temperature": 0.5,
                        "top_p": 0.9,
                        "prompt": "Review carefully.",
                        "tools": { "write": false },
                        "disable": false,
                        "description": "Reviews changes",
                        "mode": "subagent",
                        "hidden": true,
                        "options": { "existing": true },
                        "color": "#112233",
                        "steps": 10,
                        "permission": { "read": "allow" },
                        "custom": "preserved"
                    }
                }
            }"##,
        );
        assert_eq!(
            to_value(get(&encoded, "agents")),
            serde_json::json!({
                "reviewer": {
                    "model": { "providerID": "test", "model": "model", "variant": "high" },
                    "request": { "body": { "existing": true, "custom": "preserved", "temperature": 0.5, "top_p": 0.9 } },
                    "system": "Review carefully.",
                    "description": "Reviews changes",
                    "mode": "subagent",
                    "hidden": true,
                    "color": "#112233",
                    "steps": 10,
                    "disabled": false,
                    "permissions": [
                        { "action": "edit", "resource": "*", "effect": "deny" },
                        { "action": "read", "resource": "*", "effect": "allow" }
                    ]
                }
            })
        );
    }

    #[test]
    fn config_normalize_renames_provider_ids_during_migration() {
        let (encoded, _) = normalized(
            r#"{
                "model": "azure-cognitive-services/deployment",
                "enabled_providers": ["google-vertex-anthropic"],
                "disabled_providers": ["azure-cognitive-services"],
                "agent": { "reviewer": { "model": "google-vertex-anthropic/claude-sonnet" } },
                "command": { "review": { "template": "Review", "model": "azure-cognitive-services/deployment" } },
                "provider": {
                    "azure-cognitive-services": {
                        "npm": "@ai-sdk/azure",
                        "env": ["AZURE_COGNITIVE_SERVICES_RESOURCE_NAME", "AZURE_COGNITIVE_SERVICES_API_KEY"],
                        "models": { "deployment": {} }
                    },
                    "google-vertex-anthropic": {
                        "npm": "@ai-sdk/google-vertex/anthropic",
                        "options": { "project": "test-project", "location": "us-central1" },
                        "models": { "claude-sonnet": {} }
                    }
                }
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "model")),
            serde_json::json!({ "providerID": "azure", "model": "deployment" })
        );
        let agents = to_value(get(&encoded, "agents"));
        assert_eq!(
            agents["reviewer"]["model"],
            serde_json::json!({ "providerID": "google-vertex", "model": "claude-sonnet" })
        );
        let commands = to_value(get(&encoded, "commands"));
        assert_eq!(
            commands["review"]["model"],
            serde_json::json!({ "providerID": "azure", "model": "deployment" })
        );
        let policies = to_value(get(get(&encoded, "experimental"), "policies"));
        assert_eq!(
            policies,
            serde_json::json!([
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "*", "effect": "deny" },
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "google-vertex", "effect": "allow" },
                { "action": POLICY_ACTION_PROVIDER_USE, "resource": "azure", "effect": "deny" }
            ])
        );
        let providers = to_value(get(&encoded, "providers"));
        assert_eq!(providers["azure"]["package"], serde_json::json!("aisdk:@ai-sdk/azure"));
        assert_eq!(
            providers["azure"]["env"],
            serde_json::json!(["AZURE_COGNITIVE_SERVICES_API_KEY"])
        );
        assert!(providers.get("azure-cognitive-services").is_none());
        assert_eq!(
            providers["google-vertex"]["settings"],
            serde_json::json!({ "project": "test-project", "location": "us-central1" })
        );
        assert_eq!(
            providers["google-vertex"]["models"]["claude-sonnet"]["package"],
            serde_json::json!("aisdk:@ai-sdk/google-vertex/anthropic")
        );
        assert!(providers["google-vertex"].get("package").is_none());
        assert!(providers.get("google-vertex-anthropic").is_none());
    }

    #[test]
    fn config_normalize_preserves_azure_generated_base_url() {
        let (encoded, _) = normalized(
            r#"{
                "provider": {
                    "azure-cognitive-services": {
                        "npm": "@ai-sdk/openai-compatible",
                        "env": ["AZURE_COGNITIVE_SERVICES_RESOURCE_NAME", "AZURE_COGNITIVE_SERVICES_API_KEY"]
                    }
                }
            }"#,
        );
        let providers = to_value(get(&encoded, "providers"));
        assert_eq!(
            providers["azure"]["package"],
            serde_json::json!("aisdk:@ai-sdk/openai-compatible")
        );
        assert_eq!(
            providers["azure"]["settings"]["baseURL"],
            serde_json::json!("https://${AZURE_COGNITIVE_SERVICES_RESOURCE_NAME}.cognitiveservices.azure.com/openai")
        );
    }

    #[test]
    fn config_normalize_ignores_retired_ids_when_current_configured() {
        let (encoded, _) = normalized(
            r#"{
                "provider": {
                    "azure": { "models": { "current": {} } },
                    "azure-cognitive-services": { "models": { "legacy": {} } },
                    "google-vertex": { "models": { "gemini": {} } },
                    "google-vertex-anthropic": { "models": { "claude": {} } }
                }
            }"#,
        );
        let providers = to_value(get(&encoded, "providers"));
        assert_eq!(providers["azure"]["models"].as_object().unwrap().len(), 1);
        assert!(providers["azure"]["models"]["current"].is_object());
        assert!(providers["azure"]["models"]["legacy"].is_null());
        assert_eq!(providers["google-vertex"]["models"].as_object().unwrap().len(), 1);
        assert!(providers["google-vertex"]["models"]["gemini"].is_object());
    }

    #[test]
    fn config_normalize_migrates_provider_options_into_settings() {
        let (encoded, _) = normalized(
            r#"{
                "provider": {
                    "bedrock": {
                        "npm": "@ai-sdk/amazon-bedrock",
                        "models": { "claude": { "provider": { "npm": "@ai-sdk/anthropic" } } },
                        "options": {
                            "headers": { "x-test": "1" },
                            "body": { "trace": true },
                            "region": "us-east-1",
                            "profile": "dev"
                        }
                    }
                }
            }"#,
        );
        let providers = to_value(get(&encoded, "providers"));
        assert_eq!(
            providers["bedrock"]["package"],
            serde_json::json!("aisdk:@ai-sdk/amazon-bedrock")
        );
        assert_eq!(
            providers["bedrock"]["settings"],
            serde_json::json!({ "region": "us-east-1", "profile": "dev" })
        );
        assert_eq!(providers["bedrock"]["headers"], serde_json::json!({ "x-test": "1" }));
        assert_eq!(providers["bedrock"]["body"], serde_json::json!({ "trace": true }));
        assert_eq!(
            providers["bedrock"]["models"]["claude"]["package"],
            serde_json::json!("aisdk:@ai-sdk/anthropic")
        );
    }

    #[test]
    fn config_normalize_migrates_interleaved_to_compatibility() {
        let (encoded, _) = normalized(
            r#"{
                "provider": {
                    "custom": {
                        "models": {
                            "object": { "interleaved": { "field": "vendor_reasoning" } },
                            "string": { "interleaved": "reasoning_text" },
                            "boolean": { "interleaved": true }
                        }
                    }
                }
            }"#,
        );
        let providers = to_value(get(&encoded, "providers"));
        assert_eq!(
            providers["custom"]["models"]["object"]["compatibility"],
            serde_json::json!({ "reasoningField": "vendor_reasoning" })
        );
        assert_eq!(
            providers["custom"]["models"]["string"]["compatibility"],
            serde_json::json!({ "reasoningField": "reasoning_text" })
        );
        assert!(providers["custom"]["models"]["boolean"].get("compatibility").is_none());
    }

    #[test]
    fn config_normalize_migrates_model_costs_limits_and_variants() {
        let (encoded, _) = normalized(
            r#"{
                "provider": {
                    "custom": {
                        "models": {
                            "deep": {
                                "id": "deep-1",
                                "name": "Deep",
                                "family": "deep",
                                "tool_call": true,
                                "modalities": { "input": ["text", "image"] },
                                "cost": {
                                    "input": 1, "output": 2, "cache_read": 0.1, "cache_write": 0.2,
                                    "context_over_200k": { "input": 3, "output": 4 }
                                },
                                "limit": { "context": 100000, "input": 50000, "output": 20000 },
                                "headers": { "x-model": "1" },
                                "variants": { "high": { "reasoning": "high" } }
                            }
                        }
                    }
                }
            }"#,
        );
        let model = &to_value(get(&encoded, "providers"))["custom"]["models"]["deep"];
        assert_eq!(model["modelID"], serde_json::json!("deep-1"));
        assert_eq!(model["name"], serde_json::json!("Deep"));
        assert_eq!(model["capabilities"]["tools"], serde_json::json!(true));
        assert_eq!(model["capabilities"]["input"], serde_json::json!(["text", "image"]));
        assert_eq!(
            model["limit"],
            serde_json::json!({ "context": 100_000, "input": 50_000, "output": 20_000 })
        );
        assert_eq!(model["headers"], serde_json::json!({ "x-model": "1" }));
        assert_eq!(model["cost"].as_array().unwrap().len(), 2);
        assert_eq!(model["cost"][0]["input"], serde_json::json!(1));
        assert_eq!(model["cost"][0]["cache"]["read"], serde_json::json!(0.1));
        assert_eq!(model["cost"][1]["tier"]["size"], serde_json::json!(200_000));
        assert_eq!(model["variants"][0]["id"], serde_json::json!("high"));
    }

    #[test]
    fn config_normalize_deprecated_models_disabled() {
        let (encoded, _) = normalized(
            r#"{ "provider": { "custom": { "models": { "old": { "status": "deprecated" }, "live": {} } } } }"#,
        );
        let providers = to_value(get(&encoded, "providers"));
        assert_eq!(providers["custom"]["models"]["old"]["disabled"], serde_json::json!(true));
        assert!(providers["custom"]["models"]["live"].get("disabled").is_none());
    }

    #[test]
    fn config_normalize_migrates_legacy_plugins() {
        let (encoded, _) = normalized(
            r#"{
                "plugin": ["pkg-string", ["pkg-with-options", { "flag": true }]],
                "plugins": ["github:opencode/plugins"]
            }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "plugins")),
            serde_json::json!([
                "pkg-string",
                { "package": "pkg-with-options", "options": { "flag": true } },
                "github:opencode/plugins"
            ])
        );
    }

    #[test]
    fn config_normalize_migrates_attachment_and_autoshare() {
        let (encoded, _) = normalized(
            r#"{ "attachment": { "image": { "auto_resize": false } }, "autoshare": true }"#,
        );
        assert_eq!(
            to_value(get(&encoded, "media")),
            serde_json::json!({ "image": { "auto_resize": false } })
        );
        assert_eq!(get(&encoded, "share").as_str(), Some("auto"));
    }

    #[test]
    fn config_normalize_atomic_fields_round_trip() {
        let (encoded, diagnostics) = normalized(
            r#"{
                "shell": "/bin/bash",
                "default_agent": "reviewer",
                "username": "test-user",
                "snapshots": true,
                "tool_output": { "max_lines": 1000, "max_bytes": 32768 },
                "worktree": { "directory": "../worktrees" },
                "warming": { "prompt": "warm" }
            }"#,
        );
        assert!(diagnostics.is_empty());
        assert_eq!(get(&encoded, "shell").as_str(), Some("/bin/bash"));
        assert_eq!(get(&encoded, "default_agent").as_str(), Some("reviewer"));
        assert_eq!(get(&encoded, "snapshots"), &Json::Bool(true));
        assert!(serde_json::from_value::<oc_schema::config::Info>(to_value(&encoded)).is_ok());
    }
}
