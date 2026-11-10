// G-293 reload probe: every setup appends one line to the marker file
// (the observable load counter) and registers a working command. Tests
// edit their sandbox copy (never this file) to change the revision.
import { appendFileSync } from "node:fs";
import { join } from "node:path";

const SANDBOX = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";

export default {
  id: "reloadprobe",
  async setup(ctx) {
    appendFileSync(join(SANDBOX, "reload-probe-loads.txt"), "load\n");
    await ctx.command.add({
      name: "probe.ping",
      description: "reload probe command",
      async execute() {
        return "pong-v1";
      },
    });
    return async () => {};
  },
};
