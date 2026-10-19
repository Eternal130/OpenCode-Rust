// PLUG-D33's fixture: a v2 plugin whose setup declares an MCP server
// through `ctx.mcp.transform` (the whole-table editor seam —
// packages/core/src/plugin/host.ts:374-403). The host materializes
// the table into registrations.mcps; the TUI's registry sync carries
// it into the REAL server-side registry over the wire. The config is
// a v2 `Mcp.ServerConfig` (local stdio server — the command is a
// harmless cat; tests never let it connect for real: the scripted
// listener answers the PUT).
import { writeFileSync } from "node:fs";
import { join } from "node:path";

function marker(name) {
  return join(process.env.OC_PLUGIN_TEST_SANDBOX ?? "/tmp/opencode", name);
}

export default {
  id: "mcpreg",
  async setup(ctx) {
    await ctx.mcp.transform((editors) => {
      editors.set("plugindemo", {
        type: "local",
        command: ["cat"],
        environment: { "OC_PLUGIN_MCP": "1" },
      });
    });
    writeFileSync(marker("mcp-setup.txt"), "1");
  },
};
