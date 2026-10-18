// PLUG-D21 / PLUG-D18 / PLUG-D14 wiring probe: guest hooks at the three
// production hook points (tool.execute.before, session.prompt,
// shell.create.before), storage KV (written at setup, read back through a
// command), and an rpc registration (echo). Capability guards keep the
// RED state crisp: surfaces the host does not expose yet are skipped
// instead of failing the whole setup.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const SANDBOX = process.env.OC_WIRING_SANDBOX ?? "/tmp/opencode";
const marker = (name) => join(SANDBOX, name);

export default {
  id: "wiring",
  async setup(ctx) {
    // -- D21: tool.execute.before (mutation: rewrite the read target) ----
    await ctx.tool.hook("execute.before", async (event) => {
      if (
        event?.tool === "read" &&
        typeof event?.input?.path === "string" &&
        event.input.path.endsWith("decoy.txt")
      ) {
        event.input.path = marker("hook-target.txt");
      }
    });

    // -- D21: session.prompt (mutation; the engine dispatch site is the
    //    RUNTIME lane's P-01 — the registration is this lane's seam) ----
    await ctx.session.hook("prompt", async (event) => {
      if (typeof event?.prompt?.text === "string") {
        event.prompt.text += " [touched-by-plugin]";
      }
    });

    // -- D21: shell.create.before (the dispatch site is TOOLS-EXEC's
    //    02:C2 — same seam) ----------------------------------------------
    if (ctx.shell) {
      await ctx.shell.hook("create.before", async (event) => {
        if (event?.command === "echo untouched") {
          event.command = "echo touched-by-plugin";
        }
      });
    } else {
      writeFileSync(marker("shell-domain-absent.txt"), "1");
    }

    // -- D18: KV written at setup; read back through a plugin command ----
    await ctx.storage.set("wiring:note", "hello-from-setup");
    await ctx.command.add({
      name: "wiring.kv",
      description: "Read the plugin KV note",
      async execute() {
        return (await ctx.storage.get("wiring:note")) ?? null;
      },
    });

    // -- D14: rpc registration (echo) -------------------------------------
    if (ctx.rpc) {
      await ctx.rpc.register(
        {
          id: "wiring-rpc",
          methods: {
            echo: {
              input: {
                type: "object",
                properties: { text: { type: "string" } },
                required: ["text"],
                additionalProperties: false,
              },
              output: {
                type: "object",
                properties: { text: { type: "string" } },
                required: ["text"],
                additionalProperties: false,
              },
            },
          },
          events: {},
        },
        {
          echo: async (input) => ({ text: `echo:${input.text}` }),
        },
      );
    } else {
      writeFileSync(marker("rpc-domain-absent.txt"), "1");
    }

    writeFileSync(marker("setup-done.txt"), "1");
    return async () => {};
  },
};
