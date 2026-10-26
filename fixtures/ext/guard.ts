import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", async (event, ctx) => {
    const command = String(event.input?.command ?? "");
    if (event.toolName === "bash" && /rm\s+-rf/.test(command)) {
      ctx.ui.notify("guard: blocked a destructive command", "warning");
      return { block: true, reason: "destructive command blocked by guard.ts" };
    }
  });
}
