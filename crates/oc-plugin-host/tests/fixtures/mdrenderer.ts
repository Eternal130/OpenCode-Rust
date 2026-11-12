// G-332's fixture: the plugin markdown code-block renderer surface
// (`ctx.markdown.registerCodeBlockRenderer`, api.tsx:168-178). One
// renderer for a fixture language whose flattened rows echo the
// render inputs; the blank-language and intra-plugin-duplicate error
// texts land in marker files (the guest-side throws — ref-exact).
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

function row(text) {
  return { props: {}, children: [{ tag: "#text", props: { text } }] };
}

/** Every container node needs a props object (the flattener reads
 *  node.props.flexDirection on every box). */
function box(children) {
  return { props: {}, children };
}

export default {
  id: "mdrenderer",
  async setup(ctx) {
    // Blank/whitespace language: ref-exact error, no registration.
    let blankError = "";
    try {
      ctx.markdown.registerCodeBlockRenderer("   ", () => null);
    } catch (error) {
      blankError = String(error?.message ?? error);
    }
    writeFileSync(marker("md-blank-error.txt"), blankError);

    // The real renderer (rows echo the render inputs).
    const render = ({ code, language }) =>
      box([
        row(`worm: ${language}`),
        row(`len: ${code.length}`),
        row(`up: ${code.toUpperCase()}`),
      ]);
    const dispose = ctx.markdown.registerCodeBlockRenderer("wormlang", render);

    // Intra-plugin duplicate: ref-exact error naming the language.
    let dupError = "";
    try {
      ctx.markdown.registerCodeBlockRenderer("wormlang", () => null);
    } catch (error) {
      dupError = String(error?.message ?? error);
    }
    writeFileSync(marker("md-dup-error.txt"), dupError);

    // The dispose closure is idempotent and unregisters; a fresh
    // registration for the same language succeeds afterwards.
    dispose();
    dispose();
    ctx.markdown.registerCodeBlockRenderer("wormlang", render);

    writeFileSync(marker("setup-done.txt"), "1");
  },
};
