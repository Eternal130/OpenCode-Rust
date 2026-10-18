// SA-F16 fixture: a plugin whose PERIODIC callback probes an
// unreadable path (the EACCES class the scan observed: /lost+found,
// /tmp/tmux-0). Before the host's timer shields this escaped as a
// process-level uncaughtException; with them it degrades to a
// recorded plugin fault and the guest stays healthy.
import { readFileSync, writeFileSync, chmodSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const marker = join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", "timercrasher");
mkdirSync(marker, { recursive: true });
const unreadable = join(marker, "probe-deny.txt");

export default {
  id: "timercrasher",
  async setup(ctx) {
    writeFileSync(unreadable, "secret");
    chmodSync(unreadable, 0o000);
    // The periodic probe: throws EACCES every fire (sync) and once
    // asynchronously (the rejected promise arm).
    let fired = 0;
    setInterval(() => {
      fired += 1;
      readFileSync(unreadable, "utf8"); // EACCES (mode 000)
      writeFileSync(join(marker, "never.txt"), String(fired));
    }, 60);
    setInterval(async () => {
      await Promise.reject(new Error("async probe failed (EACCES-class)"));
    }, 90);
    if (ctx.command) {
      await ctx.command.add({
        name: "timercrasher.ping",
        description: "Liveness probe",
        execute: async () => `alive-after-${fired}`,
      });
    }
    writeFileSync(join(marker, "setup-done.txt"), "1");
  },
};
