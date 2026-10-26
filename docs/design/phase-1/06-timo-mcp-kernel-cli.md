# 设计 06：timo-mcp、timo-kernel 门面与 timo-cli

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0005/0009；architecture/02 §1、04；设计 01~05；pi: coding-agent/src/extensions/mcp/、core/agent-session.ts、cli.ts

## 1. timo-mcp（rmcp 薄封装，ADR-0005）

### 1.1 配置（mcp.json；对齐 pi mcp-servers.ts）

```json
{"servers": {"docs": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-xxx"],
                      "env": {"KEY": "${KEY}"},
                      "timeout": 60, "enabled": true, "exposure": "direct",
                      "tool_exposure": {"dangerous_tool": "hidden"}}}}
```
或 `{"servers": {"remote": {"url": "https://…", "headers": {"Authorization": "…"}}}}`（streamable HTTP）。

- server 名：`^[A-Za-z0-9_-]+$`；`env` 值支持 `${NAME}` / `!cmd`（复用 timo-config ValueRef）；`timeout` 秒、默认 60（每请求；进度通知重置——rmcp 支持则用之，否则请求级超时）。
- 全局 + 项目合并：项目文件仅可覆盖同名 server 的 `enabled/exposure/tool_exposure`（pi 同构）。

### 1.2 生命周期

```
Idle → Connecting → Connected → (RequestFailed→lazy 重连) ； Failed（连续失败后闲置，下次调用重试）
```
- **惰性连接**：进程启动只读配置，首次工具调用/枚举才连接（pi 同构）；stdio 经 rmcp `TokioChildProcess` 传输；HTTP 经 rmcp streamable HTTP 传输。
- 连接失败诊断进事件总线 `Error` 事件（人可读：server 名 + 根因），**不阻断内核启动**（pi 同构：单 server 失败不影响其余）。

### 1.3 工具并入统一管线（接缝 #4 检查点）

- 发现：`list_tools` → 每工具生成 `ToolDefinition`：`name = mcp__<server>__<tool>`（清洗/防撞规则见设计 03 §1）、`parameters` = MCP inputSchema 原样（JSON Schema 通货，无转换）、`namespace = {name:"mcp__<server>", description}`、annotations 透传 MCP hints。
- exposure：默认 **direct**（D9）；`exposure`/`tool_exposure` 覆盖（hidden→注册不可达；deferred/codemode Phase 1 等价"注册不入模型清单"）。
- 调用：`call_tool(<原始工具名>, args, cancel, timeout)`；进度通知→on_update 文本。
- 结果转换（对齐 pi convertMcpResult）：text/image 透传；合并文本 > **20KB** → 中截断（头+尾 + `…N chars truncated…`）+ 前缀警告 + 全文落临时文件 + `[Full output: <path>]` 尾注；`is_error`→result.is_error；structured_content 透传不截断。

## 2. timo-kernel（门面，ADR-0009）

```rust
pub struct Kernel { /* 组装 config/ai/tools/mcp/session/agent */ }
pub struct KernelOptions { data_dir: Option<PathBuf>, work_dir: PathBuf,
                           model_override: Option<(String,String)>, thinking: Option<ThinkingLevel>,
                           api_key: Option<String>, session_dir: Option<PathBuf> }
impl Kernel {
    pub fn new(opts: KernelOptions) -> Result<Kernel, KernelError>;   // 配置→目录→注册表→MCP 惰性
    pub fn events(&self) -> broadcast::Receiver<KernelEvent>;         // 事件总线唯一入口
    pub fn tools(&self) -> Vec<ToolDefinition>;                       // 含 MCP（触发连接）
    pub async fn run(&self, prompt: &str) -> Result<Vec<Message>>;    // 新会话 or 续会话（resume 后）
    pub async fn resume(&mut self, id: &SessionId) -> Result<()>;
    pub fn steer(&self, text: &str); pub fn follow_up(&self, text: &str);
    pub fn abort(&self); pub async fn wait_idle(&self);
    pub fn session_id(&self) -> Option<&SessionId>;
    pub fn list_sessions(&self) -> Vec<SessionInfo>;
}
```
- 薄壳（cli/未来 desktop）**只 import 此 crate**，不穿透内部（architecture/01 分层原则 2）。
- 内部组装：内置工具注册（默认激活集）→ MCP 注册表并入 → 每次会话创建 SessionWriter + AgentLoop。

## 3. timo-cli（非交互冒烟壳，仅验收载体）

```
timo run "把文件 X 中的 A 改成 B 并验证" [--provider P] [--model M] [--thinking L]
        [--api-key K] [--session-dir D] [--cwd D] [--max-turns N]
timo resume <SESSION_ID|last> [--once "补充指令"]      # 恢复会话；无 --once 时读 stdin 逐条
timo sessions [--cwd D]                                 # 列出会话（id/时间/首条消息）
```

- 输出协议（stdout，人读不美饰）：流式文本原样直出；`[tool] name({args 摘要})` 起止行；`[error] …` 到 stderr；usage 摘要在 `AgentFinished` 后一行。
- 交互（resume 的 stdin 模式）：每行一条 user 消息；`/abort` 中止当前轮；Ctrl+C 第一次=abort 当前 run、第二次=退出；EOF=正常退出。
- A1~A6 验收场景均以此壳执行（roadmap §4）；**不做** 参数美饰、进度条、彩色输出（roadmap 不做清单）。

## 4. 错误类型表

| crate | 变体要点 |
|---|---|
| timo-mcp | ConfigInvalid{server, reason} / ConnectFailed{server, source} / CallFailed{tool, source} / Timeout{server} |
| timo-kernel | Config(ConfigError) / Session(SessionError) / Agent(String) / NoModel(ModelNotFound) / Tool(ToolError) |
| timo-cli | 展示层：为上述错误补用户上下文（配置文件路径、建议动作）后 exit code 非 0 |
