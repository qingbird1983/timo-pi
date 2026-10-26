# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 2 实现：扩展宿主骨架**）

## 当前阶段与状态

- **Phase 1 ✅ 完成**（tag `phase-1.end`）：A1~A6 通过，CI 全绿。
- **Phase 2 进行中**：设计定稿（tag `phase-2.start`）；提交序列第 2 步（`timo-ext` 骨架）完成。
- 设计稿：`docs/design/phase-2/00-overview.md` ~ `05-shim-events.md`（6 篇，全部定稿）。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-2-extension-host.md`。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **新建 `crates/timo-ext` 并接入 workspace**：discovery / manifest / diag 三模块 + 18 个测试。
- **发现**：项目 `.timo`、pi 兼容 `.pi`、用户目录、显式路径；三种入口写法；目录/文件 id 派生；四类问题进诊断（E101/E204/E205/E206）。
- **manifest**：schema/id/相对路径/指纹格式校验；源码目录稳定 sha256 指纹（排序 + 长度前缀），为授权失效做准备。
- **诊断**：28 个错误码（E1xx/E2xx/E3xx/E4xx）、JSON（summary 现算）与人读表。
- **规范修正**：按 standards/04 §6 禁止 `eval`/`new Function`，设计 02/00 同步（D26）。
- **体量**：单文件最大 413 行；门禁四连通过；隐形字符扫描干净。

## 下一步（按优先级）

1. **pack 模块**：先登记依赖准入（`rquickjs`/`rolldown`/`oxc`），再实现 rolldown 打包、禁用面扫描（node:*/eval）、E1xx；自写夹具。
2. **shim + host**：模块别名、`VERSION=1.4.0`、惰性 Proxy、hello 样本跑通。
3. **钩子**：事件桥、`LoopHooks`（先 `before_agent_start`/`transform_context`/`before_tool_call`/`after_tool_call`）、`ProviderHooks`、30s 预算、abort 退休。
4. **权限与 CLI**：宿主闸门、`timo ext` 子命令、斜杠命令、诊断呈现。
5. 之后才做 B1~B6 验收与收尾。

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
- **500 行 gate**：纯类型/报告类测试优先放 `tests/`，避免实现文件在收尾时被迫删内容或拆模块。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
