// The wave-1 acceptance fixture: a v2 plugin ({ id, setup }) that
// registers a command (with an owned keymap entry), a theme document,
// and subscribes to one event type. Markers land in the sandbox the
// test injects via OC_PLUGIN_TEST_SANDBOX.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

export const VERSION = "v1";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

export default {
  id: "helloworld",
  async setup(ctx) {
    await ctx.command.add({
      name: "hw.hello",
      description: `Say hello from the helloworld plugin (${VERSION})`,
      keybinds: ["ctrl+x h"],
      execute: async () => {
        writeFileSync(marker("executed.txt"), `hello ${VERSION}`);
        return `hello ${VERSION}`;
      },
    });
    await ctx.command.add({
      name: "hw.hidden",
      description: "Not in the palette",
      palette: false,
      execute: async () => "hidden",
    });
    await ctx.theme.register({
      name: "helloworld-theme",
      document: {"$schema":"https://opencode.ai/theme.json","defs":{"darkBg":"#0f0f0f","darkBgPanel":"#15141b","darkBorder":"#2d2d2d","darkFgMuted":"#6d6d6d","darkFg":"#edecee","purple":"#a277ff","pink":"#f694ff","blue":"#82e2ff","red":"#ff6767","orange":"#ffca85","cyan":"#61ffca","green":"#9dff65"},"theme":{"primary":"#3b82f6","secondary":"pink","accent":"purple","error":"red","warning":"orange","success":"cyan","info":"purple","text":"darkFg","textMuted":"darkFgMuted","background":"darkBg","backgroundPanel":"darkBgPanel","backgroundElement":"darkBgPanel","border":"darkBorder","borderActive":"darkFgMuted","borderSubtle":"darkBorder","diffAdded":"cyan","diffRemoved":"red","diffContext":"darkFgMuted","diffHunkHeader":"darkFgMuted","diffHighlightAdded":"cyan","diffHighlightRemoved":"red","diffAddedBg":"#354933","diffRemovedBg":"#3f191a","diffContextBg":"darkBgPanel","diffLineNumber":"#898989","diffAddedLineNumberBg":"#162620","diffRemovedLineNumberBg":"#26161a","markdownText":"darkFg","markdownHeading":"purple","markdownLink":"pink","markdownLinkText":"purple","markdownCode":"cyan","markdownBlockQuote":"darkFgMuted","markdownEmph":"orange","markdownStrong":"purple","markdownHorizontalRule":"darkFgMuted","markdownListItem":"purple","markdownListEnumeration":"purple","markdownImage":"pink","markdownImageText":"purple","markdownCodeBlock":"darkFg","syntaxComment":"darkFgMuted","syntaxKeyword":"pink","syntaxFunction":"purple","syntaxVariable":"purple","syntaxString":"cyan","syntaxNumber":"green","syntaxType":"purple","syntaxOperator":"pink","syntaxPunctuation":"darkFg"}},
    });
    await ctx.event.subscribe(["tui.test"], async (event) => {
      writeFileSync(marker("event.txt"), `${event.type}:${event.data?.payload ?? ""}`);
    });
  },
};
