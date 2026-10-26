# ADR-0003：插件兼容策略——仅兼容编排类子集，TUI 成员降级为 no-op + 诊断

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0002、ADR-0004、ADR-0006；research/02、research/04；architecture/04

## 背景

pi 第三方扩展是进程内 jiti 加载的 TypeScript，API 面含大量 TUI 能力。全量兼容需要移植 TUI 并复刻整个宿主环境，成本高且与桌面愿景冲突；而多数 TUI 扩展在 GUI 上本就无意义。

## 决策

对 pi 官方扩展实现**三分类兼容矩阵**（完整清单见 `architecture/04-plugin-system.md`）：

1. **支持（编排类）**：registerTool、registerCommand、registerProvider、on() 事件钩子、exec、工具开关、setModel 等——真实实现。
2. **缓做（deferred v2）**：sendMessage、appendEntry、sessionManager 读、autocomplete 等——API 成员存在但返回明确"未实现"，逐版本转正。
3. **不支持（TUI 面）**：ui.setWidget/custom/编辑器操控、四类 renderer 注册、navigateTree 等——**成员存在但 no-op，永不 throw，每扩展每成员记一条诊断**。

## 理由

- "编排类扩展与 GUI 不冲突"已由参考项目 PI-Desktop 验证可行（其 16 号 spec 即此策略的工程化先例）。
- 降级而非报错：扩展无需感知运行环境差异；诊断让用户知道哪些能力没生效。

## 后果

- 兼容面收窄至"无 UI、依赖可打包"的扩展（pi 官方示例中的主流类型，见 research/02 §4）。
- 需要建设：rolldown 打包管线、QuickJS 宿主、钩子语义（超时预算/payload 拷贝/abort 退休）——Phase 2 交付。
- 不承诺逐版本追平 pi 上游扩展 API（其无 semver）；矩阵即兼容契约。
