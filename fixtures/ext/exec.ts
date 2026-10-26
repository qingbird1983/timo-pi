import { Type } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.registerTool({
    name: "exec_probe",
    label: "Exec probe",
    description: "Run a harmless command and return stdout. Call this when asked.",
    parameters: Type.Object({}),
    async execute() {
      const out = await pi.exec("cmd", ["/C", "echo timo-exec-ok"]);
      return {
        content: [{ type: "text", text: `stdout=${out.stdout.trim()} code=${out.code}` }],
        details: {},
      };
    },
  });
}
