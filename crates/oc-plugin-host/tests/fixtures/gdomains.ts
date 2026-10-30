// G-24 fixture: the four newly-bridged plugin domains —
// `ctx.vcs.get/base` (read over the engine's vcs adapters),
// `ctx.websearch.providers/query/reload/transform` (ref host.ts:479-504),
// `ctx.worktree.transform` (host.ts:505-522's strategy registration), and
// `ctx.integration.method.update` (host.ts:631's methodImplementation).
// Setup results land as sandbox files the Rust test asserts on; the
// guest closures (provider execute, worktree ops, OAuth authorize)
// answer the host's later hook dispatches.
export default {
  id: "gdomains",
  async setup(ctx) {
    const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";

    // -- vcs.get / vcs.base over the host's (mock) vcs port --------------
    try {
      const info = await ctx.vcs.get();
      await Bun.write(`${sandbox}/vcs.json`, JSON.stringify(info));
    } catch (error) {
      await Bun.write(`${sandbox}/vcs.error`, String(error));
    }

    // -- websearch.providers over the host's (mock) websearch port -------
    try {
      const providers = await ctx.websearch.providers();
      await Bun.write(`${sandbox}/ws-providers.json`, JSON.stringify(providers));
    } catch (error) {
      await Bun.write(`${sandbox}/ws-providers.error`, String(error));
    }

    // -- websearch.transform: a guest provider whose execute answers the
    //    host's websearch.execute dispatch -------------------------------
    await ctx.websearch.transform((editor) => {
      editor.add({
        id: "guestsearch",
        name: "Guest Search",
        execute: async ({ query }) => [
          { url: `https://example.invalid/guest?q=${encodeURIComponent(String(query ?? ""))}`, time: {} },
        ],
      });
    });

    // -- worktree.transform: a guest strategy answering create/remove/list
    await ctx.worktree.transform((editor) => {
      editor.add({
        id: "pluginwt",
        create: async (input) => ({ directory: `${String(input?.directory ?? "")}-guest` }),
        remove: async () => true,
        list: async () => [{ directory: "/tmp/opencode/root", kind: "root" }],
      });
    });

    // -- integration.method.update: an OAuth method whose authorize /
    //    callback / refresh closures answer the integration dispatches --
    await ctx.integration.method.update({
      integrationID: "acme",
      method: { type: "oauth", id: "acme-oauth", label: "Acme OAuth" },
      authorize: async () => ({
        url: "https://acme.invalid/auth",
        instructions: "visit the url",
        mode: "code",
        callback: async (code) => ({
          type: "oauth",
          methodID: "acme-oauth",
          refresh: `refresh-${code}`,
          access: `access-${code}`,
          expires: 0,
        }),
      }),
      refresh: async (credential) => ({
        ...credential,
        access: `refreshed-${credential?.access ?? ""}`,
      }),
    });

    return async () => {};
  },
};
