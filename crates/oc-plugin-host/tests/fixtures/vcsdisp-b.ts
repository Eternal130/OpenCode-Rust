// G-379② fixture, plugin B: registers a vcs provider that must
// SURVIVE plugin A's dispose (ownership-scoped removal).
export default {
  id: "vcsdisp-b",
  async setup(ctx) {
    await ctx.vcs.transform((editor) => {
      editor.add({ id: "vcsB", name: "VCS B" });
    });
  },
};
