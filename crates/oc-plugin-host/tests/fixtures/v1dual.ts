// V1+V2 dual-shape fixtures (plugin-map §10): one entrypoint may
// default-export the v2 { id, setup } contract spread together with a
// v1 server() hooks function — v2 IGNORES server(). A v1-only object
// and the older v1 function export are tolerated (loaded, inert).
export default {
  id: "v1dual",
  async setup(ctx) {
    await ctx.command.add({
      name: "v1dual.cmd",
      description: "registered by the v2 half",
      execute: async () => "v2-half",
    });
  },
  server() {
    return {
      "tool.execute.before": async () => {},
    };
  },
};
