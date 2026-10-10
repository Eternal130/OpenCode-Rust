// Slow-plugin fixture: the handler for slow.tick sleeps past any
// dispatch deadline; the fast.tick handler is instant. The supervisor
// must drop the slow dispatch (logged, counted) and still deliver the
// fast one.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export default {
  id: "slow",
  async setup(ctx) {
    await ctx.event.subscribe(["slow.tick"], async () => {
      await sleep(60_000);
    });
    await ctx.event.subscribe(["fast.tick"], async (event) => {
      writeFileSync(marker("fast.txt"), "fast");
    });
  },
};
