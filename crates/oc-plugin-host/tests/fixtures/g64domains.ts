// G-64 fixture: the newly-bridged Context domains — `ctx.provider.*`
// (adapter.ts:346-370), `ctx.reference.*` (adapter.ts:427-452),
// `ctx.experimental.terminal.read` (host.ts:267-269), the
// `ctx.integration` method surface (host.ts:274-357's
// list/get/connect.key/oauth.*/connection.*) — plus the G-24 residual
// surfaces: `ctx.vcs.transform`/`ctx.vcs.reload` and the websearch
// `editor.default.set` persistence. Setup results land as sandbox files
// the Rust test asserts on.
export default {
  id: "g64domains",
  async setup(ctx) {
    const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";
    const write = (name, value) =>
      Bun.write(`${sandbox}/${name}`, typeof value === "string" ? value : JSON.stringify(value));

    // -- provider.list / provider.get over the host's (mock) port ------
    try {
      const providers = await ctx.provider.list();
      await write("provider-list.json", providers);
      const one = await ctx.provider.get("mockprov");
      await write("provider-get.json", one);
    } catch (error) {
      await write("provider.error", String(error));
    }

    // -- provider.transform: an editor table materializing natively ----
    await ctx.provider.transform((editor) => {
      editor.add({ id: "plugprov", name: "Plugin Provider", baseURL: "https://plug.invalid/v1" });
      editor.models.set("plugprov", { "plug-mini": { name: "Plug Mini" } });
    });

    // -- reference.list over the host's (mock) port + transform --------
    try {
      const references = await ctx.reference.list();
      await write("reference-list.json", references);
    } catch (error) {
      await write("reference.error", String(error));
    }
    await ctx.reference.transform((editor) => {
      editor.add("plugdocs", { type: "local", path: "/tmp/opencode/plugdocs" });
    });

    // -- experimental.terminal.read ------------------------------------
    try {
      const read = await ctx.experimental.terminal.read({
        sessionID: "ses_terminal_probe",
        lines: 10,
      });
      await write("terminal-read.json", read);
    } catch (error) {
      await write("terminal.error", String(error));
    }

    // -- integration.list / connection.active / connection.status -----
    try {
      const integrations = await ctx.integration.list();
      await write("integration-list.json", integrations);
      const active = await ctx.integration.connection.active("mockint");
      await write("connection-active.json", active);
    } catch (error) {
      await write("integration.error", String(error));
    }

    // -- vcs.transform / vcs.reload (G-24 residual) --------------------
    await ctx.vcs.transform((editor) => {
      editor.add({ id: "plugvcs", name: "Plugin VCS" });
    });
    await ctx.vcs.reload();

    // -- websearch editor.default.set persistence (G-24 residual) ------
    try {
      await ctx.websearch.transform(async (editor) => {
        await editor.default.set("mocksearch");
        const current = await editor.default.get();
        await write("websearch-default.json", current ?? null);
      });
    } catch (error) {
      await write("websearch-default.error", String(error));
    }

    return async () => {};
  },
};
