// G-379.7 — the callee side: one plugin registers an RPC definition
// with a doubling method, a declared-error method (schema'd), and an
// input-schema'd method (the invalid-input gate).
export default {
  id: "rpcserver",
  async setup(ctx) {
    await ctx.rpc.register(
      {
        id: "math-rpc",
        methods: {
          double: {
            input: { type: "object", properties: { n: { type: "number" } }, required: ["n"] },
            output: { type: "object", properties: { value: { type: "number" } }, required: ["value"] },
          },
          fail: {
            input: { type: "object" },
            output: { type: "object" },
            errors: {
              boom: { type: "object", properties: { code: { type: "number" } }, required: ["code"] },
            },
          },
        },
        events: {},
      },
      {
        double: async (input) => ({ value: input.n * 2 }),
        fail: async (_input, call) => {
          throw call.error("boom", "the declared failure", { code: 7 });
        },
      },
    );
    return async () => {};
  },
};
