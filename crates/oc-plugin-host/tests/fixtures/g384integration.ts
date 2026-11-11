// G-384/G-385 fixture — `ctx.integration.transform` (the editor CRUD
// face) and `ctx.integration.reload`. The editor's reads (list/get/
// method.list) and reload ride the host's (mock) integration port;
// the mutations land in the supervisor's registration tables the Rust
// test asserts on. The transform is then disposed (retracting the
// plugin's rows, vcs.transform precedent) and a second transform
// re-registers one row so the final snapshot proves the retraction.
export default {
  id: "g384int",
  async setup(ctx) {
    const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";

    // -- G-385: ctx.integration.reload() resolves ------------------------
    try {
      await ctx.integration.reload();
      await Bun.write(`${sandbox}/reload.json`, "ok");
    } catch (error) {
      await Bun.write(`${sandbox}/reload.error`, String(error));
    }

    // -- G-384: the editor CRUD face -------------------------------------
    const first = await ctx.integration.transform(async (editor) => {
      // reads over the live port
      const list = await editor.list();
      await Bun.write(`${sandbox}/list.json`, JSON.stringify(list));
      const methods = await editor.method.list("base");
      await Bun.write(`${sandbox}/methods.json`, JSON.stringify(methods));
      // update an existing integration (rename + metadata)
      await editor.update("base", { name: "Renamed Base" });
      // create-if-missing (name defaults to the id)
      await editor.update("brandnew", { metadata: { source: "plugin" } });
      // remove one
      editor.remove("gone");
      // method.update routes through the shared G-24 path
      await editor.method.update({
        integrationID: "acme",
        method: { type: "oauth", id: "acme-oauth", label: "Acme OAuth" },
        authorize: async () => ({
          url: "https://acme.invalid/auth",
          instructions: "visit",
          mode: "code",
          callback: async (code) => ({
            type: "oauth",
            methodID: "acme-oauth",
            refresh: `refresh-${code}`,
            access: `access-${code}`,
            expires: 0,
          }),
        }),
      });
      // method.remove
      editor.method.remove("base", { type: "key" });
      // get round-trips
      const got = await editor.get("base");
      await Bun.write(`${sandbox}/got.json`, JSON.stringify(got));
    });
    await Bun.write(`${sandbox}/phase1.json`, "1");

    // -- dispose retracts the plugin's rows (vcs.transform precedent) ---
    await first.dispose();

    // -- a second transform re-registers one row -------------------------
    await ctx.integration.transform(async (editor) => {
      await editor.update("survivor", { name: "Survivor" });
    });
    await Bun.write(`${sandbox}/done.json`, "1");

    return async () => {};
  },
};
