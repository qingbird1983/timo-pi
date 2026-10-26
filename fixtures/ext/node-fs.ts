import fs from "node:fs";

export default function (pi: { registerCommand: (name: string) => void }) {
  pi.registerCommand("never");
  void fs;
}
