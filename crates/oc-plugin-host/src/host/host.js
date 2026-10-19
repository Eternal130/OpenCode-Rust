//! The guest-side host script (`bun host.js`): module loading via
//! dynamic `import()` (native ESM + npm + CJS resolution — whatever
//! Bun runs), per-plugin error containment, the Context bridge
//! (wave 1: commands/themes/events; wave 2: the flagship's minimal
//! domain set — agent/mcp/skill/tool/command transforms, hooks,
//! storage KV, session routing, the async-iterable event stream),
//! event fan-out, and the digest-scan hot-reload watcher.
//!
//! Productionized from the `/tmp/opencode/spike-bun` `host.ts`
//! prototype (spike-plugin-runtime.md): NDJSON JSON-RPC over stdio,
//! both directions; guest→host sync calls are plain Promises settled
//! by supervisor replies; the read loop NEVER awaits a handler before
//! reading the next line (the mirrored deadlock rule — see the crate
//! docs §Deadlock protocol).
//!
//! Transform-domain semantics (v2 `State.Transformable`, mirrored by
//! oc-core's `TransformDomain`): the domain state is process-global,
//! seeded from the host at load (the registration-time pre-resolution
//! flowing host→guest); each `transform(cb)` appends a SYNC editor
//! that runs immediately and replays on `reload()`; the derived table
//! materializes to the supervisor after every change. Nothing crosses
//! IPC at render time.

// The script runs under Bun; `require` is constructed for the CJS
// cache eviction the hot-reload parity needs.
import { createRequire } from "node:module";
import { existsSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { basename } from "node:path";
const require = createRequire(import.meta.url);

// ---------------------------------------------------------------------------
// Wire plumbing
// ---------------------------------------------------------------------------

// Guest request ids start at 2^40 — written as the literal, NOT
// `1 << 40` (JavaScript shifts are 32-bit: `1 << 40 === 256`), so the
// host's (small) and guest's (huge) id spaces are genuinely disjoint
// (mirrors protocol::GUEST_ID_BASE).
const GUEST_ID_BASE = 1099511627776;
let nextGuestId = GUEST_ID_BASE;
const pendingGuest = new Map(); // id -> { resolve, reject }

function send(message) {
  process.stdout.write(JSON.stringify(message) + "\n");
}

function notify(method, params) {
  send({ jsonrpc: "2.0", method, params });
}

function hostRequest(method, params) {
  return new Promise((resolve, reject) => {
    const id = nextGuestId++;
    pendingGuest.set(id, { resolve, reject });
    send({ jsonrpc: "2.0", id, method, params });
  });
}

function errText(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

// Plugin console output must never corrupt the NDJSON stdout channel:
// console methods are rerouted to `log` notifications.
for (const level of ["log", "info", "warn", "error", "debug"]) {
  console[level] = (...args) => {
    const message = args
      .map((arg) => (typeof arg === "string" ? arg : JSON.stringify(arg)))
      .join(" ");
    notify("log", { level, message });
  };
}

process.on("uncaughtException", (error) => {
  notify("log", { level: "error", message: `uncaughtException: ${errText(error)}` });
});
process.on("unhandledRejection", (reason) => {
  notify("log", { level: "error", message: `unhandledRejection: ${errText(reason)}` });
});

// SA-F16 (SCAN-AB): shield every SCHEDULED guest callback — a
// plugin's periodic filesystem probing that walks permission-denied
// directories (the observed `EACCES /lost+found`, `/tmp/tmux-0`)
// must degrade to a recorded fault, never an uncaught throw at
// process level. The wrapper preserves the timer handles and the
// callbacks' return values; only the error path changes (a
// `plugin.fault` notification the supervisor records as a health
// result, plus the log line).
function scheduledFault(error) {
  notify("plugin.fault", { message: errText(error) });
  notify("log", { level: "warn", message: `plugin scheduled callback fault: ${errText(error)}` });
}
function shieldCallback(fn) {
  return (...args) => {
    try {
      const result = fn(...args);
      if (result && typeof result.then === "function") {
        result.then(undefined, scheduledFault);
      }
      return result;
    } catch (error) {
      scheduledFault(error);
      return undefined;
    }
  };
}
{
  const nativeSetInterval = globalThis.setInterval.bind(globalThis);
  const nativeSetTimeout = globalThis.setTimeout.bind(globalThis);
  globalThis.setInterval = (fn, ms, ...args) =>
    typeof fn === "function" ? nativeSetInterval(shieldCallback(fn), ms, ...args) : nativeSetInterval(fn, ms, ...args);
  globalThis.setTimeout = (fn, ms, ...args) =>
    typeof fn === "function" ? nativeSetTimeout(shieldCallback(fn), ms, ...args) : nativeSetTimeout(fn, ms, ...args);
}

// ---------------------------------------------------------------------------
// Plugin registry
// ---------------------------------------------------------------------------

/** @type {Map<string, { id, entry, dispose, subscriptions, commands }>} */
const plugins = new Map(); // pluginId -> record
const loadingEntries = new Set(); // entries mid-import (crash attribution aid)

function deriveId(entry) {
  const base = basename(entry).replace(/\.[cm]?[jt]sx?$/, "");
  return base || "plugin";
}

// V2/V1 dual shape tolerance (plugin-map §1 + §10): `{id, setup}` is
// the v2 promise contract; `server()` (object or older function
// default export) is the v1 contract — tolerated and IGNORED by v2.
function classify(defaultExport) {
  const def = defaultExport;
  if (def && typeof def === "object") {
    if (typeof def.id === "string" && typeof def.setup === "function") {
      return { kind: "v2", id: def.id, setup: def.setup };
    }
    if (typeof def.id === "string" && typeof def.effect === "function") {
      return { kind: "effect", id: def.id };
    }
    if (typeof def.server === "function") return { kind: "v1", id: null };
    return { kind: "none" };
  }
  if (typeof def === "function") return { kind: "v1", id: null };
  return { kind: "none" };
}

// ---------------------------------------------------------------------------
// Transform domains (process-global state, seeded from the host)
// ---------------------------------------------------------------------------

/** The seeded base (host state; agents/mcps from the embedder). */
let domainBase = { agents: [], mcps: {}, appVersion: "2.0.24", directory: process.cwd(), project: { id: "global" } };

/** Current derived state: base ⊕ editors (editors replayed on reload). */
const domainStores = {
  agents: new Map(), // id -> Agent.Info (v2 agent.ts editor semantics)
  agentDefault: undefined,
  mcps: new Map(), // name -> Mcp.ServerConfig
  skills: new Map(), // id -> Skill.Info
  tools: new Map(), // name -> tool definition
};

/** Registered editors per domain, in registration order. */
const domainEditors = { agent: [], mcp: [], skill: [], tool: [], command: [] };

function defaultAgent(id) {
  // v2 Agent.Info.default (packages/schema/src/agent.ts).
  return {
    id,
    name: id,
    request: { settings: {}, headers: {}, body: {} },
    mode: "primary",
    hidden: false,
    permissions: [
      { action: "*", resource: "*", effect: "allow" },
      { action: "read", resource: "*.env", effect: "ask" },
      { action: "read", resource: "*.env.*", effect: "ask" },
      { action: "read", resource: "*.env.example", effect: "allow" },
    ],
  };
}

const editors = {
  agent: {
    list: () => Array.from(domainStores.agents.values()),
    get: (id) => domainStores.agents.get(id),
    default: (id) => {
      domainStores.agentDefault = id;
    },
    update: (id, fn) => {
      let current = domainStores.agents.get(id);
      if (!current) {
        current = defaultAgent(id);
        domainStores.agents.set(id, current);
      }
      fn(current);
      current.id = id;
    },
    remove: (id) => {
      domainStores.agents.delete(id);
    },
  },
  mcp: {
    list: () => Array.from(domainStores.mcps.entries()),
    get: (name) => domainStores.mcps.get(name),
    set: (name, config) => {
      domainStores.mcps.set(name, config);
    },
    update: (name, fn) => {
      let current = domainStores.mcps.get(name);
      if (!current) {
        current = {};
        domainStores.mcps.set(name, current);
      }
      fn(current);
    },
    remove: (name) => {
      domainStores.mcps.delete(name);
    },
  },
  skill: {
    list: () => Array.from(domainStores.skills.values()),
    get: (id) => domainStores.skills.get(id),
    add: (info) => {
      const id = typeof info?.id === "string" && info.id ? info.id : info?.name;
      if (!id) throw new Error("skill.add requires an id");
      domainStores.skills.set(id, info);
    },
    update: (id, fn) => {
      const current = domainStores.skills.get(id);
      if (!current) return;
      fn(current);
      current.id = id;
    },
    remove: (id) => {
      domainStores.skills.delete(id);
    },
  },
  tool: {
    list: () => Array.from(domainStores.tools.values()),
    get: (name) => domainStores.tools.get(name),
    add: (def) => {
      const name = typeof def?.name === "string" ? def.name : "";
      if (!name) throw new Error("tool.add requires a name");
      domainStores.tools.set(name, def);
    },
    update: (name, fn) => {
      const current = domainStores.tools.get(name);
      if (!current) return;
      fn(current);
    },
    remove: (name) => {
      domainStores.tools.delete(name);
    },
  },
  command: {
    add: (definition) => definition, // shaped in the domain wrapper
  },
};

function cloneJson(value) {
  return structuredClone(value);
}

/** Reset every domain to the seeded base (pre-replay). */
function resetDomainsToBase() {
  domainStores.agents = new Map(domainBase.agents.filter((a) => a?.id).map((a) => [a.id, cloneJson(a)]));
  domainStores.agentDefault = undefined;
  domainStores.mcps = new Map(Object.entries(domainBase.mcps ?? {}).map(([k, v]) => [k, cloneJson(v)]));
  // skills/tools have no host-discovered base in this wave; editors
  // fully own their tables.
  domainStores.skills = new Map();
  domainStores.tools = new Map();
}

let domainsSeeded = false;

function seedDomains(seed) {
  domainBase = {
    agents: Array.isArray(seed?.agents) ? seed.agents : [],
    mcps: seed?.mcps && typeof seed.mcps === "object" ? seed.mcps : {},
    appVersion: typeof seed?.appVersion === "string" && seed.appVersion ? seed.appVersion : "2.0.24",
    directory: typeof seed?.directory === "string" && seed.directory ? seed.directory : process.cwd(),
    project: { id: seed?.project?.id ?? "global" },
  };
  // Seed ONCE (the host's base state): later loads (a second plugin,
  // or a hot reload after unload disposed every editor) re-derive from
  // this base instead of wiping contributions mid-flight. The one
  // reset case: a reload generation with NO surviving editors starts
  // from a clean base.
  if (domainsSeeded) {
    const editorsAlive = Object.values(domainEditors).some((list) => list.length > 0);
    if (editorsAlive) return;
    resetDomainsToBase();
    return;
  }
  domainsSeeded = true;
  resetDomainsToBase();
}

async function materializeDomain(kind, pluginId) {
  if (kind === "agent") {
    await hostRequest("agent.transform", {
      pluginId,
      agents: Array.from(domainStores.agents.values()),
      default: domainStores.agentDefault ?? null,
    });
  } else if (kind === "mcp") {
    await hostRequest("mcp.transform", {
      pluginId,
      servers: Object.fromEntries(domainStores.mcps),
    });
  } else if (kind === "skill") {
    await hostRequest("skill.transform", {
      pluginId,
      skills: Array.from(domainStores.skills.values()),
    });
  } else if (kind === "tool") {
    await hostRequest("tool.transform", {
      pluginId,
      tools: Array.from(domainStores.tools.values()),
    });
  }
}

/**
 * Re-derive one domain: base ⊕ its editors (in order), then
 * materialize. `reload()` and editor disposal both land here.
 */
async function rederiveDomain(kind, pluginId) {
  const bases = {
    agent: () => {
      domainStores.agents = new Map(domainBase.agents.filter((a) => a?.id).map((a) => [a.id, cloneJson(a)]));
      domainStores.agentDefault = undefined;
    },
    mcp: () => {
      domainStores.mcps = new Map(Object.entries(domainBase.mcps ?? {}).map(([k, v]) => [k, cloneJson(v)]));
    },
    skill: () => {
      domainStores.skills = new Map();
    },
    tool: () => {
      domainStores.tools = new Map();
    },
  };
  (bases[kind] ?? (() => {}))();
  for (const entry of domainEditors[kind]) {
    entry.cb(editors[kind]);
  }
  await materializeDomain(kind, pluginId);
}

// ---------------------------------------------------------------------------
// RPC handler registry (PLUG-D14: rpc id -> method -> handler; the
// definitions cross to the supervisor at register time, the handlers
// stay here and run under the host's `rpc.call` requests)
// ---------------------------------------------------------------------------

/** @type {Map<string, Map<string, Function>>} */
const rpcHandlers = new Map();

/** One host→guest `rpc.call` — run the registered handler with the v2
 *  call context (`ctx.error(type, message, data)` declares a typed
 *  failure; anything else thrown is a defect → rpc.internal). */
async function handleRpcCall({ pluginId, rpcId, method, input }) {
  const table = rpcHandlers.get(String(rpcId ?? ""));
  const handler = table?.get(String(method ?? ""));
  if (typeof handler !== "function") {
    return { ok: false, failure: { type: "rpc.method_not_found", message: `Unknown RPC method: ${rpcId}.${method}` } };
  }
  notify("plugin.busy", { pluginId });
  try {
    const callContext = {
      error(type, message, data) {
        const failure = new Error(message ?? "RPC call failed");
        failure.__rpcFailure = { type: String(type ?? "rpc.internal"), message: String(message ?? ""), data };
        return failure;
      },
    };
    const output = await handler(input ?? {}, callContext);
    const result = { ok: true };
    if (output !== undefined) result.output = output;
    return result;
  } catch (error) {
    if (error && typeof error === "object" && error.__rpcFailure) {
      const failure = { type: error.__rpcFailure.type, message: error.__rpcFailure.message ?? "" };
      if (error.__rpcFailure.data !== undefined) failure.data = error.__rpcFailure.data;
      return { ok: false, failure };
    }
    // A defect (handler bug / transport throw): normalize to the v2
    // rpc.internal shape without echoing internals to the client.
    notify("log", { pluginId, level: "error", message: `rpc ${rpcId}.${method} handler failed: ${errText(error)}` });
    return { ok: false, failure: { type: "rpc.internal", message: "RPC call failed" } };
  } finally {
    notify("plugin.idle", { pluginId });
  }
}

// ---------------------------------------------------------------------------
// Runtime hook registry (guest-side handlers; coverage registered)
// ---------------------------------------------------------------------------
const hookHandlers = { session: new Map(), tool: new Map(), shell: new Map() };

const SESSION_HOOKS = new Set([
  "prompt", "context", "compaction", "generate", "title", "model.request",
  "http.request", "http.response",
  "experimental.ws.handshake", "experimental.ws.send", "experimental.ws.receive",
  "retry",
]);
const TOOL_HOOKS = new Set(["execute.before", "execute.after"]);
const SHELL_HOOKS = new Set(["create.before"]);

async function registerHook(pluginId, domain, name, handlers) {
  const known =
    domain === "session" ? SESSION_HOOKS : domain === "tool" ? TOOL_HOOKS : domain === "shell" ? SHELL_HOOKS : null;
  if (!known || !known.has(name)) {
    throw new Error(`unknown ${domain} hook ${name}`);
  }
  const map = hookHandlers[domain];
  if (!map.has(name)) map.set(name, []);
  map.get(name).push({ pluginId, handler: handlers });
  await hostRequest("hook.register", { pluginId, domain, name });
  return {
    async dispose() {
      const list = map.get(name) ?? [];
      const at = list.indexOf(list.find((e) => e.handler === handlers));
      if (at >= 0) list.splice(at, 1);
    },
  };
}

// ---------------------------------------------------------------------------
// Dialog providers (wave 3's slot bridge): plugin-registered builders
// materialized SYNCHRONOUSLY at open time by the host's
// `dialog.materialize` request (hard-timeout-bounded on the Rust side).
// ---------------------------------------------------------------------------

/** @type {Map<string, Map<string, (input: unknown) => object>>} pluginId -> id -> build */
const dialogProviders = new Map();

// ---------------------------------------------------------------------------
// The Context
// ---------------------------------------------------------------------------

function makeCtx(pluginId, options) {
  const commands = plugins.get(pluginId).commands;
  const subscriptions = plugins.get(pluginId).subscriptions;

  const commandAdd = async (spec) => {
    const name = String(spec?.name ?? "");
    if (!name) throw new Error("command.add requires a name");
    await hostRequest("command.add", {
      pluginId,
      name,
      description: String(spec?.description ?? ""),
      keybinds: Array.isArray(spec?.keybinds) ? spec.keybinds.map(String) : [],
      palette: spec?.palette !== false,
    });
    commands.set(name, spec?.execute);
    return { async dispose() { commands.delete(name); } };
  };

  const sessionMethod = (name) => async (args) => {
    const result = await hostRequest(`session.${name}`, { pluginId, args: args ?? {} });
    return result && typeof result === "object" && "value" in result ? result.value : result;
  };

  return {
    plugin: { id: pluginId },
    app: { version: domainBase.appVersion },
    directory: domainBase.directory,
    location: { directory: domainBase.directory, project: { id: domainBase.project.id } },
    hostFlavor: "v2",
    // Config-sourced plugin options (v2 `{ ...host, options }` — the
    // `plugins: [{package, options}]` config entries; plain objects
    // for option-less plugins).
    options: options && typeof options === "object" && !Array.isArray(options) ? options : {},

    // -- agent (REQUIRED by the flagship: setup bails without
    //    agent.transform) ------------------------------------------------
    agent: {
      async transform(cb) {
        const entry = { cb };
        domainEditors.agent.push(entry);
        cb(editors.agent);
        await materializeDomain("agent", pluginId);
        return {
          async dispose() {
            const at = domainEditors.agent.indexOf(entry);
            if (at >= 0) domainEditors.agent.splice(at, 1);
            await rederiveDomain("agent", pluginId);
          },
        };
      },
      async reload() {
        await rederiveDomain("agent", pluginId);
      },
      async list() {
        return editors.agent.list();
      },
      async get(id) {
        return editors.agent.get(id);
      },
    },

    // -- mcp --------------------------------------------------------------
    mcp: {
      async transform(cb) {
        const entry = { cb };
        domainEditors.mcp.push(entry);
        cb(editors.mcp);
        await materializeDomain("mcp", pluginId);
        return {
          async dispose() {
            const at = domainEditors.mcp.indexOf(entry);
            if (at >= 0) domainEditors.mcp.splice(at, 1);
            await rederiveDomain("mcp", pluginId);
          },
        };
      },
      async reload() {
        await rederiveDomain("mcp", pluginId);
      },
      async list() {
        return editors.mcp.list();
      },
    },

    // -- command ----------------------------------------------------------
    command: {
      add: commandAdd,
      async transform(cb) {
        const added = [];
        const draft = {
          add: (definition) => {
            // v2 CommandDefinition: {name, description?, execute}. The
            // registration is the same native path as command.add. Some
            // plugins call draft.add() without awaiting — contain the
            // floating rejection (it still lands before the `load`
            // reply: the supervisor answers on its IO thread).
            const registration = commandAdd(definition);
            registration.catch((error) => {
              notify("log", {
                pluginId,
                level: "error",
                message: `command registration failed: ${errText(error)}`,
              });
            });
            added.push(String(definition?.name ?? ""));
            return registration;
          },
        };
        const entry = { cb: (d) => d };
        domainEditors.command.push(entry);
        await cb(draft);
        return {
          async dispose() {
            for (const name of added) commands.delete(name);
            const at = domainEditors.command.indexOf(entry);
            if (at >= 0) domainEditors.command.splice(at, 1);
          },
        };
      },
      async reload() {},
      async list() {
        return Array.from(commands.keys()).map((name) => ({ name }));
      },
    },

    // -- skill ------------------------------------------------------------
    skill: {
      async transform(cb) {
        const entry = { cb };
        domainEditors.skill.push(entry);
        cb(editors.skill);
        await materializeDomain("skill", pluginId);
        return {
          async dispose() {
            const at = domainEditors.skill.indexOf(entry);
            if (at >= 0) domainEditors.skill.splice(at, 1);
            await rederiveDomain("skill", pluginId);
          },
        };
      },
      async reload() {
        await rederiveDomain("skill", pluginId);
      },
      async list() {
        return editors.skill.list();
      },
    },

    // -- tool ---------------------------------------------------------------
    tool: {
      async transform(cb) {
        const entry = { cb };
        domainEditors.tool.push(entry);
        cb(editors.tool);
        await materializeDomain("tool", pluginId);
        return {
          async dispose() {
            const at = domainEditors.tool.indexOf(entry);
            if (at >= 0) domainEditors.tool.splice(at, 1);
            await rederiveDomain("tool", pluginId);
          },
        };
      },
      async reload() {
        await rederiveDomain("tool", pluginId);
      },
      async list() {
        return editors.tool.list();
      },
      hook: (name, handler) => registerHook(pluginId, "tool", name, handler),
    },

    // -- session (hook registration + engine routing) ------------------------
    session: {
      hook: (name, handler) => registerHook(pluginId, "session", name, handler),
      get: sessionMethod("get"),
      interrupt: sessionMethod("interrupt"),
      context: sessionMethod("context"),
      messages: sessionMethod("messages"),
      list: sessionMethod("list"),
      prompt: sessionMethod("prompt"),
      synthetic: sessionMethod("synthetic"),
      update: sessionMethod("update"),
      remove: sessionMethod("remove"),
      wait: sessionMethod("wait"),
      switchModel: sessionMethod("switchModel"),
    },

    // -- generate (wave 4: the flagship probes ctx.generate.text and
    //    degrades without it; the engine seam answers through the
    //    session port) -------------------------------------------------------
    generate: {
      text: async (args) => {
        const result = await hostRequest("generate.text", { pluginId, args: args ?? {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
    },

    // -- permission --------------------------------------------------------
    permission: {
      async reply(args) {
        return hostRequest("permission.reply", { pluginId, args: args ?? {} });
      },
    },

    // -- shell (the v2 ShellHooks domain: `create.before`) --------------------
    shell: {
      hook: (name, handler) => registerHook(pluginId, "shell", name, handler),
    },

    // -- rpc (PLUG-D14: `ctx.rpc.register(definition, handlers)`) ------------
    rpc: {
      async register(definition, handlers) {
        const id = String(definition?.id ?? "");
        if (!id) throw new Error("rpc.register requires a definition id");
        // The portable definition crosses as JSON (Effect Schema
        // instances degrade to their method-name skeleton — dispatch
        // routes by name; schema validation stays a JS-side concern).
        let portable = null;
        try {
          portable = JSON.parse(JSON.stringify(definition));
        } catch {
          portable = null;
        }
        if (!portable || typeof portable !== "object") {
          portable = { id, methods: {}, events: {} };
        }
        portable.id = id;
        if (!portable.methods || typeof portable.methods !== "object") portable.methods = {};
        if (!portable.events || typeof portable.events !== "object") portable.events = {};
        if (!rpcHandlers.has(id)) rpcHandlers.set(id, new Map());
        const table = rpcHandlers.get(id);
        for (const [name, handler] of Object.entries(handlers ?? {})) {
          table.set(name, typeof handler === "function" ? handler : null);
        }
        await hostRequest("rpc.register", { pluginId, definition: portable });
        return {
          async dispose() {
            table.clear();
          },
        };
      },
    },

    // -- storage (plugin-ID-scoped durable KV) --------------------------------
    storage: {
      async get(key) {
        const result = await hostRequest("storage.get", { pluginId, key: String(key ?? "") });
        return result && typeof result === "object" && result.value !== null ? result.value : undefined;
      },
      async set(key, value) {
        await hostRequest("storage.set", { pluginId, key: String(key ?? ""), value: value ?? null });
      },
      async remove(key) {
        await hostRequest("storage.remove", { pluginId, key: String(key ?? "") });
      },
      async scan(options) {
        const scan = options ?? {};
        return hostRequest("storage.scan", {
          pluginId,
          prefix: typeof scan.prefix === "string" ? scan.prefix : "",
          after: typeof scan.after === "string" ? scan.after : null,
          limit: Number.isFinite(scan.limit) ? scan.limit : null,
        });
      },
    },

    // -- registration-time sync bridge (wave 1) -------------------------------
    // Every add lands in the supervisor's native structures BEFORE the
    // `load` reply.
    theme: {
      async register(theme) {
        const name = String(theme?.name ?? "");
        if (!name) throw new Error("theme.register requires a name");
        await hostRequest("theme.register", { pluginId, name, document: theme?.document ?? null });
        return { async dispose() {} };
      },
    },
    event: {
      // (handler) | (types, handler) | (types[]) — the handler forms
      // register a callback subscription (wave 1) and return a plain
      // Promise<Registration>. The NO-ARG form is the v2 EventApi
      // shape: returns an AsyncIterable SYNCHRONOUSLY (the flagship
      // calls `.subscribe()[Symbol.asyncIterator]()` without
      // awaiting). NB: never resolve awaits through a hand-rolled
      // thenable whose `then` returns a derived promise — Bun
      // 1.3.10 wedges on that shape (verified: then() returning
      // `inner.then(() => v)` never settles the awaiting frame).
      subscribe(first, second) {
        // (handler) | (types, handler) | (types[]) — the handler forms
        // register a callback subscription (wave 1) and return a plain
        // Promise<Registration>. `types` may be a single string or an
        // array (v2 overloads). The NO-ARG form is the v2 EventApi
        // shape: returns an AsyncIterable SYNCHRONOUSLY.
        if (typeof first === "function" || Array.isArray(first) || typeof first === "string") {
          return (async () => {
            const types = Array.isArray(first)
              ? first
              : typeof first === "string"
                ? [first]
                : null;
            const handler = typeof first === "function" ? first : second;
            const entry = { types, handler: handler ?? null, queue: null };
            subscriptions.push(entry);
            await hostRequest("event.subscribe", { pluginId, types });
            return {
              async dispose() {
                const at = subscriptions.indexOf(entry);
                if (at >= 0) subscriptions.splice(at, 1);
              },
            };
          })();
        }
        // Async-iterable form (v2 `ctx.event.subscribe()`).
        const queue = [];
        let waiters = [];
        let finished = false;
        const entry = { types: null, handler: null, queue: { push, finish } };
        function push(event) {
          if (finished) return;
          if (queue.length >= 512) {
            queue.shift(); // backpressure: drop oldest, log once
            notify("log", { pluginId, level: "warn", message: "event queue overflow; dropped oldest" });
          }
          queue.push(event);
          flush();
        }
        function flush() {
          while (waiters.length > 0 && queue.length > 0) {
            const waiter = waiters.shift();
            waiter({ value: queue.shift(), done: false });
          }
        }
        function finish() {
          finished = true;
          for (const waiter of waiters) waiter({ value: undefined, done: true });
          waiters = [];
        }
        subscriptions.push(entry);
        hostRequest("event.subscribe", { pluginId, types: null }).catch((error) => {
          notify("log", { pluginId, level: "error", message: `event.subscribe failed: ${errText(error)}` });
          finish();
        });
        const iterable = {
          [Symbol.asyncIterator]() {
            return {
              next() {
                if (queue.length > 0) return Promise.resolve({ value: queue.shift(), done: false });
                if (finished) return Promise.resolve({ value: undefined, done: true });
                return new Promise((resolve) => {
                  waiters.push(resolve);
                });
              },
              async return() {
                const at = subscriptions.indexOf(entry);
                if (at >= 0) subscriptions.splice(at, 1);
                finish();
                return { value: undefined, done: true };
              },
            };
          },
        };
        return iterable;
      },
    },
    // Nested sync RPC — the deadlock-rule hammer: a guest→host request
    // issued from inside setup or an event handler must be answered by
    // the supervisor's IO thread while the guest's own reply is still
    // outstanding.
    ping: async () => hostRequest("ping", { pluginId }),
    log: (...args) => notify("log", { pluginId, level: "info", message: args.join(" ") }),

    // -- ui (wave 3's TUI slot bridge) --------------------------------------
    // Runtime requests land on the host's UI-request queue (the TUI
    // drains it between frames); registrations (dialog providers,
    // group rules, home actions) land in the native snapshot. The
    // dialog content structure is built HERE, synchronously —
    // rendering afterwards is native; no per-frame IPC.
    ui: {
      toast: {
        show(options) {
          const opts = options ?? {};
          return hostRequest("ui.toast.show", {
            pluginId,
            title: typeof opts.title === "string" ? opts.title : null,
            message: String(opts.message ?? ""),
            variant: typeof opts.variant === "string" ? opts.variant : "info",
            duration: Number.isFinite(opts.duration) ? opts.duration : null,
          }).catch((error) => {
            notify("log", { pluginId, level: "error", message: `toast.show failed: ${errText(error)}` });
          });
        },
      },
      sidebar: {
        async setSection(section) {
          const spec = section ?? {};
          await hostRequest("ui.sidebar.section", {
            pluginId,
            title: String(spec.title ?? ""),
            rows: Array.isArray(spec.rows) ? spec.rows.map(String) : [],
          });
        },
        async removeSection() {
          await hostRequest("ui.sidebar.section", { pluginId, title: "", rows: [] }).catch(() => {});
        },
      },
      footer: {
        async setHint(hint) {
          const text = typeof hint === "string" ? hint : String(hint?.text ?? "");
          await hostRequest("ui.footer.hint", { pluginId, text });
        },
      },
      dialog: {
        /** Register an open-time content builder (registration-time
         *  native coverage; the host qualifies the id per plugin). */
        async register(spec) {
          const id = String(spec?.id ?? "");
          if (!id) throw new Error("ui.dialog.register requires an id");
          if (typeof spec?.build !== "function") throw new Error("ui.dialog.register requires a build function");
          if (!dialogProviders.has(pluginId)) dialogProviders.set(pluginId, new Map());
          dialogProviders.get(pluginId).set(id, spec.build);
          const reply = await hostRequest("ui.dialog.register", { pluginId, id });
          return {
            id: typeof reply?.id === "string" ? reply.id : id,
            async dispose() {
              dialogProviders.get(pluginId)?.delete(id);
            },
          };
        },
        /** Open a dialog NOW with fully-built content (fire-and-forget:
         *  the TUI opens natively from the queued structure). */
        async open(spec) {
          await hostRequest("ui.dialog.open", { pluginId, spec: spec ?? {} });
        },
      },
      grouping: {
        /** {tool} exact match or {toolPrefix} prefix match → header. */
        async addRule(rule) {
          const spec = rule ?? {};
          await hostRequest("ui.group.rule", {
            pluginId,
            tool: typeof spec.tool === "string" && spec.tool ? spec.tool : null,
            toolPrefix: typeof spec.toolPrefix === "string" && spec.toolPrefix ? spec.toolPrefix : null,
            header: String(spec.header ?? ""),
          });
        },
      },
      home: {
        /** {label, command} — a quick action on the home footer row. */
        async addQuickAction(action) {
          const spec = action ?? {};
          await hostRequest("ui.home.quickaction", {
            pluginId,
            label: String(spec.label ?? ""),
            command: String(spec.command ?? ""),
          });
        },
      },
      // -- slot / router (PLUG-TUI-SLOT: the v2 TUI-plugin UI
      //    surface, api.tsx:298-312 + 204-224) -------------------------
      slot(claim) {
        // api.tsx:298-312 — exactly one placement key, enforced at
        // runtime for untyped plugins; keys counter-suffixed so one
        // plugin may claim several places.
        const kinds = ["prepend", "append", "before", "after", "replace"];
        const present = kinds.filter((kind) => claim && claim[kind] !== undefined);
        if (present.length !== 1) {
          throw new Error("Slot claim requires exactly one placement key");
        }
        const kind = present[0];
        const target = String(claim[kind] ?? "");
        const render = claim.render;
        if (!target) throw new Error("Slot claim requires a target");
        if (typeof render !== "function") throw new Error("Slot claim requires a render function");
        if (!slotClaims.has(pluginId)) slotClaims.set(pluginId, new Map());
        const key = `slot#${slotClaimCounter(pluginId)}`;
        slotClaims.get(pluginId).set(key, { kind, target, render, clickHandlers: new Map() });
        hostRequest("ui.slot.claim", { pluginId, key, kind, target }).catch((error) => {
          notify("log", { pluginId, level: "error", message: `ui.slot claim failed: ${errText(error)}` });
        });
        let registered = true;
        return () => {
          if (!registered) return;
          registered = false;
          slotClaims.get(pluginId)?.delete(key);
        };
      },
      router: {
        /** api.tsx:204-212 — register a named plugin page. */
        async register(page) {
          const name = String(page?.name ?? "");
          if (!name) throw new Error("router.register requires a name");
          if (typeof page?.render !== "function") {
            throw new Error("router.register requires a render function");
          }
          if (!pluginRoutes.has(pluginId)) pluginRoutes.set(pluginId, new Map());
          pluginRoutes.get(pluginId).set(name, page.render);
          await hostRequest("ui.router.register", { pluginId, name });
          return () => {
            pluginRoutes.get(pluginId)?.delete(name);
          };
        },
        /** api.tsx:211-224 — `{type:"plugin", name, data?}` targets
         *  this plugin's registered page (the ref attributes the id);
         *  session targets reuse the bridge's navigation ask. */
        navigate(destination) {
          const target = destination ?? {};
          if (target.type === "plugin") {
            notify("tui.route.plugin", {
              pluginId: "id" in target && typeof target.id === "string" ? target.id : pluginId,
              name: String(target.name ?? ""),
              data: target.data && typeof target.data === "object" ? target.data : null,
            });
            return;
          }
          if (target.type === "session") {
            notify("tui.route.navigate", {
              sessionID: typeof target.sessionID === "string" ? target.sessionID : null,
              name: "session",
            });
            return;
          }
          if (typeof target.sessionID === "string") {
            notify("tui.route.navigate", { sessionID: target.sessionID, name: "session" });
          }
        },
        /** The live route view (the host-pushed `tui.route`). */
        current() {
          return tuiRoute;
        },
      },
    },

    // -- keymap (api.tsx:179-199: `ctx.keymap.layer(factory)`) -------
    keymap: {
      async layer(factory) {
        if (typeof factory !== "function") throw new Error("keymap.layer requires a factory");
        const layer = factory() ?? {};
        const commands = Array.isArray(layer.commands) ? layer.commands : [];
        const mode = typeof layer.mode === "string" ? layer.mode : "global";
        const portable = [];
        for (const command of commands) {
          const id = String(command?.id ?? command?.title ?? "");
          const bind = String(command?.bind ?? "");
          if (!id || !bind) throw new Error(`keymap.layer command requires id and bind`);
          if (typeof command?.run !== "function") {
            throw new Error(`keymap.layer command ${id} requires a run function`);
          }
          if (!keymapRuns.has(pluginId)) keymapRuns.set(pluginId, new Map());
          keymapRuns.get(pluginId).set(id, command.run);
          portable.push({
            id,
            title: String(command.title ?? id),
            group: String(command.group ?? ""),
            bind,
            palette: command.palette === true,
            slash:
              command.slash && typeof command.slash === "object" && typeof command.slash.name === "string"
                ? command.slash.name
                : null,
            mode,
          });
        }
        await hostRequest("keymap.layer", { pluginId, commands: portable });
        return () => {
          const table = keymapRuns.get(pluginId);
          for (const command of portable) table?.delete(command.id);
        };
      },
    },
  };
}

// ---------------------------------------------------------------------------
// Supervisor-facing methods
// ---------------------------------------------------------------------------

async function handleLoad({ entry, generation, seed, options }) {
  const abs = require("node:path").resolve(entry);
  // Config-sourced plugin options (the `load` params; `{}` default).
  const loadOptions = options && typeof options === "object" && !Array.isArray(options) ? options : {};
  // v2 source.bun parity: evict CJS cache under the plugin dir, then
  // re-import with a generation cache-bust. The FIRST load (generation
  // 1) imports without the `?t=` query — plugins legitimately derive
  // their package root from `import.meta.url` and Node's
  // `fileURLToPath` rejects query strings.
  const dir = require("node:path").dirname(abs);
  for (const key of Object.keys(require.cache)) {
    if (key.startsWith(dir)) delete require.cache[key];
  }
  if (seed) seedDomains(seed);
  loadingEntries.add(abs);
  let mod;
  try {
    const bust = generation && generation > 1 ? `?t=${generation}` : "";
    mod = await import(`file://${abs}${bust}`);
  } finally {
    loadingEntries.delete(abs);
  }
  const shape = classify(mod.default);
  if (shape.kind === "none") {
    throw new Error(`${entry}: default export is not a plugin ({ id, setup } expected)`);
  }
  if (shape.kind === "v1") {
    const id = `v1:${deriveId(abs)}`;
    plugins.set(id, { id, entry: abs, dispose: null, subscriptions: [], commands: new Map() });
    notify("log", { level: "info", message: `${id}: v1 server() export tolerated (ignored)` });
    return { ok: true, pluginId: id, skipped: "v1" };
  }
  if (shape.kind === "effect") {
    const id = shape.id;
    plugins.set(id, { id, entry: abs, dispose: null, subscriptions: [], commands: new Map() });
    notify("log", {
      level: "info",
      message: `${id}: effect contract not runnable in this host wave; loaded inert`,
    });
    return { ok: true, pluginId: id, skipped: "effect" };
  }
  const id = shape.id;
  const record = { id, entry: abs, dispose: null, subscriptions: [], commands: new Map() };
  plugins.set(id, record);
  const ctx = makeCtx(id, loadOptions);
  const maybeDispose = await shape.setup(ctx);
  if (typeof maybeDispose === "function" || (maybeDispose && typeof maybeDispose.dispose === "function")) {
    record.dispose = typeof maybeDispose === "function" ? maybeDispose : maybeDispose.dispose;
  }
  return { ok: true, pluginId: id, skipped: null };
}

async function handleUnload({ pluginId }) {
  const record = plugins.get(pluginId);
  if (!record) return { ok: true, unknown: true };
  try {
    if (record.dispose) await record.dispose();
  } catch (error) {
    notify("log", { level: "warn", message: `${pluginId}: dispose threw ${errText(error)}` });
  }
  const tuiDispose = tuiDisposers.get(pluginId);
  if (tuiDispose) {
    try {
      await tuiDispose();
    } catch (error) {
      notify("log", { level: "warn", message: `${pluginId}: tui dispose threw ${errText(error)}` });
    }
    tuiDisposers.delete(pluginId);
  }
  plugins.delete(pluginId);
  dialogProviders.delete(pluginId);
  sidebarSlots.delete(pluginId);
  slotClaims.delete(pluginId);
  slotClaimCounters.delete(pluginId);
  pluginRoutes.delete(pluginId);
  keymapRuns.delete(pluginId);
  return { ok: true };
}

// The open-time dialog materialization (wave 3): the host asks, the
// plugin's registered builder runs SYNCHRONOUSLY here, and the
// structure crosses once — the Rust side bounds the whole call with a
// hard timeout and renders natively afterwards.
async function handleDialogMaterialize({ pluginId, id }) {
  let build = dialogProviders.get(pluginId)?.get(id);
  if (typeof build !== "function") {
    // The host may send the `plugin:id`-qualified form.
    for (const [owner, map] of dialogProviders) {
      for (const [key, fn] of map) {
        if (`${owner}:${key}` === id) build = fn;
      }
    }
  }
  if (typeof build !== "function") {
    throw new Error(`${pluginId}:${id}: no dialog provider`);
  }
  const spec = await build({ pluginId });
  return {
    title: String(spec?.title ?? ""),
    rows: Array.isArray(spec?.rows) ? spec.rows.map(String) : [],
    options: Array.isArray(spec?.options)
      ? spec.options
          .filter((option) => typeof option?.title === "string" && typeof option?.value === "string")
          .map((option) => ({
            title: option.title,
            value: option.value,
            description: String(option.description ?? ""),
          }))
      : [],
  };
}

async function handleDispatch({ event }) {
  const type = event?.type ?? "";
  let delivered = 0;
  const errors = [];
  for (const [pluginId, record] of plugins) {
    const matching = record.subscriptions.filter(
      (sub) => !sub.types || sub.types.includes(type),
    );
    if (matching.length === 0) continue;
    // Crash attribution markers: the supervisor blames plugins that
    // were mid-dispatch when the process died.
    notify("plugin.busy", { pluginId });
    try {
      for (const sub of matching) {
        if (!sub.handler && !sub.queue) { delivered++; continue; }
        try {
          if (sub.queue) {
            sub.queue.push(event);
            delivered++;
          } else {
            await sub.handler(event);
            delivered++;
          }
        } catch (error) {
          errors.push(`${pluginId}: ${errText(error)}`);
        }
      }
    } finally {
      notify("plugin.idle", { pluginId });
    }
  }
  return { ok: errors.length === 0, delivered, errors };
}

async function handleCommandExecute({ pluginId, name, args, sessionID }) {
  const record = plugins.get(pluginId);
  const execute = record?.commands.get(name);
  if (typeof execute !== "function") {
    throw new Error(`${pluginId}:${name}: no such command`);
  }
  const invocation = {
    sessionID: typeof sessionID === "string" ? sessionID : "",
    agent: undefined,
    delivery: "steer",
    prompt: { text: typeof args === "string" ? args : args?.text ?? null },
    args,
  };
  const result = await execute(invocation, makeCtx(pluginId, {}));
  return { ok: true, result: result ?? null };
}

// ---------------------------------------------------------------------------
// Tool execution dispatch (wave 4): the wave-2 metadata-only surface
// becomes callable — `ctx.tool.transform`'s `execute` runs on demand.
// ---------------------------------------------------------------------------

async function handleToolExecute({ name, input }) {
  const def = domainStores.tools.get(String(name ?? ""));
  if (!def || typeof def.execute !== "function") {
    throw new Error(`tool ${name}: no such plugin tool`);
  }
  // v2 ToolContext: an abort signal + progress reporter. The abort
  // seam is the process lifetime (a killed guest aborts everything);
  // progress notifications are a later surface.
  const controller = new AbortController();
  const context = {
    signal: controller.signal,
    progress: () => {},
    sessionID: undefined,
    agent: undefined,
    messageID: undefined,
    id: undefined,
  };
  const result = await def.execute(input ?? {}, context);
  // v2 Tool.Result: {output?, content?, metadata?} — string content is
  // the common shape; the host side joins parts.
  return { ok: true, result: result ?? null };
}

// ---------------------------------------------------------------------------
// Code Mode (wave 4): one `execute` program over a tool catalog. The
// guest is the runtime (v2 uses its in-process confined interpreter —
// the Rust port's boundary is this sandboxed subprocess plus the
// host-side HARD timeout; imports/timers are reachable inside the
// sandbox, a documented divergence).
// ---------------------------------------------------------------------------

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

/** v2 `displayInput` (codemode/tool.ts:303-308). */
function displayInput(input) {
  if (input === null || input === undefined) return undefined;
  if (typeof input !== "object" || Array.isArray(input)) return { input };
  if (Object.keys(input).length === 0) return undefined;
  return input;
}

/**
 * Build the `tools` tree from catalog entries. A path that is BOTH a
 * callable tool and a namespace (e.g. `pricing` + `pricing.sales`)
 * becomes a function with the children attached — the observable
 * semantics of v2's callable-namespace merge.
 */
function buildToolsTree(entries, calls, token) {
  const root = {};
  const makeCallable = (entry) => {
    const fn = async (input) => {
      const shown = displayInput(input);
      const record = { tool: entry.path, status: "running", ...(shown ? { input: shown } : {}) };
      calls.push(record);
      try {
        const result = await hostRequest("codemode.tool", {
          name: entry.path,
          input: input ?? {},
          // The host-assigned evaluation token routes the nested call
          // onto THIS evaluation's executor closure (per-request
          // session context + location registry).
          token,
        });
        const value = result && typeof result === "object" && "value" in result ? result.value : result;
        record.status = "completed";
        return value ?? null;
      } catch (error) {
        record.status = "error";
        throw error;
      }
    };
    // Discovery marker for the (test-facing) catalog equality checks.
    Object.defineProperty(fn, "__codemode_tool", { value: entry.path });
    return fn;
  };
  for (const entry of entries ?? []) {
    const segments = String(entry?.path ?? "").split(".").filter((segment) => segment !== "");
    if (segments.length === 0) continue;
    let node = root;
    for (let at = 0; at < segments.length - 1; at += 1) {
      const segment = segments[at];
      let child = node[segment];
      if (child === undefined) {
        child = {};
        node[segment] = child;
      }
      node = child;
    }
    const leaf = segments[segments.length - 1];
    const existing = node[leaf];
    const callable = makeCallable(entry);
    if (existing === undefined) {
      node[leaf] = callable;
    } else if (typeof existing === "function") {
      // Tool + deeper namespace: attach children onto the callable.
      node[leaf] = Object.assign(callable, existing);
    } else {
      // Namespace recorded first, tool now: turn the object into a
      // function carrying the same children.
      node[leaf] = Object.assign(callable, existing);
    }
  }
  return root;
}

/** v2 `search` over the catalog (tool-runtime.ts tokenize/rank). */
function makeSearch(entries) {
  const tokenize = (query) =>
    query
      .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
      .toLowerCase()
      .split(/[^a-z0-9]+/)
      .filter((term) => term.length > 0 && term !== "*");
  const termForms = (term) => {
    const forms = [term];
    if (term.endsWith("es") && term.length > 3) forms.push(term.slice(0, -2));
    if (term.endsWith("s") && term.length > 2) forms.push(term.slice(0, -1));
    return forms;
  };
  const identifierSegment = /^[A-Za-z_$][A-Za-z0-9_$]*$/;
  const toolExpression = (path) =>
    "tools" +
    path
      .split(".")
      .map((segment) => (identifierSegment.test(segment) ? `.${segment}` : `[${JSON.stringify(segment)}]`))
      .join("");
  const ranked = (scoped, query) => {
    const terms = tokenize(query).map(termForms);
    return entries
      .map((entry) => {
        const path = entry.path.toLowerCase();
        const description = String(entry.description ?? "").toLowerCase();
        // v2's searchText (path + description + signature, lowercased).
        const searchText = `${path} ${description} ${String(entry.signature ?? "").toLowerCase()}`;
        const pathWords = tokenize(entry.path);
        const score = terms.reduce(
          (total, forms) =>
            total +
            (forms.some((form) => path === form || path.endsWith(`.${form}`)) ? 20 : 0) +
            (forms.some((form) => pathWords.includes(form)) ? 12 : 0) +
            (forms.some((form) => path.includes(form)) ? 8 : 0) +
            (forms.some((form) => description.includes(form)) ? 4 : 0) +
            (forms.some((form) => searchText.includes(form)) ? 2 : 0),
          0,
        );
        return { entry, score };
      })
      .filter(({ score }) => terms.length === 0 || score > 0)
      .sort((left, right) => right.score - left.score || (left.entry.path < right.entry.path ? -1 : left.entry.path > right.entry.path ? 1 : 0))
      .map(({ entry }) => entry);
  };
  // NB: `identifierSegment` is declared once above (a duplicate
  // declaration from a concurrent lane's merge crashed the guest —
  // removed; identical regex, first binding is the live one).
  return ({ query, namespace, limit, offset } = {}) => {
    let ns = namespace;
    if (ns !== undefined && !entries.some((entry) => entry.path.startsWith("tools."))) {
      if (ns === "tools") ns = undefined;
      else if (ns.startsWith("tools.")) ns = ns.slice("tools.".length);
    }
    const scoped =
      ns === undefined
        ? entries
        : entries.filter((entry) => entry.path === ns || entry.path.startsWith(`${ns}.`));
    const trimmed = String(query ?? "").trim();
    const pathQuery = trimmed.startsWith("tools.") ? trimmed.slice("tools.".length) : trimmed;
    const exact =
      pathQuery === ""
        ? undefined
        : scoped.find((entry) => entry.path === pathQuery || toolExpression(entry.path) === trimmed);
    const ordered = exact !== undefined ? [exact] : ranked(scoped, String(query ?? ""));
    const start = Number.isFinite(offset) && offset > 0 ? Math.floor(offset) : 0;
    const size = Number.isFinite(limit) && limit > 0 ? Math.floor(limit) : 25;
    const items = ordered.slice(start, start + size).map((entry) => ({
      path: toolExpression(entry.path),
      description: String(entry.description ?? ""),
      signature: String(entry.signature ?? ""),
    }));
    const remaining = Math.max(0, ordered.length - start - items.length);
    return { items, remaining, next: remaining > 0 ? { offset: start + items.length } : null };
  };
}

async function handleCodemodeEvaluate({ code, tools, token }) {
  if (typeof code !== "string" || code.length === 0) {
    return { ok: false, error: "Code Mode execution requires a `code` string" };
  }
  const calls = [];
  const catalog = Array.isArray(tools) ? tools : [];
  const evaluationToken = typeof token === "number" && Number.isFinite(token) ? token : 0;
  const toolsTree = buildToolsTree(catalog, calls, evaluationToken);
  const search = makeSearch(catalog);
  let value;
  try {
    const program = new AsyncFunction("tools", "search", `"use strict";\n${code}`);
    value = await program(toolsTree, search);
  } catch (error) {
    return { ok: false, error: errText(error), calls };
  }
  // `undefined` (no explicit return) is OMITTED so the host formats
  // "undefined"; explicit null stays null.
  const result = { ok: true, calls };
  if (value !== undefined) result.value = value;
  return result;
}

async function handleHookDispatch({ pluginId, domain, name, event }) {
  const list = hookHandlers[domain]?.get(name) ?? [];
  const target = list.filter((entry) => !pluginId || entry.pluginId === pluginId);
  if (target.length === 0) return { ok: true, event: event ?? null, errors: [] };
  notify("plugin.busy", { pluginId: target[0].pluginId });
  try {
    for (const entry of target) {
      // v2 trigger semantics (hooks.ts:88-95): a handler failure FAILS
      // the dispatch (`yield*` propagates; later handlers are skipped).
      // Throw — the read loop turns it into a JSON-RPC error so the
      // host bridge surfaces HostError::Guest instead of a swallowed
      // mutation chain.
      await entry.handler(event);
    }
  } finally {
    notify("plugin.idle", { pluginId: target[0].pluginId });
  }
  return { ok: true, event: event ?? null, errors: [] };
}

// ---------------------------------------------------------------------------
// Hot-reload digest scan (Bun.Transpiler.scan over the relative import
// graph; bare/npm specifiers are Bun-managed and excluded)
// ---------------------------------------------------------------------------

const EXTENSIONS = ["", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".mts", "/index.ts", "/index.js"];

function resolveLocal(from, spec) {
  if (!spec.startsWith(".") && !spec.startsWith("/")) return null;
  const path = require("node:path");
  const base = path.resolve(path.dirname(from), spec);
  for (const ext of EXTENSIONS) {
    if (existsSync(base + ext)) return base + ext;
  }
  return null;
}

async function scanDigests(entry) {
  const path = require("node:path");
  const root = path.resolve(entry);
  const digests = {};
  const seen = new Set();
  async function walk(file) {
    if (seen.has(file)) return;
    seen.add(file);
    let source;
    try {
      source = await Bun.file(file).text();
    } catch {
      return;
    }
    digests[file] = createHash("sha256").update(source).digest("hex");
    let imports = [];
    try {
      imports = new Bun.Transpiler({ loader: "tsx" }).scanImports(source);
    } catch {
      // Unparseable file: the digest still covers it.
    }
    for (const spec of imports) {
      const resolved = resolveLocal(file, spec.path ?? spec);
      if (resolved) await walk(resolved);
    }
  }
  await walk(root);
  return digests;
}

// ---------------------------------------------------------------------------
// The v2 TUI-plugin surface (beta.13 bug 1 — the REAL plugin's
// sidebar). oh-my-opencode-slim's `./tui` export (`dist/tui2.js`)
// drives its sidebar through `api.slots.register({slots:
// {sidebar_content: render}})` building OpenTUI trees with
// @opentui/{core,solid} + solid-js. The desktop host provides those
// packages from its own module graph; this host MATERIALIZES minimal
// filesystem shims next to the install whose elements are plain
// JSON-able records, then flattens each rendered tree into the native
// sidebar-section shape (title + rows) at materialize time.
// ---------------------------------------------------------------------------

/** pluginId -> { render, dispose } for the sidebar_content slot. */
const sidebarSlots = new Map();
/** pluginId -> disposal for the loaded tui entry. */
const tuiDisposers = new Map();
/** PLUG-TUI-SLOT: pluginId -> Map(claim key -> {kind, target, render,
 * clickHandlers}) — the generic `ctx.ui.slot` claim table
 * (api.tsx:298-312; keys counter-suffixed `slot#N`). */
const slotClaims = new Map();
/** Per-plugin claim counter (the `slot#N` suffix source). */
const slotClaimCounters = new Map();
/** pluginId -> Map(route name -> render) — `ctx.ui.router.register`. */
const pluginRoutes = new Map();
/** pluginId -> Map(command id -> run) — `ctx.keymap.layer` runs. */
const keymapRuns = new Map();

function slotClaimCounter(pluginId) {
  const next = (slotClaimCounters.get(pluginId) ?? 0) + 1;
  slotClaimCounters.set(pluginId, next);
  return next - 1;
}
/** The host-pushed current route ({type:"session",sessionID}|{name:"home"}). */
let tuiRoute = { name: "home" };
/** The host-pushed flat v2 theme view (TuiThemeCurrent's sidebar subset). */
let tuiThemeView = {
  accent: "#8b5cf6",
  background: "#0a0a0a",
  borderActive: "#3c3c3c",
  text: "#eeeeee",
  textMuted: "#888888",
};
/** The host-pushed sidebar layout budget (oc-tui's painter row width:
 *  SESSION_SIDEBAR_WIDTH − 4 = 38; the plugin root box's border pair
 *  brings the section's own content to 36). SA-F5: the flattener lays
 *  row boxes out against this so `justifyContent: "space-between"`
 *  headers and flexGrow-spacer rows materialize with the padded
 *  columns the plugin's own renderer shows. */
let tuiSidebarContentWidth = 38;

/** Ensure the host-provided OpenTUI shims resolve from the tui entry
 *  (sibling node_modules; `solid-js` stays the real package when the
 *  install carries it, else the minimal signal shim). */
function ensureOpentuiShims(packageRoot) {
  // The package root's PARENT is the install's node_modules (the
  // hoisted deps' dir — where bare imports from dist/ resolve).
  const nm = require("node:path").dirname(packageRoot);
  const write = (name, files) => {
    const dir = require("node:path").join(nm, name);
    if (require("node:fs").existsSync(require("node:path").join(dir, "package.json"))) return;
    require("node:fs").mkdirSync(dir, { recursive: true });
    for (const [file, body] of Object.entries(files)) {
      require("node:fs").writeFileSync(require("node:path").join(dir, file), body);
    }
  };
  write("@opentui/core", {
    "package.json": JSON.stringify({
      name: "@opentui/core",
      version: "0.0.0-host-shim",
      type: "module",
      main: "index.js",
    }),
    "index.js": `\
// Host shim (the Rust plugin host's sidebar bridge): the two color
// utilities the flagship's sidebar uses. Values are plain records.
export function parseColor(value) {
  if (value && typeof value === "object") return value;
  const text = String(value ?? "").trim();
  let hex = null;
  if (text.startsWith("#")) hex = text.slice(1);
  else if (/^[0-9a-f]{6}$/i.test(text)) hex = text;
  if (!hex || hex.length !== 6) throw new Error("unparseable color: " + text);
  const channel = (at) => parseInt(hex.slice(at * 2, at * 2 + 2), 16) / 255;
  return { r: channel(0), g: channel(1), b: channel(2), a: 1 };
}
export class RGBA {
  constructor(r, g, b, a) { this.r = r; this.g = g; this.b = b; this.a = a; }
  static fromValues(r, g, b, a = 1) { return new RGBA(r, g, b, a); }
  static fromInts(r, g, b, a = 255) { return new RGBA(r / 255, g / 255, b / 255, a / 255); }
}
`,
  });
  write("@opentui/solid", {
    "package.json": JSON.stringify({
      name: "@opentui/solid",
      version: "0.0.0-host-shim",
      type: "module",
      main: "index.js",
    }),
    "index.js": `\
// Host shim: element records + an EAGER insert (each materialize
// re-runs the plugin's render function, so eager evaluation of
// function children reads the current signal values — the reactive
// re-run becomes re-materialize).
export function createElement(tag) {
  return { tag, props: {}, children: [] };
}
export function setProp(node, key, value) {
  node.props[key] = value;
}
export function insert(parent, child) {
  const mount = (value) => {
    if (value === null || value === undefined || value === false || value === true) return;
    if (Array.isArray(value)) { for (const item of value) mount(item); return; }
    if (typeof value === "function") { try { mount(value()); } catch { /* contained */ } return; }
    if (typeof value === "string" || typeof value === "number") {
      parent.children.push({ tag: "#text", props: { text: String(value) }, children: [] });
      return;
    }
    if (typeof value === "object" && value.tag !== undefined) { parent.children.push(value); return; }
  };
  mount(child);
}
`,
  });
  write("solid-js", {
    "package.json": JSON.stringify({
      name: "solid-js",
      version: "0.0.0-host-shim",
      type: "module",
      main: "index.js",
    }),
    "index.js": `\
// Host shim: the minimal signal surface the flagship's sidebar needs
// (only used when the install does not carry the real solid-js).
export function createSignal(value) {
  const subscribers = new Set();
  const read = () => value;
  const write = (next) => {
    const resolved = typeof next === "function" ? next(value) : next;
    if (Object.is(resolved, value)) return resolved;
    value = resolved;
    for (const subscriber of [...subscribers]) subscriber(value);
    return value;
  };
  return [read, write];
}
`,
  });
}

/** The display width of one flattened line (ASCII-fast path). */
function lineWidth(line) {
  let width = 0;
  for (const ch of line) {
    const code = ch.codePointAt(0) ?? 0;
    // East-asian wide/fullwidth ranges (the pragmatic wcwidth the
    // flagship's rows need — braille/dots are narrow).
    width +=
      (code >= 0x1100 && code <= 0x115f) ||
      (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) ||
      (code >= 0xac00 && code <= 0xd7a3) ||
      (code >= 0xf900 && code <= 0xfaff) ||
      (code >= 0xfe30 && code <= 0xfe6f) ||
      (code >= 0xff00 && code <= 0xff60) ||
      (code >= 0xffe0 && code <= 0xffe6) ||
      (code >= 0x1f300 && code <= 0x1f64f) ||
      (code >= 0x1f900 && code <= 0x1f9ff)
        ? 2
        : 1;
  }
  return width;
}

/** Pad (never truncate past content) one line to `width` display
 *  cells — a text/box node with a numeric `width` occupies exactly
 *  that many columns in its parent's row. */
function padTo(line, width) {
  const natural = lineWidth(line);
  return natural >= width ? line : line + " ".repeat(width - natural);
}

/** Walk one rendered tree into flat lines at a resolved content
 *  `width`. Columns stack their children's lines; ROW boxes lay their
 *  children out horizontally honoring the tree's width cues (SA-F5
 *  fidelity): a numeric `width` fixes a child's column span, `marginLeft`
 *  prefixes spaces, `flexGrow` children absorb the leftover, and
 *  `justifyContent: "space-between"` spreads the leftover across the
 *  gaps — the shapes the flagship's sidebar rows actually use (the
 *  header's `▼ name … version` and the agent rows' padded name column
 *  with the right-aligned model). Rows without any flex/space-between
 *  cue keep the legacy tight join (single space, runs collapsed). */
function treeLines(node, width = tuiSidebarContentWidth) {
  if (!node || typeof node !== "object") return [];
  if (node.tag === "#text") {
    const text = String(node.props.text ?? "");
    return text === "" ? [] : [text];
  }
  const children = (node.children ?? []).filter(Boolean);
  const direction = node.props.flexDirection;
  if (direction === "row" && children.length > 0) {
    const laid = rowLine(node, children, width);
    if (laid !== null) return [laid];
    // Legacy tight join (no layout cues resolvable).
    const childResults = children.map((child) => treeLines(child, width));
    const height = childResults.reduce((max, lines) => Math.max(max, lines.length), 0);
    const row = [];
    for (let index = 0; index < height; index++) {
      const joined = childResults
        .map((lines) => lines[index] ?? "")
        .filter((part) => part !== "")
        .join(" ")
        .replace(/ {2,}/g, " ");
      if (joined !== "") row.push(joined);
    }
    return row;
  }
  return children.flatMap((child) => treeLines(child, contentWidthOf(node, width)));
}

/** A box's content width: numeric `width` wins; a border consumes its
 *  pair; `width: "100%"` resolves against the parent-provided budget.
 *  (paddingRight is not subtracted — the flagship's space-between rows
 *  empirically span the full inner width, ref capture cols 120..155.) */
function contentWidthOf(node, parentWidth) {
  const declared = node?.props?.width;
  if (typeof declared === "number" && declared > 0) return declared;
  let width = parentWidth;
  if (node?.props?.border) width = Math.max(0, width - 2);
  if (typeof node?.props?.paddingLeft === "number") width = Math.max(0, width - node.props.paddingLeft);
  return width;
}

/** One row box's laid-out line, or null when the shape is not
 *  resolvable (multi-line children, overflow, no width budget). */
function rowLine(node, children, width) {
  if (!Number.isFinite(width) || width <= 0) return null;
  const inner = contentWidthOf(node, width);
  const parts = [];
  for (const child of children) {
    const lines = treeLines(child, inner);
    if (lines.length > 1) return null;
    let line = lines[0] ?? "";
    const declared = child.props?.width;
    if (typeof declared === "number" && declared > 0) {
      line = padTo(line, declared);
    }
    if (typeof child.props?.paddingLeft === "number" && child.props.paddingLeft > 0) {
      line = " ".repeat(child.props.paddingLeft) + line;
    }
    let margin = 0;
    if (typeof child.props?.marginLeft === "number" && child.props.marginLeft > 0) {
      margin = child.props.marginLeft;
    }
    parts.push({ line, margin, flex: Boolean(child.props?.flexGrow) });
  }
  const occupied =
    parts.reduce((sum, part) => sum + part.margin + lineWidth(part.line), 0);
  if (occupied > inner) return null;
  const flexCount = parts.filter((part) => part.flex).length;
  const spaceBetween = node.props?.justifyContent === "space-between";
  if (flexCount === 0 && !spaceBetween) return null;
  let slack = inner - occupied;
  const gapCount = spaceBetween && flexCount === 0 ? parts.length - 1 : 0;
  const out = [];
  parts.forEach((part, index) => {
    if (part.margin) out.push(" ".repeat(part.margin));
    out.push(part.line);
    if (part.flex && flexCount > 0) {
      const share = Math.floor(slack / flexCount);
      if (share > 0) out.push(" ".repeat(share));
      // Remainder spaces trail the LAST flexible part (integer slack).
      if (index === parts.length - 1) {
        const remainder = slack - share * flexCount;
        if (remainder > 0) out.push(" ".repeat(remainder));
      }
    } else if (gapCount > 0 && index < gapCount) {
      const share = Math.floor(slack / gapCount);
      if (share > 0) out.push(" ".repeat(share));
      if (index === gapCount - 1) {
        const remainder = slack - share * gapCount;
        if (remainder > 0) out.push(" ".repeat(remainder));
      }
    }
  });
  const line = out.join("");
  return line.trim() === "" ? null : line;
}

/** Does this node (or the box that produced a line) carry a pointer
 *  activate handler? `decorateInteractiveRow` sets `onMouseUp` on the
 *  ROW box itself (tui2.js:30462-30467). */
function rowActivateHandler(node) {
  return typeof node?.props?.onMouseUp === "function" ? node.props.onMouseUp : undefined;
}

/** `load.tui` — import the package's ./tui export and drive its
 *  `tui(api)` with the bridge's TuiApi shape. */
async function handleLoadTui({ entry, options }) {
  const abs = require("node:path").resolve(entry);
  // The package root (package.json dir) — the shims land in the
  // sibling node_modules so bare imports from dist/ resolve.
  let packageRoot = require("node:path").dirname(abs);
  for (;;) {
    if (require("node:fs").existsSync(require("node:path").join(packageRoot, "package.json"))) break;
    const parent = require("node:path").dirname(packageRoot);
    if (parent === packageRoot) break;
    packageRoot = parent;
  }
  ensureOpentuiShims(packageRoot);
  const mod = await import(`file://${abs}`);
  const def = mod.default;
  if (!def || typeof def !== "object" || typeof def.tui !== "function") {
    throw new Error(`${entry}: default export has no tui(api) entry`);
  }
  const pluginId = typeof def.id === "string" && def.id ? def.id : `tui:${deriveId(abs)}`;
  // SA-F5: the badge must show the PLUGIN's own version — v2 passes
  // the loaded package's version as the tui() metadata (omo-slim
  // renders `v${meta.version}`, tui2.js: `meta.version ?? readPackageVersion()
  // ?? "dev"`); the app version we passed before materialized as the
  // wrong badge (`v2.0.24` where the ref showed `v3.0.4`).
  let pluginVersion = "dev";
  try {
    const pkgText = require("node:fs").readFileSync(
      require("node:path").join(packageRoot, "package.json"),
      "utf8",
    );
    const pkg = JSON.parse(pkgText);
    if (typeof pkg.version === "string" && pkg.version) pluginVersion = pkg.version;
  } catch {
    // Unreadable/absent package.json: "dev" (the plugin's own fallback).
  }
  const loadOptions = options && typeof options === "object" && !Array.isArray(options) ? options : {};
  const record = { id: pluginId, entry: abs, dispose: null, subscriptions: [], commands: new Map() };
  plugins.set(pluginId, record);

  const clientStub = new Proxy(
    {},
    {
      get(_target, prop) {
        if (prop === "then" || prop === "catch" || prop === "finally") return undefined;
        // Every SDK surface degrades to a rejecting promise (the
        // sidebar's remote hydration treats failures as "no data").
        return () => Promise.reject(new Error(`tui-bridge client stub: ${String(prop)} unavailable`));
      },
    },
  );
  const disposeLifecycle = [];
  const api = {
    app: { version: domainBase.appVersion },
    version: domainBase.appVersion,
    options: loadOptions,
    tuiConfig: undefined,
    state: {
      ready: true,
      path: { state: "", config: "", worktree: domainBase.directory, directory: domainBase.directory },
      vcs: undefined,
      session: {
        count: () => 0,
        get: () => undefined,
        diff: () => [],
        todo: () => [],
        messages: () => [],
        status: () => undefined,
        permission: () => [],
        question: () => [],
      },
      part: () => [],
      lsp: () => [],
      mcp: () => [],
    },
    route: {
      current: () => tuiRoute,
      navigate(name, params) {
        const sessionID = typeof params?.sessionID === "string" ? params.sessionID : undefined;
        notify("tui.route.navigate", { sessionID: sessionID ?? null, name: String(name ?? "") });
      },
      register: () => () => {},
    },
    theme: {
      get current() {
        return tuiThemeView;
      },
    },
    client: clientStub,
    renderer: { requestRender: () => { hostRequest("ui.slot.rerender", { pluginId }).catch(() => {}); } },
    event: { on: () => () => {} },
    keys: { map: () => ({}) },
    keymap: { registerLayer: () => () => {}, dispatchCommand: () => {} },
    mode: { set: () => {}, push: () => {}, pop: () => {} },
    attention: {
      request: () => {},
      notify: () => Promise.resolve({ ok: false, reason: "attention_disabled" }),
    },
    kv: { get: async () => undefined, set: async () => {} },
    ui: {
      toast: () => {},
      dialog: { open: () => {}, close: () => {}, alert: () => {}, confirm: () => {} },
      slot: (spec) => {
        const append = String(spec?.append ?? "");
        const render = spec?.render;
        if (typeof render !== "function") throw new Error("ui.slot requires a render function");
        if (append === "sidebar.content") registerSidebarSlot(pluginId, render);
        return () => {};
      },
      Slot: () => null,
    },
    slots: {
      register(spec) {
        const slots = spec?.slots ?? {};
        const render = slots.sidebar_content ?? slots["sidebar.content"];
        if (typeof render === "function") registerSidebarSlot(pluginId, render);
        return `${pluginId}:slots`;
      },
    },
    command: {
      register(builder) {
        try {
          builder?.();
        } catch (error) {
          notify("log", { pluginId, level: "warn", message: `tui command register: ${errText(error)}` });
        }
        return () => {};
      },
    },
    lifecycle: {
      signal: new AbortController().signal,
      onDispose(fn) {
        disposeLifecycle.push(fn);
        return () => {};
      },
    },
    plugins: { list: () => [], activate: async () => false, deactivate: async () => false, add: async () => false },
  };
  await def.tui(api, loadOptions, { id: pluginId, version: pluginVersion, state: "first" });
  const dispose = async () => {
    for (const fn of disposeLifecycle.splice(0)) {
      try {
        await fn();
      } catch (error) {
        notify("log", { pluginId, level: "warn", message: `tui dispose: ${errText(error)}` });
      }
    }
    sidebarSlots.delete(pluginId);
  };
  tuiDisposers.set(pluginId, dispose);
  return { ok: true, pluginId, sidebar: sidebarSlots.has(pluginId) };
}

function registerSidebarSlot(pluginId, render) {
  sidebarSlots.set(pluginId, { render });
  hostRequest("ui.slot.register", { pluginId, slot: "sidebar.content" }).catch(() => {});
}

/** `slot.materialize` — run the plugin's render function NOW and
 *  flatten the tree into the native section shape. Rows that carry
 *  the plugin's own `onMouseUp` (v2 `decorateInteractiveRow` — the
 *  sidebar's session/preset targets, tui2.js:30462-30467) are marked
 *  clickable and their handlers stashed per row index for
 *  `slot.click`. */
async function handleSlotMaterialize({ pluginId }) {
  const slot = sidebarSlots.get(pluginId);
  if (!slot || typeof slot.render !== "function") {
    throw new Error(`${pluginId}: no sidebar_content slot registered`);
  }
  const { section, handlers } = flattenRenderTree(slot.render());
  slot.clickHandlers = handlers;
  return section;
}

/** Flatten one rendered tree (the claim/route renders use the same
 *  native section shape as the sidebar bridge — PLUG-TUI-SLOT):
 *  `{title, rows:[{text,click}]}` plus the per-row activate handler
 *  table (index-aligned with the body rows). */
function flattenRenderTree(tree) {
  const lines = treeLines(tree)
    .map((line) => line.replace(/\s+$/, ""))
    .filter((line) => line.trim() !== "");
  const handlers = new Map();
  const flatChildren = [];
  for (const child of tree?.children ?? []) {
    const handler = rowActivateHandler(child);
    const childLines = treeLines(child)
      .map((line) => line.replace(/\s+$/, ""))
      .filter((line) => line.trim() !== "");
    for (const _line of childLines) {
      flatChildren.push(handler);
    }
  }
  const bodyRows = lines.slice(1);
  bodyRows.forEach((_text, index) => {
    const handler = flatChildren[index + 1];
    if (handler) handlers.set(index, handler);
  });
  return {
    section: {
      title: lines.length > 0 ? lines[0] : "",
      rows: bodyRows.map((text, index) => ({ text, click: handlers.has(index) })),
    },
    handlers,
  };
}

/** `slot.claim.render` — run one `ctx.ui.slot` claim's render NOW
 *  (api.tsx:298-312's claim table; bounded by the host's request
 *  timeout) and flatten to the native section shape. */
async function handleClaimRender({ pluginId, key }) {
  const claim = slotClaims.get(pluginId)?.get(String(key ?? ""));
  if (!claim || typeof claim.render !== "function") {
    throw new Error(`${pluginId}: no slot claim ${key}`);
  }
  const { section, handlers } = flattenRenderTree(claim.render({}));
  claim.clickHandlers = handlers;
  return section;
}

/** `slot.claim.click` — run one claim row's stashed `onMouseUp`. */
async function handleClaimClick({ pluginId, key, row }) {
  const claim = slotClaims.get(pluginId)?.get(String(key ?? ""));
  if (!claim) throw new Error(`${pluginId}: no slot claim ${key}`);
  const handler = claim.clickHandlers?.get(Number(row));
  if (typeof handler !== "function") return { ok: false };
  handler();
  return { ok: true };
}

/** `route.render` — run one registered route page's render
 *  (api.tsx:204-212) with the navigation data and flatten. */
async function handleRouteRender({ pluginId, name, data }) {
  const render = pluginRoutes.get(pluginId)?.get(String(name ?? ""));
  if (typeof render !== "function") {
    throw new Error(`${pluginId}: no route ${name}`);
  }
  const { section, handlers } = flattenRenderTree(render({ data: data ?? undefined }));
  return section;
}

/** `keymap.command.run` — run one keymap-layer command's `run`
 *  (api.tsx:179-199; the TUI dispatches a resolved bind back). */
async function handleKeymapRun({ pluginId, id }) {
  const run = keymapRuns.get(pluginId)?.get(String(id ?? ""));
  if (typeof run !== "function") return { ok: false };
  await run();
  return { ok: true };
}

/** `slot.click` — run one row's stashed activate handler (the click
 *  side of the bridge: the handler navigates / opens what the plugin
 *  wants; navigation flows back through `tui.route.navigate`). */
async function handleSlotClick({ pluginId, row }) {
  const slot = sidebarSlots.get(pluginId);
  if (!slot) throw new Error(`${pluginId}: no sidebar_content slot registered`);
  const handler = slot.clickHandlers?.get(Number(row));
  if (typeof handler !== "function") return { ok: false };
  handler();
  return { ok: true };
}

// ---------------------------------------------------------------------------
// Method table + read loop
// ---------------------------------------------------------------------------

const METHODS = {
  initialize: async () => ({ ok: true, bun: Bun.version }),
  ping: async () => ({ ok: true }),
  load: handleLoad,
  "load.tui": handleLoadTui,
  unload: handleUnload,
  dispatch: handleDispatch,
  "command.execute": handleCommandExecute,
  "hook.dispatch": handleHookDispatch,
  "rpc.call": handleRpcCall,
  "tool.execute": handleToolExecute,
  "codemode.evaluate": handleCodemodeEvaluate,
  "dialog.materialize": handleDialogMaterialize,
  "slot.materialize": handleSlotMaterialize,
  "slot.click": handleSlotClick,
  "slot.claim.render": handleClaimRender,
  "slot.claim.click": handleClaimClick,
  "route.render": handleRouteRender,
  "keymap.command.run": handleKeymapRun,
  scan: async ({ entry }) => ({ digests: await scanDigests(entry) }),
};

async function handleLine(line) {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  if (message.id !== undefined && message.method !== undefined) {
    // Supervisor request: handled WITHOUT blocking the read loop —
    // the handler may issue nested hostRequests whose replies arrive
    // on this same stdin stream.
    const handler = METHODS[message.method];
    if (!handler) {
      send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: `unknown method ${message.method}` } });
      return;
    }
    try {
      const result = await handler(message.params ?? {});
      send({ jsonrpc: "2.0", id: message.id, result });
    } catch (error) {
      send({ jsonrpc: "2.0", id: message.id, error: { code: -32000, message: errText(error) } });
    }
    return;
  }
  if (message.id !== undefined) {
    const entry = pendingGuest.get(message.id);
    if (entry) {
      pendingGuest.delete(message.id);
      if (message.error) entry.reject(new Error(message.error.message ?? "guest error"));
      else entry.resolve(message.result ?? {});
    }
    return;
  }
  if (message.method === "tui.route") {
    // Host-pushed current route (the TUI's live session): the
    // sidebar render reads it at every materialize.
    const sessionID = message.params?.sessionID;
    tuiRoute =
      typeof sessionID === "string" && sessionID
        ? { name: "session", params: { sessionID } }
        : { name: "home" };
    return;
  }
  if (message.method === "tui.theme") {
    const view = message.params ?? {};
    tuiThemeView = {
      accent: typeof view.accent === "string" ? view.accent : tuiThemeView.accent,
      background: typeof view.background === "string" ? view.background : tuiThemeView.background,
      borderActive: typeof view.borderActive === "string" ? view.borderActive : tuiThemeView.borderActive,
      text: typeof view.text === "string" ? view.text : tuiThemeView.text,
      textMuted: typeof view.textMuted === "string" ? view.textMuted : tuiThemeView.textMuted,
    };
    return;
  }
  if (message.method === "tui.layout") {
    // SA-F5: the sidebar's row budget (the native painter's cell span)
    // — the flattener lays the plugin's space-between/flex rows out
    // against it so the padded columns materialize.
    const width = message.params?.sidebarContentWidth;
    if (typeof width === "number" && Number.isFinite(width) && width > 0) {
      tuiSidebarContentWidth = Math.floor(width);
    }
    return;
  }
  if (message.method === "log") {
    // Supervisor-originated notifications (unused today) — contained.
  }
}

// The read loop: NEVER await handleLine — reentrancy is the rule
// (see crate docs). Errors on one line never stop the loop.
const decoder = new TextDecoder();
let buffer = "";
for await (const chunk of process.stdin) {
  buffer += decoder.decode(chunk, { stream: true });
  let at;
  while ((at = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, at);
    buffer = buffer.slice(at + 1);
    if (line.trim()) {
      handleLine(line).catch((error) => {
        notify("log", { level: "error", message: `handler crashed: ${errText(error)}` });
      });
    }
  }
}
