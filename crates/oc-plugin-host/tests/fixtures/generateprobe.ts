// Wave-4 context-generalization fixture: exercises ctx.generate.text
// (the flagship probes it and degrades without it — here we prove the
// wired domain round-trips through the session port).
export default {
  id: "generateprobe",
  async setup(ctx) {
    await ctx.command.add({
      name: "genprobe.ask",
      description: "One-off generation through the engine seam",
      async execute() {
        const text = await ctx.generate.text({
          prompt: "summarize: wave 4",
          model: { providerID: "zhipuai", modelID: "glm-5.3" },
        });
        const fs = await import("node:fs");
        fs.writeFileSync(
          `${process.env.OC_PLUGIN_TEST_SANDBOX}/generate-out.txt`,
          String(text),
        );
        return "generated";
      },
    });
    ctx.log("generateprobe: armed");
    return async () => {};
  },
};
