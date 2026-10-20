//! PLUG-D35 — plugin-registered skills enter the engine skill table.
//!
//! The host's registration snapshot carries `ctx.skill.transform`
//! results (`PluginSkill`, v2 `Skill.Info` JSON); the engine seam
//! ([`oc_core::plugin_apply::apply_location_plugins`]) must merge them
//! into the location's [`oc_core::engine::skills::ConfigSkillTable`] —
//! v2 `plugin/skill.ts` registers the built-ins through the SAME
//! `Skill.Service` editor the config plugin and third-party plugins
//! share, so a plugin skill is listable and `@skill`-usable.
//!
//! Hermetic: pure in-memory (no fs, no bun, no network).

use std::sync::Arc;

use oc_core::engine::skills::ConfigSkillTable;
use oc_core::hooks::HookRegistry;
use oc_core::location::{AgentRegistry, ToolRegistryState};
use oc_core::plugin_apply::{apply_location_plugins, LocationPluginSource, PluginSkillDef};
use oc_schema::skill::Skill;
use serde_json::json;

struct SkillOnlySource {
    skills: Vec<serde_json::Value>,
}

impl LocationPluginSource for SkillOnlySource {
    fn skills(&self) -> Vec<PluginSkillDef> {
        self.skills
            .iter()
            .map(|info| PluginSkillDef { plugin_id: "skillplugin".to_owned(), info: info.clone() })
            .collect()
    }
}

fn skill_info(id: &str, description: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": id,
        "description": description,
        "path": format!("/plugin/{id}/SKILL.md"),
        "content": format!("# {id}\nplugin-bundled body"),
    })
}

/// A location-shaped table pair (the same registries the engine's
/// `assemble` builds).
struct Table {
    skills: Arc<ConfigSkillTable>,
    hooks: Arc<HookRegistry>,
    agents: Arc<AgentRegistry>,
    tools: Arc<ToolRegistryState>,
}

fn table() -> Table {
    Table {
        skills: Arc::new(ConfigSkillTable::empty()),
        hooks: Arc::new(HookRegistry::new()),
        agents: Arc::new(AgentRegistry::new()),
        tools: Arc::new(ToolRegistryState::new()),
    }
}

/// The core regression: a plugin skill lands in the engine table (the
/// same `SkillTable` the session context and the skill tool consult).
#[tokio::test]
async fn plugin_skill_in_table() {
    let table = table();
    let source = SkillOnlySource { skills: vec![skill_info("reflect", "Reflect on the session")] };
    apply(&source, &table).await;
    let skills = table.skills.list();
    let reflect = skills.iter().find(|skill| skill.id.as_str() == "reflect").expect("skill merged");
    assert_eq!(reflect.description.as_deref(), Some("Reflect on the session"));
    assert!(reflect.content.contains("plugin-bundled body"));
    // The SkillTable read side (what the skill tool / @skill hits).
    let fetched = oc_core::tool::skill::SkillTable::get(table.skills.as_ref(), "reflect");
    assert!(fetched.is_some(), "plugin skill is get()-able through the SkillTable trait");
}

/// v2 ordering: plugins run AFTER the config skill plugin, so a plugin
/// skill wins the id collision (`editor.add` overwrite).
#[tokio::test]
async fn plugin_skill_overwrites_config_collision() {
    let table = table();
    table.skills.merge_plugin_skill(Skill {
        id: oc_schema::id::SkillId::parse("reflect"),
        name: oc_schema::id::SkillName::parse("ConfigReflect"),
        description: Some("config-provided".into()),
        auto_invoke: None,
        path: oc_schema::common::AbsolutePath::parse("/config/reflect/SKILL.md"),
        content: "config body".into(),
    });
    let source = SkillOnlySource { skills: vec![skill_info("reflect", "plugin version")] };
    apply(&source, &table).await;
    let skills = table.skills.list();
    let reflect = skills.iter().find(|skill| skill.id.as_str() == "reflect").unwrap();
    assert_eq!(reflect.description.as_deref(), Some("plugin version"), "plugin wins the collision");
}

/// A malformed entry declines ITSELF only (contained — the same rule
/// the agents arm applies); healthy neighbors still merge.
#[tokio::test]
async fn malformed_plugin_skill_is_contained() {
    let table = table();
    let source = SkillOnlySource {
        skills: vec![
            json!({ "nope": 1 }),
            json!({ "id": "good", "name": "Good", "path": "/p/good/SKILL.md", "content": "c" }),
        ],
    };
    apply(&source, &table).await;
    let listed = table.skills.list();
    let ids: Vec<&str> = listed.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["good"], "garbage declined, healthy merged: {ids:?}");
}

async fn apply(source: &SkillOnlySource, table: &Table) {
    let mcp = test_mcp_registry().await;
    apply_location_plugins(
        source,
        &table.hooks,
        &table.agents,
        &table.tools,
        None,
        &mcp,
        &table.skills,
        &[],
    )
    .await;
}

async fn test_mcp_registry() -> oc_core::mcp::McpRegistry {
    let database = oc_core::database::Database::open_in_memory().await.expect("database");
    let bus = oc_core::bus::Bus::new(database, oc_core::bus::BusOptions::deterministic());
    let connector: oc_core::mcp::Connector = Arc::new(|input: oc_core::mcp::ConnectInput<'_>| {
        let server = input.server.to_string();
        Box::pin(async move {
            Err(oc_core::mcp::ConnectFailure::Connect(oc_core::mcp::ConnectError {
                server,
                message: "test connector".to_owned(),
            }))
        })
    });
    oc_core::mcp::McpRegistry::with_connector(bus, connector, Arc::new(oc_core::mcp::NoCredentials))
        .with_directory("/tmp")
}
