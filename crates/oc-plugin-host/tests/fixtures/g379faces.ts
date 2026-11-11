// G-379.8 — the guest vcs + worktree read/op call faces over a real
// git project: commands return JSON result strings the test asserts
// on (every call routes hostRequest -> supervisor -> EngineVcsPort /
// EngineWorktreePort -> the shared handler cores).
export default {
  id: "g379faces",
  async setup(ctx) {
    const describe = async (promise) => {
      try {
        return { ok: true, value: await promise };
      } catch (error) {
        return { ok: false, message: error instanceof Error ? error.message : String(error) };
      }
    };
    await ctx.command.add({
      name: "g379.vcs",
      description: "vcs branch list / status / diff read faces",
      async execute() {
        return JSON.stringify({
          branches: await describe(ctx.vcs.branch()),
          branchesSearch: await describe(ctx.vcs.branch({ search: "fea", limit: 5 })),
          status: await describe(ctx.vcs.status()),
          diff: await describe(ctx.vcs.diff({ mode: "working" })),
          diffBranch: await describe(ctx.vcs.diff({ mode: "branch" })),
        });
      },
    });
    await ctx.command.add({
      name: "g379.worktree.roundtrip",
      description: "worktree list -> create -> list -> remove -> list round-trip",
      async execute() {
        const projectID = ctx.location?.project?.id ?? "";
        const parent = process.env.OC_G379_PARENT ?? "";
        const listBefore = await describe(ctx.worktree.list({ projectID }));
        const created = await describe(
          ctx.worktree.create({ projectID, name: "lane5b", directory: parent }),
        );
        const listAfterCreate = await describe(ctx.worktree.list({ projectID }));
        let removed = null;
        if (created.ok) {
          removed = await describe(
            ctx.worktree.remove({ projectID, directory: created.value.directory }),
          );
        }
        const listAfterRemove = await describe(ctx.worktree.list({ projectID }));
        return JSON.stringify({ projectID, listBefore, created, listAfterCreate, removed, listAfterRemove });
      },
    });
    await ctx.command.add({
      name: "g379.worktree.refresh",
      description: "worktree.refresh reconciles a host-made linked worktree",
      async execute() {
        const projectID = ctx.location?.project?.id ?? "";
        const before = await describe(ctx.worktree.list({ projectID }));
        await describe(ctx.worktree.refresh({ projectID }));
        const after = await describe(ctx.worktree.list({ projectID }));
        return JSON.stringify({ projectID, before, after });
      },
    });
    return async () => {};
  },
};
