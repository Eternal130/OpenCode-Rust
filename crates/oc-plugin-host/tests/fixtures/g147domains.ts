// G-147/G-148 fixture: the ctx.model domain (adapter.ts:342-346 —
// list/default over the host's model catalog; the transform editor
// table of promise/model.ts:6-23) and ctx.plugin.list
// (promise/plugin.ts:49). Setup results land as sandbox files the
// Rust test asserts on.
export default {
  id: "g147domains",
  async setup(ctx) {
    const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";
    const write = (name, value) =>
      Bun.write(`${sandbox}/${name}`, typeof value === "string" ? value : JSON.stringify(value));

    // -- model.list / model.default over the host's (mock) port --------
    try {
      const models = await ctx.model.list();
      await write("model-list.json", models);
      const narrowed = await ctx.model.list("mockprov");
      await write("model-list-narrowed.json", narrowed);
      const def = await ctx.model.default();
      await write("model-default.json", def);
    } catch (error) {
      await write("model.error", String(error));
    }

    // -- model.transform: the editor table materializing natively ------
    await ctx.model.transform((editor) => {
      const base = editor.get("mockprov", "mock-large");
      editor.update("mockprov", "mock-mini", (model) => {
        model.name = "Mock Mini (patched)";
        model.enabled = true;
      });
      editor.default.set("mockprov", "mock-mini");
      editor.remove("mockprov", "mock-large");
      // The editor's reads: base rows merged with the overrides.
      editorListSanity(editor, base);
    });
    function editorListSanity(editor, base) {
      const rows = editor.list("mockprov");
      const ids = rows.map((row) => row.id);
      write(
        "editor-list.json",
        ids,
      );
      write("editor-default.json", editor.default.get());
      write("editor-provider-list.json", editor.provider.list());
      if (base && !ids.includes("mock-large")) {
        write("editor-removed-mark", "removed");
      }
    }

    // -- model.reload (re-derives base + editors) ------------------------
    try {
      await ctx.model.reload();
      await write("model-reloaded.mark", "1");
    } catch (error) {
      await write("model.reload.error", String(error));
    }

    // -- ctx.plugin.list (G-148) -----------------------------------------
    try {
      const plugins = await ctx.plugin.list();
      await write("plugin-list.json", plugins);
    } catch (error) {
      await write("plugin.error", String(error));
    }
  },
};
