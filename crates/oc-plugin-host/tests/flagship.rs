//! The wave-2 acceptance test (oracle-gate11 §4 波 2): the flagship
//! plugin — oh-my-opencode-slim v3.0.3 — loads on the deliberately
//! minimal Context, its setup completes, and everything it registers
//! materializes into the Rust structures:
//!
//! * agents (with the config **preset applied** — `own/glm-5.3` on the
//!   orchestrator proves `loadPluginConfig` processed the staged
//!   `oh-my-opencode-slim.json`),
//! * commands (reflect/deepwork/loop/interview),
//! * skills (the bundled `SKILL.md` documents),
//! * tools + MCP servers + hook coverage,
//! * then the agent table flows into the oc-core `AgentRegistry`
//!   through the wave-2 seam.
//!
//! Hermetic: the installed tree is pre-staged by COPYING the
//! read-only live install into a `/tmp/opencode` sandbox (skip when
//! the machine has neither the install nor bun); no network at any
//! point. The config preset is the machine's own (also read-only,
//! copied); `OPENCODE_CONFIG_DIR` redirects every user-config read.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use oc_plugin_host::{PluginHost, PluginHostConfig, RegistrationSnapshot, SessionPort};

/// The live install (read-only; never loaded in place).
fn flagship_source() -> Option<PathBuf> {
    if let Ok(root) = std::env::var("OC_OMOS_ROOT") {
        let root = PathBuf::from(root);
        return root.join("dist/server/index.js").is_file().then_some(root);
    }
    let base = Path::new("/root/.cache/opencode/npm/oh-my-opencode-slim@latest");
    let mut stamps: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect();
    stamps.sort();
    stamps
        .into_iter()
        .rev()
        .map(|stamp| stamp.join("node_modules").join("oh-my-opencode-slim"))
        .find(|root| root.join("dist/server/index.js").is_file())
}

/// The machine's config preset for the flagship (read-only; copied).
fn flagship_preset() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OC_OMOS_CONFIG") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let path = PathBuf::from("/root/.config/opencode/oh-my-opencode-slim.json");
    path.is_file().then_some(path)
}

/// The agent table the engine seeds (engine/mod.rs `seed_builtin_agents`
/// shape) — the host state the flagship's transform editors snapshot.
fn seed_agents() -> Vec<Value> {
    fn agent(id: &str, name: &str, mode: &str, hidden: bool) -> Value {
        json!({
            "id": id,
            "name": name,
            "mode": mode,
            "hidden": hidden,
            "request": { "settings": {}, "headers": {}, "body": {} },
            "permissions": [{ "action": "*", "resource": "*", "effect": "allow" }],
        })
    }
    vec![
        agent("build", "Build", "primary", false),
        agent("general", "General", "subagent", false),
        agent("compaction", "Compaction", "primary", true),
        agent("title", "Title", "primary", true),
    ]
}

/// Stage the sandbox: package copy (+ jsonc-parser dep), config preset,
/// project dir, redirected HOME. Returns (sandbox, entry).
fn stage(name: &str) -> Option<(PathBuf, PathBuf)> {
    let source = flagship_source()?;
    let sandbox = common::sandbox(name);
    let node_modules = sandbox.join("plugins").join("node_modules");
    let package = node_modules.join("oh-my-opencode-slim");
    for dir in ["dist", "src"] {
        copy_dir(&source.join(dir), &package.join(dir));
    }
    for file in ["package.json", "oh-my-opencode-slim.schema.json"] {
        std::fs::copy(source.join(file), package.join(file)).ok();
    }
    // jsonc-parser: imported at the top level of the bundled server
    // entry (the build externalizes it).
    if let Some(dep) = source.parent().map(|parent| parent.join("jsonc-parser")) {
        if dep.is_dir() {
            copy_dir(&dep, &node_modules.join("jsonc-parser"));
        }
    }
    // Redirected user config: the plugin's config candidates, logs and
    // runtime state all live under OPENCODE_CONFIG_DIR.
    std::fs::create_dir_all(sandbox.join("config")).expect("config dir");
    if let Some(preset) = flagship_preset() {
        std::fs::copy(preset, sandbox.join("config").join("oh-my-opencode-slim.json"))
            .expect("preset staged");
    }
    std::fs::create_dir_all(sandbox.join("home")).expect("home dir");
    std::fs::create_dir_all(sandbox.join("project")).expect("project dir");
    let entry = package.join("dist").join("server").join("index.js");
    Some((sandbox, entry))
}

fn copy_dir(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).expect("create dir");
    for entry in std::fs::read_dir(source).expect("read dir") {
        let entry = entry.expect("dir entry");
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

fn config_for(sandbox: &Path, _entry: &Path, port: Arc<dyn SessionPort>) -> PluginHostConfig {
    PluginHostConfig {
        env: vec![
            ("OPENCODE_CONFIG_DIR".to_owned(), sandbox.join("config").display().to_string()),
            // Belt-and-braces: the plugin's config search also consults
            // XDG_CONFIG_HOME/HOME for its DEFAULT dir — pin both so
            // nothing outside the sandbox is ever read (hermetic gate).
            ("XDG_CONFIG_HOME".to_owned(), sandbox.join("config").display().to_string()),
            ("HOME".to_owned(), sandbox.join("home").display().to_string()),
            ("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string()),
        ],
        registration_timeout: Duration::from_secs(30),
        dispatch_timeout: Duration::from_secs(15),
        host_state: oc_plugin_host::HostState {
            app_version: "2.0.24-rust".into(),
            project_id: "proj_flagship".into(),
            directory: sandbox.join("project").display().to_string(),
            agents: seed_agents(),
            mcps: std::collections::BTreeMap::default(),
        },
        session_port: Some(port),
        ..PluginHostConfig::default()
    }
}

/// Boot the host over the staged flagship; `None` = skipped.
fn boot_flagship(
    name: &str,
    extra_env: Vec<(String, String)>,
) -> Option<(PluginHost, Arc<common::RecordingSessionPort>)> {
    let (sandbox, entry) = stage(name)?;
    let port = Arc::new(common::RecordingSessionPort::default());
    let mut config = config_for(&sandbox, &entry, port.clone());
    config.env.extend(extra_env);
    let host = PluginHost::detect().start_detached_with(config, std::slice::from_ref(&entry));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(60)), "host ready: {:?}", host.start_error());
    Some((host, port))
}

#[test]
fn flagship_loads_and_registers_on_the_minimal_context() {
    let Some((host, _port)) = boot_flagship("flagship-load", Vec::new()) else {
        return;
    };
    let snapshot = host.registrations();
    assert_flagship_active(&snapshot);
    assert_flagship_agents_and_preset(&snapshot);
    assert_flagship_palette_surfaces(&snapshot);
    assert_flagship_registry_seam(&snapshot);

    // -- the event stream: a tapped event reaches the guest, host alive ----
    let tap = host.tap();
    assert!(tap.is_live());
    tap.publish(oc_plugin_host::TapEvent::new(
        "session.created",
        json!({ "info": { "id": "ses_flagship", "title": "flagship" } }),
    ));
    assert!(
        common::wait_for(Duration::from_secs(10), || host.dispatch_counters().0 >= 1),
        "event dispatch reached the guest ({:?})",
        host.dispatch_counters()
    );

    host.shutdown();
}

fn assert_flagship_active(snapshot: &RegistrationSnapshot) {
    let status = snapshot
        .plugins
        .iter()
        .find(|status| status.plugin_id == "oh-my-opencode-slim")
        .expect("flagship status");
    assert_eq!(status.state, oc_plugin_host::PluginState::Active, "setup completed");
}

/// One agent's materialized `Agent.Info`.
fn flagship_agent(snapshot: &RegistrationSnapshot, id: &str) -> Value {
    snapshot
        .agents
        .iter()
        .find(|agent| agent.info.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("agent {id} registered"))
        .info
        .clone()
}

fn assert_flagship_agents_and_preset(snapshot: &RegistrationSnapshot) {
    for id in ["orchestrator", "build", "general"] {
        let _ = flagship_agent(snapshot, id);
    }
    for id in ["oracle", "librarian", "explorer", "designer", "fixer"] {
        assert_eq!(flagship_agent(snapshot, id)["mode"], json!("subagent"), "{id} is a subagent");
    }
    // The preset is the machine's config (`preset: "zhipuai"` pins
    // own/glm-5.3 on the orchestrator): proving the model landed
    // proves the plugin parsed + applied its config preset.
    let orchestrator = flagship_agent(snapshot, "orchestrator");
    let model = orchestrator["model"].as_object().expect("orchestrator model");
    assert_eq!(model["providerID"], json!("own"), "preset provider: {model:?}");
    assert!(
        model["id"].as_str().is_some_and(|id| id.contains("glm")),
        "preset model pin (own/glm): {model:?}"
    );
    let default = snapshot.default_agent.as_deref().unwrap_or_default();
    assert!(!default.is_empty(), "a default agent was set");
    assert_eq!(
        flagship_agent(snapshot, default)["mode"],
        json!("primary"),
        "the default agent is selectable"
    );
}

fn assert_flagship_palette_surfaces(snapshot: &RegistrationSnapshot) {
    // -- commands -----------------------------------------------------------
    let command_names: Vec<&str> =
        snapshot.commands.iter().map(|command| command.name.as_str()).collect();
    for expected in ["reflect", "deepwork", "loop", "interview"] {
        assert!(
            command_names.contains(&expected),
            "command {expected} registered (got {command_names:?})"
        );
    }

    // -- skills (the bundled SKILL.md documents) -----------------------------
    let skill_ids: Vec<&str> = snapshot.skills.iter().map(|skill| skill.id.as_str()).collect();
    for expected in ["reflect", "simplify", "codemap", "deepwork", "worktrees"] {
        assert!(skill_ids.contains(&expected), "skill {expected} registered (got {skill_ids:?})");
    }
    let reflect = snapshot.skills.iter().find(|skill| skill.id == "reflect").expect("reflect");
    assert!(
        reflect
            .info
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.is_empty()),
        "skill content carried"
    );

    // -- tools + MCP + hook coverage ------------------------------------------
    let tool_names: Vec<&str> = snapshot.tools.iter().map(|tool| tool.name.as_str()).collect();
    assert!(
        tool_names.contains(&"marketplace_inspect"),
        "marketplace_inspect tool (got {tool_names:?})"
    );
    assert!(!snapshot.mcps.is_empty(), "MCP servers registered");
    let hook = |domain: &str, name: &str| {
        snapshot.hooks.iter().any(|hook| hook.domain == domain && hook.name == name)
    };
    assert!(hook("session", "prompt"), "session.prompt hook");
    assert!(hook("session", "context"), "session.context hook");
    assert!(hook("session", "compaction"), "session.compaction hook");
    assert!(hook("tool", "execute.before"), "tool.execute.before hook");
    assert!(hook("tool", "execute.after"), "tool.execute.after hook");
}

/// The oc-core seam: the agent table lands in the `AgentRegistry`.
fn assert_flagship_registry_seam(snapshot: &RegistrationSnapshot) {
    let registry = oc_core::location::AgentRegistry::new();
    let infos: Vec<Value> = snapshot.agents.iter().map(|agent| agent.info.clone()).collect();
    let applied = oc_core::plugin_apply::apply_plugin_agents(&registry, &infos);
    assert_eq!(applied, snapshot.agents.len(), "every agent parsed into the registry");
    oc_core::plugin_apply::apply_plugin_agent_default(&registry, snapshot.default_agent.as_deref());
    let orchestrator = registry
        .get(&oc_schema::id::AgentId::parse("orchestrator"))
        .expect("orchestrator in the AgentRegistry");
    assert!(
        orchestrator.model.as_ref().is_some_and(|model| model.provider_id.0 == *"own"),
        "preset model survived the registry seam"
    );
}

/// Skill end-to-end (flagship acceptance step 1): the `reflect`
/// command dispatches through the host, its marker prompt crosses the
/// session seam, and the plugin's session-context hook chain rewrites
/// it into the reflect activation prompt — no model needed (the
/// non-model path IS the rewrite).
#[test]
fn flagship_reflect_skill_end_to_end() {
    let Some((host, port)) = boot_flagship("flagship-e2e", Vec::new()) else {
        return;
    };

    let result = host.execute_command_in_session(
        "reflect",
        Some("--sessions --last 7 retry logic"),
        "ses_reflect",
        Duration::from_secs(20),
    );
    result.expect("reflect command executed");

    // 1. The command submitted its marker prompt into the session.
    let prompts = port.prompts.lock().unwrap();
    let (session_id, text) = prompts.first().expect("prompt crossed the session seam").clone();
    assert_eq!(session_id, "ses_reflect");
    assert_eq!(
        text,
        "<omos-cmd-command data-name=\"reflect\">--sessions --last 7 retry logic</omos-cmd-command>"
    );

    // 2. The context hook chain rewrote it into the activation prompt
    //    (reflect's non-model path).
    let contexts = port.contexts.lock().unwrap();
    let event = contexts.last().expect("context event captured");
    let trailing =
        event["messages"].as_array().expect("messages").last().expect("trailing").clone();
    assert_eq!(trailing["role"], json!("user"));
    let content = trailing["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        content.contains("Use the reflect skill for this request."),
        "reflect activation prompt rewritten (got: {content:.200})"
    );
    assert!(
        content.contains("Analyze the last 7 sessions"),
        "--last 7 propagated (got: {content:.400})"
    );
    assert!(content.contains("retry logic"), "focus argument propagated (got: {content:.400})");

    host.shutdown();
}

/// Config-preset processing, live: the same installed tree driven with
/// a DIFFERENT preset (`OH_MY_OPENCODE_SLIM_PRESET=openai`) must land
/// different models on the same agents — proving the plugin parses
/// and applies its config preset per-run, not a baked default.
#[test]
fn flagship_config_preset_processing() {
    let Some((host, _port)) = boot_flagship(
        "flagship-preset",
        vec![("OH_MY_OPENCODE_SLIM_PRESET".to_owned(), "openai".to_owned())],
    ) else {
        return;
    };

    let snapshot = host.registrations();
    let model_of = |agent: &str| -> (String, String) {
        flagship_agent(&snapshot, agent)
            .get("model")
            .and_then(Value::as_object)
            .map(|model| {
                (
                    model.get("providerID").and_then(Value::as_str).unwrap_or_default().to_owned(),
                    model.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                )
            })
            .expect("model present")
    };
    let (orchestrator_provider, orchestrator_model) = model_of("orchestrator");
    assert_eq!(orchestrator_provider, "openai", "the openai preset overrides the default");
    assert!(
        orchestrator_model.contains("gpt-5.6"),
        "openai preset pins gpt-5.6 (got {orchestrator_model})"
    );
    let (_, librarian_model) = model_of("librarian");
    assert!(
        librarian_model.contains("luna"),
        "openai preset pins luna on the librarian (got {librarian_model})"
    );
    // The variant rode along as well (preset pins `"variant": "low"`).
    assert_eq!(
        flagship_agent(&snapshot, "librarian")["model"]["variant"],
        json!("low"),
        "variant propagated"
    );

    host.shutdown();
}
