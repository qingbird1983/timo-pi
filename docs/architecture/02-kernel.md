# 架构 02：内核设计

- 状态：定稿（crate 划分为预留命名，Phase 1 定稿可微调，需 ADR）
- 日期：2026-10-27（2026-10-26 初稿；2026-10-27 按实现校正事件集、管线次序与取消覆盖）
- 关联：ADR-0001/0005/0006/0009；architecture/01、03、05

## 1. crate 划分与职责

| crate | 职责 | 对应 pi 参照 |
|---|---|---|
| timo-ai | 协议 trait + anthropic-messages / openai-completions 实现；模型目录（数据文件）；认证枚举（api-key / env / 命令 / OAuth 预留）；重试与流式 | pi-ai |
| timo-agent | agent 循环（消息→模型→工具调用→回填→再请求）；**事件总线**；取消传播 | pi-agent-core |
| timo-tools | ToolDefinition 元模型 + 内置工具（read/write/edit/bash·powershell/glob/grep）；命名空间与 exposure | coding-agent/core/tools |
| timo-session | 会话 entry 读写、树结构、恢复；磁盘格式唯一所有者 | session-manager / agent-session |
| timo-mcp | rmcp 薄封装：server 发现→工具并入统一管线 | pi-mcp |
| timo-ext | QuickJS 宿主、rolldown 打包管线、pi 兼容 shim、诊断 | coding-agent/core/extensions |
| timo-config | 设置文件、模型/供应商解析、凭证引用（密钥本体不入配置文件） | coding-agent/core/settings |
| timo-kernel | 门面：组装以上模块，输出唯一公共 API | coding-agent/src/core/sdk.ts |

## 2. 核心数据类型（草案，Phase 1 详设定稿）

- **Message**：user / assistant / tool_call / tool_result / system，带 provider 无关的统一表示（对齐 pi 的 AgentMessage 思路：内部全程统一消息，仅在协议边界转换）。
- **StreamEvent（富枚举，接缝 #6）**：TextDelta / ThinkingDelta / ToolCallDelta{index,...} / ToolCallComplete / Usage{input,output,cache} / Done / Error。
- **ToolDefinition**：name（命名空间强制）、description、parameters: JSON Schema、execute(ctx, params, cancel_token, on_update)、exposure、annotations。
- **SessionEntry**：见 architecture/05。
- **AuthMethod（枚举）**：ApiKey{value_ref} / EnvVar{name} / Command{cmd} / OAuth{flow}（Phase 5+ 实现，枚举先占位）。

## 3. 事件总线（全项目最重要的一条设计）

- 内核在所有关键节点 **emit 事件**，不在循环里硬编码任何 hook 调用点；扩展宿主、GUI、日志都是订阅者。
- MVP 事件集（emit 全集、消费方渐进）：agent_started/finished、turn_started/finished、message_added、stream_event、tool_call_requested（**仅通告**；veto 走 `LoopHooks` 请求-响应，见 §4）、tool_call_started/update/finished、provider_request（元数据）、provider_response、compacted（实现名 `Compacted`，Phase 3.5 已落地）、error（**仅循环级**：压缩失败、MCP 装配失败、系统提示装载失败；供应商/认证/流错误走 `stop_reason=Error` 终态消息，不进总线）。
- 事件携带 payload **快照**（不可变），订阅者不得直接改内核态；需要修改的节点（如 tool_call veto）走显式请求-响应式 API。
- 语义约束（Phase 2 落地）：单订阅者 handler 预算 30s；abort 后订阅者退休；payload 与内核态隔离（detach 拷贝）——参照 research/04 §4。

## 4. 工具执行管线（统一入口）

```
模型请求工具
→ tool_call_requested 事件（仅通告）
→ LoopHooks::before_tool_call（扩展可 veto / 改参 / 终止；Phase 2）
→ 按完整名查表 → 校验参数（JSON Schema）
→ 执行（取消令牌；审阅器仅在 write/edit/bash 内部调用）
→ LoopHooks::after_tool_call → tool_call_finished 事件 → 回填消息流
```

> **2026-10-27 修订（管线次序按实现校正）**：实现无独立「命名空间路由」层——前缀在注册期拼好，运行期按完整名查表，且注册期**无前缀强制校验**；「权限/信任检查」**不是管线环节**，审阅器只在写类工具内部触发（`read`/`grep`/`find` 不经过）；Schema 校验发生在 veto **之后**。`builtin_` 前缀不存在，MCP 实际命名为 `mcp__<server>__<tool>`（双下划线）。进度回调通道已建但当前无生产者。清偿项见 `docs/roadmap/audit-debt.md` B2/B3/B6。

MCP 工具与扩展工具与内置工具走**同一条管线**（对齐 pi 行为，research/02 §3）。

## 5. 取消与错误

- CancellationToken 从用户动作一路传到 HTTP 流、子进程、工具执行；中断后会话文件必须完好（append-only 保证）。
- **2026-10-27 修订（覆盖范围按实现校正）**：内置工具（`shell`/`write`/`edit`）、HTTP 发送与流读取、子进程（Windows `taskkill /T`）已接取消；**MCP 工具执行与扩展工具执行当前未接** `CancellationToken`——前者完全不响应取消，后者走 `AtomicBool` + QuickJS 中断处理器。非 Windows 子进程无进程组清理。清偿项见 `docs/roadmap/audit-debt.md` B4。
- 库层错误 thiserror 类型化；壳层加上下文展示；一切错误进事件总线（error 事件），不 panic（standards/04）。
