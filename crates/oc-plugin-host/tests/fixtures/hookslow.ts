// RC-03 fixture: a session.* hook that sleeps past the 5 s query
// dispatch budget (body-mutating class — must complete) and a
// shell.create.before hook that sleeps past a short query budget
// (query class — must time out). Markers land when handlers finish.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const marker = (name) => join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export default {
  id: "hookslow",
  async setup(ctx) {
    await ctx.session.hook("context", async (event) => {
      await sleep(6_500);
      writeFileSync(marker("session-done.txt"), "done");
      return event;
    });
    await ctx.shell.hook("create.before", async (event) => {
      await sleep(60_000);
      writeFileSync(marker("shell-done.txt"), "done");
      return event;
    });
    ctx.log("hookslow: armed");
    return async () => {};
  },
};
