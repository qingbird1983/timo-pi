# ADR-0012：工具执行前的显式审阅接缝

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0009；architecture/02 §3、03 #2；design/phase-3/04

## 背景

Phase 1/2 的 `write`/`edit`/`bash` 在参数校验后直接执行。桌面端的核心承诺是“看清它做了什么，并对文件改动做决定”（roadmap/phase-3 C1/C2）。事件总线是只读通知，扩展钩子是扩展宿主的能力，二者都不能让 GUI 在副作用前等待用户决定。

## 决策

在工具层引入一个**显式的请求-响应接缝**：

- `timo-tools::ToolReview`：`async fn review(&self, ToolReviewRequest) -> ReviewDecision`；
- `ToolCallCtx` 携带 `Option<Arc<dyn ToolReview>>`；
- `KernelOptions.tool_review` 注入，`None` 时完全不改变现有执行路径；
- `write`/`edit` 在计算出 after、持有路径互斥锁后请求审阅；`bash`/`powershell` 在 spawn 前请求；
- 拒绝映射为 `ToolError::Rejected`，模型收到错误结果，工具不产生副作用；
- diff 的生成与展示属于桌面壳，内核只传 before/after 文本。

## 理由

- 事件总线必须保持“不可变快照、订阅者不能改内核态”；把 GUI 等待塞进事件会破坏 architecture/02 §3 与接缝 #2。
- 审阅是显式请求/响应，与“需要修改的节点走显式 API”的既定原则一致。
- 工具层最了解自己的语义：`edit` 的匹配/模糊规则、`write` 的覆盖范围，只有在工具内部才能算出准确的 after。
- 默认关闭保证 CLI、库用户与 Phase 1/2 行为等价；桌面壳按需注入。
- 拒绝发生在副作用前，避免“先写再撤销”的不可靠状态与竞态。

## 备选与否决

| 方案 | 否决理由 |
|---|---|
| GUI 订阅 `ToolCallRequested` 后广播否决 | 事件总线只通知；无法等待；会引入第二条内核修改路径 |
| 扩展 `before_tool_call` 承担 GUI 审阅 | 扩展宿主有 30s 预算、按扩展上下文执行；GUI 不是扩展；会混淆权限模型 |
| 先执行再提供撤销 | 无法保证无副作用；模型已基于新文件继续；命令副作用不可逆 |
| 内核里判断“GUI 模式” | 违反 ADR-0009：内核不得知道界面 |

## 后果

- `timo-tools` 增加一个 trait 与错误变体；`ToolCallCtx` 增加字段，所有构造点需补 `None`（测试/CLI）。
- CLI 行为不变，但多了一条必须长期守住的兼容测试：`review = None` 时既有测试一字不改全绿。
- 扩展/MCP 工具本阶段不经过该接缝：它们各自的授权/沙箱仍是强制点；Phase 4 统一权限 UI 时再评估逐调用审阅。
- 审阅等待必须可取消；关窗/停止 run 不得留下挂起的工具或写入。
