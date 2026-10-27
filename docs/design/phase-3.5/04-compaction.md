# Phase 3.5 / 04：compaction（自动上下文压缩）

- 状态：定稿（2026-10-27）
- 关联：research/06 §3；ADR-0007（会话格式）；`design/phase-3.5/00-overview.md` K8/D34；pi `core/compaction/`（只读参照）

## 1. 目标

长会话在接近模型上下文窗口时，把较早的对话压成一份结构化摘要，保留近期原文继续工作；压缩写进会话树，恢复后行为一致（D3）。

对齐 pi 的可观察语义：**在每次模型请求之前**按阈值判断；摘要格式固定；原始 entry 永不删除；压缩失败不影响当前 run。

## 2. 范围与偏离

P1 实现：阈值触发、手动 `/compact`、摘要 entry、恢复投影、文件操作清单、失败降级。

| # | 偏离 | 类型 | 说明 |
|---|---|---|---|
| D39 | 只在 user 消息处切分，不做"半个轮次"的 turn-prefix 摘要 | d（范围收敛） | pi 允许切在 assistant 消息并为前半轮再生成一份小结、两次调用合并。P1 退到"切在最近的 user 消息"：保留量可能略多于预算，但上下文永远从完整轮次开始。需要更精确预算时再补。 |
| D40 | 摘要请求仍按适配器默认写 prompt cache | a（适配器无 retention 开关） | pi 用 `cacheRetention: "none"` 让一次性摘要不写缓存；我们的 `StreamOptions` 暂无该旋钮。代价是一次摘要的缓存写费用，不影响正确性。 |

不使用 pi 的 compaction 分支摘要（`branch-summarization`）：本项目会话树 MVP 只有线性链，分支摘要在 GUI 分叉功能一起再议。

## 3. 配置

`settings.json`（K9：多词键转 snake_case）：

```json
{"compaction": {"enabled": true, "reserve_tokens": 16384, "keep_recent_tokens": 20000}}
```

- `enabled` 缺省 `true`；`reserve_tokens` 缺省 16384；`keep_recent_tokens` 缺省 20000（与 pi 默认一致）。
- 全局与项目设置沿用现有深合并；P1 不做按模型覆盖。

## 4. 触发与估算

在 agent 循环**每次请求模型之前**（含首轮）判断，具体为每轮工具批结束后、下一次请求前：

1. 从消息尾部向前找最后一条 assistant 消息，`stop_reason ∉ {Error, Aborted}` 且 `input+output+cache_read+cache_write > 0` 时，以其为基准：
   `context_tokens = usage_total + Σ estimate(message) (基准之后的消息)`
2. 找不到有效 usage 时：`context_tokens = Σ estimate(message)`。
3. `estimate(message)`：文本/思考/参数 JSON 字符数 ÷ 4 向上取整；图片按 4800 字符；tool result 取内容。**故意偏保守（高估）**。
4. `should_compact = context_tokens > context_window - reserve_tokens`（`context_window == 0` 视为未知，不压缩）。

## 5. 切点

1. 从尾部向前累加每条消息的估算，累计到 `>= keep_recent_tokens` 时停下。
2. 从该位置继续向前找最近的一条 **user 消息**，作为 `cut_index`。
3. 若 `cut_index` 不在第一条之后（没有可摘要的内容），放弃本次压缩（避免空摘要与死循环）。
4. 被摘要范围 = `messages[..cut_index]`（不含 System；我们的 history 无 System 消息）；保留范围 = `messages[cut_index..]`。
5. 新上下文 = **摘要消息** + 保留范围。摘要消息是一条 user 消息，文本为：

```
The conversation history before this point was compacted into the following summary:

<summary>
{summary}
</summary>
```

（前缀/后缀常量放 `timo-session`，读端投影与写端共用。）

## 6. 摘要请求

- 系统提示固定：

```
You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.

Do NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.
```

- 会话序列化（不进 thinking；tool result 截断 2000 字符）：`[User]: …`、`[Assistant]: …`、`[Assistant tool calls]: name(k=v, …); …`、`[Tool result]: …`，段间空行。
- 请求文本：`<conversation>…</conversation>` + 摘要要求。若被摘要范围的第一条是压缩摘要消息（见 §8），先提取其 summary，改用"更新摘要"要求，并附 `<previous-summary>`。
- 摘要格式（固定标题，要求原样输出）：

```
## Goal
## Constraints & Preferences
## Progress（Done / In Progress / Blocked）
## Key Decisions
## Next Steps
## Critical Context
```

- 文件操作清单：扫描被摘要消息里的 assistant 工具调用（`read`/`write`/`edit` 的 `path` 参数）；`modified = write ∪ edit`，`read_only = read − modified`；按路径排序后追加：

```
<read-files>...</read-files>
<modified-files>...</modified-files>
```

- 调用方式：复用当前 `ProviderApi::stream`，`Context { system_prompt: 上面系统提示, messages: [user 请求], tools: [], thinking_level: 当前档位 }`，`StreamOptions` 带已解析 api_key 与当前取消令牌；`max_tokens` 用模型默认。摘要请求**不显示流式输出**、不发 `StreamEvent`。

## 7. 失败语义

下列任一情况视为压缩失败，**不写会话 entry、不改内存上下文**，发 `KernelEvent::Error { context: "compaction: …" }` 后继续当前 run（下一次请求可能仍然超窗，由模型或供应商报错）：

- 摘要请求 `stop_reason ∈ {Error, Aborted, Length}`（length 说明摘要被截断，不能当检查点）；
- 摘要响应里出现工具调用；
- 摘要正文为空；
- 被摘要消息为空（切点不成立）。

手动 `/compact` 失败时向用户打印错误，不改变当前会话。

## 8. 会话 entry 与投影

新增 `kinds::COMPACTION = "session.compaction"`（ADR-0007 预留位）。payload：

```json
{"summary": "…", "first_kept_entry_id": "01J…", "tokens_before": 123456, "tokens_after": 20001,
 "reason": "threshold|manual", "usage": {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0}, "details": {"read_files": [], "modified_files": []}}
```

- `first_kept_entry_id` = 保留范围第一条消息的 entry id。
- entry 的 `parent_id` = 当前 leaf；写完后 leaf = 压缩 entry，后续消息接在它后面。
- 原始 entry 全部保留在文件里（只增不改）；投影时最新压缩 entry 生效。
- `tokens_after` 用估算值；`usage` 记录摘要请求的用量（P1 不计入 `/session` 总量，登记为已知限制）。

投影算法（`timo-session::project_context`）：

1. 取 leaf 链；
2. 找链上最后一条 `session.compaction`；
3. 有的情况：从 `first_kept_entry_id` 在链上的位置开始取消息（找不到则从压缩 entry 之后取）；把摘要消息（§5 的包装文本，`timestamp` = entry 时间）放在最前；压缩 entry 本身跳过；
4. 没有：现状不变（只取 `message` entry）；
5. 多条压缩：最后一条胜出；更早的压缩 entry 与它之前的消息都不再投影。

## 9. 内核接口

- 自动：`Kernel::run` 内的循环按 §4 判断，无需调用方参与。
- 手动：`Kernel::compact(&mut self, instructions: Option<&str>) -> Result<CompactionOutcome>`；没有会话/没有可摘要内容时报错。
  - `CompactionOutcome { summary, tokens_before, tokens_after, first_kept_entry_id }`。
- 事件：`KernelEvent::Compacted { reason: CompactionReason, tokens_before: u64, tokens_after: u64 }`；失败走 `Error`。
- 设置入口：`Kernel::compaction_settings()` 只读快照（桌面/CLI 展示用）。

## 10. CLI / 桌面

- CLI：`/compact [补充要求]` → 调用手动压缩；输出 `[compact] tokens: 123456 → 20001`。完整摘要不落 stdout（在会话文件里）；错误走 stderr。
- 桌面：P1 不做“压缩中”界面；`KernelEvent::Compacted` 到来时聊天区插入一条分隔提示（沿用现有事件订阅，无需新组件）。命令面板的 `/compact` 仍标 Planned，等 GUI 批次接按钮。

## 11. 测试

单测（不进网络）：

1. `estimate`：文本/思考/工具参数/图片/tool result 的字符计数与向上取整。
2. `should_compact`：边界值 `>`、`reserve` 大于窗口、`enabled=false`。
3. 切点：从尾部累计到预算、回退到最近 user、无可摘要内容返回 None、tool result 不会被切在中间。
4. 序列化与文件清单：tool result 截断、thinking 不出现、read/write/edit 去重与排序、XML 段落。
5. entry/投影：压缩 entry 往返；投影得到"摘要 + 保留尾部"；多条压缩只认最后一条；缺 `first_kept_entry_id` 时退化为压缩 entry 之后。
6. 摘要失败：Error/Length/工具调用/空正文四种都不写 entry。

集成（fixture SSE，无真实模型）：

7. 预置 3 轮对话 + 低 `reserve_tokens` 触发压缩：会话文件出现一条 `session.compaction`，随后消息 parent 指向它；同一进程继续提问时上下文已是摘要版。
8. 结束进程后 `resume`：`history()` 第一项是摘要消息，后续消息数与保留段一致。
9. 手动 `/compact` 在草稿会话上报错；在已有会话上成功并打印 token 前后值。

真机（D3 验收步骤）：

10. 把 `reserve_tokens` 设成大于模型窗口的值（如 200000 > 128000）使阈值必然触发；跑两轮后确认文件里有压缩 entry、`/session` 消息数下降、继续提问行为连贯；重启恢复后再问一次，答案引用了摘要里的目标。

## 12. 体量

- `crates/timo-agent/src/compaction/`（估算、切点、序列化、提示）≈ 350 行 + tests；循环接入约 40 行。
- `timo-session` 投影约 60 行；内核手动入口约 80 行。
- 单文件 ≤500 行：若 `mod.rs` 超限，拆 `estimate.rs` / `prompt.rs` / `project.rs`。
