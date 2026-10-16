// D-03 corpus fixture (mutation shapes): plugin B runs AFTER A in the
// host-side per-plugin chain (tests/common/mod.rs — exactly the engine
// loop shape). B receives the SERIALIZED result of A's dispatch, so it
// observes A's replacement/rewrite/deletion (cross-plugin visibility
// of every v2-guaranteed shape) and composes what it saw into the
// message text for the host-side assertion. It also reports the JSON
// type of A's stashed Date (R4: coerced to a string) and — by
// construction — can never observe A's post-return mutation (R2).
export default {
  id: "hookswap-b",
  async setup(ctx) {
    await ctx.session.hook("context", async (event) => {
      const text = event?.messages?.[0]?.content?.[0]?.text ?? "";
      const stash = event && "stash" in event ? `${typeof event.stash}` : "absent";
      const system = event && "system" in event ? "present" : "deleted";
      event.messages[0].content[0].text = `${text} [seen-by-B stash=${stash} system=${system}]`;
    });
    return async () => {};
  },
};
