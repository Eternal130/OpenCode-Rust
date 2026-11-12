// G-239/G-331's fixture: the session.panel activation — a
// `session.panel` claim rendering the PanelInput props as rows, plus
// keymap-layer commands driving `ctx.ui.panel.open/close/current`
// (context.ts:501-508). Markers land in the injected sandbox.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

function row(text, onMouseUp) {
  const props = onMouseUp ? { onMouseUp } : {};
  return { props, children: [{ tag: "#text", props: { text } }] };
}

/** Every container node needs a props object (the flattener reads
 *  node.props.flexDirection on every box). */
function box(children) {
  return { props: {}, children };
}

export default {
  id: "panelclaim",
  async setup(ctx) {
    // The panel claim (context.ts:199 — `session.panel`'s input is
    // PanelInput): the render echoes the reactive props back so the
    // materialized rows assert them.
    ctx.ui.slot({
      append: "session.panel",
      render: (input) =>
        box([
          { tag: "#text", props: { text: "Panel" } },
          row(`name: ${input?.name ?? "none"}`),
          row(`focus: ${input?.focused ? "yes" : "no"}`, () =>
            writeFileSync(marker("panel-click.txt"), "row"),
          ),
          row(`width: ${input?.width ?? 0}`),
          row(`presentation: ${input?.presentation ?? "none"}`),
        ]),
    });

    // The keymap layer (api.tsx:179-199): open/close/current drive
    // the host face — the open resolves the route-check boolean, the
    // close is the owner-scoped release.
    await ctx.keymap.layer(() => ({
      mode: "global",
      commands: [
        {
          id: "panel.open",
          title: "Open the panel",
          bind: "f9",
          palette: true,
          run: async () => {
            const ok = await ctx.ui.panel.open("board");
            writeFileSync(marker("panel-open.txt"), String(ok));
          },
        },
        {
          id: "panel.close",
          title: "Close the panel",
          bind: "f8",
          palette: true,
          run: async () => {
            const current = ctx.ui.panel.current();
            writeFileSync(marker("panel-current.txt"), JSON.stringify(current ?? null));
            await ctx.ui.panel.close();
          },
        },
      ],
    }));

    writeFileSync(marker("setup-done.txt"), "1");
  },
};
