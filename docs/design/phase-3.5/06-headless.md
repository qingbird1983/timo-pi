# Phase 3.5 / 06：无头模式（print / JSON）

- 状态：定稿（2026-10-27）
- 关联：ADR-0009（内核即库）；pi `modes/print-mode.ts`、`modes/json-event.ts`（只读参照）；D5/D6

## 1. 目标

提供脚本/CI/子进程消费的单次执行模式：

- `timo -p "<任务>"`：只输出最终回答（text）；
- `timo --mode json -p "<任务>"`：逐行输出 JSON 事件流（stdout），供程序解析。

两种模式都走完整内核（工具、扩展、会话持久化），不是旁路。

## 2. CLI 形态

```text
timo -p "<prompt>" [--mode text|json] [--cwd DIR] [--provider P --model M]
     [--thinking LEVEL] [--session-dir DIR] [--api-key KEY]
     [--extension PATH]... [--no-extensions] [--ext-flag NAME=VALUE]... [--max-turns N]
```

- `-p/--print` 可重复：按顺序在同一会话里执行多条 prompt（相当于 pi 的 messages 数组）。
- `--mode` 缺省 `text`；与 `-p` 一起使用。给出 `-p` 时忽略子命令；`-p` 与子命令同时出现 → 用法错误。
- 沿用现有 `run/resume/sessions/ext` 子命令；`timo run` 的人工输出（流式正文 + `[tool]` + `[usage]`）保持不变。
- 缺 `-p` 且无子命令 → 打印用法，退出码 2。

## 3. text 模式

- stdout：仅最终 assistant 消息的 text 块，每条 prompt 一段，末尾换行；不打印 `[tool]`、`[usage]`、流式增量。
- stderr：错误、资源告警、扩展降级报告。
- 终止原因：
  - `Stop` → 0；
  - `Length`（输出被截断）→ stderr 提示，**1**；
  - `Error` / `Aborted` → stderr 原因，**1**。
- 多条 prompt：前一条失败即停止，不再执行后续 prompt。

## 4. JSON 模式

### 4.1 线协议

- stdout 每行一个 JSON 对象（JSONL，UTF-8，逐行 flush）；stderr 只放进程级错误。
- 第一行恒为会话头（与 pi 相同）：

```json
{"type":"session_header","v":1,"session_id":"01J…","cwd":"…","model":"deepseek/deepseek-chat"}
```

- 末行之后进程退出，退出码同 §3。

### 4.2 事件映射

| 内核事件 | JSON `type` | 载荷 |
|---|---|---|
| `AgentStarted` | `agent_start` | `{}` |
| `TurnStarted` | `turn_start` | `{}` |
| `StreamEvent::Start` | `message_start` | `message`（partial 快照） |
| `StreamEvent::{BlockStart,Delta,BlockEnd,Done,Error}` | `message_update` | `usage` + `assistant_message_event`（**去掉 `partial`**） |
| `MessageAdded` | `message` | `message`（完整消息，含 user/tool result） |
| `ToolCallRequested` | `tool_call_requested` | `call` |
| `ToolCallStarted` | `tool_execution_start` | `tool_call_id`, `tool_name`, `args` |
| `ToolCallUpdate` | `tool_execution_update` | `tool_call_id`, `tool_name`, `partial` |
| `ToolCallFinished` | `tool_execution_end` | `tool_call_id`, `tool_name`, `result`, `duration_ms` |
| `ProviderRequest` | `provider_request` | `provider`, `model` |
| `ProviderResponse` | `provider_response` | `provider`, `model`, `usage` |
| `Compacted`（P1） | `compaction` | `reason`, `tokens_before`, `tokens_after` |
| `Error` | `error` | `context` |
| `TurnFinished` | `turn_end` | `message`, `tool_results` |
| `AgentFinished` | `agent_end` | `messages`（本次新增） |

- `message_update.assistant_message_event` 保留内核枚举的 `type` 与字段（`start/block_start/delta/block_end/done/error`），只剔除 `partial`；`toolcall_start` 额外补 `id` 与 `tool_name`（从 partial 当前块取）。
- 字段命名：内核结构体 serde 输出（snake_case），不做二次改名；`usage` 用 `timo_ai::Usage` 的现有字段。
- 顺序保证：`agent_start` 恒为首事件、`agent_end` 恒为末事件（含错误/取消）；错误也一定以 `agent_end` 收尾。
- 未知/新增事件类型：消费者必须忽略未知 `type`（向前兼容）。

### 4.3 示例（节选）

```json
{"type":"session_header","v":1,"session_id":"01J8…","cwd":"D:\\work\\demo","model":"deepseek/deepseek-chat"}
{"type":"agent_start"}
{"type":"turn_start"}
{"type":"message_update","usage":{},"assistant_message_event":{"type":"delta","index":0,"delta":"暗号"}}
{"type":"tool_execution_start","tool_call_id":"call_1","tool_name":"read","args":{"path":"README.md"}}
{"type":"tool_execution_end","tool_call_id":"call_1","tool_name":"read","result":{…},"duration_ms":3}
{"type":"agent_end","messages":[…]}
```

## 5. 取消与退出

- 第一次 Ctrl+C：调用 `Kernel::abort`，等待当前 run 收尾（最多 5 秒）后退出 **130**；JSON 模式下错误路径仍以 `agent_end` 收尾。
- 第二次 Ctrl+C：立即退出 130（不再清理）。
- 不注册 SIGTERM 之外的其他信号（Windows 只有 Ctrl+C；与现有 CLI 行为一致）。

## 6. 会话与安全

- 默认创建并持久化会话（与 `timo run` 相同）；`--session-dir` 可指定目录（P1 起即按项目分区，见 07）。
- 无 UI → `tool_review = None`（工具调用自动执行）。文档与 `--help` 明示该风险；需要审阅的场景使用桌面端。
- 密钥优先级不变（`--api-key` > 配置引用 > 环境变量）；JSON 输出**不含**密钥、请求头与完整 base_url 之外的凭证。

## 7. 与 D6 的关系

扩展（subagent）用子进程调用 `timo --mode json -p "<任务>"`：

- 子进程消费 `agent_end.messages` 取最终回答，`provider_response.usage` 汇总子代理用量；
- 退出码用于失败判定；
- 父进程不传递会话 id（P1 子进程独立会话），`cwd` 由调用方指定。

## 8. 实现落点

- `apps/timo-cli/src/main.rs`：`Cli` 增加 `-p/--print`（Vec）与 `--mode`；`main` 在 `print` 非空时走 `run_print`；`run_print` 复用 `Kernel::run` 与现有事件订阅。
- 新增 `apps/timo-cli/src/json_mode.rs`：`JsonWriter`（逐事件映射 + 逐行 flush）与 `session_header`。text 模式只取 `AgentFinished.messages` 最后一条 assistant 的 text。
- 内核不新增接口；只依赖 `KernelEvent` 与 `Compacted`（04）。

## 9. 测试

1. `--mode text` fixture：stdout 只有最终文本；`[tool]` 不出现在 stdout；错误退出 1。
2. `Length` 终止：stderr 有截断提示、退出 1。
3. `--mode json` fixture：第一行 header；首/末事件类型正确；`message_update` 不含 `partial`；`toolcall_start` 带 `id`/`tool_name`；每行可被 `serde_json` 解析。
4. 多条 `-p`：按顺序执行，会话消息累计；中途失败停止。
5. 用法错误（`-p` + 子命令、无提示无子命令）：退出 2，stderr 说明。
6. 真机：`timo --mode json -p "回答两个字"` 在 DeepSeek 上产出合法 JSONL，`jq` 可逐行解析，退出 0。
7. 取消：启动长请求后发送 Ctrl+C（真机手工），退出 130，JSON 流以 `agent_end` 结束。

## 10. 体量

- `json_mode.rs` ≈ 180 行 + tests；`main.rs` 增量 ≈ 80 行（仍 <500）。
- 不新增依赖。
