// G-296 RPC schema probe: one method with JSON-Schema input/output and
// a DECLARED error, driven through every validation leg by input value:
//   n === 0 → ctx.error("boom", ..., {why})       (declared, valid data)
//   n === 1 → ctx.error("NoSuchDeclaredType", …)  (undeclared type)
//   n === 2 → {wrong: true}                       (output violation)
//   else     → {doubled: n * 2}                   (happy path)
// The handler-run marker distinguishes "never dispatched" legs.
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const SANDBOX = process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode";
const marker = () => join(SANDBOX, "rpc-schema-handler-ran.txt");

export default {
  id: "rpcschema",
  async setup(ctx) {
    writeFileSync(marker(), "0");
    await ctx.rpc.register(
      {
        id: "schema-rpc",
        methods: {
          compute: {
            input: {
              type: "object",
              properties: { n: { type: "number" } },
              required: ["n"],
              additionalProperties: false,
            },
            output: {
              type: "object",
              properties: { doubled: { type: "number" } },
              required: ["doubled"],
              additionalProperties: false,
            },
            errors: {
              boom: {
                type: "object",
                properties: { why: { type: "string" } },
                required: ["why"],
                additionalProperties: false,
              },
            },
          },
        },
        events: {},
      },
      {
        compute: async (input, call) => {
          writeFileSync(marker(), "1");
          if (input.n === 0) throw call.error("boom", "it broke", { why: "because" });
          if (input.n === 1) throw call.error("NoSuchDeclaredType", "sneaky");
          if (input.n === 2) return { wrong: true };
          return { doubled: input.n * 2 };
        },
      },
    );
    return async () => {};
  },
};
