//! The JS-facing Context's Rust backing — wave 2 (flagship bring-up).
//!
//! `host.js` implements the minimal `ctx` surface the flagship plugin
//! (oh-my-opencode-slim v3.0.3) actually touches; this module holds the
//! Rust side of the domains that need more than a registration insert:
//!
//! * [`HostState`] — the host-owned base state seeded INTO the guest at
//!   load (the agent/MCP tables the flagship's transform editors
//!   snapshot and mutate; registration-time pre-resolution, the
//!   oracle-gate11 §4 sync-callback rule, flows host→guest here).
//! * [`SessionPort`] — `ctx.session.*` call routing. Guest session
//!   requests are answered **asynchronously** (spawned off the IO
//!   reader — deadlock rule 1 stays intact), because a real port (the
//!   oc-core engine seam) may need to dispatch session hooks BACK into
//!   the guest through [`Bridge::dispatch_hook`] — a nested RPC that
//!   must never block the reader.
//! * [`Bridge`] — the port's view of the host: hook dispatch + the
//!   registration snapshot.
//! * [`install`] — the pinned, frozen npm/bun install path
//!   (`opencode plugin add` → real installation).

pub mod codemode_defaults;
pub mod install;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::registrations::RegistrationSnapshot;
use crate::supervisor::Shared;
use crate::HostError;

/// Host-owned base state for the guest's transform domains (seeded at
/// load; the guest replays plugin editors over it, `reload()`-style).
#[derive(Debug, Clone, Default)]
pub struct HostState {
    /// Reported via `ctx.app.version` (logged by plugins).
    pub app_version: String,
    /// `ctx.location.project.id`.
    pub project_id: String,
    /// `ctx.directory` / `ctx.location.directory` — the project the
    /// host was started for.
    pub directory: String,
    /// The native agent table (v2 `Agent.Info` JSON array; the oc-core
    /// `AgentRegistry` contents — build/general/…).
    pub agents: Vec<Value>,
    /// The native MCP server configs (name → v2 `Mcp.ServerConfig`).
    pub mcps: BTreeMap<String, Value>,
}

impl HostState {
    /// The serialized seed sent in the `load` params. The MCP table
    /// passes through the v2 `opencode.mcp.codemode.defaults`
    /// transform first (executor.sh / `executor mcp` ⇒ our Code Mode
    /// off; `PostHog` ⇒ the `tools` header) — the guest's editors
    /// snapshot the defaulted base.
    #[must_use]
    pub fn to_seed_json(&self) -> Value {
        let mut mcps = self.mcps.clone();
        for config in mcps.values_mut() {
            codemode_defaults::apply_to_server(config);
        }
        serde_json::json!({
            "appVersion": self.app_version,
            "project": { "id": self.project_id },
            "directory": self.directory,
            "agents": self.agents,
            "mcps": mcps,
        })
    }
}

/// `ctx.session.*` routing target. Implementations own the real session
/// engine loop (the oc-core engine seam); the default port declines
/// every call and the plugin shims degrade (logged guest-side).
///
/// Calls arrive spawned off the IO reader; a port may freely call back
/// into the guest through `bridge` (nested RPC is the port's to manage
/// — the reader is never blocked on it).
#[async_trait]
pub trait SessionPort: Send + Sync {
    /// One `ctx.session.<method>` call; `args` is the guest's argument
    /// object verbatim. `Ok(value)` answers the guest (accessible as
    /// the resolved `value`); `Err(message)` rejects it.
    async fn call(&self, bridge: &Bridge, method: &str, args: &Value) -> Result<Value, String>;
}

/// The declining default: every method errors with a clear message.
pub struct NoSessionEngine;

#[async_trait]
impl SessionPort for NoSessionEngine {
    async fn call(&self, _bridge: &Bridge, method: &str, _args: &Value) -> Result<Value, String> {
        Err(format!("session.{method}: no session engine attached to the plugin host"))
    }
}

/// The port's view of the running host.
pub struct Bridge {
    pub(crate) shared: Arc<Shared>,
}

/// Hard per-dispatch timeout for BODY-MUTATING hooks — aligned with
/// oc-server's `EXECUTE_TIMEOUT` (120 s) per the RC-03 ruling: a
/// `tool.execute.*` / `tool.transform` / `chat.*` / `session.*`
/// handler legitimately rides a full tool execution or model round
/// trip, so the old 5 s dispatch budget killed healthy handlers.
/// Cheap query hooks (e.g. `shell.create.before`) keep the configured
/// [`crate::PluginHostConfig::dispatch_timeout`] (5 s default), and
/// registration stays at its own 2 s budget.
pub const MUTATING_DISPATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// The two-class dispatch classification (RC-03): which hook points
/// are body-mutating and thus ride [`MUTATING_DISPATCH_TIMEOUT`].
#[must_use]
pub fn is_mutating_hook(domain: &str, name: &str) -> bool {
    match domain {
        // session.* — every point observes/mutates a live message body
        // or rides the model round trip. chat.message-style body
        // mutators (future domain) are classified wholesale like
        // session.
        "session" | "chat" => true,
        // tool.execute.* wrap a full tool execution; tool.transform
        // rewrites the tool body itself.
        "tool" => name == "transform" || name.starts_with("execute."),
        _ => false,
    }
}

impl Bridge {
    /// Dispatch one runtime hook event into the guest for `plugin_id`
    /// (sequential handlers, same object mutated). Returns the possibly
    /// mutated event. Timeout-bounded by the dispatch timeout — the
    /// 120 s mutating class for body-mutating hook points, the
    /// configured (5 s) query class otherwise.
    ///
    /// # Errors
    /// [`HostError`] on timeout/guest failure.
    pub async fn dispatch_hook(
        &self,
        plugin_id: &str,
        domain: &str,
        name: &str,
        event: Value,
    ) -> Result<Value, HostError> {
        let timeout = if is_mutating_hook(domain, name) {
            MUTATING_DISPATCH_TIMEOUT
        } else {
            self.shared.config.dispatch_timeout
        };
        let result = self
            .shared
            .request(
                "hook.dispatch",
                serde_json::json!({
                    "pluginId": plugin_id,
                    "domain": domain,
                    "name": name,
                    "event": event,
                }),
                timeout,
            )
            .await?;
        Ok(result.get("event").cloned().unwrap_or(Value::Null))
    }

    /// Plugins subscribed to one hook point (registration order — v2
    /// fires callbacks in registration order, so the bridge chain must
    /// dispatch plugins in the order their subscriptions arrived; a
    /// plugin registering the same point twice dispatches once).
    #[must_use]
    pub fn hook_subscribers(&self, domain: &str, name: &str) -> Vec<String> {
        let registrations = self.shared.registrations.read().expect("registrations poisoned");
        let mut seen = std::collections::BTreeSet::new();
        registrations
            .hooks
            .iter()
            .filter(|hook| hook.domain == domain && hook.name == name)
            .map(|hook| hook.plugin_id.clone())
            .filter(|plugin_id| seen.insert(plugin_id.clone()))
            .collect()
    }

    /// The current registration snapshot.
    #[must_use]
    pub fn registrations(&self) -> RegistrationSnapshot {
        self.shared.registrations.read().expect("registrations poisoned").snapshot()
    }
}

/// The guest-facing session method allowlist (`ctx.session.*`) — the
/// v2 `SessionDomain` 15-method `Pick` (plugin/effect/session.ts) plus
/// the `messages`/`list` reads the flagship probes.
pub const SESSION_METHODS: &[&str] = &[
    "create",
    "get",
    "interrupt",
    "context",
    "messages",
    "list",
    "prompt",
    "generate",
    "command",
    "compact",
    "synthetic",
    "update",
    "remove",
    "move",
    "wait",
    "switchAgent",
    "switchModel",
];

/// The Code Mode bridge methods (`codemode.*`) — the same
/// async-off-the-reader routing the session methods use (deadlock rule
/// 1), applied to `tools.<path>(input)` calls from inside an `execute`
/// program.
pub const CODEMODE_METHODS: &[&str] = &["codemode.tool"];

/// The `ctx.generate.*` methods routed through the session port (the
/// engine seam owns one-off generation, `GenerateApi`).
pub const GENERATE_METHODS: &[&str] = &["generate.text"];

/// `ctx.tool` execution routing target (the wave-4 dispatch seam): a
/// nested `tools.<name>(input)` call inside a Code Mode program lands
/// here. The implementation owns the real engine loop — typically
/// resolving the tool from the oc-core registry (the full pipeline:
/// hooks fire per nested call) and answering with the tool's JSON
/// result; `Err` rejects inside the program.
///
/// Calls arrive spawned off the IO reader exactly like
/// [`SessionPort::call`].
#[async_trait]
pub trait ToolPort: Send + Sync {
    async fn call(&self, bridge: &Bridge, name: &str, input: &Value) -> Result<Value, String>;

    /// [`ToolPort::call`] with the evaluation token the host assigned
    /// to the enclosing `codemode.evaluate` request (the production
    /// adapter routes nested calls onto the REQUEST's executor closure
    /// — the right location registry and session context — keyed by
    /// it). The default ignores the token (port-owned routing).
    async fn call_in(
        &self,
        bridge: &Bridge,
        name: &str,
        input: &Value,
        token: u64,
    ) -> Result<Value, String> {
        let _ = token;
        self.call(bridge, name, input).await
    }
}

/// The declining default: every tool call errors with a clear message.
pub struct NoToolEngine;

#[async_trait]
impl ToolPort for NoToolEngine {
    async fn call(&self, _bridge: &Bridge, name: &str, _input: &Value) -> Result<Value, String> {
        Err(format!("tool call {name}: no tool engine attached to the plugin host"))
    }
}

/// `ctx.permission.*` routing target (PLUG-D11 — the v2
/// `PermissionDomain`: `list`/`get`/`reply`). The implementation owns
/// the real permission machinery (the oc-core engine seam: pending
/// asks per session, `Reply` resolution); calls arrive spawned off the
/// IO reader exactly like [`SessionPort::call`].
#[async_trait]
pub trait PermissionPort: Send + Sync {
    /// The pending requests for one session (v2
    /// `permission.forSession`) as `Permission.Request` JSON.
    async fn list(&self, session_id: &str) -> Result<Value, String>;

    /// One pending request (v2 errors `Permission request not found`
    /// when the id is unknown or belongs to another session).
    async fn get(&self, session_id: &str, request_id: &str) -> Result<Value, String>;

    /// Resolve one pending request (`decision`: `once` | `always` |
    /// `reject`, v2 `Permission.Reply`).
    async fn reply(
        &self,
        session_id: &str,
        request_id: &str,
        decision: &str,
        message: Option<String>,
    ) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_json_carries_the_domain_bases() {
        let state = HostState {
            app_version: "2.0.24".into(),
            project_id: "proj_1".into(),
            directory: "/work".into(),
            agents: vec![serde_json::json!({ "id": "build" })],
            mcps: BTreeMap::from([("demo".to_owned(), serde_json::json!({ "type": "local" }))]),
        };
        let seed = state.to_seed_json();
        assert_eq!(seed["directory"], "/work");
        assert_eq!(seed["project"]["id"], "proj_1");
        assert_eq!(seed["agents"][0]["id"], "build");
        assert_eq!(seed["mcps"]["demo"]["type"], "local");
    }

    #[test]
    fn session_methods_cover_the_flagship_surface() {
        for method in ["prompt", "context", "get", "list", "messages", "switchModel", "wait"] {
            assert!(SESSION_METHODS.contains(&method), "missing {method}");
        }
    }

    /// PLUG-D10: the v2 `SessionDomain` guest surface
    /// (`plugin/effect/session.ts` — the 15-method `Pick`).
    #[test]
    fn session_methods_cover_the_v2_domain() {
        for method in [
            "create",
            "get",
            "remove",
            "switchAgent",
            "switchModel",
            "prompt",
            "generate",
            "command",
            "compact",
            "synthetic",
            "interrupt",
            "update",
            "move",
            "wait",
            "context",
        ] {
            assert!(SESSION_METHODS.contains(&method), "missing v2 session method {method}");
        }
    }
}
