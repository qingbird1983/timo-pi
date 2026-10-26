# 工作上下文交接卡

- 更新：2026-10-26（会话主题：**Phase 3 设计定稿：桌面壳、审阅接缝、NSH PoC**）

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`，报告 `docs/design/phase-2/report.md`）。
- **Phase 3 进行中**：设计定稿（tag `phase-3.start`），实现未开始。
- 设计：`docs/design/phase-3/00-overview.md` ~ `04-tool-review.md` + `prototype.html`；决策 ADR-0012。
- 看板：`docs/roadmap/00-roadmap.md` §1；阶段文档：`docs/roadmap/phase-3-desktop-shell.md`。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **窗口与状态流**：侧栏会话 + 聊天流 + 审阅 sheet；消息流式拼装、工具卡片状态机、diff 接受/拒绝、命令面板、设置页、空/加载/错误/权限态、键盘路径。
- **工具审阅**：ADR-0012 + 设计 04；执行前审阅，拒绝无副作用；CLI 默认关闭；并发队列、取消/关窗语义、并发路径锁。
- **密钥**：`keychain:<service>/<account>` 引用，桌面写系统钥匙串，配置只存引用，Debug/日志脱敏；无钥匙串平台明确报错。
- **NSH**：AppView 隔离层、`verify_package` → 健康后 `accept_package`、v1→v2 热更/回退、未签名/篡改/降级拒绝；`nsh-poc` feature 门控。
- **版本实测**：`gpui-kit 0.8.0` 家族 + `gpui-fast 0.1.10` + NSH 0.3.0 编译探针通过；0.8.1 / 0.1.12 下 NSH 0.2/0.3 均报 E0716。锁 0.8.0 家族 + 0.1.10。
- **缺陷登记**：`Kernel::run` 多轮丢上下文（K0，见设计 00 §5），实现第一提交修复并加回归测试。

## 下一步（按优先级）

1. **最小窗口**：按锁定版本建 `apps/timo-desktop` 空壳，复跑 NSH 共存探针并记录。
2. **K0/K1**：多轮上下文保留；`new_session()` / `history()` / `delete_session(id)`；既有测试一字不改全绿。
3. **K2**：审阅接缝 + CLI `None` 等价性测试（ADR-0012）。
4. 会话/聊天 → 审阅 → 设置/钥匙串 → NSH PoC。
5. 最后才做 C1~C6 验收与 `phase-3.end` 收尾。

> 施工顺序唯一来源：`docs/design/phase-3/00-overview.md` §8。设计错误先改文档再写码。

## 未决问题

- NSH 0.3.0 + gpui-fast 0.1.10 的**运行期**行为（渲染/热更）尚未验证，只有编译探针。
- C4 官方 CLI 生成 v1/v2 包的具体命令参数（`nsh keygen/certify/publish/verify`，实现期验证）。
- 审阅“本会话始终允许”是否跨会话持久化：本阶段只做会话内。
- 助手 Markdown 长文/代码块体验、会话重命名：留待真实使用驱动。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **审阅前无副作用**：拒绝必须不写文件、不 spawn；等待中取消/关窗 → 拒绝。
- **CLI 等价**：`tool_review = None` 时 Phase 1/2 测试一字不改全绿。
- **0.2.0 不可用**：已实测 `chrome.rs:1211` E0716；不要再尝试 0.2.0。
- **版本只认 registry 源码签名**，不按技能文档/参考项目 0.2.x 推断 0.8 API。
- **门禁四连**：`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && bash scripts/check-size`。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容。
