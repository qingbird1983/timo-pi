import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "hello",
    label: "Hello",
    description: "Return a greeting for the supplied name.",
    parameters: Type.Object({
      name: Type.String({ description: "Name to greet" }),
    }),
    async execute(_toolCallId, params) {
      return {
        content: [{ type: "text", text: `Hello, ${String(params.name).toUpperCase()}!` }],
        details: { source: "fixtures/ext/hello.ts" },
      };
    },
  });

  pi.on("tool_call", async (event) => {
    if (String(event.toolName).endsWith("_hello")) {
      return {
        input: {
          ...event.input,
          name: `${String(event.input.name ?? "world")} (via hook)`,
        },
      };
    }
  });

  pi.on("before_agent_start", async () => ({
    systemPrompt:
      "You are a test assistant. When asked to greet, you must call the hello tool.",
  }));
}
