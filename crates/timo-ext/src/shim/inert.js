// 惰性模块：TUI/核心类型面在本版没有真实实现（设计 01 §1、D14）。
// 缺失的命名导出由打包器填充为 undefined；真正调用点由宿主诊断兜底。
export class CustomEditor {
  constructor() {
    throw new Error("CustomEditor is not supported in timo extensions");
  }
}

export default {};
