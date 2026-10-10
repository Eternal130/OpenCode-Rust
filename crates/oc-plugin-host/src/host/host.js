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
// Runtime hook registry (guest-side handlers; coverage registered)
// ---------------------------------------------------------------------------

const hookHandlers = { session: new Map(), tool: new Map() };

const SESSION_HOOKS = new Set([
  "prompt", "context", "compaction", "generate", "title", "model.request",
  "http.request", "http.response",
  "experimental.ws.handshake", "experimental.ws.send", "experimental.ws.receive",
  "retry",
]);
const TOOL_HOOKS = new Set(["execute.before", "execute.after"]);

async function registerHook(pluginId, domain, name, handlers) {
  if (domain === "session" ? !SESSION_HOOKS.has(name) : !TOOL_HOOKS.has(name)) {
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
        if (typeof first === "function" || Array.isArray(first)) {
          return (async () => {
            const types = Array.isArray(first) ? first : null;
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
  plugins.delete(pluginId);
  dialogProviders.delete(pluginId);
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
  const result = await execute(invocation, makeCtx(pluginId));
  return { ok: true, result: result ?? null };
}

async function handleHookDispatch({ pluginId, domain, name, event }) {
  const list = hookHandlers[domain]?.get(name) ?? [];
  const errors = [];
  const target = list.filter((entry) => !pluginId || entry.pluginId === pluginId);
  if (target.length === 0) return { ok: true, event: event ?? null, errors };
  notify("plugin.busy", { pluginId: target[0].pluginId });
  try {
    for (const entry of target) {
      try {
        await entry.handler(event);
      } catch (error) {
        errors.push(`${entry.pluginId}: ${errText(error)}`);
      }
    }
  } finally {
    notify("plugin.idle", { pluginId: target[0].pluginId });
  }
  return { ok: errors.length === 0, event: event ?? null, errors };
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
// Method table + read loop
// ---------------------------------------------------------------------------

const METHODS = {
  initialize: async () => ({ ok: true, bun: Bun.version }),
  ping: async () => ({ ok: true }),
  load: handleLoad,
  unload: handleUnload,
  dispatch: handleDispatch,
  "command.execute": handleCommandExecute,
  "hook.dispatch": handleHookDispatch,
  "dialog.materialize": handleDialogMaterialize,
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
