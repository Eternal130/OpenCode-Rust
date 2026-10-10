// The wave-3 TUI slot fixture: a dialog provider whose build never
// settles — proves the materialize HARD timeout (a hung plugin costs
// exactly its own call).
export default {
  id: "slowdialog",
  async setup(ctx) {
    await ctx.ui.dialog.register({
      id: "slow",
      build: async () => {
        await new Promise((resolve) => setTimeout(resolve, 60_000));
        return { title: "never", rows: [], options: [] };
      },
    });
  },
};
