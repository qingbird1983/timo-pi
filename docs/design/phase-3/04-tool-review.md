# 设计 04：工具审阅接缝与 diff 审查

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0012；architecture/03 接缝 #2；设计 00/01

## 1. 问题

Phase 1/2 的内置 `write`/`edit`/`bash` 在校验参数后**直接执行**：文件写入不可撤回，命令副作用不可撤回。桌面端的核心承诺是“看清它做了什么，并对文件改动做决定”，所以必须在**副作用发生之前**有一个可拒绝的节点。

现有事件总线是**只通知**的（`KernelEvent` 快照，订阅者不能改内核态）；扩展的 `before_tool_call` 是同步钩子，属于扩展宿主；两者都不能作为 GUI 的等待点。需要一个显式的请求-响应接缝（ADR-0012）。

## 2. 决策

在 `timo-tools` 定义审阅 trait，`ToolCallCtx` 持有可选审阅器；内置工具在“已算出最终副作用、尚未落地”处调用。`KernelOptions` 注入，CLI 不注入（`None`）→ 行为与 Phase 1/2 逐字节等价。

```rust
// timo-tools/src/review.rs
#[async_trait]
pub trait ToolReview: Send + Sync {
    async fn review(&self, request: ToolReviewRequest) -> ReviewDecision;
}

pub struct ToolReviewRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub kind: ReviewKind,
}

pub enum ReviewKind {
    FileChange {
        path: PathBuf,              // 绝对路径
        display_path: String,       // 相对 work_dir
        before: Option<String>,     // None = 新文件
        after: String,
        binary: bool,               // before 非 UTF-8 时为 true，不做文本 diff
    },
    Command {
        command: String,
        cwd: PathBuf,
        shell: String,              // "bash" / "powershell"
    },
}

pub enum ReviewDecision {
    Allow,
    Reject { reason: String },
}
```

- 新增 `ToolError::Rejected { reason }`，显示为 `Rejected by user: {reason}`；工具结果 `is_error = true`，模型可据此调整。
- 审阅器可缓存“本会话允许此工具”策略；trait 不增加 `AllowForSession` 变体，避免第二套状态源（缓存属于 GUI）。
- 审阅器不参与参数校验、不做权限判定；权限仍由工具/扩展既有机制负责。

## 3. 注入与调用点

```
KernelOptions.tool_review ──► LoopConfig.review ──► ToolCallCtx.review
                                                        │
write/edit: 计算 after → 拿路径互斥锁 → 构造 FileChange → review
bash/powershell: 构造 Command → review → spawn
```

- `ToolCallCtx` 增加 `pub review: Option<Arc<dyn ToolReview>>`；测试构造处补 `None`。
- `write`：在路径互斥队列内读旧内容（不存在为 `None`），构造 `FileChange`，允许后才创建父目录并写入。
- `edit`：在互斥队列内读旧内容、完成匹配/规范化得到 `after`，构造 `FileChange`，允许后才写回。**匹配失败在审阅之前报错**，不打扰用户。
- `bash`/`powershell`：在探测到实际 shell、确定 cwd 后、`spawn` 前审阅；命令文本原样传入，不截断语义（界面可折叠展示）。
- `read`/`grep`/`find` 等只读工具不审阅。
- MCP/扩展工具本阶段不接入：它们有自己的授权/沙箱；GUI 只展示结果（登记为 Phase 4 权限 UI 的输入）。

顺序（与既有机制的关系）：

```
参数校验 → 扩展 before_tool_call（可否决/改参）→ 工具审阅（GUI）
        → 执行 → 扩展 after_tool_call → 工具结果
```

扩展已否决的调用不会进入审阅；扩展改参后的最终参数才进入审阅。

## 4. 并发与生命周期

- 审阅请求可并发到达（工具执行模式为并行）。GUI 侧 `ReviewBroker` 用队列串行展示；每条请求带 `tool_call_id`，UI 决定后通过 `oneshot` 回传。
- `write`/`edit` 在等待审阅期间**持有该路径的互斥锁**：保证 `before` 与真正写入之间没有第三方改动；不同路径可并行。
- 运行取消 / 用户点停止 / 窗口关闭 → drop 审阅发送端 → 工具收到拒绝（`reason = "run cancelled"`），不执行副作用。
- 审阅等待**没有自动放行超时**；只要应用在运行就等用户决定。可取消性必须由取消臂保证（测试覆盖“取消后无写入”）。
- UI 关闭审阅 sheet 视为拒绝（`reason = "review dismissed"`），不是“稍后再说”；避免悬空等待。
- 审阅器实现必须 `Send + Sync`；GUI 侧用 `std::sync::Mutex` 保护队列，不跨 await 持锁。

## 5. diff 生成与展示

- 审阅请求传 `before`/`after` 原文，**不在内核生成 patch**：diff 的呈现属于桌面壳。
- GUI 用 `similar 2.7` 生成 unified diff（3 行上下文），再用 `DiffFile::parse` 交给 `DiffState`：
  - 新文件：`before = None`，全部为新增；
  - 删除不存在（Phase 3 无删除工具）；
  - 二进制/非 UTF-8：`binary = true`，显示“无法生成文本 diff” + 文件大小 + 接受/拒绝。
- 上限：单个文件 `before`/`after` 各 2 MiB、diff 5,000 行；超限只显示摘要（路径、字节数、增删行数近似不保证）与接受/拒绝，不在卡片内联全文。
- 相对路径：`display_path` 由工具按 `work_dir` 计算；绝对路径只在详情里显示。
- 界面：右侧 `Sheet` + `Diff`（设计 01 §7）。接受/拒绝按钮固定底部，不随 diff 滚动。

## 6. CLI 与兼容性

- `KernelOptions.tool_review` 为 `None` 时：
  - `ToolCallCtx.review = None`；
  - 所有工具走原路径，不构造审阅请求；
  - 现有单元/集成测试**一字不改**必须全绿；
  - 新增一条“None 时不调用审阅器”的测试（用计数假实现）与一条“Reject 时不写文件/不 spawn”的负对照。
- CLI 不暴露审阅开关（本阶段）；未来若加，必须另写 ADR 说明交互模型。

## 7. 会话策略与设置

- 桌面设置 `review.files`（默认 true）、`review.commands`（默认 true）。关闭后对应工具不请求审阅，等价于旧行为。
- “本会话始终允许”缓存：
  - 键为工具名 + 路径（文件）或工具名（命令）；
  - 只存在内存，run/会话结束后清空；不写磁盘；
  - 设置页提供“清空本会话允许项”。
- 拒绝原因：可选输入，默认“用户拒绝”。写入工具结果的只有原因文本，不含路径之外的敏感信息。

## 8. 安全与失败态

| 场景 | 行为 |
|---|---|
| 审阅器 panic | 工具捕获为 `ToolError::Execution("review failed")`，不执行副作用；测试覆盖 |
| 审阅器返回拒绝 | 结果 `is_error`，卡片 Blocked；会话继续 |
| 用户关闭窗口 | 取消 run → 审阅拒绝 → 工具不执行；会话文件只含已确认结果 |
| 文件在审阅期间被外部修改 | 不可能经本应用；外部进程修改不在防护内（记录限制） |
| 超大/二进制文件 | 不做文本 diff，仍需显式接受 |
| 审阅请求含密钥 | 命令文本可能含用户/模型写入的敏感串：不写日志、不入会话（只有拒绝原因入结果） |

## 9. 测试矩阵

1. `write` 接受 → 文件存在、内容正确；拒绝 → 文件不存在、结果含 `Rejected by user`。
2. `edit` 接受 → 精确替换；拒绝 → 原文件逐字节不变；匹配失败 → 不触发审阅。
3. `bash` 接受 → 命令执行；拒绝 → 子进程未创建（负对照：标记文件不存在）。
4. 取消：审阅等待中触发 CancellationToken → 返回拒绝，无写入/无 spawn。
5. 并发：两个不同路径同时请求审阅，队列两条、各自回传正确；同一路径串行。
6. 二进制/超限：不生成 diff，仍可接受/拒绝。
7. CLI 兼容：`review = None` 时既有测试全绿；假审阅器调用计数为 0。
8. GUI：`#[gpui_kit::test]` 覆盖 Accept/Reject/Esc/队列第二条。

## 10. 不做

行内编辑后接受；按 hunk 选择性接受；多文件补丁；撤销已接受的改动；审阅历史持久化；扩展/MCP 工具的逐调用审阅。以上进 backlog，Phase 4 权限 UI 时统一考虑。
