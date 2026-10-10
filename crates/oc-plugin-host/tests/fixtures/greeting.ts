// Corpus fixture (plugin.test.ts "loads a local plugin with its
// configured options"): the command description comes from
// ctx.options.description — config-sourced plugin options.
export default {
  id: "greeting",
  async setup(ctx) {
    await ctx.command.transform((editor) =>
      editor.add({
        name: "greet",
        description: ctx.options.description,
        async execute() {},
      }),
    );
    return async () => {};
  },
};
