// PLUG-TUI-SLOT's fixture: a v2 plugin ({ id, setup }) exercising the
// three missing TUI-plugin surfaces — `ctx.ui.slot` claims (the
// generic placement path), `ctx.ui.router.register`/`navigate`, and
// `ctx.keymap.layer` (registration-time AND installed dynamically
// from an `app` claim's render, the ref stats.tsx:185-203 pattern).
// Markers land in the injected sandbox.
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
  id: "slotclaims",
  async setup(ctx) {
    // The router surface (api.tsx:204-212): a named plugin page whose
    // render receives the navigation data.
    await ctx.ui.router.register({
      name: "board",
      render: ({ data }) =>
        box([
          { tag: "#text", props: { text: "Board" } },
          { tag: "#text", props: { text: `story: ${data?.story ?? "none"}` } },
        ]),
    });

    // The slot surface (api.tsx:298-312): a sidebar claim whose rows
    // carry the plugin's own onMouseUp handlers.
    ctx.ui.slot({
      append: "sidebar.content",
      render: () =>
        box([
          { tag: "#text", props: { text: "Claims" } },
          row("target: board", () => writeFileSync(marker("claim-click.txt"), "board")),
          row("plain row"),
        ]),
    });

    // G-04: the footer faces — prompt.footer receives the
    // PromptFooterInput props (mode echoes back for the assertion),
    // home.footer/sidebar.footer render plain rows.
    ctx.ui.slot({
      append: "prompt.footer",
      render: (input) => box([row(`prompt footer: ${input?.mode ?? "none"}`)]),
    });
    ctx.ui.slot({
      append: "home.footer",
      render: () => box([row("home footer row")]),
    });
    ctx.ui.slot({
      append: "sidebar.footer",
      render: () => box([{ tag: "#text", props: { text: "Side footer" } }, row("sidebar footer row")]),
    });

    // The `app` claim (stats.tsx's pattern): its render installs a
    // keymap layer at mount — a DYNAMIC registration the host's
    // snapshot picks up after the first materialize.
    ctx.ui.slot({
      append: "app",
      render: () => {
        ctx.keymap.layer(() => ({
          mode: "global",
          commands: [
            {
              id: "claims.ping",
              title: "Ping the plugin",
              bind: "ctrl+x p",
              palette: true,
              run: () => writeFileSync(marker("keymap-run.txt"), "ping"),
            },
          ],
        }));
        return null;
      },
    });

    // A registration-time layer too (api.tsx:179-199): the open bind
    // navigates the plugin route as its run.
    await ctx.keymap.layer(() => ({
      mode: "global",
      commands: [
        {
          id: "claims.open",
          title: "Open the board",
          bind: "ctrl+x o",
          run: () => {
            writeFileSync(marker("open-run.txt"), "open");
            ctx.ui.router.navigate({ type: "plugin", name: "board", data: { story: "alpha" } });
          },
        },
      ],
    }));

    writeFileSync(marker("setup-done.txt"), "1");
  },
};
