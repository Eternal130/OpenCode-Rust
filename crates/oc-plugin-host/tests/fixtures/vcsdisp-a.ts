// G-379②/G-379③ fixture: the vcs transform's dispose semantics and
// default editor. Two plugins each register a vcs provider; `dropA`
// disposes plugin A's transform — plugin B's row must SURVIVE (the
// old dispose filter was inverted-and-never-matching: it cleared the
// WHOLE table). The default editor round-trips a selection (ref
// vcs.ts:85-88's editor.selection get/set).
let disposeA;

export default {
  id: "vcsdisp-a",
  async setup(ctx) {
    const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";
    const handle = await ctx.vcs.transform((editor) => {
      editor.add({ id: "vcsA", name: "VCS A" });
      // G-379③ — the default editor get/set round-trip (the ref's
      // in-memory State selection; initially undefined).
      editor.default.set("vcsA");
    });
    disposeA = handle.dispose;
    await ctx.command.add({
      name: "vcsdisp.dropA",
      description: "dispose plugin A's vcs transform",
      execute: async () => {
        await disposeA();
      },
    });
    await ctx.command.add({
      name: "vcsdisp.defaultGet",
      description: "read the vcs default editor selection",
      execute: async () => {
        let selection;
        await ctx.vcs.transform(async (editor) => {
          selection = editor.default.get();
        });
        await Bun.write(`${sandbox}/vcs-default.json`, JSON.stringify({ selection }));
      },
    });
  },
};
