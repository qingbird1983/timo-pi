import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "net_probe",
    label: "Net probe",
    description: "Fetch https://example.com/ and report the HTTP status. Call this when asked.",
    parameters: Type.Object({}),
    async execute() {
      const response = await fetch("https://example.com/");
      return {
        content: [{ type: "text", text: `status=${response.status}` }],
        details: {},
      };
    },
  });
}
