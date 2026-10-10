//! The guest-side host script (`bun host.js`): module loading via
//! dynamic `import()` (native ESM + npm + CJS resolution — whatever
//! Bun runs), per-plugin error containment, the registration-time
//! `ctx` bridge, event fan-out, and the digest-scan hot-reload
//! watcher.
//!
//! Productionized from the `/tmp/opencode/spike-bun` `host.ts`
//! prototype (spike-plugin-runtime.md): NDJSON JSON-RPC over stdio,
//! both directions; guest→host sync calls are plain Promises settled
//! by supervisor replies; the read loop NEVER awaits a handler before
//! reading the next line (the mirrored deadlock rule — see the crate
//! docs §Deadlock protocol).

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

const GUEST_ID_BASE = 1 << 40;
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

function makeCtx(pluginId) {
  const commands = plugins.get(pluginId).commands;
  const subscriptions = plugins.get(pluginId).subscriptions;
  return {
    plugin: { id: pluginId },
    // The registration-time sync bridge: every add lands in the
    // supervisor's native structures BEFORE the `load` reply.
    command: {
      async add(spec) {
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
      },
    },
    theme: {
      async register(theme) {
        const name = String(theme?.name ?? "");
        if (!name) throw new Error("theme.register requires a name");
        await hostRequest("theme.register", { pluginId, name, document: theme?.document ?? null });
        return { async dispose() {} };
      },
    },
    event: {
      // (handler) | (types, handler) | (types[]) — the last form is a
      // filter-only subscription (no callback; parity with the client
      // EventApi shape).
      async subscribe(first, second) {
        const types = Array.isArray(first) ? first : null;
        const handler = typeof first === "function" ? first : second;
        const entry = { types, handler: handler ?? null };
        subscriptions.push(entry);
        await hostRequest("event.subscribe", { pluginId, types });
        return {
          async dispose() {
            const at = subscriptions.indexOf(entry);
            if (at >= 0) subscriptions.splice(at, 1);
          },
        };
      },
    },
    // Nested sync RPC — the deadlock-rule hammer: a guest→host request
    // issued from inside setup or an event handler must be answered by
    // the supervisor's IO thread while the guest's own reply is still
    // outstanding.
    ping: async () => hostRequest("ping", { pluginId }),
    log: (...args) => notify("log", { pluginId, level: "info", message: args.join(" ") }),
  };
}

// ---------------------------------------------------------------------------
// Supervisor-facing methods
// ---------------------------------------------------------------------------

async function handleLoad({ entry, generation }) {
  const abs = require("node:path").resolve(entry);
  // v2 source.bun parity: evict CJS cache under the plugin dir, then
  // re-import with a generation cache-bust.
  const dir = require("node:path").dirname(abs);
  for (const key of Object.keys(require.cache)) {
    if (key.startsWith(dir)) delete require.cache[key];
  }
  loadingEntries.add(abs);
  let mod;
  try {
    mod = await import(`file://${abs}?t=${generation ?? Date.now()}`);
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
  const ctx = makeCtx(id);
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
  return { ok: true };
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
        if (!sub.handler) { delivered++; continue; }
        try {
          await sub.handler(event);
          delivered++;
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

async function handleCommandExecute({ pluginId, name, args }) {
  const record = plugins.get(pluginId);
  const execute = record?.commands.get(name);
  if (typeof execute !== "function") {
    throw new Error(`${pluginId}:${name}: no such command`);
  }
  const result = await execute({ args: args ?? null }, makeCtx(pluginId));
  return { ok: true, result: result ?? null };
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
