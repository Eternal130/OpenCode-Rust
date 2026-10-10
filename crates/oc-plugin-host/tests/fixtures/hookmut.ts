// Corpus fixture (hook mutation ordering): each copy derives its TAG
// from its own file name (the test stages hookmut-a.ts /
// hookmut-b.ts), so both plugins load in ONE guest process and their
// session.context mutations must compose in registration (entry)
// order. The `ask` command drives a prompt through the session port —
// the port dispatches the context hook chain exactly like the engine.
import { basename, extname } from "node:path";
const TAG = basename(import.meta.url.replace(/^file:/, ""), extname(import.meta.url))
  .replace(/^hookmut-?/, "")
  .toUpperCase() || "A";

export default {
  id: `hookmut-${TAG.toLowerCase()}`,
  async setup(ctx) {
    await ctx.session.hook("context", async (event) => {
      const messages = event?.messages ?? [];
      const last = messages[messages.length - 1];
      if (last && Array.isArray(last.content) && last.content[0]) {
        last.content[0].text += ` [mutated-by-${TAG}]`;
      }
    });
    await ctx.command.add({
      name: `hookmut-${TAG.toLowerCase()}.ask`,
      description: "Prompt through the hook chain",
      async execute() {
        await ctx.session.prompt({ sessionID: "ses_corpus", text: "original text" });
        return "asked";
      },
    });
    ctx.log(`hookmut ${TAG}: armed`);
    return async () => {};
  },
};
