// G-295 rpc events probe: an RPC definition with a DECLARED event plus
// commands that emit declared / undeclared event names.
import { join } from "node:path";

export default {
  id: "rpcevents",
  async setup(ctx) {
    const registration = await ctx.rpc.register(
      { id: "evt-rpc", methods: {}, events: { changed: {} } },
      {},
    );
    await ctx.command.add({
      name: "rpcev.poke",
      description: "emit a declared rpc event",
      async execute() {
        await registration.events.emit("changed", { answer: 42 });
        return "emitted";
      },
    });
    await ctx.command.add({
      name: "rpcev.bad",
      description: "emit an undeclared rpc event",
      async execute() {
        try {
          await registration.events.emit("nope", { x: 1 });
          return "no-error";
        } catch (error) {
          return `rejected:${error instanceof Error ? error.message : String(error)}`;
        }
      },
    });
    return async () => {};
  },
};
