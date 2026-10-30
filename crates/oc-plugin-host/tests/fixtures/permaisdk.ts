// PLUG-G2 fixture: the three formerly-unbridged v2 hook domains —
// `ctx.permission.hook("evaluate", cb)` (ref promise/permission.ts:23) and
// `ctx.aisdk.hook("sdk"|"language", cb)` (ref promise/aisdk.ts:21,
// promise/aisdk.ts's AISDKHooks). Registration must land in the
// supervisor's snapshot (no "unknown hook" rejection) and the host's
// hook.dispatch round-trips the guest's mutation.
export default {
  id: "permaisdk",
  async setup(ctx) {
    await ctx.permission.hook("evaluate", async (event) => {
      if (event && typeof event === "object") {
        event.effect = "deny";
        event.message = "denied-by-plugin";
      }
    });
    await ctx.aisdk.hook("sdk", async (event) => {
      if (event && typeof event === "object") {
        event.package = "@ai-sdk/swapped";
        event.sdk = { wrapped: true };
      }
    });
    await ctx.aisdk.hook("language", async (event) => {
      if (event && typeof event === "object") {
        event.options = { ...(event.options ?? {}), touched: "by-plugin" };
      }
    });
    return async () => {};
  },
};
