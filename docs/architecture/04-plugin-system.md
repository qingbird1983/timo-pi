# 架构 04：插件系统设计（双轨制）

- 状态：定稿（A 轨矩阵定稿；B 轨为框架级定稿，Phase 4 开工前出详细设计并定稿）
- 日期：2026-10-27（2026-10-26 初稿；2026-10-27 按实现校正命名空间写法与 §1.1 实现清单；同日第二轮：`registerVirtualModel` 落地）
- 关联：ADR-0003/0004/0006；research/02、research/04

## 0. 双轨总览

| | A 轨：编排兼容（跑 pi 扩展） | B 轨：GUI 原生插件（本项目自有生态） |
|---|---|---|
| 目标 | 复用 pi 现有"不碰 UI"的扩展价值 | 面向 GUI 特性的插件生态（自有契约） |
| 形态 | pi 格式 `.ts` 扩展文件 | manifest + TSX + 签名包 |
| 运行时 | QuickJS（打包后 bundle） | QuickJS（NSH 栈） |
| 界面能力 | 无（TUI 成员 no-op + 诊断） | TSX slot（沙箱渲染进 gpui-kit 窗口） |
| 交付阶段 | Phase 2 | Phase 4 |

## 1. A 轨：三分类兼容矩阵（兼容契约，本表即承诺）

### 1.1 支持的成员（真实实现）

| 分类 | 成员 |
|---|---|
| 注册 | `registerTool`、`registerCommand`、`registerFlag`、`registerVirtualModel`/`unregisterVirtualModel`（真实实现；虚拟条目由 `models.json` 声明，注册只提供路由函数——设计 10 D69）；`registerProvider`/`unregisterProvider`、`registerMcpServer`、`events` 总线 → **no-op + 诊断，见 §1.2** |
| 事件钩子 | `on()`：before_agent_start（含 systemPrompt 替换）、context、before_provider_request/headers、after_provider_response、tool_call（veto/改参）、tool_result（改写）、agent_start/end、turn_start/end、session_start、compact、error |
| 动作 | `exec`、`sendUserMessage`、`setActiveTools`/`getAllTools`/`getActiveTools`、`setModel`、`getThinkingLevel`/`setThinkingLevel`、`getSettings`、`getCommands`、`ctx.modelRegistry.find`/`getModels`/`getAllModels`（目录快照） |
| 工具定义 | TypeBox/JSON Schema 参数、execute、outputSchema、exposure、namespace、annotations、promptSnippet/promptGuidelines |

### 1.2 缓做（deferred v2，成员存在、返回"未实现"诊断）

`sendMessage`、`appendEntry`、sessionManager 读接口、`switchSession`/`newSession`/`fork`、`registerShortcut`、markdown transformer、editor 读写、autocomplete。

> **2026-10-27 修订（逐项状态按实现校正；当时口径，已被下一条部分覆盖）**：`registerProvider`/`unregisterProvider`、`registerMcpServer`、`registerVirtualModel`、`events` 总线**不在"真实实现"之列**——当前为 no-op + 诊断（`E202`），且 `route_model` 恒返回空；`registerVirtualModel` 的"受限支持"亦未落地。已从 §1.1 移入本口径。详见 `docs/design/phase-2/report.md` §4 与 `docs/roadmap/audit-debt.md`。
>
> **2026-10-27 第二轮修订（P2 第二题 B2）**：`registerVirtualModel`/`unregisterVirtualModel` 已实装真实路由（JS `route(request, ctx)` 每次请求前同步调用，reason ∈ user/continuation/retry/direct），`ctx.modelRegistry` 提供目录快照；`route_model` 不再是恒空占位。`registerProvider`/`registerMcpServer`/`events` 总线仍为 no-op + 诊断。契约见 `docs/design/phase-3.5/10-provider-composer.md` §4.2。

### 1.3 不支持（TUI 面：成员存在但 no-op，永不 throw，每扩展每成员一条诊断）

`ctx.ui` 全部（select/confirm/input/notify 之外的自定义 UI、setWidget/setFooter/setHeader/setTitle/onTerminalInput/pasteToEditor/setEditorComponent、`custom<T>()`）、`registerMessageRenderer`/`registerEntryRenderer`/`registerMarkdownTransformer`/`registerToolRenderer`、工具的 `renderCall`/`renderResult`、`navigateTree`、`shutdown`。

> 例外：`ctx.ui.notify`/`select`/`confirm`/`input` 属于通用对话框语义，映射到当前前端的对应实现（CLI 用简单文本交互、GUI 用 gpui-kit 对话框）——列为**支持**。

> **2026-10-26 修订（事件名与成员状态校准）**：本节事件清单以 `docs/design/phase-2/05-shim-events.md` 为准——pi 1.4.0 不存在 `error` 事件；`compact` 实际是 `session_before_compact`/`session_compact`/`session_compact_failed`，且本版无 compaction，归缓做；`message_*`/`tool_execution_*` 由内核事件直接映射，列为只读通知支持；`registerVirtualModel` 为受限支持（仅同 provider 路由）。本次修订不改变三分类策略，只校准名称与逐项状态。

### 1.4 约束与兼容标记

- 版本检查：shim 导出的 `VERSION` 常量独立维护（仿 PI-Desktop 假版本策略），与本项目真实版本解耦。
- 环境差异：无 `node:*` 模块、无进程环境写能力、无随意网络（见 §3 权限）；装入失败/降级项全部进诊断报告，不静默。

## 2. A 轨加载管线

```
发现（项目 .pi/extensions/、用户目录、显式配置）
→ 安装期：rolldown/oxc 打包（含 npm 依赖；node:* 引用与解析失败 → 诊断）
→ 运行期：QuickJS 载入 bundle → 注入 pi shim（按矩阵）→ 执行工厂
→ 注册产物（工具/命令/订阅）并入内核统一管线
→ 生成《扩展装载诊断报告》（支持/降级/失败逐项）
```

钩子语义（Phase 2 硬性验收）：单 handler 30s 预算；abort 即退休；payload detach 拷贝（订阅者改动对内核不可见）；错误只影响该订阅者不影响内核。

## 3. 权限模型（两轨共用原则，借鉴 research/04 §3）

- **声明 + 授予双闸**：manifest 声明能力 → 用户逐项批准；高风险能力（文件写删、网络、执行）默认拒绝。
- fs 作用域 glob：**写/删禁止全树声明**（读可）；网络：域名白名单 **fail-closed**（未声明 = 无网）。
- 硬拒：`.env*`、SSH 凭证、`.git/**`。
- 命名空间：扩展工具/命令为 `plugin_<id>_<name>`；MCP 工具为 `mcp__<server>__<tool>`（**双下划线**，避免工具名内下划线造成歧义）；内置工具**无前缀**（裸名 `read`/`write`/`edit`/`bash`/`powershell`/`grep`/`find`）。注册期当前**不做前缀校验**（约定而非强制），清偿项见 `docs/roadmap/audit-debt.md` B2。

## 4. B 轨：GUI 原生插件（框架级定稿，细节 Phase 4 设计文档定）

- 包结构：`manifest.json`（能力声明、slot 声明）+ TSX 源码 → 打包为 NSH 签名热更包。
- **slot 白名单**（宿主窗口内可渲染位，初期候选）：toolCard（工具卡片渲染）、panel（停靠面板）、composerTrigger/composerControl（输入区扩展）、entryExtra（条目附加区）、theme（design-token 覆盖，宿主消毒）。
- 渲染动作走**白名单词汇表**（受控的宿主动作集合，如打开文件/发送提示/通知），越界即拒。
- 沙箱：QuickJS 无环境能力 + cap-std 显式授予；签名校验通过方可装载。
- 自证要求（借鉴 PI-Desktop）：Phase 4 验收必须包含"本项目自家一个功能以 B 轨插件形态实现"。
- B 轨 API 自第一天带版本号与弃用流程（本项目的差异化承诺，pi 未做到）。
