// A minimal MCP stdio server for the wave-12 acceptance battery:
// newline-delimited JSON-RPC over stdin/stdout. It answers initialize
// (mirroring the client's protocolVersion), tools/list with ONE tool
// (`probe`), and tools/call with a text content result. Used to prove
// the production engine boots MCP tools behind the synthesized
// `execute` tool (codemode-default-TRUE, mcp.ts:45) with ZERO plugins.
const decoder = new TextDecoder();
const encoder = new TextEncoder();

let buffer = "";
let promptsListCalls = 0;
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
        capabilities: { tools: { listChanged: false }, resources: { subscribe: false, listChanged: false }, prompts: { listChanged: true } },
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
  if (message.method === "resources/list") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        resources: [
          { uri: "fixture://doc/guide", name: "guide", description: "The guide resource", mimeType: "text/plain" },
        ],
      },
    });
    return;
  }
  if (message.method === "resources/templates/list") {
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        resourceTemplates: [
          { uriTemplate: "fixture://doc/{name}", name: "doc", description: "Any doc", mimeType: "text/plain" },
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
  // G-215 — prompts surface: the FIRST list carries one prompt and then
  // announces list_changed; every later list adds a second prompt (the
  // client refetches), proving the prompts.changed → command reload
  // chain end to end.
  if (message.method === "prompts/list") {
    promptsListCalls++
    const prompts =
      promptsListCalls === 1
        ? [
            {
              name: "code review",
              description: "Review the code",
              arguments: [{ name: "who", description: "Who to greet", required: false }],
            },
          ]
        : [
            {
              name: "code review",
              description: "Review the code",
              arguments: [{ name: "who", description: "Who to greet", required: false }],
            },
            { name: "deploy", description: "Ship it", arguments: [] },
          ]
    send({ jsonrpc: "2.0", id: message.id, result: { prompts } })
    if (promptsListCalls === 1) {
      send({ jsonrpc: "2.0", method: "notifications/prompts/list_changed" })
    }
    return;
  }
  if (message.method === "prompts/get") {
    const who = message.params?.arguments?.who ?? ""
    send({
      jsonrpc: "2.0",
      id: message.id,
      result: {
        description: "A greeting",
        messages: [{ role: "user", content: { type: "text", text: "hello " + who } }],
      },
    })
    return;
  }
  if (message.id !== undefined) {
    send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "unknown method" } });
  }
}
