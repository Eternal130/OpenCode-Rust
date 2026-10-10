// Wave-4 adversarial fixture: EVERY handler surface sleeps past the
// bridge timeouts — event dispatch (slow.tick), command execution
// (slowall.crawl), tool execution (slow_tool), and the session.context
// hook (behind slowall.ask's ctx.session.prompt). Registration (setup)
// is fast so the plugin is Active for the slow-surface probes; the
// wake.tick/wake2.tick subscriptions answer immediately (recovery
// probes).
const SLEEP_MS = 4000;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sandbox = () => process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";

export default {
  id: "slowall",
  async setup(ctx) {
    await ctx.command.add({
      name: "slowall.crawl",
      description: "Sleeps past the dispatch timeout",
      async execute() {
        await sleep(SLEEP_MS);
        return "finally";
      },
    });
    // Prompts the session engine — the port dispatches the (sleeping)
    // session.context hook chain, so the hook timeout must bound it.
    await ctx.command.add({
      name: "slowall.ask",
      description: "Prompts through the (sleeping) hook chain",
      async execute() {
        await ctx.session.prompt({ sessionID: "ses_fuzz", text: "ask" });
        return "asked";
      },
    });
    await ctx.tool.transform((editor) => {
      editor.add({
        name: "slow_tool",
        description: "Sleeps past every timeout",
        input: { type: "object", properties: {} },
        async execute() {
          await sleep(SLEEP_MS);
          return "late";
        },
      });
    });
    await ctx.session.hook("context", async () => {
      await sleep(SLEEP_MS);
    });
    const fs = await import("node:fs");
    await ctx.event.subscribe("slow.tick", async () => {
      await sleep(SLEEP_MS);
      fs.writeFileSync(`${sandbox()}/slowall-event.txt`, "slept");
    });
    await ctx.event.subscribe("wake.tick", async () => {
      fs.writeFileSync(`${sandbox()}/slowall-wake.txt`, "awake");
    });
    await ctx.event.subscribe("wake2.tick", async () => {
      fs.writeFileSync(`${sandbox()}/slowall-wake-2.txt`, "awake2");
    });
    ctx.log("slowall: armed (every handler sleeps)");
    return async () => {};
  },
};
