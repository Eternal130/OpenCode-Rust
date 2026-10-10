// Wave-4 codemode e2e fixture: registers one code-mode tool (the
// default) and one direct tool through ctx.tool.transform, keeping the
// guest-side `execute` for host dispatch.
export default {
  id: "codetool",
  async setup(ctx) {
    await ctx.tool.transform((editor) => {
      editor.add({
        name: "greet",
        description: "Greet someone by name",
        input: {
          type: "object",
          properties: { name: { type: "string", description: "Who to greet" } },
          required: ["name"],
        },
        async execute(input) {
          return `hello ${input?.name ?? "world"}`;
        },
      });
      editor.add({
        name: "direct_probe",
        description: "A direct (codemode:false) probe tool",
        input: { type: "object", properties: {} },
        options: { codemode: false },
        async execute() {
          return "direct";
        },
      });
      editor.add({
        name: "boom",
        description: "Always fails",
        input: { type: "object", properties: {} },
        async execute() {
          throw new Error("tool kaboom");
        },
      });
    });
    // A command that submits a REAL session prompt through the session
    // port (the concurrency probe in tests/codemode.rs).
    await ctx.command.add({
      name: "codetool.ask",
      description: "Prompt the session engine",
      async execute() {
        await ctx.session.prompt({ sessionID: "ses_codemode_e2e", text: "from codetool" });
        return "asked";
      },
    });
    ctx.log("codetool: registered greet/direct_probe/boom");
    return async () => {};
  },
};
