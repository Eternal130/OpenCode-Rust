// Crash fixture: a kill.tick handler exits the process — the
// supervisor restarts with backoff, attributes the crash to this
// plugin, and disables it after max_plugin_crashes strikes.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

export default {
  id: "crasher",
  async setup(ctx) {
    await ctx.command.add({
      name: "crash.noop",
      description: "never crashes by itself",
      execute: async () => "noop",
    });
    await ctx.event.subscribe(["kill.tick"], async () => {
      writeFileSync(marker("crash-attempt.txt"), "dying");
      process.exit(86);
    });
  },
};
