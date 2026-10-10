// A minimal MCP stdio server for the wave-12 acceptance battery:
// newline-delimited JSON-RPC over stdin/stdout. It answers initialize
// (mirroring the client's protocolVersion), tools/list with ONE tool
// (`probe`), and tools/call with a text content result. Used to prove
// the production engine boots MCP tools behind the synthesized
// `execute` tool (codemode-default-TRUE, mcp.ts:45) with ZERO plugins.
const decoder = new TextDecoder();
const encoder = new TextEncoder();

let buffer = "";
for await (const chunk of Bun.stdin.stream()) {
  buffer += decoder.decode(chunk);
  let at;
  while ((at = buffer.indexOf("\n")) >= 0) {
    const line = buffer.slice(0, at).trim();
    buffer = buffer.slice(at + 1);
    if (line === "") continue;
    handle(JSON.parse(line));
  }
}

function send(message) {
  process.stdout.write(encoder.encode(JSON.stringify(message) + "\n"));
}

function handle(message) {
  if (message.method === "initialize") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        protocolVersion: message.params?.protocolVersion ?? "2025-06-18",
        capabilities: { tools: { listChanged: false } },
        serverInfo: { name: "fixture-probe", version: "1.0.0" },
        instructions: "A fixture MCP server.",
      },
    });
    return;
  }
  if (message.method === "notifications/initialized") return; // notification
  if (message.method === "tools/list") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        tools: [
          {
            name: "probe",
            description: "Probe the fixture once.",
            inputSchema: {
              type: "object",
              properties: { note: { type: "string", description: "A note" } },
            },
          },
        ],
      },
    });
    return;
  }
  if (message.method === "tools/call") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: { content: [{ type: "text", text: "probed" }], isError: false },
    });
    return;
  }
  if (message.id !== undefined) {
    send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "unknown method" } });
  }
}
