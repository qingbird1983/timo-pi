# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 2 设计定稿**）

## 当前阶段与状态

- **Phase 1 ✅ 完成**（tag `phase-1.end`）：A1~A6 通过，CI 全绿。
- **Phase 2 进行中（设计定稿，实现未开始）**：开工登记 tag `phase-2.start`。
- 设计稿：`docs/design/phase-2/00-overview.md` ~ `05-shim-events.md`（6 篇，全部定稿）。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-2-extension-host.md`。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **通读蓝本与现状**：pi 1.4.0 扩展类型/派发实现、本机内核 7 个 crate、CLI 冒烟壳。
- **定稿 6 篇设计**：总览与偏离 D11~D25、shim API 与类型映射、打包与 E1xx~E4xx、权限 v1、钩子语义、41 事件矩阵。
- **关键决策**：rquickjs 0.14（每扩展一 Context、单专用线程）；rolldown 1.2 + oxc 0.155 安装期打包（日常运行不需要 Node）；可改态钩子走 `LoopHooks`/`ProviderHooks`，通知走事件总线；权限双闸 fail-closed。
- **矩阵校准**：pi 无 `error` 事件；`compact` 实为 `session_*_compact` 且缓做；`registerVirtualModel` 受限（仅同 provider）。已写入 architecture/04 修订注记。
- **登记**：docs/README 6 行、roadmap 状态、CHANGELOG 0.1.14；体量 docs 53/60、.agents 12/40。

## 下一步（按优先级）

1. **建 `crates/timo-ext` 骨架**：discovery / manifest / diag 三模块，先能扫描目录并输出空报告（`00 §8` 第 2 步）。
2. **pack 模块**：rolldown 打包 + 禁用面扫描 + E1xx；自写夹具。
3. **shim + host**：模块别名、`VERSION=1.4.0`、惰性 Proxy、hello 样本跑通。
4. **钩子**：事件桥、`LoopHooks`（先 `before_agent_start`/`transform_context`/`before_tool_call`/`after_tool_call`）、`ProviderHooks`、30s 预算、abort 退休。
5. **权限与 CLI**：宿主闸门、`timo ext` 子命令、斜杠命令、诊断呈现。
6. 之后才做 B1~B6 验收与收尾。

> 施工顺序唯一来源：`docs/design/phase-2/00-overview.md` §8。实现前不再补设计文档；发现设计错误先改文档再写码。

## 未决问题

- `rquickjs` 中断后 Context 可复用性：首日探针；不可恢复则整 Context 重建并标 degraded。
- rolldown Rust API 在 1.2.x 的稳定性：锁次版本、适配层隔离。
- `registerVirtualModel` 跨 provider 路由（D23 已收敛为同 provider）。
- `fs.*` 能力本版无暴露面（契约先行，B4 只验声明校验）。
- 30s 预算与 UI 等待/exec/fetch 的边界（已定：后三者不计入，需实现期验证）。
- `message_end`/`input` 等可改写事件是否升级为支持：等真实样本驱动。

## 关键提醒

- **接缝 #2**：`timo-agent` 循环只调用注入的 `LoopHooks` 固定方法，禁止出现扩展/插件名分支；`None` 时与 Phase 1 逐字节等价。
- **不得复制 pi 源码**（铁律 7）：仓内夹具一律自写；B1/B2 人工验收直接指向本机 pi 检出目录。
- **权限强制点唯一**：QuickJS 无 OS 能力，所有 exec/net/fs 只能走宿主函数；不得出现第二条放行路径。
- **诊断可执行**：每条错误必须含码 + 原因 + 定位 + 下一步；`failed/degraded` 逐项可见。
- **门禁四连**：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash scripts/check-size`。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
