// PLUG-D07 probe: registrations the engine filters per location (a hook
// + a tool row) plus the ctx.location view the host pushes on each
// activation (the one-guest approximation of v2's per-instance hosts).
export default {
  id: "perloc",
  async setup(ctx) {
    await ctx.tool.hook("execute.before", async () => {});
    await ctx.command.add({
      name: "perloc.dir",
      description: "Report the ctx location view",
      async execute() {
        return { directory: ctx.directory, project: ctx.location?.project?.id ?? null };
      },
    });
    return async () => {};
  },
};
