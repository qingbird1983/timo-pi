# H-003：Phase 2 设计定稿

- 日期：2026-10-26
- 主题：pi 编排扩展宿主（A 轨）实现级详设定稿

## 本轮做了什么

1. 通读蓝本：pi 1.4.0 `core/extensions/types.ts`（2,308 行）、`runner.ts` 派发实现、`loader.ts` 发现逻辑；本机内核 7 个 crate 与 CLI 冒烟壳。
2. 定稿 `docs/design/phase-2/` 6 篇（总览、shim API、打包、权限、钩子语义、事件矩阵）。
3. 校准架构 04 的事件名/成员状态（`error` 不存在、`compact` 实为 `session_*_compact`、`registerVirtualModel` 受限）。
4. 选定 B1 样本：hello / permission-gate / protected-paths；排除 todo / auto-commit（依赖缓做的会话读接口）。
5. 登记：docs/README、roadmap 状态、CHANGELOG 0.1.14、CONTEXT；体量 docs 53/60、.agents 9/40。

## 遗留与下一步

- 按 `design/phase-2/00-overview.md` §8 提交序列实现，第一步 `crates/timo-ext` 骨架 + discovery/manifest/diag。
- 实现首日探针：rquickjs 中断后 Context 可复用性；rolldown Rust API 最小打包样例。
- 权限的 `fs.*` 只有契约、无暴露面；B4 只验声明校验。
- `registerVirtualModel` 跨 provider 路由仍收敛（D23）。

## 已提取的沉淀

- P-004：上游 API 二手摘要不能当兼容合同。
- S-002：兼容矩阵的逐项判定与写法。
