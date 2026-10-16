// D-03 corpus fixture (mutation shapes): plugin A applies every
// v2-guaranteed mutation surface to the `session.context` event —
// whole-field REPLACEMENT (the `event.input = repair(...)` pattern,
// v2 tool-input-repair.ts:33), scalar rewrite, key deletion — plus
// the two REGISTERED-divergence arms: a post-return mutation (R2:
// invisible across the copy boundary — the isolation benefit) and a
// non-JSON stash value (R4: crosses only as its JSON coercion).
export default {
  id: "hookswap-a",
  async setup(ctx) {
    await ctx.session.hook("context", async (event) => {
      if (!event || !Array.isArray(event.messages)) return;
      // v2-guaranteed shape 1: whole-field replacement.
      event.messages = [
        { id: "msg_replaced", role: "user", content: [{ type: "text", text: "replaced-by-A" }] },
      ];
      // v2-guaranteed shape 2: scalar rewrite.
      event.sessionID = "ses_rewritten_by_A";
      // v2-guaranteed shape 3: key deletion.
      delete event.system;
      // Registered divergence R4: a non-JSON stash value survives the
      // copy only as its JSON coercion (a Date becomes a string), never
      // as the live object v2 would hand to later handlers.
      event.stash = new Date(0);
      // Registered divergence R2: mutating AFTER the handler resolves
      // lands on the guest-local object only — the host snapshot
      // already crossed the boundary. In v2 this write would hit the
      // live event and race later handlers; here it is invisible by
      // design (crash/isolation boundary).
      const held = event;
      setTimeout(() => {
        held.messages[0].content[0].text += " [late-by-A]";
      }, 1);
    });
    await ctx.command.add({
      name: "hookswap-a.ask",
      description: "Prompt through the hook chain",
      async execute() {
        await ctx.session.prompt({ sessionID: "ses_corpus", text: "original text" });
        return "asked";
      },
    });
    return async () => {};
  },
};
