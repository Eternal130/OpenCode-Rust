// The wave-3 TUI slot fixture: a v2 plugin ({ id, setup }) that fills
// every slot seam — a dialog provider (materialize-at-open), a group
// rule, a home quick action (all registration-time), and — on the
// `tui.poke` event — a toast, a sidebar section, a footer hint and a
// fully-built dialog open (runtime UI requests). Markers land in the
// injected sandbox.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

export default {
  id: "tuislots",
  async setup(ctx) {
    // Registration-time: palette command with a leader chord, group
    // rule, home quick action, dialog provider.
    await ctx.command.add({
      name: "slots.open",
      description: "Open the plugin dialog",
      keybinds: ["ctrl+x j"],
      execute: async () => {
        writeFileSync(marker("opened.txt"), "palette");
        return "opened";
      },
    });
    await ctx.ui.grouping.addRule({ toolPrefix: "omo_", header: "OMO tools" });
    await ctx.ui.grouping.addRule({ tool: "specialprobe", header: "Probe" });
    await ctx.ui.home.addQuickAction({ label: "/slots", command: "slots.open" });
    const registration = await ctx.ui.dialog.register({
      id: "picker",
      build: () => ({
        title: "Slot picker",
        rows: [],
        options: [
          { title: "First", value: "slots.open", description: "the palette command" },
          { title: "Second", value: "slots.other", description: "" },
        ],
      }),
    });

    // Runtime: on tui.poke fill every dynamic slot.
    await ctx.event.subscribe(["tui.poke"], async (event) => {
      writeFileSync(marker("poked.txt"), String(event.data?.payload ?? ""));
      await ctx.ui.toast.show({
        title: "Slots",
        message: "poked by the fixture",
        variant: "success",
        duration: 1234,
      });
      await ctx.ui.sidebar.setSection({
        title: "Fixture",
        rows: ["preset: zhipuai", "sessions: 7"],
      });
      await ctx.ui.footer.setHint({ text: "slots on" });
      await ctx.ui.dialog.open({
        title: "Poked",
        rows: ["row one", "row two"],
        options: [],
      });
      writeFileSync(marker("registration.txt"), String(registration.id));
    });
  },
};
