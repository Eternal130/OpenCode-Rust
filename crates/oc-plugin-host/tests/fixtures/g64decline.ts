// G-64 decline fixture: with no ports bound, the domain reads reject
// with clear errors the guest records.
export default {
  id: "g64decline",
  async setup(ctx) {
    const sandbox = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";
    const errors = {};
    try {
      await ctx.provider.list();
    } catch (error) {
      errors.provider = String(error);
    }
    try {
      await ctx.integration.list();
    } catch (error) {
      errors.integration = String(error);
    }
    try {
      await ctx.experimental.terminal.read({ sessionID: "ses_x" });
    } catch (error) {
      errors.terminal = String(error);
    }
    await Bun.write(`${sandbox}/decline.json`, JSON.stringify(errors));
    return async () => {};
  },
};
