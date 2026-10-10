// Say hello from the helloworld plugin (a duplicate of the
// oc-plugin-host fixture so this test owns its copy; markers land in
// the sandbox injected via OC_PLUGIN_TEST_SANDBOX).
import { writeFileSync } from "node:fs";
import { join } from "node:path";

export default {
  id: "helloworld",
  async setup(ctx) {
    await ctx.command.add({
      name: "hw.hello",
      description: "Say hello from the helloworld plugin",
      keybinds: ["ctrl+x h"],
      execute: async () => {
        writeFileSync(join(process.env.OC_PLUGIN_TEST_SANDBOX, "tui-executed.txt"), "ran");
        return "hello";
      },
    });
    await ctx.theme.register({
      name: "helloworld-theme",
      document: {"$schema":"https://opencode.ai/theme.json","defs":{"darkBg":"#0f0f0f","darkBgPanel":"#15141b","darkBorder":"#2d2d2d","darkFgMuted":"#6d6d6d","darkFg":"#edecee","purple":"#a277ff","pink":"#f694ff","blue":"#82e2ff","red":"#ff6767","orange":"#ffca85","cyan":"#61ffca","green":"#9dff65"},"theme":{"primary":"#3b82f6","secondary":"pink","accent":"purple","error":"red","warning":"orange","success":"cyan","info":"purple","text":"darkFg","textMuted":"darkFgMuted","background":"darkBg","backgroundPanel":"darkBgPanel","backgroundElement":"darkBgPanel","border":"darkBorder","borderActive":"darkFgMuted","borderSubtle":"darkBorder","diffAdded":"cyan","diffRemoved":"red","diffContext":"darkFgMuted","diffHunkHeader":"darkFgMuted","diffHighlightAdded":"cyan","diffHighlightRemoved":"red","diffAddedBg":"#354933","diffRemovedBg":"#3f191a","diffContextBg":"darkBgPanel","diffLineNumber":"#898989","diffAddedLineNumberBg":"#162620","diffRemovedLineNumberBg":"#26161a","markdownText":"darkFg","markdownHeading":"purple","markdownLink":"pink","markdownLinkText":"purple","markdownCode":"cyan","markdownBlockQuote":"darkFgMuted","markdownEmph":"orange","markdownStrong":"purple","markdownHorizontalRule":"darkFgMuted","markdownListItem":"purple","markdownListEnumeration":"purple","markdownImage":"pink","markdownImageText":"purple","markdownCodeBlock":"darkFg","syntaxComment":"darkFgMuted","syntaxKeyword":"pink","syntaxFunction":"purple","syntaxVariable":"purple","syntaxString":"cyan","syntaxNumber":"green","syntaxType":"purple","syntaxOperator":"pink","syntaxPunctuation":"darkFg"}},
    });
    await ctx.event.subscribe(["tui.test"], async (event) => {
      writeFileSync(
        join(process.env.OC_PLUGIN_TEST_SANDBOX, "tui-event.txt"),
        `${event.type}:${event.data?.payload ?? ""}`,
      );
    });
  },
};
