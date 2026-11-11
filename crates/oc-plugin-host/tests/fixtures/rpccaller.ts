// G-379.7 — the caller side: a SECOND plugin invokes the rpc the
// first one registered through the guest client face
// (`ctx.rpc.call` / `ctx.rpc.client(definition)`). Every command
// returns a JSON result string the test asserts on.
export default {
  id: "rpccaller",
  async setup(ctx) {
    const describe = async (promise) => {
      try {
        const output = await promise;
        return { ok: true, output };
      } catch (error) {
        return {
          ok: false,
          type: error && typeof error.type === "string" ? error.type : String(error),
          message: error instanceof Error ? error.message : String(error),
          data: error && typeof error.data !== "undefined" ? error.data : null,
        };
      }
    };
    const definition = {
      id: "math-rpc",
      methods: {
        double: {
          input: { type: "object", properties: { n: { type: "number" } }, required: ["n"] },
          output: { type: "object", properties: { value: { type: "number" } }, required: ["value"] },
        },
      },
      events: {},
    };
    await ctx.command.add({
      name: "rpcc.call.ok",
      description: "ctx.rpc.call across plugins",
      async execute() {
        return JSON.stringify(await describe(ctx.rpc.call("math-rpc", "double", { n: 21 })));
      },
    });
    await ctx.command.add({
      name: "rpcc.client.ok",
      description: "ctx.rpc.client(definition) typed method face",
      async execute() {
        const client = ctx.rpc.client(definition);
        return JSON.stringify(await describe(client.double({ n: 2 })));
      },
    });
    await ctx.command.add({
      name: "rpcc.call.declared-error",
      description: "a declared failure surfaces typed",
      async execute() {
        return JSON.stringify(await describe(ctx.rpc.call("math-rpc", "fail", {})));
      },
    });
    await ctx.command.add({
      name: "rpcc.call.invalid-input",
      description: "a failing input never reaches the handler",
      async execute() {
        return JSON.stringify(await describe(ctx.rpc.call("math-rpc", "double", { wrong: true })));
      },
    });
    await ctx.command.add({
      name: "rpcc.call.unknown-method",
      description: "an undeclared method is rpc.method_not_found",
      async execute() {
        return JSON.stringify(await describe(ctx.rpc.call("math-rpc", "triple", { n: 1 })));
      },
    });
    await ctx.command.add({
      name: "rpcc.call.unavailable",
      description: "an unregistered id is rpc.unavailable",
      async execute() {
        return JSON.stringify(await describe(ctx.rpc.call("no-such-rpc", "double", { n: 1 })));
      },
    });
    return async () => {};
  },
};
