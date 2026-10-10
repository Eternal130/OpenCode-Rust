// The deadlock/reentrancy storm fixture: setup registers 30 commands
// through 30 NESTED guest→host requests (each answered while the
// supervisor's `load` reply is outstanding); every event handler
// issues a nested sync `ping` before finishing; command execution
// nests five more.
export default {
  id: "storm",
  async setup(ctx) {
    for (let i = 0; i < 30; i++) {
      // eslint-disable-next-line no-await-in-loop -- nesting is the point
      await ctx.command.add({
        name: `storm.${i}`,
        description: `storm command ${i}`,
        execute: async () => {
          let value = 0;
          for (let j = 0; j < 5; j++) {
            // eslint-disable-next-line no-await-in-loop -- nesting is the point
            await ctx.ping();
            value += j;
          }
          return `storm-${value}`;
        },
      });
    }
    await ctx.event.subscribe(["storm.tick"], async (event) => {
      await ctx.ping(); // nested sync RPC while the dispatch reply is owed
    });
    await ctx.event.subscribe(["storm.echo"], async (event) => {
      await ctx.ping();
    });
  },
};
