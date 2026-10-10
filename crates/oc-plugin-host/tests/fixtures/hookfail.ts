// Corpus fixture (hook failure semantics, D-03): a plugin whose
// session.context handler THROWS. v2's trigger fails the dispatch on a
// handler error (hooks.ts:88-95 — `yield*` propagates, later handlers
// are skipped, the caller aborts); the guest must reject the
// `hook.dispatch` RPC so the host bridge surfaces the failure instead
// of silently continuing the mutation chain.
export default {
  id: "hookfail",
  async setup(ctx) {
    await ctx.session.hook("context", async () => {
      throw new Error("hookfail: context rejected");
    });
    await ctx.command.add({
      name: "hookfail.ask",
      description: "Prompt through a failing hook chain",
      async execute() {
        await ctx.session.prompt({ sessionID: "ses_hookfail", text: "doomed" });
        return "asked";
      },
    });
    return async () => {};
  },
};
