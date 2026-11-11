// G-350 fixture: registers two vcs providers — one WITH an `init`
// closure that materializes a minimal git repository under the event's
// `worktree` plus a marker, one WITHOUT `init` (the 501 "unsupported"
// case). The host's /api/vcs/init endpoint reaches the closure through
// the "vcs" domain "init" hook dispatch (ref vcs.ts:178-191).
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export default {
  id: "fakevcs",
  async setup(ctx) {
    await ctx.vcs.transform((editor) => {
      editor.add({
        id: "fakevcs",
        name: "Fake VCS",
        init: async (input) => {
          const worktree = input?.worktree ?? input?.directory ?? ".";
          const git = join(worktree, ".git");
          mkdirSync(join(git, "objects"), { recursive: true });
          mkdirSync(join(git, "refs"), { recursive: true });
          writeFileSync(join(git, "HEAD"), "ref: refs/heads/main\n");
          writeFileSync(join(git, "config"), "[core]\n\trepositoryformatversion = 0\n");
          const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX;
          if (sandbox) writeFileSync(join(sandbox, "fakevcs-init.json"), JSON.stringify(input ?? {}));
        },
      });
      editor.add({ id: "novcs", name: "No Init VCS" });
    });
  },
};
