// pi 兼容 shim：只提供扩展实际运行时需要的导出（设计 01 §1/§3）。
export const VERSION = "1.4.0";

export function defineTool(tool) {
  return tool;
}

export function isToolCallEventType(toolName, event) {
  return Boolean(event) && event.toolName === toolName;
}

export function isBashToolResult(event) {
  return Boolean(event) && event.toolName === "bash";
}
export function isPowerShellToolResult(event) {
  return Boolean(event) && event.toolName === "powershell";
}
export function isReadToolResult(event) {
  return Boolean(event) && event.toolName === "read";
}
export function isEditToolResult(event) {
  return Boolean(event) && event.toolName === "edit";
}
export function isWriteToolResult(event) {
  return Boolean(event) && event.toolName === "write";
}
export function isGrepToolResult(event) {
  return Boolean(event) && event.toolName === "grep";
}
export function isFindToolResult(event) {
  return Boolean(event) && event.toolName === "find";
}
export function isLsToolResult(event) {
  return Boolean(event) && event.toolName === "ls";
}

export class CustomEditor {
  constructor() {
    throw new Error("CustomEditor is not supported in timo extensions");
  }
}

export default {
  VERSION,
  defineTool,
  isToolCallEventType,
  CustomEditor,
};
