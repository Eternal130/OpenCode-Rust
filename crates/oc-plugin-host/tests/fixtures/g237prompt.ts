// G-237 — the ctx.ui.prompt fixture: each `tui.poke` round opens one
// prompt; the host resolves the first with a typed value and dismisses
// the second (null). The resolved values land as JSON markers in the
// injected sandbox.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

export default {
  id: "g237prompt",
  async setup(ctx) {
    await ctx.event.subscribe(["tui.poke"], async (event) => {
      const round = String(event.data?.round ?? "0");
      const value = await ctx.ui.prompt({
        title: "Name the preset",
        description: "One line of description",
        placeholder: "preset name",
        value: "seed-value",
      });
      writeFileSync(marker(`prompt-${round}.json`), JSON.stringify(value));
    });
  },
};
