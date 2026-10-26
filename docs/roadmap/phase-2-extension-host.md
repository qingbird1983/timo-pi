# Phase 2：编排类扩展宿主（A 轨）

- 状态：进行中（设计定稿 2026-10-26，实现未开始）
- 参考估时：3~6 周
- 关联：ADR-0003/0006/0011；architecture/02~04；design/phase-2/00~05

## 1. 目标（一句话）

pi 官方生态中"不碰 UI 的编排类扩展"能在本项目内核上装载并真实生效；碰 UI 的扩展装载不崩、降级透明。

> **方向纪律（ADR-0011）**：同 Phase 1——编排兼容层以 pi 的 ExtensionAPI 行为为唯一蓝本（含钩子语义、降级模式），创新只记录不执行。

## 2. 范围

### 做
- timo-ext：QuickJS 宿主；rolldown/oxc 安装期打包管线（npm 依赖内联；`node:*` 引用与失败→诊断）
- pi 兼容 shim：按 architecture/04 §1 三分类矩阵实现（支持/deferred/no-op+诊断）
- `VERSION` 兼容标记（与真实版本解耦）
- 钩子语义：30s 预算、payload detach 拷贝、abort 退休、订阅者错误隔离
- 权限双闸 CLI 简版：manifest 能力声明 → 用户批准提示（文件写删/网络/执行 fail-closed）
- 《扩展装载诊断报告》：每个扩展的支持/降级/失败逐项输出

### 不做
B 轨 TSX 插件面（Phase 4）；A 轨 v2 成员的实现（仅保留"未实现"诊断）；图形化权限管理界面

## 3. 文档交付物（2026-10-26 全部定稿）

- `docs/design/phase-2/00-overview.md`：模块布局、装载管线、关键决策、偏离登记 D11~D25、接缝核对
- `docs/design/phase-2/01-shim-api.md`：shim API 对照表（pi 签名 → 语义差异）、类型映射
- `docs/design/phase-2/02-packaging.md`：安装期打包管线、错误分类总表 E1xx~E4xx、诊断报告
- `docs/design/phase-2/03-permissions.md`：权限能力清单 v1、双闸、强制点
- `docs/design/phase-2/04-hook-semantics.md`：30s 预算、detach、abort 退休、错误隔离
- `docs/design/phase-2/05-shim-events.md`：pi 1.4.0 全量事件矩阵（41 项逐项分类）

## 4. 验收场景

| # | 场景 | 通过标准 |
|---|---|---|
| B1 | 装载 pi 官方无依赖编排扩展样本 3~5 个（hello、permission-gate、protected-paths、todo、auto-commit-on-exit 中选定） | 各自工具/命令/钩子在 CLI 会话中真实生效（如 permission-gate 能拦截危险命令） |
| B2 | 装载一个纯 TUI 扩展（如 modal-editor 或 snake） | 不崩溃；诊断报告逐项列出被 no-op 的成员 |
| B3 | 钩子语义专项测试 | handler 超 30s 被切断且会话继续；订阅者改 payload 不影响内核态；abort 后不再收到事件 |
| B4 | 权限专项 | 未声明网络能力的扩展访问网络被拒并留痕；fs 写删全树声明被拒 |
| B5 | 打包失败透明 | 一个带 `node:fs` 的扩展装载失败，诊断报告明确指出原因 |
| B6 | 接缝核对 + CI 全绿 | architecture/03 达标 |

## 5. 风险与对策

- 个别样本扩展因语法/依赖打包失败 → 备选样本池；失败个案计入诊断并评估是否调整打包策略（ADR-0006 备选路径 deno_core 仍保留）。
- 钩子语义漏洞导致内核不稳定 → 语义测试与 B3 并列为一票否决项。
