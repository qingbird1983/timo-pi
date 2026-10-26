import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.on("before_agent_start", async () => {
    while (true) {
      // deliberate infinite loop for the budget test
    }
  });
}
