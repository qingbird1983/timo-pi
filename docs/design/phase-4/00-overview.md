# 设计 00：Phase 4 总览与跨模块决策

- 状态：草稿（2026-10-28；待所有者评审后定稿，定稿前不写实现代码）
- 关联：roadmap/phase-4；ADR-0002/0004/0010/0015；architecture/04；design/phase-3/03
- 行为蓝本：无（B 轨为本项目自有契约；ADR-0004）

## 1. 定位

本目录是 Phase 4「GUI 插件系统」的实现级详设：统一插件管理器、manifest 与签名包、权限挡位、slot 与动作词汇表、TSX 宿主 API v1、运行时隔离与 devkit。范围以阶段文档 §2 为准，本目录不扩范围。

Phase 4 同时管理两类插件，**同一个左栏入口**：

| 轨道 | 插件形态 | 运行时 | 来源 |
|---|---|---|---|
| A 轨（兼容） | pi 系扩展：`manifest.json` + `bundle.cjs`（rquickjs） | `timo-ext` 宿主 | Phase 2 已有 |
| B 轨（原生 UI） | TSX → NSH 签名热更包（QuickJS + GPUI） | NSH `AppView` | 本阶段新建 |

## 2. 一句话方案

把「扩展管理」升级为**统一插件管理器**：左栏固定入口，一个列表同时管理 A/B 两轨插件；安装、启用/禁用、更新、卸载、权限挡位、诊断都在这里。B 轨插件用 TSX 写界面，经签名包分发，只能落在宿主声明的 slot 里，业务能力按**挡位**一次授权，写操作经宿主代理调用工具。

## 3. 模块布局

| 位置 | 职责 | 批次 | 详设 |
|---|---|---|---|
| `timo-kernel/src/plugins/` | 统一插件清单、安装/卸载/启停、挡位映射与撤销 | P0 | 01 |
| `timo-ext`（manifest 扩展） | A 轨 manifest 兼容；B 轨字段（kind/ui/tier/api/slots） | P0 | 02 |
| `timo-kernel/src/plugins/nsh.rs` | NSH 包索引、签名校验、健康检查、热更交换 | P1 | 02/05 |
| `apps/timo-desktop/src/features/plugins/` | 插件页、安装向导、授权面板、诊断面板 | P0/P1 | 01 |
| `apps/timo-desktop/src/features/nsh_poc/` | 并入正式插件页；`nsh-poc` feature 退场 | P1 | 05 |
| `crates/timo-devkit/`（新 bin crate） | `check` / `pack` / `sign` / `verify` | P2 | 06 |
| `examples/plugins/session-search/` | 自证插件（TSX） | P2 | 07 |
| `docs/design/phase-4/` | 本目录 | — | — |

## 4. 批次与提交序列

1. **本批**：00 总览 + 插件管理器原型；所有者看方向后定稿。
2. P0 详设 01（管理器）+ 02（manifest/包）：统一清单、安装/卸载/启停、权限挡位映射。
3. P1 详设 03（slot/动作）+ 05（运行时隔离）：B 轨运行、健康检查、崩溃/死循环处理。
4. P1 详设 04（宿主 API v1）：`timo.*` 命名空间、只读查询、动作与工具代理、版本契约。
5. P2 详设 06（devkit）+ 07（自证插件）：签名打包、会话搜索面板。
6. 验收 D1~D5 → `report.md` → 看板/CHANGELOG → tag `phase-4.end`。

> 每批门禁四连不变（fmt / clippy / workspace tests / check-size）；UI 批次另附真机截图与逐态核对。

## 5. 关键实现决策

| # | 决策 | 理由 / 约束 |
|---|---|---|
| K1 | 两轨共用**一个**插件管理器与左栏入口，不按轨分家 | 用户明确要求；两轨共用权限与生命周期，分入口会让用户不知道去哪里管 |
| K2 | 权限以**挡位**呈现（`read-only` / `workspace` / `trusted`），底层仍是现有 `Grant` 结构 | 挡位是产品层预设：安装时一次授权，档内直接运行；A 轨旧 manifest 无挡位时按其声明逐项展示 |
| K3 | 挡位只收不放：插件声明的能力必须落在所选挡位内；`trusted` 也要域名白名单 | 与 A 轨「只收不放」一致；挡位高不等于无限 |
| K4 | B 轨 manifest 用 `schema: 2`；A 轨保持 `schema: 1` | 不改 Phase 2 已安装扩展；加载器按 schema 分流，不迁移旧文件 |
| K5 | B 轨运行在 NSH `AppView`，与 A 轨 rquickjs 宿主**不共享状态** | 两套运行时是既定事实（design/phase-3/03 §6）；跨轨通信只经宿主代理 |
| K6 | 插件执行工具走宿主代理 `timo.actions.runTool`，不直接持有执行权 | 统一经过内核注册表与取消/审计；挡位决定是否允许 |
| K7 | 审阅是**附带功能**，不是插件调用的必经路径：默认按挡位放行并记审计，全局审阅开关保留为可选 | 所有者定调（2026-10-28）；挡位本身即用户授权 |
| K8 | B 轨 API 从 v1 起带版本号与弃用流程 | ADR-0004 的差异化承诺；manifest 的 `api` 字段与宿主能力表对齐 |
| K9 | 左栏「插件」入口固定存在，不再用 `nsh-poc` feature 门控 | 正式产品面；实验页在 P1 并入后删除 |
| K10 | 分发只做本地文件与 URL，不做 marketplace | 阶段文档 §2「不做」；URL 下载也要签名校验 |
| K11 | 自证插件默认「会话搜索面板」（`read-only` 挡位） | 能同时证明 panel slot 与只读查询面；若所有者改选 todo，只换 07，不影响 00~06 |

## 6. 与既有设计的衔接

- **A 轨不变**：`ExtensionHost`、`PermissionStore`、`discover/load_extensions` 的语义保持；管理器只增加读清单、启停与撤销入口。
- **B 轨复用 Phase 3 PoC**：`verify_package` → 暂存 `AppView` → 健康检查 → `accept_package` → 交换；失败保留旧包（design/phase-3/03 §4）。
- **内核门面**：桌面只调 `timo-kernel`。插件清单与操作属于内核装配层，NSH 类型不出 `apps/timo-desktop/src/features/plugins/nsh/`。
- **无项目信任门控**（ADR-0015）：插件安装是显式动作，不再叠加工作区信任弹框。

## 7. 验收映射

| 阶段场景 | 本目录落点 |
|---|---|
| D1 示例插件全链路 | 02（包/签名）、06（devkit）、07（示例） |
| D2 自家功能插件化 | 07（会话搜索面板） |
| D3 安全专项 | 02（签名）、03（slot/动作越界）、04（API 权限） |
| D4 稳定性 | 05（隔离、超时、卸载无残留） |
| D5 接缝与版本契约 | 04（API v1）、01（统一入口） |

## 8. 风险与对策

- **NSH 0.x API 漂移**：隔离层先行的 PoC 已验证；升级窗口化，B 轨只在隔离目录内 `use nsh_*`。
- **B 轨 API 返工**：v1 定稿前先用自证插件走一遍全部调用；发布后按弃用流程演进。
- **两轨清单不一致**：统一清单由内核重建（目录扫描 + 包索引），不在 UI 侧拼数据；A/B 轨字段差异由 `kind` 区分。
- **挡位被误解为「无限授权」**：安装向导同时展示档内明细；`trusted` 仍受域名白名单与硬拒路径约束。
