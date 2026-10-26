import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "tui_probe",
    label: "TUI probe",
    description: "Touch TUI-only members and return ok. Call this when asked.",
    parameters: Type.Object({}),
    async execute(_id, _params, _signal, _update, ctx) {
      ctx.ui.setWidget("timo-demo", []);
      ctx.ui.setFooter(() => "demo");
      ctx.ui.custom(() => undefined);
      return {
        content: [{ type: "text", text: "ui-noop-ok" }],
        details: {},
      };
    },
  });
}
