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

// ---------------------------------------------------------------------------
// IPC liveness (incident 2026-10-22: the orphaned plugin host)
// ---------------------------------------------------------------------------
// The supervisor owns the write end of our stdin and the read end of our
// stdout. When it goes away — graceful shutdown, abrupt exit, SIGKILL —
// BOTH pipe ends close: stdin reads EOF and stdout writes fail with
// EPIPE. Neither may be ignored:
//
//   * EOF with plugin `setInterval` timers alive used to leave the
//     for-await read loop finished but the PROCESS alive forever
//     (Bun keeps the event loop up for timers) — the orphan leak.
//   * A failed `process.stdout.write` throws SYNCHRONOUSLY in Bun; the
//     throw surfaced inside `uncaughtException`/`scheduledFault`
//     handlers whose own `notify` wrote again → a synchronous
//     write-throw loop measured at ~230k failed writes/s pegging one
//     core at ~101% (the spin).
//
// Every supervisor-going-away signal therefore funnels into
// `hostGone()` — a one-shot clean `process.exit(0)`. The host owns our
// lifetime; without it there is nothing to talk to and nothing to do.
let supervisorGone = false;

function hostGone(reason) {
  if (supervisorGone) return;
  supervisorGone = true;
  // Best-effort last line — the pipe may already be dead; never throw
  // on the way out.
  try {
    nativeProcess.stdout.write(
      JSON.stringify({ jsonrpc: "2.0", method: "host.exit", params: { reason } }) + "\n",
    );
  } catch {
    // EPIPE — the reason we are exiting.
  }
  nativeProcess.exit(0);
}

function send(message) {
  if (supervisorGone) return; // pipe is dead; a write would throw
  try {
    nativeProcess.stdout.write(JSON.stringify(message) + "\n");
  } catch (error) {
    // EPIPE: the supervisor's read end is gone. Clean shutdown — the
    // throw must NEVER escape into error handlers (the spin).
    hostGone(`stdout write failed: ${errText(error)}`);
  }
}

function notify(method, params) {
  send({ jsonrpc: "2.0", method, params });
}

// --- Structural resource caps (interpreter/limits.ts, ported) ---
// v2's confined interpreter bounds what one builtin call may build
// inside its own allocator. A real JS engine has no such bookkeeping
// machine, so the port enforces the same caps where the guest
// controls the resource: every string/array crossing the host↔guest
// bridge (tool inputs, tool results, the program return value), and
// the count of promises pending on the bridge at once. Values copied
// from interpreter/limits.ts:7-11.
const MAX_STRING_LENGTH = 1 << 24;
const MAX_ARRAY_LENGTH = 10_000_000;
const MAX_PENDING_PROMISES = 10_000;

// checkStringLength / checkArrayLength (interpreter/limits.ts:15-21)
// as one boundary walk: the ref's exact RangeError messages, thrown
// at the crossing instead of at allocation.
function checkValueLimits(value) {
  const stack = [value];
  while (stack.length > 0) {
    const current = stack.pop();
    if (typeof current === "string") {
      if (current.length > MAX_STRING_LENGTH) throw new RangeError("Invalid string length");
    } else if (Array.isArray(current)) {
      if (current.length > MAX_ARRAY_LENGTH) throw new RangeError("Invalid array length");
      for (let i = 0; i < current.length; i += 1) stack.push(current[i]);
    } else if (current !== null && typeof current === "object") {
      const values = Object.values(current);
      for (let i = 0; i < values.length; i += 1) stack.push(values[i]);
    }
  }
}

function hostRequest(method, params) {
  return new Promise((resolve, reject) => {
    // MAX_PENDING_PROMISES (interpreter/limits.ts:11, enforced at
    // promises.ts:45-49): a bridge request is the guest's unit of
    // pending work — the (N+1)-th rejects with the ref's exact
    // message instead of queueing without bound.
    if (pendingGuest.size >= MAX_PENDING_PROMISES) {
      reject(
        new RangeError(
          `Too many pending promises (limit ${MAX_PENDING_PROMISES}); await promises before creating more.`,
        ),
      );
      return;
    }
    const id = nextGuestId++;
    pendingGuest.set(id, { resolve, reject });
    send({ jsonrpc: "2.0", id, method, params });
  });
}

function errText(error) {
  if (error instanceof Error) return error.message;
  return String(error);
}

// The deferred ctx.ui dialog promises settle `null` on an unresolved
// close (api.tsx:318-325's `settle`); the wire renders a null result
// as {"ok": true} (protocol.rs) — unwrap it back so the guest surface
// keeps the ref's Promise<… | null> shape.
function dialogResult(reply) {
  if (
    reply !== null &&
    typeof reply === "object" &&
    reply.ok === true &&
    Object.keys(reply).length === 1
  ) {
    return null;
  }
  return reply;
}

// The async halves of the pipe-death detection: Bun surfaces some
// EPIPE/EOF conditions as stream events rather than synchronous write
// throws. Every one of them is the same fact — the supervisor is gone.
process.stdout.on("error", (error) => {
  hostGone(`stdout error: ${errText(error)}`);
});
process.stdin.on("error", (error) => {
  hostGone(`stdin error: ${errText(error)}`);
});
process.stdin.on("end", () => {
  hostGone("stdin end (EOF)");
});
process.stdin.on("close", () => {
  hostGone("stdin closed");
});

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
  // Guarded by send(): if the pipe is already dead, notify() is a
  // no-op and this handler returns instead of throwing again (the
  // re-entrant write-throw spin).
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
// Module-scope native timers: the SA-F16 shield wrappers below
// replace globalThis.set*, and the Code Mode window (G-247) replaces
// them again with throwing placeholders — internal uses (the Code
// Mode soft-timeout race) must keep scheduling on the real ones.
const nativeSetInterval = globalThis.setInterval.bind(globalThis);
const nativeSetTimeout = globalThis.setTimeout.bind(globalThis);
// G-334c — module-scope natives for `process`/`Bun`: the Code Mode
// window swaps those globals for throwing facades (the sandbox must
// not expose Bun.file/spawn or process to guest programs), so every
// HOST-plumbing use (the NDJSON stdout channel, cwd fallbacks, the
// hot-reload digest scan, the bun-version probe) goes through these
// captured bindings instead of the global lookups.
const nativeProcess = globalThis.process;
const nativeBun = globalThis.Bun;
{
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
let domainBase = { agents: [], mcps: {}, appVersion: "2.0.24", directory: nativeProcess.cwd(), project: { id: "global" } };

/** Current derived state: base ⊕ editors (editors replayed on reload). */
const domainStores = {
  agents: new Map(), // id -> Agent.Info (v2 agent.ts editor semantics)
  agentDefault: undefined,
  mcps: new Map(), // name -> Mcp.ServerConfig
  skills: new Map(), // id -> Skill.Info
  tools: new Map(), // name -> tool definition
  providers: new Map(), // G-64: id -> provider definition
  // G-147: model editor state — the lazily fetched catalog base, the
  // override records, the removal marks, and the editor-set default.
  modelBase: undefined, // undefined = not fetched; null = fetch failed
  modelOverrides: new Map(), // "providerID/modelID" -> Model.Info
  modelRemoved: new Set(), // "providerID/modelID"
  modelDefault: undefined,
  references: new Map(), // G-64: name -> Reference.Source
  vcs: new Map(), // G-24 residual: id -> vcs provider row
  // G-384: the integration editor's derived table — id -> {id, name,
  // metadata} rows this host's plugins registered (per-plugin rows;
  // the whole table re-crosses at every materialization).
  integrations: new Map(),
};

// G-384 — integration editor ownership + tombstones (the vcs.transform
// pattern): `${pluginId}:${id}` → id for live rows, and the remove
// markers (integration removals and method removals) the next
// materialization carries so the engine retracts them.
const integrationOwners = new Map();
const integrationRemovals = new Map(); // `${pluginId}:${id}` -> id
const integrationMethodRemovals = new Map(); // `${pluginId}:${iid}:${methodKey}` -> {integrationID, method}

/** Registered editors per domain, in registration order. */
const domainEditors = { agent: [], mcp: [], skill: [], tool: [], command: [], provider: [], reference: [], model: [] };

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
  provider: {
    // G-64 (adapter.ts:346-370's editor surface): list/get/add/update/
    // remove plus the models sub-editor, mirroring the ref
    // ProviderDomain (IDs are strings on the wire).
    list: () => Array.from(domainStores.providers.values()),
    get: (id) => domainStores.providers.get(String(id)),
    add: (definition) => {
      const id = String(definition?.id ?? "");
      if (!id) throw new Error("provider.add requires an id");
      domainStores.providers.set(id, cloneJson(definition));
    },
    update: (id, update) => {
      const current = domainStores.providers.get(String(id));
      if (!current) return;
      deepMerge(current, update ?? {});
    },
    remove: (id) => {
      domainStores.providers.delete(String(id));
    },
    models: {
      set: (id, models) => {
        const current = domainStores.providers.get(String(id));
        if (!current) return;
        current.models = typeof models === "object" && models !== null ? cloneJson(models) : {};
      },
      update: (providerID, modelID, update) => {
        const current = domainStores.providers.get(String(providerID));
        if (!current) return;
        current.models = current.models ?? {};
        if (!current.models[modelID]) current.models[modelID] = { id: modelID };
        deepMerge(current.models[modelID], update ?? {});
      },
      remove: (providerID, modelID) => {
        const current = domainStores.providers.get(String(providerID));
        if (!current?.models) return;
        delete current.models[modelID];
      },
    },
  },
  model: {
    // G-147 (ref promise/model.ts:6-23 ModelEditor): list/get over
    // the port-fetched catalog base merged with the override table;
    // update/remove edit overrides; default get/set; provider.list/
    // get expose the immutable provider inputs.
    list: (providerID) =>
      modelRows()
        .filter((model) => !providerID || String(model.providerID ?? "") === String(providerID))
        .map(cloneJson),
    get: (providerID, modelID) => {
      const row = modelRows().find(
        (model) =>
          String(model.providerID ?? "") === String(providerID) &&
          String(model.id ?? model.modelID ?? "") === String(modelID),
      );
      return row ? cloneJson(row) : undefined;
    },
    update: (providerID, modelID, fn) => {
      const key = `${String(providerID)}/${String(modelID)}`;
      let current = domainStores.modelOverrides.get(key);
      if (!current) {
        const base = (domainStores.modelBase ?? []).find(
          (model) =>
            String(model.providerID ?? "") === String(providerID) &&
            String(model.id ?? model.modelID ?? "") === String(modelID),
        );
        current = cloneJson(base ?? { id: String(modelID) });
        domainStores.modelOverrides.set(key, current);
      }
      fn(current);
      current.providerID = String(providerID);
      if (current.id === undefined) current.id = String(modelID);
      domainStores.modelRemoved.delete(key);
    },
    remove: (providerID, modelID) => {
      const key = `${String(providerID)}/${String(modelID)}`;
      domainStores.modelOverrides.delete(key);
      domainStores.modelRemoved.add(key);
    },
    default: {
      get: () => cloneJson(domainStores.modelDefault),
      set: (providerID, modelID) => {
        domainStores.modelDefault = { providerID: String(providerID), modelID: String(modelID) };
      },
    },
    provider: {
      // Immutable provider inputs (the base rows' providers).
      list: () => {
        const ids = new Map();
        for (const model of modelRows()) {
          const id = String(model.providerID ?? "");
          if (id && !ids.has(id)) ids.set(id, { id, name: model.providerName ?? id });
        }
        return Array.from(ids.values());
      },
      get: (providerID) =>
        modelRows().find((model) => String(model.providerID ?? "") === String(providerID))
          ? { id: String(providerID), name: String(providerID) }
          : undefined,
    },
  },
  reference: {
    // G-64 (adapter.ts:427-452): add(name, source)/remove/list/get.
    list: () =>
      Array.from(domainStores.references.entries()).map(([name, source]) => ({ name, source })),
    get: (name) => domainStores.references.get(String(name)),
    add: (name, source) => {
      const key = String(name ?? "");
      if (!key) throw new Error("reference.add requires a name");
      domainStores.references.set(key, cloneJson(source ?? {}));
    },
    remove: (name) => {
      domainStores.references.delete(String(name));
    },
  },
};

/** Shallow-recursive plain-object merge (the ref editor `update`
 *  semantics: the patch's keys win; nested objects merge). */
function deepMerge(target, patch) {
  for (const [key, value] of Object.entries(patch)) {
    if (
      value !== null &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      target[key] !== null &&
      typeof target[key] === "object" &&
      !Array.isArray(target[key])
    ) {
      deepMerge(target[key], value);
    } else {
      target[key] = value;
    }
  }
}

function cloneJson(value) {
  return structuredClone(value);
}

// G-147: the merged model view — the catalog base (lazily fetched via
// `model.list`) with the override records applied and the removed
// entries filtered out.
function modelRows() {
  const base = Array.isArray(domainStores.modelBase) ? domainStores.modelBase : [];
  const overridden = new Set(
    Array.from(domainStores.modelOverrides.values()).map(
      (model) => `${String(model.providerID ?? "")}/${String(model.id ?? model.modelID ?? "")}`,
    ),
  );
  const rows = base
    .filter((model) => !overridden.has(`${String(model.providerID ?? "")}/${String(model.id ?? model.modelID ?? "")}`))
    .filter((model) => !domainStores.modelRemoved.has(`${String(model.providerID ?? "")}/${String(model.id ?? model.modelID ?? "")}`))
    .map(cloneJson);
  for (const model of domainStores.modelOverrides.values()) {
    if (!domainStores.modelRemoved.has(`${String(model.providerID ?? "")}/${String(model.id ?? model.modelID ?? "")}`)) {
      rows.push(cloneJson(model));
    }
  }
  return rows;
}

// G-147: fetch the catalog base once per editor generation (a failed
// or absent port degrades to override-only editing).
async function ensureModelBase(pluginId) {
  if (domainStores.modelBase !== undefined) return;
  try {
    const result = await hostRequest("model.list", { pluginId, args: {} });
    const value = result && typeof result === "object" && "value" in result ? result.value : result;
    domainStores.modelBase = Array.isArray(value) ? value : [];
  } catch {
    domainStores.modelBase = [];
  }
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
  domainStores.providers = new Map();
  domainStores.references = new Map();
  domainStores.vcs = new Map();
  vcsOwners.clear();
  domainStores.integrations = new Map();
  integrationOwners.clear();
  integrationRemovals.clear();
  integrationMethodRemovals.clear();
  // G-147: the model editor's derived state resets with the base
  // (the next transform re-fetches the catalog).
  domainStores.modelBase = undefined;
  domainStores.modelOverrides = new Map();
  domainStores.modelRemoved = new Set();
  domainStores.modelDefault = undefined;
  domainStores.websearch = [];
  domainStores.worktree = [];
}

let domainsSeeded = false;

function seedDomains(seed) {
  domainBase = {
    agents: Array.isArray(seed?.agents) ? seed.agents : [],
    mcps: seed?.mcps && typeof seed.mcps === "object" ? seed.mcps : {},
    appVersion: typeof seed?.appVersion === "string" && seed.appVersion ? seed.appVersion : "2.0.24",
    directory: typeof seed?.directory === "string" && seed.directory ? seed.directory : nativeProcess.cwd(),
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
  } else if (kind === "websearch") {
    await hostRequest("websearch.transform", {
      pluginId,
      providers: domainStores.websearch,
    });
  } else if (kind === "worktree") {
    await hostRequest("worktree.transform", {
      pluginId,
      strategies: domainStores.worktree,
    });
  } else if (kind === "provider") {
    await hostRequest("provider.transform", {
      pluginId,
      providers: Array.from(domainStores.providers.values()),
    });
  } else if (kind === "reference") {
    await hostRequest("reference.transform", {
      pluginId,
      references: Array.from(domainStores.references.entries()).map(([name, source]) => ({ name, source })),
    });
  } else if (kind === "model") {
    await hostRequest("model.transform", {
      pluginId,
      models: Array.from(domainStores.modelOverrides.values()),
      default: domainStores.modelDefault ?? null,
    });
  } else if (kind === "vcs") {
    await hostRequest("vcs.transform", {
      pluginId,
      providers: Array.from(domainStores.vcs.values()),
    });
  } else if (kind === "integration") {
    // G-384 — the whole derived table + this host's remove markers
    // cross at once; rows keep plugin attribution, removals retract.
    await hostRequest("integration.transform", {
      pluginId,
      integrations: Array.from(domainStores.integrations.values()),
      removed: Array.from(integrationRemovals.values()),
      removedMethods: Array.from(integrationMethodRemovals.values()),
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
    provider: () => {
      domainStores.providers = new Map();
    },
    model: () => {
      domainStores.modelBase = undefined;
      domainStores.modelOverrides = new Map();
      domainStores.modelRemoved = new Set();
      domainStores.modelDefault = undefined;
    },
    reference: () => {
      domainStores.references = new Map();
    },
  };
  (bases[kind] ?? (() => {}))();
  if (kind === "model") await ensureModelBase(pluginId);
  for (const entry of domainEditors[kind]) {
    entry.cb(editors[kind]);
  }
  await materializeDomain(kind, pluginId);
}

// ---------------------------------------------------------------------------
// G-384 — the shared `ctx.integration.method.update` implementation: the
// plain `ctx.integration.method.update` face AND the transform editor's
// `editor.method.update` both route here, so the authorize/refresh
// closure markers always ride the existing G-24 hook dispatch
// (`integration/authorize|callback|refresh`) into the SAME
// integration_methods registration table (never forked).
// ---------------------------------------------------------------------------

async function integrationMethodUpdate(pluginId, input) {
  const hasAuthorize = typeof input?.authorize === "function";
  const hasRefresh = typeof input?.refresh === "function";
  const args = {
    integrationID: input?.integrationID,
    method: input?.method,
    authorize: hasAuthorize,
    refresh: hasRefresh,
  };
  if (hasAuthorize) {
    await registerHook(pluginId, "integration", "authorize", async (event) => {
      if (!event || typeof event !== "object") return;
      const authorization = await input.authorize(event.answer ?? {});
      if (!authorization || typeof authorization !== "object") return;
      if (authorization.mode === "auto" && typeof authorization.callback === "function") {
        pendingIntegrationCallback = authorization.callback;
        const credential = await authorization.callback();
        event.result = { ...authorization, callback: undefined, credential };
      } else {
        pendingIntegrationCallback = authorization.callback ?? null;
        event.result = { ...authorization, callback: undefined };
      }
    });
    await registerHook(pluginId, "integration", "callback", async (event) => {
      if (!event || typeof event !== "object" || typeof pendingIntegrationCallback !== "function") {
        return;
      }
      const credential = await pendingIntegrationCallback(event.code);
      event.credential = credential;
    });
  }
  if (hasRefresh) {
    await registerHook(pluginId, "integration", "refresh", async (event) => {
      if (!event || typeof event !== "object") return;
      event.credential = await input.refresh(event.credential);
    });
  }
  await hostRequest("integration.method.update", { pluginId, args });
}

// ---------------------------------------------------------------------------
// RPC handler registry (PLUG-D14: rpc id -> method -> handler; the
// definitions cross to the supervisor at register time, the handlers
// stay here and run under the host's `rpc.call` requests)
// ---------------------------------------------------------------------------

/** @type {Map<string, Map<string, Function>>} */
const rpcHandlers = new Map();

/** G-296 — the portable definitions by rpc id (the declared-errors
 *  gate for `ctx.error` and the declared-events surface). */
const rpcDefinitions = new Map();

/** One host→guest `rpc.call` — run the registered handler with the v2
 *  call context (`ctx.error(type, message, data)` declares a typed
 *  failure; anything else thrown is a defect → rpc.internal).
 *  G-296: `ctx.error` with a type NOT declared in the definition's
 *  `methods[method].errors` follows the ref's encodeError die path —
 *  logged + normalized to `rpc.internal` (rpc.ts:232-241 +
 *  catchDefect :127-131), never an undeclared typed failure. */
async function handleRpcCall({ pluginId, rpcId, method, input }) {
  const key = String(rpcId ?? "");
  const table = rpcHandlers.get(key);
  const handler = table?.get(String(method ?? ""));
  if (typeof handler !== "function") {
    return { ok: false, failure: { type: "rpc.method_not_found", message: `Unknown RPC method: ${rpcId}.${method}` } };
  }
  const declaredErrors = rpcDefinitions.get(key)?.methods?.[method]?.errors;
  const isErrorTypeDeclared = (type) =>
    !!declaredErrors && typeof declaredErrors === "object" && Object.prototype.hasOwnProperty.call(declaredErrors, String(type));
  notify("plugin.busy", { pluginId });
  try {
    const callContext = {
      error(type, message, data) {
        const name = String(type ?? "");
        if (!isErrorTypeDeclared(name)) {
          // ref encodeError: undeclared type ⇒ Effect.die ⇒ the host's
          // catchDefect normalizes to rpc.internal after logging.
          notify("log", { pluginId, level: "error", message: `rpc ${rpcId}.${method}: undeclared RPC error type: ${name}` });
          const internal = new Error("RPC call failed");
          internal.__rpcFailure = { type: "rpc.internal", message: "RPC call failed" };
          return internal;
        }
        const failure = new Error(message ?? "RPC call failed");
        failure.__rpcFailure = { type: name, message: String(message ?? ""), data };
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
const hookHandlers = { session: new Map(), tool: new Map(), shell: new Map(), permission: new Map(), aisdk: new Map(), websearch: new Map(), worktree: new Map(), integration: new Map(), vcs: new Map() };

const SESSION_HOOKS = new Set([
  "prompt", "context", "compaction", "generate", "title", "model.request",
  "http.request", "http.response",
  "experimental.ws.handshake", "experimental.ws.send", "experimental.ws.receive",
  "retry",
]);
const TOOL_HOOKS = new Set(["execute.before", "execute.after"]);
const SHELL_HOOKS = new Set(["create.before"]);
// PLUG-G2: the remaining v2 hook domains (host.ts:405 `permission.hook`,
// host.ts:157-188 `aisdk.hook` — promise/permission.ts's `evaluate` and
// promise/aisdk.ts's `sdk`/`language`).
const PERMISSION_HOOKS = new Set(["evaluate"]);
const AISDK_HOOKS = new Set(["sdk", "language"]);
// G-24: the vcs/websearch/worktree/integration domains (host.ts:468-522,
// 631) — the guest serves provider executes, worktree ops, and the
// integration OAuth closures through these dispatch points.
const WEBSEARCH_HOOKS = new Set(["execute"]);
// G-24: the guest-side closure tables — provider executes, worktree
// strategy impls, and the pending OAuth callback survive only here
// (functions never cross the JSON boundary; the host reaches them
// through the domain hook dispatches).
const websearchExecutors = new Map();
const worktreeImpls = new Map();
// G-379② — vcs provider ownership: `domainStores.vcs` keys are the
// RAW provider ids (no plugin prefix), so dispose filters by this
// `${pluginId}:${id}` index instead of a (never-matching, and
// formerly inverted — it kept `pluginId:`-prefixed ids and dropped
// everyone else's) prefix test on the store keys.
const vcsOwners = new Map();
// G-350 — the vcs init closure table (functions never cross the JSON
// boundary; the host's vcs.init dispatch reaches them through the
// "vcs" domain "init" hook). Keyed `${pluginId}:${id}` like the
// websearch executors.
const vcsInits = new Map();
// G-379③ — the vcs default editor's selection (ref vcs.ts:85-88:
// `get: () => editor.selection, set: (selection) => editor.selection
// = selection` — the State editor's in-memory selection; initially
// undefined, shared across plugins like the ref's single vcs State).
let vcsSelection;
let pendingIntegrationCallback = null;
const WORKTREE_HOOKS = new Set(["create", "remove", "list"]);
const INTEGRATION_HOOKS = new Set(["authorize", "callback", "refresh"]);
// G-350 — the vcs init closure (ref vcs.ts:178-191 initialize →
// provider.init): the host's vcs.init endpoint dispatches into the
// guest through this hook point, mirroring the websearch execute seam.
const VCS_HOOKS = new Set(["init"]);

async function registerHook(pluginId, domain, name, handlers) {
  const known =
    domain === "session"
      ? SESSION_HOOKS
      : domain === "tool"
        ? TOOL_HOOKS
        : domain === "shell"
          ? SHELL_HOOKS
          : domain === "permission"
            ? PERMISSION_HOOKS
            : domain === "aisdk"
              ? AISDK_HOOKS
              : domain === "websearch"
                ? WEBSEARCH_HOOKS
                : domain === "worktree"
                  ? WORKTREE_HOOKS
                  : domain === "integration"
                    ? INTEGRATION_HOOKS
                    : domain === "vcs"
                      ? VCS_HOOKS
                      : null;
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

/** G-239/G-331: the `ctx.ui.panel` face (context.ts:501-508). Shared by
 *  the v2 makeCtx and the legacy TUI stub — the open ask rides the
 *  deferred-reply seam (`ui.panel.open` resolves the route-check
 *  boolean), the close is the owner-scoped fire-and-forget ask, and
 *  `current()` is plugin-filtered over the host-pushed `tui.panel`
 *  view. */
function makePanelFace(pluginId) {
  return {
    /** Opens the session.panel slot in the current session; resolves
     *  false when the TUI is not on a session route. */
    open(name, options) {
      const presentation =
        options && typeof options === "object" && options.presentation === "fullscreen"
          ? "fullscreen"
          : "panel";
      return hostRequest("ui.panel.open", {
        pluginId,
        name: String(name ?? ""),
        presentation,
      }).then(
        (result) => result === true,
        (error) => {
          notify("log", { pluginId, level: "error", message: `panel.open failed: ${errText(error)}` });
          return false;
        },
      );
    },
    /** Closes THIS plugin's active panel (other plugins' panels are
     *  unaffected). */
    async close() {
      await hostRequest("ui.panel.close", { pluginId }).catch(() => {});
    },
    /** This plugin's active panel, if any (the `tui.panel` view). */
    current() {
      return tuiPanel && tuiPanel.pluginId === pluginId
        ? { name: tuiPanel.name, sessionID: tuiPanel.sessionID }
        : undefined;
    },
  };
}

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

  const ctx = {
    plugin: {
      id: pluginId,
      // G-148 (ref promise/plugin.ts:49 `Pick<PluginApi, "list">`):
      // the loaded plugin table (id/name/version/active projection
      // of the ref PluginInfo shape).
      async list() {
        const result = await hostRequest("plugin.list", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
    },
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
      create: sessionMethod("create"),
      get: sessionMethod("get"),
      interrupt: sessionMethod("interrupt"),
      context: sessionMethod("context"),
      messages: sessionMethod("messages"),
      list: sessionMethod("list"),
      prompt: sessionMethod("prompt"),
      generate: sessionMethod("generate"),
      command: sessionMethod("command"),
      compact: sessionMethod("compact"),
      synthetic: sessionMethod("synthetic"),
      update: sessionMethod("update"),
      remove: sessionMethod("remove"),
      move: sessionMethod("move"),
      wait: sessionMethod("wait"),
      switchAgent: sessionMethod("switchAgent"),
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

    // -- permission (PLUG-D11: the v2 PermissionDomain —
    //    list/get/reply over the engine's permission machinery) --------
    permission: {
      // PLUG-G2: `ctx.permission.hook("evaluate", cb)` (host.ts:405).
      hook: (name, handler) => registerHook(pluginId, "permission", name, handler),
      async list(args) {
        const result = await hostRequest("permission.list", { pluginId, args: args ?? {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async get(args) {
        const result = await hostRequest("permission.get", { pluginId, args: args ?? {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async reply(args) {
        return hostRequest("permission.reply", { pluginId, args: args ?? {} });
      },
    },

    // -- shell (the v2 ShellHooks domain: `create.before`) --------------------
    shell: {
      hook: (name, handler) => registerHook(pluginId, "shell", name, handler),
    },

    // -- aisdk (PLUG-G2: the v2 AISDKHooks domain — host.ts:157-188's
    //    `aisdk.hook("sdk"|"language", cb)`) ------------------------------
    aisdk: {
      hook: (name, handler) => registerHook(pluginId, "aisdk", name, handler),
    },

    // -- vcs (G-24: host.ts:468-470's read surface — get/base over the
    //    engine's vcs adapters; G-24 residual: transform registers
    //    guest vcs providers, reload invalidates host vcs state) -----
    vcs: {
      async get() {
        const result = await hostRequest("vcs.get", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async base() {
        const result = await hostRequest("vcs.base", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async reload() {
        await hostRequest("vcs.reload", { pluginId, args: {} });
      },
      // G-379.8 — ref promise adapter.ts:510-514's read faces over the
      // engine's vcs adapters (branch list, status, diff).
      async branch(input) {
        const args = typeof input === "string" ? { search: input } : input ?? {};
        const result = await hostRequest("vcs.branch.list", {
          pluginId,
          args: {
            search: typeof args.search === "string" ? args.search : null,
            limit: typeof args.limit === "number" ? args.limit : null,
          },
        });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async status() {
        const result = await hostRequest("vcs.status", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async diff(input) {
        const args = input ?? {};
        const result = await hostRequest("vcs.diff", {
          pluginId,
          args: {
            mode: typeof args.mode === "string" ? args.mode : "working",
            base: typeof args.base === "string" ? args.base : null,
            context: typeof args.context === "number" ? args.context : null,
          },
        });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async transform(cb) {
        await cb({
          add: (definition) => {
            const id = String(definition?.id ?? "");
            if (!id) return;
            vcsOwners.set(`${pluginId}:${id}`, id);
            vcsInits.set(`${pluginId}:${id}`, definition?.init);
            domainStores.vcs.set(id, {
              id,
              name: String(definition?.name ?? definition?.id ?? id),
              // G-350 — ref vcs.ts:182: a provider without `init` is the
              // 501 "unsupported" InitializeError kind; the host reads
              // this flag before dispatching.
              init: typeof definition?.init === "function",
            });
          },
          default: {
            // G-379③ — ref vcs.ts:85-88: the State editor's
            // selection get/set (in-memory, initially undefined).
            get: () => vcsSelection,
            set: (selection) => {
              vcsSelection = selection;
            },
          },
        });
        // G-350 — the init dispatch seam: one handler per plugin, the
        // event's `provider` selects the registered provider; a throw
        // fails the dispatch (the host maps it to the 503 "failed"
        // InitializeError kind).
        await registerHook(pluginId, "vcs", "init", async (event) => {
          if (!event || typeof event !== "object") return;
          if (String(event.provider ?? "") === "") return;
          const run = vcsInits.get(`${pluginId}:${String(event.provider)}`);
          if (typeof run !== "function") return;
          await run(event.input ?? {});
        });
        await materializeDomain("vcs", pluginId);
        return {
          async dispose() {
            // G-379② — remove ONLY this plugin's registrations (the
            // ownership index), then re-materialize; other plugins'
            // rows survive.
            for (const [key, id] of Array.from(vcsOwners.entries())) {
              if (key.startsWith(`${pluginId}:`)) {
                vcsOwners.delete(key);
                vcsInits.delete(key);
                domainStores.vcs.delete(id);
              }
            }
            await materializeDomain("vcs", pluginId);
          },
        };
      },
    },

    // -- provider (G-64: adapter.ts:346-370 — list/get over the host's
    //    provider registry; transform's editor table mirrors the ref
    //    ProviderDomain; reload re-derives the registry) ---------------
    provider: {
      async list() {
        const result = await hostRequest("provider.list", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async get(input) {
        const id = typeof input === "string" ? input : input?.id;
        const result = await hostRequest("provider.get", { pluginId, args: { id: String(id ?? "") } });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async reload() {
        await hostRequest("provider.reload", { pluginId, args: {} });
      },
      async transform(cb) {
        const entry = { cb };
        domainEditors.provider.push(entry);
        cb(editors.provider);
        await materializeDomain("provider", pluginId);
        return {
          async dispose() {
            const at = domainEditors.provider.indexOf(entry);
            if (at >= 0) domainEditors.provider.splice(at, 1);
            await rederiveDomain("provider", pluginId);
          },
        };
      },
    },

    // -- model (G-147: adapter.ts:342-346 — list/default over the
    //    host's model catalog; transform's editor table mirrors the
    //    ref ModelEditor; reload re-derives the catalog + editors) ---
    model: {
      async list(input) {
        const args = typeof input === "string" ? { providerID: input } : input ?? {};
        const result = await hostRequest("model.list", {
          pluginId,
          args: { providerID: typeof args.providerID === "string" ? args.providerID : null },
        });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async default() {
        const result = await hostRequest("model.default", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async reload() {
        await hostRequest("model.reload", { pluginId, args: {} });
        domainStores.modelBase = undefined;
        await rederiveDomain("model", pluginId);
      },
      async transform(cb) {
        await ensureModelBase(pluginId);
        const entry = { cb };
        domainEditors.model.push(entry);
        cb(editors.model);
        await materializeDomain("model", pluginId);
        return {
          async dispose() {
            const at = domainEditors.model.indexOf(entry);
            if (at >= 0) domainEditors.model.splice(at, 1);
            await rederiveDomain("model", pluginId);
          },
        };
      },
    },

    // -- reference (G-64: adapter.ts:427-452's reference domain — list
    //    over the `@`-completion registry; transform adds/removes
    //    named sources; reload refreshes) -------------------------------
    reference: {
      async list() {
        const result = await hostRequest("reference.list", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async reload() {
        await hostRequest("reference.reload", { pluginId, args: {} });
      },
      async transform(cb) {
        const entry = { cb };
        domainEditors.reference.push(entry);
        cb(editors.reference);
        await materializeDomain("reference", pluginId);
        return {
          async dispose() {
            const at = domainEditors.reference.indexOf(entry);
            if (at >= 0) domainEditors.reference.splice(at, 1);
            await rederiveDomain("reference", pluginId);
          },
        };
      },
    },

    // -- experimental (G-64: host.ts:267-269 — persistentPty.read) ----
    experimental: {
      terminal: {
        async read(input) {
          const args = typeof input === "string" ? { sessionID: input } : input ?? {};
          const result = await hostRequest("experimental.terminal.read", {
            pluginId,
            args: {
              sessionID: String(args.sessionID ?? ""),
              lines: typeof args.lines === "number" ? args.lines : null,
            },
          });
          return result && typeof result === "object" && "value" in result ? result.value : result;
        },
      },
    },


    // -- websearch (G-24: host.ts:479-504 — providers/query/reload over the
    //    engine service + the transform that registers guest providers) ---
    websearch: {
      async providers() {
        const result = await hostRequest("websearch.providers", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async query(input) {
        const result = await hostRequest("websearch.query", { pluginId, args: input ?? {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async reload() {
        await materializeDomain("websearch", pluginId);
      },
      async transform(cb) {
        await cb({
          add: (definition) => {
            const id = String(definition?.id ?? "");
            if (!id) return;
            websearchExecutors.set(`${pluginId}:${id}`, definition?.execute);
            domainStores.websearch.push({
              id,
              name: String(definition?.name ?? definition?.id ?? id),
            });
          },
          default: {
            // G-24 residual: the persisted websearch editor default
            // (ref websearch.ts lands the selection in the auth/config
            // store; the port persists it in the engine KV).
            get: async () => {
              const result = await hostRequest("websearch.default.get", { pluginId, args: {} });
              return result && typeof result === "object" && "value" in result ? result.value : result;
            },
            set: async (selection) => {
              await hostRequest("websearch.default.set", {
                pluginId,
                args: { selection: selection === undefined ? null : selection },
              });
            },
          },
        });
        await registerHook(pluginId, "websearch", "execute", async (event) => {
          if (!event || typeof event !== "object") return;
          const run = websearchExecutors.get(`${pluginId}:${String(event.provider ?? "")}`);
          if (typeof run !== "function") return;
          const results = await run({ query: event.query });
          event.results = Array.isArray(results) ? results : [];
        });
        await materializeDomain("websearch", pluginId);
        return {
          async dispose() {
            for (const key of Array.from(websearchExecutors.keys())) {
              if (key.startsWith(`${pluginId}:`)) websearchExecutors.delete(key);
            }
            domainStores.websearch = domainStores.websearch.filter(
              (entry) => !websearchExecutors.has(`${pluginId}:${entry.id}`),
            );
            await materializeDomain("websearch", pluginId);
          },
        };
      },
    },

    // -- worktree (G-24/G-4: host.ts:505-522's transform — plugin-registered
    //    strategies; the ops dispatch back as worktree hooks) ---------------
    worktree: {
      async reload() {
        await materializeDomain("worktree", pluginId);
      },
      // G-379.8 — ref promise adapter.ts:558-566's op faces over the
      // engine's worktree inventory (the same service the HTTP
      // /api/worktree handlers ride).
      async list(input) {
        const projectID = typeof input === "string" ? input : input?.projectID;
        const result = await hostRequest("worktree.list", {
          pluginId,
          args: { projectID: String(projectID ?? "") },
        });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async create(input) {
        const result = await hostRequest("worktree.create", { pluginId, args: input ?? {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async remove(input) {
        await hostRequest("worktree.remove", { pluginId, args: input ?? {} });
      },
      async refresh(input) {
        const projectID = typeof input === "string" ? input : input?.projectID;
        await hostRequest("worktree.refresh", { pluginId, args: { projectID: String(projectID ?? "") } });
      },
      async transform(cb) {
        await cb({
          add: (definition) => {
            const id = String(definition?.id ?? "");
            if (!id) return;
            worktreeImpls.set(`${pluginId}:${id}`, definition);
            domainStores.worktree.push({ id });
          },
        });
        for (const op of ["create", "remove", "list"]) {
          await registerHook(pluginId, "worktree", op, async (event) => {
            if (!event || typeof event !== "object") return;
            const impl = worktreeImpls.get(`${pluginId}:${String(event.strategy ?? "")}`);
            if (!impl || typeof impl[op] !== "function") return;
            const result = await impl[op](event.input ?? {});
            if (op === "create") {
              event.directory = typeof result === "string" ? result : result?.directory;
            } else if (op === "remove") {
              event.removed = result !== false;
            } else {
              event.entries = Array.isArray(result) ? result : [];
            }
          });
        }
        await materializeDomain("worktree", pluginId);
        return {
          async dispose() {
            for (const key of Array.from(worktreeImpls.keys())) {
              if (key.startsWith(`${pluginId}:`)) worktreeImpls.delete(key);
            }
            domainStores.worktree = domainStores.worktree.filter(
              (entry) => !worktreeImpls.has(`${pluginId}:${entry.id}`),
            );
            await materializeDomain("worktree", pluginId);
          },
        };
      },
    },

    // -- integration (G-24: host.ts:357-367's method channel — the
    //    methodImplementation registration; authorize/refresh closures
    //    dispatch back as integration hooks. G-64: host.ts:274-357's
    //    list/get/connect.key/oauth.*/command.*/connection.* method
    //    surface over the engine's IntegrationHost) ---------------------
    integration: {
      async list() {
        const result = await hostRequest("integration.list", { pluginId, args: {} });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      async get(input) {
        const result = await hostRequest("integration.get", {
          pluginId,
          args: { integrationID: String(input?.integrationID ?? input ?? "") },
        });
        return result && typeof result === "object" && "value" in result ? result.value : result;
      },
      connect: {
        async key(input) {
          const args = input ?? {};
          return hostRequest("integration.connect.key", { pluginId, args });
        },
      },
      oauth: {
        async connect(input) {
          const args = input ?? {};
          return hostRequest("integration.oauth.connect", { pluginId, args });
        },
        async status(input) {
          const args = input ?? {};
          return hostRequest("integration.oauth.status", { pluginId, args });
        },
        async complete(input) {
          const args = input ?? {};
          return hostRequest("integration.oauth.complete", { pluginId, args });
        },
        async cancel(input) {
          const args = input ?? {};
          return hostRequest("integration.oauth.cancel", { pluginId, args });
        },
      },
      command: {
        async connect(input) {
          const args = input ?? {};
          return hostRequest("integration.command.connect", { pluginId, args });
        },
        async status(input) {
          const args = input ?? {};
          return hostRequest("integration.command.status", { pluginId, args });
        },
        async cancel(input) {
          const args = input ?? {};
          return hostRequest("integration.command.cancel", { pluginId, args });
        },
      },
      connection: {
        async active(id) {
          const result = await hostRequest("integration.connection.active", {
            pluginId,
            args: { id: String(id ?? "") },
          });
          return result && typeof result === "object" && "value" in result ? result.value : result;
        },
        async resolve(connection) {
          const result = await hostRequest("integration.connection.resolve", {
            pluginId,
            args: connection ?? {},
          });
          return result && typeof result === "object" && "value" in result ? result.value : result;
        },
        async status(input) {
          const args = input ?? {};
          return hostRequest("integration.connection.status", { pluginId, args });
        },
      },
      method: {
        async update(input) {
          return integrationMethodUpdate(pluginId, input);
        },
        // G-384 — the editor's read face: the method definitions of
        // one integration (ref editor.method.list).
        async list(id) {
          const result = await hostRequest("integration.method.list", {
            pluginId,
            args: { integrationID: String(id ?? "") },
          });
          return result && typeof result === "object" && "value" in result ? result.value : result;
        },
      },
      // G-385 — ref adapter.ts:426 `reload: () => run(host.integration
      // .reload())`: the host invalidates its registry (re-installing
      // the catalog methods) and publishes `integration.updated`.
      async reload() {
        await hostRequest("integration.reload", { pluginId, args: {} });
      },
      // G-384 — ref adapter.ts:386-425 `ctx.integration.transform(cb)`:
      // the editor CRUD face. BOUNDED DEVIATION (G-5 precedent, like
      // vcs.transform): the ref replays transforms lazily on every
      // read; the rust port materializes EAGERLY at registration time
      // (mutations write the guest-side pending table, then the whole
      // derived table crosses via `integration.transform`). Results
      // are session-scoped in-memory rows — never persisted.
      async transform(cb) {
        await cb({
          list: async () => {
            const result = await hostRequest("integration.list", { pluginId, args: {} });
            return result && typeof result === "object" && "value" in result ? result.value : result;
          },
          get: async (id) => {
            const result = await hostRequest("integration.get", {
              pluginId,
              args: { integrationID: String(id ?? "") },
            });
            return result && typeof result === "object" && "value" in result ? result.value : result;
          },
          // ref editor.update (integration.ts:299-309): upsert — a
          // missing integration is created with the name defaulting to
          // its id; an existing one's untouched fields survive (the
          // base is fetched through the live port first).
          update: async (id, update) => {
            const integrationId = String(id ?? "");
            if (!integrationId) return;
            let row = domainStores.integrations.get(integrationId);
            if (!row) {
              let name = integrationId;
              let metadata;
              try {
                const current = await hostRequest("integration.get", {
                  pluginId,
                  args: { integrationID: integrationId },
                });
                const info =
                  current && typeof current === "object" && "value" in current
                    ? current.value
                    : current;
                if (info && typeof info === "object") {
                  if (typeof info.name === "string" && info.name) name = info.name;
                  if (info.metadata !== undefined && info.metadata !== null) {
                    metadata = info.metadata;
                  }
                }
              } catch {
                // unknown id → create-if-missing with name = id
              }
              row = { id: integrationId, name, metadata };
            }
            if (typeof update === "function") {
              update(row);
            } else if (update && typeof update === "object") {
              if (typeof update.name === "string" && update.name) row.name = update.name;
              if ("metadata" in update) row.metadata = update.metadata;
            }
            row.id = integrationId;
            if (typeof row.name !== "string" || !row.name) row.name = integrationId;
            integrationOwners.set(`${pluginId}:${integrationId}`, integrationId);
            integrationRemovals.delete(`${pluginId}:${integrationId}`);
            domainStores.integrations.set(integrationId, row);
          },
          remove: (id) => {
            const integrationId = String(id ?? "");
            if (!integrationId) return;
            integrationOwners.delete(`${pluginId}:${integrationId}`);
            domainStores.integrations.delete(integrationId);
            integrationRemovals.set(`${pluginId}:${integrationId}`, integrationId);
          },
          method: {
            // MUST route through the same path as
            // ctx.integration.method.update (the G-24 hook dispatch
            // owns the authorize/refresh closures) — plus a
            // materialization so a remove→update sequence in one
            // transform settles to the upsert.
            update: async (input) => {
              const result = await integrationMethodUpdate(pluginId, input);
              const methodKey =
                input?.method?.id !== undefined && input?.method?.id !== null
                  ? String(input.method.id)
                  : String(input?.method?.type ?? "");
              if (methodKey) {
                integrationMethodRemovals.delete(
                  `${pluginId}:${String(input?.integrationID ?? "")}:${methodKey}`,
                );
              }
              return result;
            },
            list: async (id) => {
              const result = await hostRequest("integration.method.list", {
                pluginId,
                args: { integrationID: String(id ?? "") },
              });
              return result && typeof result === "object" && "value" in result
                ? result.value
                : result;
            },
            remove: (id, method) => {
              const integrationId = String(id ?? "");
              const methodKey =
                method && (method.id !== undefined || method.type !== undefined)
                  ? String(method.id ?? method.type)
                  : "";
              if (!integrationId || !methodKey) return;
              integrationMethodRemovals.set(`${pluginId}:${integrationId}:${methodKey}`, {
                integrationID: integrationId,
                method,
              });
            },
          },
        });
        await materializeDomain("integration", pluginId);
        return {
          async dispose() {
            // Only THIS plugin's rows retract (the ownership index),
            // then re-materialize; other plugins' rows survive.
            for (const key of Array.from(integrationOwners.keys())) {
              if (key.startsWith(`${pluginId}:`)) {
                integrationOwners.delete(key);
                domainStores.integrations.delete(key.slice(pluginId.length + 1));
              }
            }
            for (const key of Array.from(integrationRemovals.keys())) {
              if (key.startsWith(`${pluginId}:`)) integrationRemovals.delete(key);
            }
            for (const key of Array.from(integrationMethodRemovals.keys())) {
              if (key.startsWith(`${pluginId}:`)) integrationMethodRemovals.delete(key);
            }
            await materializeDomain("integration", pluginId);
          },
        };
      },
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
        rpcDefinitions.set(id, portable);
        await hostRequest("rpc.register", { pluginId, definition: portable });
        return {
          async dispose() {
            table.clear();
            rpcDefinitions.delete(id);
          },
          // G-295 — the v2 register return shape (rpc.ts:87-105):
          // `events.emit(name, data)` publishes the ephemeral
          // `rpc.{id}.{name}` bus event through the host. The name is
          // validated against the DEFINITION's declared events here
          // (the host re-validates — both sides gate, like every
          // registration surface); an undeclared name rejects the
          // guest promise with the ref's error.
          events: {
            async emit(name, data) {
              const event = String(name ?? "");
              if (!portable.events || !Object.prototype.hasOwnProperty.call(portable.events, event)) {
                throw new Error(`Unknown RPC event: ${id}.${event}`);
              }
              await hostRequest("rpc.event", { pluginId, id, name: event, data: data ?? null });
            },
          },
        };
      },
      // G-379.7 — the guest-side CLIENT face (ref promise adapter.ts:77-113
      // rpcFromEffect's `client(definition)` over core rpc.ts:108's
      // in-process `Rpc.call`): a call resolves the registration in the
      // host registry (any plugin's, including this one) and routes into
      // the owning handler. Unknown ids/methods throw typed errors
      // (`error.type` = "rpc.unavailable" / "rpc.method_not_found" /
      // declared error types), mirroring the ref client's failures.
      async call(rpcId, method, input) {
        const result = await hostRequest("rpc.invoke", {
          pluginId,
          rpcId: String(rpcId ?? ""),
          method: String(method ?? ""),
          input: input === undefined ? null : input,
        });
        if (result && typeof result === "object" && result.ok === false) {
          const failure = result.failure ?? {};
          const error = new Error(String(failure.message ?? "RPC call failed"));
          error.type = String(failure.type ?? "rpc.internal");
          if (failure.data !== undefined) error.data = failure.data;
          throw error;
        }
        return result && typeof result === "object" && "output" in result ? result.output : undefined;
      },
      // The ref's `ctx.rpc(definition)` client constructor: one method
      // function per declared method, all routing through `call` (the
      // bounded deviation: the ref makes ctx.rpc itself callable —
      // here the same face lives at `ctx.rpc.client(definition)`).
      client(definition) {
        const id = String(definition?.id ?? "");
        const methods = definition?.methods && typeof definition.methods === "object" ? definition.methods : {};
        const client = {};
        for (const name of Object.keys(methods)) {
          client[name] = (input) => this.call(id, name, input);
        }
        return client;
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

    // -- markdown code-block renderers (api.tsx:168-178; G-332) —
    //    the render function stays in the guest; the TUI consults the
    //    registration table and materializes content through the
    //    bounded `markdown.render` call.
    markdown: {
      registerCodeBlockRenderer(language, render) {
        return registerMarkdownRenderer(pluginId, language, render);
      },
    },

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
      // TUI-25 (v2 api.tsx:344-366): the promise resolves when the
      // host answers — the TUI's dialog confirm / any close.
      alert(options) {
        const opts = options ?? {};
        return hostRequest("ui.alert", {
          pluginId,
          title: String(opts.title ?? ""),
          message: String(opts.message ?? ""),
        }).then(dialogResult)
      },
      confirm(options) {
        const opts = options ?? {};
        const label = opts.label ?? {};
        return hostRequest("ui.confirm", {
          pluginId,
          title: String(opts.title ?? ""),
          message: String(opts.message ?? ""),
          label: {
            confirm: typeof label.confirm === "string" ? label.confirm : null,
            cancel: typeof label.cancel === "string" ? label.cancel : null,
          },
        }).then(dialogResult)
      },
      // G-237 (v2 api.tsx:367-386): the generic text-input prompt —
      // resolves the typed string on submit, null on dismiss; the
      // optional fields cross as null (the TUI applies the ref's
      // placeholder default "Enter text").
      prompt(options) {
        const opts = options ?? {};
        return hostRequest("ui.prompt", {
          pluginId,
          title: String(opts.title ?? ""),
          description: typeof opts.description === "string" ? opts.description : null,
          placeholder: typeof opts.placeholder === "string" ? opts.placeholder : null,
          value: typeof opts.value === "string" ? opts.value : null,
        }).then(dialogResult)
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
      // -- G-239/G-331: the session-panel face (context.ts:501-508) --
      // `open` resolves the route-check boolean through the DEFERRED
      // reply (the alert/confirm seam); `close` is owner-scoped
      // (only this plugin's panel closes); `current` is
      // plugin-filtered over the host-pushed `tui.panel` view.
      panel: makePanelFace(pluginId),
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
  liveContexts.add(ctx);
  return ctx;
}

// ---------------------------------------------------------------------------
// Supervisor-facing methods
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// G-293 ①: staged-load snapshot/rollback. The reload path loads the NEW
// generation BEFORE retiring the old one: the guest snapshots its
// registration tables, runs the load, and on failure restores them
// wholesale (the host restores its own registration store the same
// way) — the previous generation keeps its place and stays functional
// (ref supervisor.ts:70-86, plugin.ts:139-169).
// ---------------------------------------------------------------------------

function snapshotPluginState() {
  const pluginCopies = new Map();
  for (const [id, record] of plugins) {
    pluginCopies.set(id, {
      ...record,
      subscriptions: record.subscriptions.slice(),
      commands: new Map(record.commands),
    });
  }
  const handlerCopies = {};
  for (const [domain, map] of Object.entries(hookHandlers)) {
    handlerCopies[domain] = new Map();
    for (const [name, list] of map) handlerCopies[domain].set(name, list.slice());
  }
  return {
    plugins: pluginCopies,
    domainEditors: Object.fromEntries(
      Object.entries(domainEditors).map(([kind, list]) => [kind, list.slice()]),
    ),
    domainStores: {
      agents: new Map(domainStores.agents),
      agentDefault: domainStores.agentDefault,
      mcps: new Map(domainStores.mcps),
      skills: new Map(domainStores.skills),
      tools: new Map(domainStores.tools),
      providers: new Map(domainStores.providers),
      modelBase: domainStores.modelBase,
      modelOverrides: new Map(domainStores.modelOverrides),
      modelRemoved: new Set(domainStores.modelRemoved),
      modelDefault: domainStores.modelDefault,
      references: new Map(domainStores.references),
      vcs: new Map(domainStores.vcs),
    },
    hookHandlers: handlerCopies,
    rpcHandlers: new Map([...rpcHandlers].map(([id, table]) => [id, new Map(table)])),
    rpcDefinitions: new Map(rpcDefinitions),
    dialogProviders: new Map([...dialogProviders].map(([id, table]) => [id, new Map(table)])),
    sidebarSlots: new Map(sidebarSlots),
    tuiDisposers: new Map(tuiDisposers),
    slotClaims: new Map(slotClaims),
    slotClaimCounters: new Map(slotClaimCounters),
    pluginRoutes: new Map(pluginRoutes),
    keymapRuns: new Map(keymapRuns),
    markdownRenderers: new Map(markdownRenderers),
    websearchExecutors: new Map(websearchExecutors),
    worktreeImpls: new Map(worktreeImpls),
    vcsInits: new Map(vcsInits),
    pendingIntegrationCallback,
  };
}

function restorePluginState(snap) {
  const reinstate = (map, entries) => {
    map.clear();
    for (const [key, value] of entries) map.set(key, value);
  };
  reinstate(plugins, snap.plugins);
  for (const [kind, list] of Object.entries(snap.domainEditors)) domainEditors[kind] = list;
  Object.assign(domainStores, snap.domainStores);
  for (const [domain, map] of Object.entries(snap.hookHandlers)) {
    reinstate(hookHandlers[domain], map);
  }
  reinstate(rpcHandlers, snap.rpcHandlers);
  reinstate(rpcDefinitions, snap.rpcDefinitions);
  reinstate(dialogProviders, snap.dialogProviders);
  reinstate(sidebarSlots, snap.sidebarSlots);
  reinstate(tuiDisposers, snap.tuiDisposers);
  reinstate(slotClaims, snap.slotClaims);
  reinstate(slotClaimCounters, snap.slotClaimCounters);
  reinstate(pluginRoutes, snap.pluginRoutes);
  reinstate(keymapRuns, snap.keymapRuns);
  reinstate(markdownRenderers, snap.markdownRenderers);
  reinstate(websearchExecutors, snap.websearchExecutors);
  reinstate(worktreeImpls, snap.worktreeImpls);
  reinstate(vcsInits, snap.vcsInits);
  pendingIntegrationCallback = snap.pendingIntegrationCallback;
}

/** Drop one plugin id's entries from an id-keyed registry, preserving
 *  a value the NEW generation just registered under the same id (the
 *  staged-success retirement — mirrors `handleUnload`'s cleanup without
 *  clobbering the replacement). */
function retireKeyed(map, id) {
  const current = map.get(id);
  map.delete(id);
  if (current !== undefined) map.set(id, current);
}

async function handleLoad({ entry, generation, seed, options, staged }) {
  if (!staged) return loadGeneration({ entry, generation, seed, options });
  // The staged reload: snapshot → load → (failure: restore + rethrow;
  // success: retire the previous generation for this entry quietly).
  // The host commits its side only after this load replies ok.
  const abs = require("node:path").resolve(entry);
  const snap = snapshotPluginState();
  let result;
  let loadedId = null;
  try {
    result = await loadGeneration({ entry, generation, seed, options });
    loadedId = result?.pluginId ?? null;
  } catch (error) {
    restorePluginState(snap);
    throw error;
  }
  for (const [id, record] of snap.plugins) {
    if (record.entry !== abs || id === loadedId) continue;
    try {
      if (record.dispose) await record.dispose();
    } catch (error) {
      notify("log", { level: "warn", message: `${id}: staged retire dispose threw ${errText(error)}` });
    }
    const tuiDispose = snap.tuiDisposers.get(id);
    if (tuiDispose) {
      try {
        await tuiDispose();
      } catch (error) {
        notify("log", { level: "warn", message: `${id}: staged retire tui dispose threw ${errText(error)}` });
      }
      tuiDisposers.delete(id);
    }
    retireKeyed(dialogProviders, id);
    retireKeyed(sidebarSlots, id);
    retireKeyed(slotClaims, id);
    retireKeyed(slotClaimCounters, id);
    retireKeyed(pluginRoutes, id);
    retireKeyed(keymapRuns, id);
    retireKeyed(markdownRenderers, id);
    plugins.delete(id);
  }
  return result;
}

async function loadGeneration({ entry, generation, seed, options }) {
  const abs = require("node:path").resolve(entry);
  // G-148②: detect `@opencode/plugin/effect` imports up front and fail
  // with a clear error instead of an obscure module-not-found from the
  // dynamic import. The Effect-style plugin API (ref effect/ directory,
  // 21 modules) is an ecosystem breaking change of this port — the
  // same class as G-28's bounded TSX shim — and is NOT ported.
  const effectImport = await detectEffectImports(abs);
  if (effectImport) {
    throw new Error(
      `${entry}: imports "${effectImport}" — the Effect-style plugin API (@opencode/plugin/effect) ` +
        `is not available in the Rust plugin host (G-148, an ecosystem breaking change alongside G-28). ` +
        `Rewrite the plugin against the promise-style Context API ({ id, setup }).`,
    );
  }
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

// G-148②: walk the entry's LOCAL import graph (node_modules specs are
// checked as strings only) looking for `@opencode/plugin/effect`
// import specifiers. Bounded scan — cycles and unreadable files stop
// the walk, mirroring `scanDigests`' traversal.
async function detectEffectImports(entryFile) {
  const seen = new Set();
  async function walk(file) {
    if (seen.has(file)) return null;
    seen.add(file);
    let source;
    try {
      source = await nativeBun.file(file).text();
    } catch {
      return null;
    }
    let imports = [];
    try {
      imports = new nativeBun.Transpiler({ loader: "tsx" }).scanImports(source);
    } catch {
      return null;
    }
    for (const spec of imports) {
      const path = typeof spec === "string" ? spec : spec?.path;
      if (typeof path === "string" && path.startsWith("@opencode/plugin/effect")) return path;
      const resolved = resolveLocal(file, path ?? spec);
      if (resolved) {
        const hit = await walk(resolved);
        if (hit) return hit;
      }
    }
    return null;
  }
  return walk(entryFile);
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
  markdownRenderers.delete(pluginId);
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
  // G-379⑨ — ref CommandInvocation (promise/command.ts:7-11 +
  // command.ts:14-18): `{ sessionID, prompt, delivery }` where prompt
  // is a PromptInput.Prompt (`text` a REQUIRED string) and delivery
  // is a SessionInbox.Delivery literal. The port approximation keeps
  // `args` (the raw wire args) and the ctx second parameter as a
  // harmless superset for port-local consumers; ref-shaped handlers
  // see the ref shape.
  const invocation = {
    sessionID: typeof sessionID === "string" ? sessionID : "",
    prompt: { text: typeof args === "string" ? args : String(args?.text ?? "") },
    delivery: "steer",
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
// the Rust port's boundary is this sandboxed subprocess). The v2
// ExecutionLimits (timeoutMs between event-loop turns, the tool-call
// admission counter, the output bound, the depth guard) are enforced
// here; a wedged guest (sync loop — not interruptible in-process) is
// killed by the supervisor at the limits-aligned deadline. G-247:
// module loading and timers are OUT of the sandbox — the program
// source is statically scanned for `import(`/`require` before it
// compiles (scanProgramSource below; the oc-core leaf applies the
// identical rule at the tool entry), and setTimeout/setInterval/
// setImmediate are swapped to throwing placeholders for the run's
// duration. The ref's confined interpreter has neither (no
// ImportExpression node, no require/timer globals), so the
// verbatim-ref tool description ("Imports, direct filesystem access,
// and timers are unavailable") is enforced, not just declared.
// ---------------------------------------------------------------------------

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

// v2 execution limits (codemode.ts:22-41 + 126-137): absent = unlimited
// (no defaults — the ref's core callers pass none); present values must
// be safe integers ≥ min, else the ref-identical RangeError text.
function resolveExecutionLimits(raw) {
  const validate = (name, value, minimum) => {
    if (value !== undefined && (!Number.isSafeInteger(value) || value < minimum)) {
      throw new RangeError(`${name} must be a safe integer greater than or equal to ${minimum}.`);
    }
    return value;
  };
  const limits = raw !== null && typeof raw === "object" ? raw : {};
  return {
    timeoutMs: validate("timeoutMs", limits.timeoutMs, 1),
    maxToolCalls: validate("maxToolCalls", limits.maxToolCalls, 0),
    maxOutputBytes: validate("maxOutputBytes", limits.maxOutputBytes, 0),
  };
}

// MAX_VALUE_DEPTH (interpreter/limits.ts:13) — one of the four
// structural caps, all ported now: the depth guard walks every value
// crossing guest↔host (data.ts:53 counts the root at depth 0 and
// throws entering depth 33: 32 container levels cross, the 33rd does
// not), while MAX_STRING_LENGTH, MAX_ARRAY_LENGTH and
// MAX_PENDING_PROMISES (see the caps block at hostRequest) bound the bridge's
// payload sizes and pending-request count — a real JS engine needs no
// interpreter-allocator bookkeeping, so the same limits are enforced
// where the guest owns the resource.
const MAX_VALUE_DEPTH = 32;

function valueDepthExceeded(value, depth = 0) {
  if (depth > MAX_VALUE_DEPTH) return true;
  if (value === null || typeof value !== "object") return false;
  if (Array.isArray(value)) return value.some((item) => valueDepthExceeded(item, depth + 1));
  return Object.values(value).some((item) => valueDepthExceeded(item, depth + 1));
}

function utf8ByteLength(text) {
  return new TextEncoder().encode(text).byteLength;
}

// utf8Truncate (interpreter/execute.ts:122-127): cut at a byte bound,
// dropping a partial UTF-8 sequence.
function utf8Truncate(value, maxBytes) {
  const bytes = new TextEncoder().encode(value);
  if (bytes.byteLength <= maxBytes) return value;
  const text = new TextDecoder("utf-8").decode(bytes.slice(0, Math.max(0, maxBytes)));
  return text.endsWith("\uFFFD") ? text.slice(0, -1) : text;
}

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
function buildToolsTree(entries, calls, token, maxToolCalls) {
  const root = {};
  const makeCallable = (entry) => {
    const fn = async (input) => {
      // ToolRuntime.make's admission counter (tool-runtime.ts:386-394):
      // the (n+1)-th call rejects before it is recorded or crosses.
      if (maxToolCalls !== undefined && calls.length >= maxToolCalls) {
        throw new Error(`Execution exceeded its tool-call limit of ${maxToolCalls}.`);
      }
      // The boundary walk's depth guard (data.ts:53) on tool inputs
      // crossing guest→host, plus the structural caps
      // (checkStringLength/checkArrayLength) on the same walk.
      if (valueDepthExceeded(input)) {
        throw new RangeError(`Value exceeds the maximum depth of ${MAX_VALUE_DEPTH}.`);
      }
      checkValueLimits(input);
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
        // Host→guest tool results get the same structural caps.
        checkValueLimits(value);
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
    return scoped
      .map((entry) => {
        const path = entry.path.toLowerCase();
        const description = String(entry.description ?? "").toLowerCase();
        // v2's searchText (toSearchEntry: path + description + input
        // property names/descriptions), rendered host-side; the local
        // approximation is only a fallback for pre-searchText hosts.
        const searchText = String(
          entry.searchText ??
            `${path} ${description} ${String(entry.signature ?? "").toLowerCase()}`,
        );
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
    const size = Number.isFinite(limit) && limit > 0 ? Math.floor(limit) : 10;
    const items = ordered.slice(start, start + size).map((entry) => ({
      path: toolExpression(entry.path),
      description: String(entry.description ?? ""),
      signature: String(entry.signature ?? ""),
    }));
    const remaining = Math.max(0, ordered.length - start - items.length);
    return { items, remaining, next: remaining > 0 ? { offset: start + items.length } : null };
  };
}

// G-180① — ref codemode console capture (stdlib/console.ts:9-37):
// during a Code Mode execution every console method appends one
// formatted line to the execution's `logs` (exposed in the result's
// optional `logs`, codemode.ts:98/108). Prefixes per ref
// formatConsoleMessage: "[warn] ", "[error] ", "[debug] "; values
// render like `console.log` shows them (strings verbatim, others via
// an inspect close to the ref's Obj.inspect).
const CONSOLE_METHODS = ["log", "info", "debug", "warn", "error", "dir", "table"];
const MAX_CONSOLE_DEPTH = 32;

function formatConsoleValue(value, seen, depth) {
  if (value === undefined) return "undefined";
  if (typeof value === "string") return value;
  if (value === null) return "null";
  if (typeof value === "function") {
    return `[Function: ${value.name || "anonymous"}]`;
  }
  if (typeof value === "symbol" || typeof value === "bigint") return String(value);
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  if (depth > MAX_CONSOLE_DEPTH) return "...";
  if (seen.has(value)) return "[Circular]";
  seen.add(value);
  try {
    if (Array.isArray(value)) {
      return `[${value.map((item) => formatConsoleValue(item, seen, depth + 1)).join(", ")}]`;
    }
    if (value instanceof Error) return value.stack ?? String(value);
    if (value instanceof Date) return value.toISOString();
    const entries = Object.entries(value).map(
      ([key, item]) => `${/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(key) ? key : JSON.stringify(key)}: ${formatConsoleValue(item, seen, depth + 1)}`,
    );
    const tag = value.constructor && value.constructor.name !== "Object" ? `${value.constructor.name} ` : "";
    return `${tag}{ ${entries.join(", ")} }`;
  } finally {
    seen.delete(value);
  }
}

// formatConsoleMessage (ref stdlib/console.ts:32-37): `dir`/`table`
// have their own arms; the rest prefix + join formatted args.
function formatConsoleMessage(name, args) {
  if (name === "dir") return args.length === 0 ? "undefined" : formatConsoleValue(args[0], new Set(), 0);
  if (name === "table") return formatConsoleTable(args[0], args[1]);
  const prefix = name === "warn" ? "[warn] " : name === "error" ? "[error] " : name === "debug" ? "[debug] " : "";
  return `${prefix}${args.map((arg) => formatConsoleValue(arg, new Set(), 0)).join(" ")}`;
}

// formatConsoleTable (ref stdlib/console.ts:63-98): tab-joined rows
// with an `(index)` header over array items / object entries.
function formatConsoleTable(data, columnsArgument) {
  if (data === undefined) return "undefined";
  const columns = Array.isArray(columnsArgument) ? columnsArgument.map(String) : undefined;
  let rows;
  if (Array.isArray(data)) {
    rows = data.map((item, index) => ({ index: String(index), values: consoleTableValues(item, columns) }));
  } else if (data !== null && typeof data === "object") {
    rows = Object.entries(data).map(([index, item]) => ({ index, values: consoleTableValues(item, columns) }));
  } else {
    rows = [{ index: "0", values: { Value: data } }];
  }
  const keys = columns ?? Array.from(new Set(rows.flatMap((row) => Object.keys(row.values))));
  const header = ["(index)", ...keys].join("\t");
  return [
    header,
    ...rows.map((row) => [row.index, ...keys.map((key) => formatConsoleTableCell(row.values[key]))].join("\t")),
  ].join("\n");
}

function consoleTableValues(value, columns) {
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    if (columns !== undefined) return Object.fromEntries(columns.map((column) => [column, value[column]]));
    return { ...value };
  }
  return { Value: value };
}

function formatConsoleTableCell(value) {
  if (value === undefined) return "";
  if (typeof value === "string") return value;
  return formatConsoleValue(value, new Set(), 0);
}

// G-180② / G-334a — ref web extension fetch (packages/core/src/
// codemode/web.ts): the sandbox's ONLY fetch surface is this wrapper —
// a 30s AbortSignal.timeout and a sanitized response (url/status/ok/
// headers get|has|entries, text/json/bytes). The raw Bun fetch (no
// timeout, raw Response) must not leak into Code Mode programs: the
// native fetch lives ONLY in the closure the wrapper is created with
// (plus a WeakMap for the nesting chain) — NO global property holds
// it (the old `globalThis.__rawFetch` escape face is gone).
const CODEMODE_FETCH_TIMEOUT_MS = 30_000;

// Wrapper → the native fetch it captured (overlapping evaluations
// nest: the next wrapper unwraps the previous one's raw reference
// through here). A WeakMap keyed by the live wrapper function only —
// unreachable from guest code.
const codemodeRawFetches = new WeakMap();

function makeCodemodeFetch(rawFetch) {
  const wrapper = async (input, init = {}) => {
    return rawFetch(input, {
      method: init.method,
      headers: init.headers,
      body: init.body,
      signal: AbortSignal.timeout(CODEMODE_FETCH_TIMEOUT_MS),
    }).then(async (response) => {
      const bytes = new Uint8Array(await response.arrayBuffer());
      const headers = Object.fromEntries(response.headers);
      const text = () => new TextDecoder().decode(bytes);
      return {
        url: response.url,
        status: response.status,
        statusText: response.statusText,
        ok: response.ok,
        redirected: response.redirected,
        headers: {
          get: (name) => headers[name.toLowerCase()] ?? null,
          has: (name) => name.toLowerCase() in headers,
          entries: () => Object.entries(headers),
        },
        text: async () => text(),
        json: async () => JSON.parse(text()),
        bytes: async () => bytes,
      };
    });
  };
  codemodeRawFetches.set(wrapper, rawFetch);
  return wrapper;
}

// G-180①/G-180② — overlapping evaluations nest: live captures stack;
// a finishing frame restores only when it is the last live one.
const codemodeCaptureStack = [];

// G-247 — the module-loading scan (guest side; the oc-core leaf runs
// the IDENTICAL rule at the tool entry — crates/oc-core/src/tool/
// codemode.rs `scan_program`, pinned by tests on both sides). The ref
// confined interpreter is structurally incapable of module loading
// (its evaluator has no ImportExpression arm; globals.ts defines no
// `require`), so the contract of the verbatim-ref description
// (tool.ts:64 "Imports, direct filesystem access, and timers are
// unavailable") is enforced here with a static token scan:
//
//   * line/block comments, string bodies ('…'/"…"), template bodies
//     (with ${…} interpolations scanned as CODE — a hiding import()
//     is still caught), and regex literals (operator-position
//     heuristic) are stripped first;
//   * a dynamic `import` + `(` rejects, EXCEPT a `.`-preceded
//     property call (obj.import(…) — reserved words are legal
//     property names and a property call never loads a module);
//   * a `require` identifier rejects in ANY position, `.`-prefixed
//     included (globalThis.require is a live module-loading face on
//     require-capable hosts). Pinned false-positive surface: dot
//     access to a property named `require`; the bracket form
//     tools.ns["require"](…) stays legal (the name is a string).
//
// Static `import … from` needs no rule: a Function body cannot carry
// one (SyntaxError at compile — same observable as the ref's missing
// node). The static scan's known limit — runtime-built code
// (`new Function(...)`, eval) smuggling the tokens — is CLOSED at the
// runtime face (G-334b): the guest evaluation window swaps eval/
// Function/AsyncFunction for throwing stubs, so dynamically built
// code cannot run at all (see codemodeDynamicCodeDisabled).
const CODEMODE_MODULE_LOADING_REJECTED =
  "Code Mode programs cannot use import()/require — the sandbox forbids module loading.";

function scanProgramSource(code) {
  const len = code.length;
  let i = 0;
  // Lexer contexts: inTemplate + the stack of contexts to return to
  // ("code" = a template closing returns to code; "tpl" = an
  // interpolation's matching `}` returns to the template).
  let inTemplate = false;
  const stack = [];
  let depth = 0; // brace depth of the current code region
  let prevCode = "\0"; // last significant char of the current code region
  let prevRegexKeyword = false; // last word was return/typeof/…
  const idStart = /[A-Za-z_$]/;
  const idPart = /[A-Za-z0-9_$]/;
  const REGEX_KEYWORDS = new Set([
    "return", "typeof", "instanceof", "in", "of", "new", "delete", "void",
    "case", "do", "else", "yield", "await", "throw",
  ]);
  const regexAllowedAfter = (c) => "([{,;=:?!&|+-*%^~<>".includes(c);
  while (i < len) {
    const c = code[i];
    if (inTemplate) {
      if (c === "\\") {
        i += 2;
      } else if (c === "`") {
        inTemplate = false;
        stack.pop();
        prevCode = "`";
        prevRegexKeyword = false;
        i += 1;
      } else if (c === "$" && code[i + 1] === "{") {
        stack.push("tpl");
        inTemplate = false;
        depth = 0;
        prevCode = "\0";
        prevRegexKeyword = false;
        i += 2;
      } else {
        i += 1;
      }
      continue;
    }
    if (/\s/.test(c)) {
      i += 1;
    } else if (c === "/" && code[i + 1] === "/") {
      while (i < len && code[i] !== "\n") i += 1;
    } else if (c === "/" && code[i + 1] === "*") {
      i += 2;
      while (i + 1 < len && !(code[i] === "*" && code[i + 1] === "/")) i += 1;
      i = Math.min(i + 2, len);
    } else if (c === '"' || c === "'") {
      i += 1;
      while (i < len && code[i] !== c) {
        if (code[i] === "\\") i += 1;
        i += 1;
      }
      i += 1;
      prevCode = '"';
      prevRegexKeyword = false;
    } else if (c === "`") {
      stack.push("code");
      inTemplate = true;
      i += 1;
    } else if (c === "/" && (prevCode === "\0" || prevRegexKeyword || regexAllowedAfter(prevCode))) {
      // Regex literal: skip to the closing `/`, honoring escapes and
      // character classes; a newline means it was never a regex (an
      // unterminated literal is a SyntaxError anyway).
      i += 1;
      let inClass = false;
      while (i < len) {
        const r = code[i];
        if (r === "\\") {
          i += 2;
          continue;
        }
        if (r === "\n") break;
        if (inClass) {
          if (r === "]") inClass = false;
        } else if (r === "[") inClass = true;
        else if (r === "/") break;
        i += 1;
      }
      i += 1;
      while (i < len && idPart.test(code[i])) i += 1; // flags
      prevCode = "_";
      prevRegexKeyword = false;
    } else if (idStart.test(c)) {
      let j = i + 1;
      while (j < len && idPart.test(code[j])) j += 1;
      const word = code.slice(i, j);
      if (word === "require") return "require";
      if (word === "import" && prevCode !== ".") {
        // Dynamic import iff the next significant token is `(` (a
        // `.`-preceded `import` is a property call — see above).
        let k = j;
        for (;;) {
          while (k < len && /\s/.test(code[k])) k += 1;
          if (code[k] === "/" && code[k + 1] === "*") {
            k += 2;
            while (k + 1 < len && !(code[k] === "*" && code[k + 1] === "/")) k += 1;
            k = Math.min(k + 2, len);
            continue;
          }
          break;
        }
        if (code[k] === "(") return "import";
      }
      prevRegexKeyword = REGEX_KEYWORDS.has(word);
      prevCode = word[word.length - 1];
      i = j;
    } else if (c === "{") {
      depth += 1;
      prevCode = c;
      prevRegexKeyword = false;
      i += 1;
    } else if (c === "}") {
      if (depth > 0) depth -= 1;
      else if (stack[stack.length - 1] === "tpl") {
        stack.pop();
        inTemplate = true;
      }
      prevCode = c;
      prevRegexKeyword = false;
      i += 1;
    } else {
      prevCode = c;
      prevRegexKeyword = false;
      i += 1;
    }
  }
  return null;
}

// G-247 — the timer placeholders swapped in for the program's
// duration: the ref interpreter's globals carry no timers at all, so
// "timers are unavailable" (tool.ts:64) is enforced by making the
// scheduling calls throw. The guest's OWN soft-timeout race arms on
// the module-scope native binding, so limits keep working.
function codemodeTimerDisabled(name) {
  return () => {
    throw new Error(`Code Mode programs cannot use ${name} — timers are unavailable in the sandbox.`);
  };
}

// G-334b — dynamic-code faces (eval/Function/AsyncFunction): the ref
// interpreter has no eval/Function node at all (its programs are
// direct expressions), so `eval("require(…)")` / `new Function(…)`
// must be unavailable, not merely scanned. For the program's duration
// the globals (and the constructor escape hatches on the function
// prototypes) are throwing stubs; the host's own `new AsyncFunction`
// compiles against the module-scope native binding captured above.
function codemodeDynamicCodeDisabled(name) {
  const message =
    `Code Mode programs cannot use ${name} — dynamic code evaluation is unavailable in the sandbox.`;
  // A regular function (NOT an arrow): the construct trap only fires
  // for constructable targets.
  function fail() {
    throw new Error(message);
  }
  return new Proxy(fail, {
    construct() {
      throw new Error(message);
    },
  });
}

// G-334c — `Bun`/`process` host-global facades for the program's
// duration (G-247 ledgered the structural follow-up; this is the
// cheap mitigation): ANY access throws. The host's own plumbing runs
// on the module-scope `nativeProcess`/`nativeBun` bindings.
function codemodeHostGlobalDisabled(name) {
  const message = `Code Mode programs cannot use ${name} — host runtime access is unavailable in the sandbox.`;
  const fail = () => {
    throw new Error(message);
  };
  return new Proxy(fail, {
    get() {
      throw new Error(message);
    },
    set() {
      throw new Error(message);
    },
    apply() {
      throw new Error(message);
    },
    construct() {
      throw new Error(message);
    },
  });
}

async function handleCodemodeEvaluate({ code, tools, token, limits }) {
  // resolveExecutionLimits (codemode.ts:133-137, called at make()):
  // throws the ref's RangeError text for invalid budgets — before any
  // program concern.
  const resolved = resolveExecutionLimits(limits);
  if (typeof code !== "string" || code.length === 0) {
    return { ok: false, error: "Code Mode execution requires a `code` string" };
  }
  // G-247 — scan before compile: a program that would load a module
  // never runs (the leaf applies the same rule at the tool entry, so
  // both faces reject identically).
  if (scanProgramSource(code) !== null) {
    return { ok: false, error: CODEMODE_MODULE_LOADING_REJECTED, calls: [] };
  }
  const calls = [];
  const logs = [];
  // Swap console → logs capture and fetch → the ref web wrapper for
  // the duration of the program; restored when it settles.
  const savedConsole = {};
  for (const name of CONSOLE_METHODS) {
    savedConsole[name] = console[name];
    console[name] = (...args) => {
      logs.push(formatConsoleMessage(name, args));
    };
  }
  const savedFetch = globalThis.fetch;
  // G-334a — the native fetch goes ONLY into the wrapper closure: if
  // the current global fetch is itself a codemode wrapper (nested
  // evaluation), unwrap its raw reference through the WeakMap; no
  // global property ever holds the raw fetch.
  const rawFetch = codemodeRawFetches.has(savedFetch)
    ? codemodeRawFetches.get(savedFetch)
    : savedFetch;
  const fetchWrapper = makeCodemodeFetch(rawFetch);
  globalThis.fetch = fetchWrapper;
  // G-247 — timers join the swapped surface: the scheduling globals
  // become throwing placeholders for the program's duration (the ref
  // interpreter has no timer globals at all). Restore rides the
  // capture frame, so overlapping evaluations nest correctly.
  const savedTimers = {};
  for (const name of ["setTimeout", "setInterval", "setImmediate"]) {
    savedTimers[name] = globalThis[name];
    globalThis[name] = codemodeTimerDisabled(name);
  }
  // G-334b — dynamic-code faces: eval/Function globals become
  // throwing stubs and the AsyncFunction/Function constructor escape
  // hatches (fn.constructor) are poisoned for the run; the host's own
  // compilation uses the module-scope native bindings.
  const savedDynamicCode = {
    eval: globalThis.eval,
    Function: globalThis.Function,
    asyncFunctionConstructor: Object.getOwnPropertyDescriptor(AsyncFunction.prototype, "constructor"),
    functionConstructor: Object.getOwnPropertyDescriptor(Function.prototype, "constructor"),
  };
  // (the intrinsic prototypes are captured BEFORE the global
  // `Function` swap — `Function.prototype` after the swap would
  // resolve through the stub)
  const fnProto = Function.prototype;
  const asyncFnProto = AsyncFunction.prototype;
  globalThis.eval = codemodeDynamicCodeDisabled("eval");
  globalThis.Function = codemodeDynamicCodeDisabled("Function");
  // (the two intrinsic `.constructor` properties get the SAME stub;
  // the descriptors above restore the natives afterwards)
  Object.defineProperty(
    asyncFnProto,
    "constructor",
    { value: codemodeDynamicCodeDisabled("AsyncFunction"), writable: true, configurable: true },
  );
  Object.defineProperty(
    fnProto,
    "constructor",
    { value: codemodeDynamicCodeDisabled("Function"), writable: true, configurable: true },
  );
  // G-334c — `Bun`/`process` facades: Bun pins `globalThis.Bun` as a
  // non-writable, non-configurable property, and swapping the WRITABLE
  // `globalThis.process` breaks Bun's own stdio plumbing (the stream
  // internals touch the global — observed: `stdin error: … cannot use
  // process` → a clean host exit mid-program). The shadow therefore
  // rides the EVALUATION SCOPE only: the runner takes `Bun`/`process`
  // parameters (strict-mode parameters shadow the globals for the
  // program's body). Residual faces: `globalThis.Bun` /
  // `globalThis.process` stay reachable (Bun-the-runtime pins them;
  // ledgered with G-247's structural follow-ups).
  const bunFacade = codemodeHostGlobalDisabled("Bun");
  const processFacade = codemodeHostGlobalDisabled("process");
  const frame = { savedConsole, savedFetch, savedTimers, savedDynamicCode };
  codemodeCaptureStack.push(frame);
  const catalog = Array.isArray(tools) ? tools : [];
  const evaluationToken = typeof token === "number" && Number.isFinite(token) ? token : 0;
  const toolsTree = buildToolsTree(catalog, calls, evaluationToken, resolved.maxToolCalls);
  const search = makeSearch(catalog);
  let program = (async () => {
    // G-334c — the `Bun`/`process` facades ride the evaluation scope
    // (strict-mode parameters shadow the globals; `globalThis.Bun`
    // itself is pinned non-configurable by Bun and stays a documented
    // residual face).
    const runner = new AsyncFunction(
      "tools",
      "search",
      "Bun",
      "process",
      `"use strict";\n${code}`,
    );
    return runner(toolsTree, search, bunFacade, processFacade);
  })();
  // timeoutMs fires between event-loop turns (v2's interpreter fires
  // between steps): async hangs resolve with the ref's TimeoutExceeded
  // message; a sync loop cannot be interrupted in-process — the
  // supervisor's deadline kill covers that (see request_codemode_kill).
  // G-247: the race arms on the module-scope NATIVE timer — the
  // globalThis placeholder swapped in above is the program's view.
  if (resolved.timeoutMs !== undefined) {
    let timer;
    program = Promise.race([
      program,
      new Promise((_resolve, reject) => {
        timer = nativeSetTimeout(
          () => reject(new Error(`Execution timed out after ${resolved.timeoutMs}ms.`)),
          resolved.timeoutMs,
        );
      }),
    ]).finally(() => clearTimeout(timer));
  }
  let value;
  try {
    value = await program;
  } catch (error) {
    return withCodemodeLogs({ ok: false, error: errText(error), calls }, logs);
  } finally {
    restoreCodemodeConsole(frame);
  }
  // The boundary walk's depth guard (data.ts:53) FIRST — a cyclic value
  // (a.self = a) would otherwise infinite-loop the structural walk
  // below; depth is cycle-safe and bounds the walk to <= 32 levels
  // (oracle gate followup F1). Then the structural caps
  // (interpreter/limits.ts:15-21) on the result crossing guest→host.
  if (value !== undefined) {
    if (valueDepthExceeded(value)) {
      return withCodemodeLogs(
        { ok: false, error: `Value exceeds the maximum depth of ${MAX_VALUE_DEPTH}.`, calls },
        logs,
      );
    }
    try {
      checkValueLimits(value);
    } catch (error) {
      return withCodemodeLogs({ ok: false, error: errText(error), calls }, logs);
    }
  }
  // `undefined` (no explicit return) is OMITTED so the host formats
  // "undefined"; explicit null stays null.
  const result = { ok: true, calls };
  if (value !== undefined) {
    // maxOutputBytes bounds the value HERE (the interpreter boundary;
    // the leaf bounds warnings/logs and skips a pre-bounded value).
    // G-180①: console output is captured into the result's `logs` on
    // this side; the logs byte budget lives in the oc-core leaf's
    // bound_output.
    if (resolved.maxOutputBytes !== undefined) {
      const serialized = JSON.stringify(value);
      if (serialized !== undefined) {
        const bytes = utf8ByteLength(serialized);
        if (bytes > resolved.maxOutputBytes) {
          value = `${utf8Truncate(serialized, resolved.maxOutputBytes)} [result truncated: ${bytes} bytes exceeds the ${resolved.maxOutputBytes}-byte output limit; return a smaller value]`;
          result.truncated = true;
        }
      }
    }
    result.value = value;
  }
  return withCodemodeLogs(result, logs);
}

// The optional `logs` key (codemode.ts:98/108): present only when the
// program captured console output.
function withCodemodeLogs(result, logs) {
  if (logs.length > 0) result.logs = logs;
  return result;
}

// Restore the NDJSON-rerouted console, the ref web fetch wrapper,
// and the native timers/dynamic-code faces/host globals after a Code
// Mode program settles (the sandbox swaps live only for the run).
// Overlapping evaluations: a finishing frame drops itself and
// restores the globals only when no other capture is still live.
function restoreCodemodeConsole(frame) {
  const index = codemodeCaptureStack.indexOf(frame);
  if (index !== -1) codemodeCaptureStack.splice(index, 1);
  if (codemodeCaptureStack.length > 0) return;
  for (const [name, method] of Object.entries(frame.savedConsole)) {
    console[name] = method;
  }
  globalThis.fetch = frame.savedFetch;
  // G-334a — `__rawFetch` is never created anymore; the defensive
  // delete keeps a stale key (a pre-upgrade guest's leftover) gone.
  delete globalThis.__rawFetch;
  // G-247 — the timer placeholders go back to the shielded natives.
  if (frame.savedTimers) {
    for (const [name, saved] of Object.entries(frame.savedTimers)) {
      globalThis[name] = saved;
    }
  }
  // G-334b — the dynamic-code stubs go back to the natives (and the
  // intrinsic constructor escape hatches are restored).
  if (frame.savedDynamicCode) {
    globalThis.eval = frame.savedDynamicCode.eval;
    globalThis.Function = frame.savedDynamicCode.Function;
    if (frame.savedDynamicCode.asyncFunctionConstructor) {
      Object.defineProperty(
        AsyncFunction.prototype,
        "constructor",
        frame.savedDynamicCode.asyncFunctionConstructor,
      );
    }
    if (frame.savedDynamicCode.functionConstructor) {
      Object.defineProperty(
        Object.getPrototypeOf(function () {}),
        "constructor",
        frame.savedDynamicCode.functionConstructor,
      );
    }
  }
  // G-334c — nothing to restore: the `Bun`/`process` facades rode the
  // evaluation scope as runner parameters (the host globals were
  // never swapped; Bun pins `Bun` and its stdio plumbing reads the
  // global `process`).
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
      source = await nativeBun.file(file).text();
    } catch {
      return;
    }
    digests[file] = createHash("sha256").update(source).digest("hex");
    let imports = [];
    try {
      imports = new nativeBun.Transpiler({ loader: "tsx" }).scanImports(source);
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
/** pluginId -> Map(raw language -> {language, render}) —
 * `ctx.markdown.registerCodeBlockRenderer` (api.tsx:168-178; keys are
 * the RAW language — normalization happens at consumption). Mirrors
 * the slotClaims lifecycle (snapshot/reinstate/retire/unload). */
const markdownRenderers = new Map();

function slotClaimCounter(pluginId) {
  const next = (slotClaimCounters.get(pluginId) ?? 0) + 1;
  slotClaimCounters.set(pluginId, next);
  return next - 1;
}
/** The host-pushed current route ({type:"session",sessionID}|{name:"home"}). */
let tuiRoute = { name: "home" };

/** G-239/G-331: the host-pushed session-panel view (`tui.panel`,
 *  the `tui.route` mirror) — `{pluginId, name, sessionID}` or null
 *  while no panel is open. `ctx.ui.panel.current()` reads it
 *  (plugin-filtered). */
let tuiPanel = null;

// PLUG-D07: the host pushes the LATEST activated location (the one-guest
// approximation of v2's per-instance hosts); every live ctx object's
// location view follows it.
const liveContexts = new Set();
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
    markdown: {
      registerCodeBlockRenderer(language, render) {
        return registerMarkdownRenderer(pluginId, language, render);
      },
    },
    kv: { get: async () => undefined, set: async () => {} },
    ui: {
      toast: () => {},
      dialog: { open: () => {}, close: () => {}, alert: () => {}, confirm: () => {} },
      // G-239/G-331: the session-panel face (the same seam the v2 ctx
      // exposes — the deferred open, the owner-scoped close, the
      // plugin-filtered `tui.panel` view).
      panel: makePanelFace(pluginId),
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

/** `ctx.markdown.registerCodeBlockRenderer(language, render)`
 * (api.tsx:168-178): blank language and INTRA-PLUGIN duplicates throw
 * ref-exact (cross-plugin duplicates are allowed — the registry
 * merges last-wins at consumption). The language crosses the wire
 * RAW; the host-side registration keys on it and oc-tui normalizes
 * at consumption. Returns an idempotent dispose closure. */
function registerMarkdownRenderer(pluginId, language, render) {
  const raw = String(language ?? "");
  if (raw.trim() === "") throw new Error("Markdown code-block language is required");
  if (typeof render !== "function") {
    throw new Error("Markdown code-block renderer requires a render function");
  }
  if (!markdownRenderers.has(pluginId)) markdownRenderers.set(pluginId, new Map());
  if (markdownRenderers.get(pluginId).has(raw)) {
    throw new Error(`Markdown code-block renderer already registered: ${raw}`);
  }
  markdownRenderers.get(pluginId).set(raw, { language: raw, render });
  hostRequest("markdown.renderer.register", { pluginId, language: raw }).catch((error) => {
    // The host is authoritative for duplicates too — surface the
    // failure and unwind the guest-side stash.
    notify("log", {
      pluginId,
      level: "error",
      message: `markdown renderer register failed: ${errText(error)}`,
    });
    markdownRenderers.get(pluginId)?.delete(raw);
  });
  let registered = true;
  return () => {
    if (!registered) return;
    registered = false;
    markdownRenderers.get(pluginId)?.delete(raw);
    hostRequest("markdown.renderer.unregister", { pluginId, language: raw }).catch(() => {});
  };
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
function flattenRenderTree(tree, width = tuiSidebarContentWidth) {
  const lines = treeLines(tree, width)
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
async function handleClaimRender({ pluginId, key, props }) {
  const claim = slotClaims.get(pluginId)?.get(String(key ?? ""));
  if (!claim || typeof claim.render !== "function") {
    throw new Error(`${pluginId}: no slot claim ${key}`);
  }
  const { section, handlers } = flattenRenderTree(
    claim.render(props && typeof props === "object" ? props : {}),
  );
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

/** `markdown.render` — run one registered markdown code-block
 *  renderer NOW (api.tsx:168-178's render receives `{code,
 *  language}`) and flatten to the native section shape against the
 *  caller's content width. The language key is the RAW registration
 *  language. Unlike the sidebar section shape, EVERY flattened line
 *  lands as a row (the block's content — the flatten tree's
 *  section-title split would otherwise eat the first line). */
async function handleMarkdownRender({ pluginId, language, code, width }) {
  const entry = markdownRenderers.get(pluginId)?.get(String(language ?? ""));
  if (!entry || typeof entry.render !== "function") {
    throw new Error(`${pluginId}: no markdown renderer for ${language}`);
  }
  const budget = Number(width) > 0 ? Number(width) : tuiSidebarContentWidth;
  const lines = treeLines(
    entry.render({ code: String(code ?? ""), language: entry.language }),
    budget,
  )
    .map((line) => line.replace(/\s+$/, ""))
    .filter((line) => line.trim() !== "");
  return { title: "", rows: lines.map((text) => ({ text, click: false })) };
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
  initialize: async () => ({ ok: true, bun: nativeBun.version }),
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
  "markdown.render": handleMarkdownRender,
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
  if (message.method === "location.update") {
    // PLUG-D07: the latest activated location — every live ctx's
    // location view follows (plugins reading ctx.location at handler
    // time observe the location whose assembly last activated them).
    const directory = typeof message.params?.directory === "string" ? message.params.directory : null;
    const projectId =
      typeof message.params?.project?.id === "string" ? message.params.project.id : null;
    if (directory) {
      domainBase.directory = directory;
      if (projectId) domainBase.project.id = projectId;
      for (const ctx of liveContexts) {
        ctx.directory = directory;
        if (ctx.location) {
          ctx.location.directory = directory;
          if (projectId && ctx.location.project) ctx.location.project.id = projectId;
        }
      }
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
  if (message.method === "tui.panel") {
    // G-239/G-331: host-pushed session-panel state (the `tui.route`
    // mirror) — `ctx.ui.panel.current()` reads it (plugin-filtered).
    const params = message.params ?? {};
    const usable =
      typeof params.pluginId === "string" &&
      params.pluginId &&
      typeof params.name === "string" &&
      params.name &&
      typeof params.sessionID === "string" &&
      params.sessionID;
    tuiPanel = usable
      ? { pluginId: params.pluginId, name: params.name, sessionID: params.sessionID }
      : null;
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
// stdin EOF: the supervisor closed its end — shutdown, abrupt exit, or
// SIGKILL of the host process. Plugin `setInterval` timers would keep
// Bun's event loop (and this process) alive forever: exit NOW. This is
// the orphan-leak fix (incident 2026-10-22); the `stdin end`/`close`
// watchers above are the belt in case the iterator ever wedges before
// completing.
hostGone("stdin EOF (read loop finished)");
