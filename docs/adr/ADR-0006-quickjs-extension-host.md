# ADR-0006：扩展宿主采用内嵌 QuickJS + rolldown 安装期打包

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0001、ADR-0003；research/02、research/03

## 背景

A 轨编排兼容需要在 Rust 进程内执行 TypeScript 扩展。pi 靠 Node 的 jiti 免费获得该能力；本项目无 Node。JS 引擎候选：QuickJS（轻、NSH 已内置、LLRT 子集 stdlib）与 deno_core/V8（重、npm 支持好）。

## 决策

- 引擎：**QuickJS**（与 NSH 同款 quickjs-jit 栈）。
- 加载模型：**安装期打包**——扩展安装时用 **rolldown/oxc** 把 `.ts` 源码连同其 npm 依赖打包成单文件 bundle，运行期由 QuickJS 直接执行。
- 宿主：Rust 侧实现 pi 兼容 shim（`pi.registerTool/registerCommand/on(...)` 等，按 ADR-0003 矩阵），TypeBox schema 作为 JSON Schema 传给 Rust 侧校验。

## 理由

- 打包期解决依赖解析，绕开 QuickJS 无 node_modules 解析的短板；pi 官方主流编排扩展多为无依赖单文件（research/02 §4），覆盖面足够。
- QuickJS 与 NSH 共栈，一套基础设施服务 A/B 两轨；启动快、内存小、可强沙箱。
- deno_core 可换得更宽兼容面，但引入 V8 体积与复杂度，与"轻量原生"目标冲突——列为备选升级路径而非默认。

## 后果

- 直接使用 `node:fs`/`child_process` 等内建模块的扩展不可用（能力须经宿主显式注入）；重 npm 依赖扩展可能打包失败——均计入诊断，不静默。
- TS 语法支持范围由 oxc 决定（成熟覆盖日常语法）；扩展不能用 Node 专属 API（`node:*`）。
- 钩子语义工程（30s 预算、payload detach 拷贝、abort 退休）为 Phase 2 硬性交付项。
