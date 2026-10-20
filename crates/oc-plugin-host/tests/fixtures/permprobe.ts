// PLUG-D11 probe: ctx.permission's v2 surface (list/get/reply) reaches a
// real PermissionPort through the host bridge. The Rust side stubs the
// port with canned values; the guest asserts shapes + writes results to
// KV for the Rust side to check.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const SANDBOX = process.env.OC_WIRING_SANDBOX ?? "/tmp/opencode";
const marker = (name) => join(SANDBOX, name);

export default {
  id: "permprobe",
  async setup(ctx) {
    const record = (key, value) => {
      writeFileSync(marker(`${key.replace(/:/g, "-")}.json`), JSON.stringify(value));
    };

    // Surface presence (the RED state: methods absent on the ctx).
    record("perm:has", {
      list: typeof ctx.permission?.list === "function",
      get: typeof ctx.permission?.get === "function",
      reply: typeof ctx.permission?.reply === "function",
    });

    // list: the port's pending array (stubbed: one request).
    const listed = await ctx.permission.list({ sessionID: "ses_probe" });
    record("perm:list", listed);

    // get: the single request shape.
    const got = await ctx.permission.get({ sessionID: "ses_probe", requestID: "per_probe1" });
    record("perm:get", got);

    // get on the wrong session: the v2 not-found error.
    let wrongSession = null;
    try {
      await ctx.permission.get({ sessionID: "ses_other", requestID: "per_probe1" });
      wrongSession = "no-error";
    } catch (error) {
      wrongSession = String(error?.message ?? error).includes("not found") ? "not-found" : "other";
    }
    record("perm:wrong-session", wrongSession);

    // reply: resolves (the stub answers null).
    const replied = await ctx.permission.reply({
      sessionID: "ses_probe",
      requestID: "per_probe1",
      decision: "once",
      message: "by plugin",
    });
    record("perm:reply", replied ?? null);

    writeFileSync(marker("perm-setup-done.txt"), "1");
    return async () => {};
  },
};
