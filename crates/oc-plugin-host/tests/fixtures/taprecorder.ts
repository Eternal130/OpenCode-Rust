// Wave-4 event-order fixture: records every observed event envelope id
// (one per line, arrival order) for the 1:1 stream comparison.
export default {
  id: "taprecorder",
  async setup(ctx) {
    const fs = await import("node:fs");
    const out = `${process.env.OC_PLUGIN_TEST_SANDBOX}/observed-ids.txt`;
    fs.writeFileSync(out, "");
    await ctx.event.subscribe("session.renamed", async (event) => {
      fs.appendFileSync(out, `${event.id}\n`);
    });
    ctx.log("taprecorder: armed");
    return async () => {};
  },
};
