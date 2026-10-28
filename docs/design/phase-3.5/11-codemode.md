# Phase 3.5 / 11：codemode 与嵌套工具调用（P2 第三题）

- 状态：定稿（2026-10-27）
- 范围：pi `core/nested-tool-calls.ts`（263 行）+ `extensions/codemode/`（tool/execute/worker，约 1,500 行）的**行为面**；不含 TUI 渲染器。
- 蓝本（只读，MIT 不得复制）：`NestedToolCallRunner` / `NestedCallRecorder` / `NESTED_CALL_LIMITS`、`createCodemodeToolDefinition` / `executeCodemode` / `createCodemodeDescription` / `toCodemodeIdentifier` / `parseCodemodeSource`。
- 前置：architecture/03 §1（工具元模型与 exposure）、architecture/02 §5（工具管线）、phase-3.5/03（用量聚合，D4 的嵌套合并）、phase-2/00 §宿主线程（rquickjs 单线程驱动）。

## 1. 目标（一句话）

让模型写一段 JavaScript，在沙箱里通过**同一条工具管线**调用其它工具（可并发）、只把脚本输出送回模型，并把嵌套调用的用量合并回本轮工具结果。

## 2. 蓝本行为（只读摘录，不含实现代码）

### 2.1 工具与脚本面

| 面 | 行为 |
|---|---|
| 输入 | 工具参数只有 `code`（**原始 JavaScript**，不是 JSON 字符串、不是代码围栏）；脚本是 async 函数体，顶层 `await`/`return` 可用 |
| 选项行 | 可选的**第一行** `// @options: {"max_output_tokens": 10000, "timeout_ms": 60000}`；出现在其它位置直接拒绝 |
| 全局 | `tools.<name>(args)`、`text()`、`image()`、`console.*`、`return`、`exit()`、`store/load`、`ALL_TOOLS`、`searchTools/describeTool/describeNamespace`、`models` |
| 工具值 | 有 `output_schema` 的工具解析为 `structuredContent`（错误结果带 structuredContent 时也解析）；其余解析为文本拼接；失败/被阻止/参数非法 → 脚本侧 `throw`，可被 `try/catch` 或 `allSettled` 接住 |
| 输出 | `Script completed` / `Script failed` + 实际耗时（1 位小数）+ `Output:`；多个文本项各带 `==> text N/M <==`；`console` 行统一进 `<console_output>` 块；失败保留部分输出，末尾 `Script error:` 与错误、以及"失败前已发生的调用（不回滚）"清单 |
| 命名 | 工具名中非法标识符字符变 `_`（`mcp__dev-radius__search` → `tools.mcp__dev_radius__search`） |
| 可调集合 | 除 `codemode` 自身外的全部工具（含 codemode/deferred 暴露面），脚本不能启动另一个 codemode |
| 描述 | `codemode` 的描述里按命名空间分组列出可调工具的紧凑声明；超预算的、deferred 的由 `ALL_TOOLS`/`describeTool` 兜底 |

### 2.2 嵌套调用

| 面 | 行为 |
|---|---|
| 身份 | 嵌套调用 id 为 `<父调用 id>/<序号>`；不带 `parentToolCallId` 时无法在事件流里区分层级（蓝本带） |
| 管线 | 与模型直调走同一条：参数校验、`tool_call`/`tool_result` 钩子、权限/审阅全部生效；嵌套结果**不进对话消息** |
| 顺序 | 调用方整批串行（`tool_execution=sequential`）或目标工具自身 `executionMode=sequential` 时，嵌套调用经宿主内一把队列串行化；本轮只有 codemode 会嵌套且它不能嵌套自身，所以不存在"持锁再入" |
| 记录 | 每次模型调用一份记录：最多 256 条；单次参数 >8 KiB 或累计 >32 KiB 时省略参数并标记 `complete=false`；错误文本截 500 字符；未结束的调用标记 unfinished；用量求和后并入工具结果 |
| 取消 | 父调用取消 → 仍在跑的嵌套调用标记 cancelled；脚本结束后未等待的 Promise 丢弃 |

### 2.3 资源与限制

| 面 | 行为 |
|---|---|
| 沙箱 | QuickJS；无 Node、文件系统、网络、定时器；内存上限 256 MiB（越界抛 `InternalError: out of memory`） |
| 并发 | 同一进程最多 2 个 worker（可由环境变量覆盖）；取不到 worker 最多等 60 秒；等待时间计入脚本 deadline |
| 输出上限 | 文本与 base64 合计 16,777,216 字符，或 `text()`/`image()`/`console` 合计 100,000 次 → 脚本失败 |
| 截断 | 输出超过 `max_output_tokens`（默认 10,000）时保留头尾、中间标注截断 token 数，全文写临时文件并在结果里给路径 |
| 图像 | `image()` 接受 base64/图像块，落临时文件并在结果里给路径；远程 URL 不接受 |

## 3. 我们的现状与缺口

| # | 缺口 | 现状（已核对） |
|---|---|---|
| G1 | 工具拿不到"调用别的工具"的入口 | `ToolCallCtx` 只有 id/params/cancel/updates/cwd/review；`run_one` 在 `timo-agent` 内部，返回 `Message` 而非 `ToolOutput` |
| G2 | 没有脚本沙箱 | `timo-ext` 的 QuickJS 宿主只跑扩展 bundle（`Runtime`/`Context` 专用线程），不能跑任意脚本；工作区已锁 `rquickjs 0.14`（full-async） |
| G3 | 事件枚举没有父调用字段 | `ToolCallRequested/Started/Update/Finished` 均无 `parent_tool_call_id`；`ToolResultMessage` 没有 `nestedCalls` |
| G4 | 工具描述只有一句话 | `ToolDefinition.description` 是短句；没有"从 JSON Schema 渲染声明"的渲染器 |
| G5 | 工具结果形态不齐 | 内置工具都不带 `output_schema`/`structured_content`（`bash` 只回截断文本）；`read` 的图像以 `Content::Image` 返回 |
| G6 | codemode 没有会话访问面 | 自定义 entry（`custom`）可读写，但工具执行期拿不到会话句柄 |
| G7 | 默认激活集没有它 | `DEFAULT_ACTIVE = read/bash/edit/write`；`--tools`/`default_tools` 是唯一激活通道 |
| G8 | 登记面 | 接缝清单（architecture/03）没有"嵌套工具执行"条目；本设计补 #8 |

## 4. 接口契约

### 4.1 嵌套工具接缝（`timo-tools` 定义、`timo-agent` 实现）

```rust
// timo-tools/src/nested.rs（新）
pub struct CallableTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,          // JSON Schema
    pub output_schema: Option<Value>,
    pub namespace: Option<Namespace>,
    pub prompt_guidelines: Vec<String>,
    pub execution_mode: ExecutionMode,
    pub exposure: Exposure,         // Deferred 默认不进描述；Hidden 由宿主剔除
}

pub struct NestedCall {
    pub id: String,                 // 宿主分配 "<父>/<n>"
    pub parent_id: String,
    pub name: String,
    pub arguments: Value,
    pub cancel: CancellationToken,  // 父取消时一并取消
    pub updates: tokio::sync::mpsc::Sender<ToolOutput>,
}

pub struct NestedOutcome {
    pub output: ToolOutput,
    pub duration_ms: u64,
}

pub enum NestedCallStatus {
    Ok,
    Error,
    Cancelled,
}

pub struct NestedCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: Option<Value>,   // 超限时 None
    pub arguments_bytes: Option<usize>,
    pub status: NestedCallStatus,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,      // 截 500 字符
}

pub struct NestedCallSummary {
    pub calls: Vec<NestedCallRecord>,
    pub complete: bool,
    pub usage: Option<timo_ai::Usage>,
}

pub trait NestedToolHost: Send + Sync {
    fn catalog(&self) -> Vec<CallableTool>;
    fn is_sequential(&self) -> bool;
    fn execute(&self, call: NestedCall)
        -> Pin<Box<dyn Future<Output = NestedOutcome> + Send + '_>>;
    fn snapshot(&self, parent_id: &str) -> Option<NestedCallSummary>;
    fn take(&self, parent_id: &str) -> Option<NestedCallSummary>;
}
```

- `ToolCallCtx` 增 `pub nested: Option<Arc<dyn NestedToolHost>>`；既有构造点一律补 `None`（测试同型补齐，断言语义不动）。
- `catalog()` 返回**除 `Hidden` 外的全部工具**（含 `codemode` 自身）；调用方自行去掉自己，宿主不写死任何工具名。
- `timo-agent/src/run/nested.rs` 实现宿主：持注册表快照、钩子、父取消令牌、cwd、审阅器与"是否整批串行"。`execute` 复用 `run_one` 中抽出的管线段（before 钩子 → 审阅/校验/执行 → after 钩子），但**不发总线事件**、不产生 `Message`。
- 失败语义：注册表错误 / 审阅拒绝 / 钩子阻止一律变成 `is_error=true` 的 `ToolOutput`（结果文本由 `format::failure_output` / `format::script_value` 生成），由脚本侧转成 `throw`；钩子阻止文案保持 `blocked by extension: <reason>`。
- 顺序：`is_sequential()` 或目标工具 `execution_mode == Sequential` 时持有宿主内的一把 `tokio::sync::Mutex<()>` 再执行；并发由脚本泵侧调度（多个 `execute` future 同时进行），宿主不再另起任务。
- 记录：按 `parent_id` 建一份 `NestedCallRecorder`（256/8 KiB/32 KiB/500 字符四条限），`take` 由 agent 在顶层调用结束后取走并把 `usage` 并进该次 `ToolOutput.usage`；`snapshot` 供 codemode 失败时写清单。
- 取消：父令牌取消 → 宿主给所有子调用派生子令牌并取消；`execute` 入口先查父令牌。

### 4.2 `codemode` 工具（`timo-kernel/src/codemode/`）

```rust
// 参数：{"type":"object","properties":{"code":{"type":"string"}},"required":["code"]}
// exposure: ModelOnly；未激活时 Hidden；activation: default_tools 的 "+codemode" 或 --tools
```

- **注册点**：`Kernel::ensure_ready()` 分两步——先在 `apply_allowlist` **之前**注册一个占位定义（使 `--tools codemode` 不被报"unknown tool"），allowlist 之后再按**最终注册表**重建一次：描述来自最终可调集合，暴露面 = 激活时 `ModelOnly`、否则 `Hidden`。`apply_allowlist` 可能把占位项提升为 `Direct`，重建会覆盖掉它。
- **默认关**：不进 `DEFAULT_ACTIVE`；未激活时 `Hidden`，脚本面不存在。
- **描述**：`prompt_snippet = "codemode(code): run JavaScript that calls other tools"`；一条 guideline 说明"批量/串联/过滤大结果优先用它"；`description` 由注册表快照按与运行时 `catalog()` 同源的可调视图渲染：
  - 头部：脚本形态、`await tools.<name>({...})` 的解析规则、选项行；
  - `Globals:` 每行一个全局；
  - `Nested tools:` 按命名空间分组（`Deferred` 默认不列，但仍可调），每个工具一节：``### `id` `` + `name(arg: type, opt?: type) -> result`（参数按名字排序，声明不依赖 JSON Map 的遍历顺序）；
  - 结果类型：`output_schema` 是对象 → 一层字段 `{ a, b? }`，否则 `string`；
  - 预算：估计 token = 字符/4，默认 3,000；无命名空间的组先上，其后按命名空间轮转、每组每轮放最便宜的一个，放不下则该组退出；未列出的工具仍可调，描述末尾注明"用 ALL_TOOLS/describeTool 查未列出的工具"。
- **可调集合**：宿主 `catalog()` 已去掉 `Hidden`；codemode 再从里面去掉自己。名字消毒后冲突按注册表序先到先得（冲突的名字在描述里标注，代理里只保留先者）。
- **值映射**：`output_schema && structured_content` → 结构化值；否则文本块拼接；`is_error` 且无结构化值 → 错误文本；`Content::Image` 转占位文本（D79）。

### 4.3 脚本运行时

- 包装：`globalThis.__timo_main = async function(){ try { const v = await (async function(){ <code> })(); if (v !== undefined) text(v); } catch (e) { if (!(e && e.__timo_exit)) globalThis.__timo_error = describeError(e); } }`；错误文本 = `name: message` + 栈；`exit()` 的哨兵被接住也不写错误（按成功结束）；泵在 promise settle 后取回输出/错误/console。
- 选项行：只认第一行；非首行出现 `// @options:` → 不执行脚本直接失败；值必须是对象，`max_output_tokens` 为正整数，`timeout_ms` 为正整数（未给则不设 deadline）。
- 调用协议（JS 侧只排队，不阻塞）：
  1. `tools.x(args)` 生成 `{id, name, args}`，登记 deferred 的 `resolve/reject`，调用原生 `__timo_post(json)` 后立即返回 Promise；参数无法 JSON 序列化（如循环引用）→ 直接 reject，不发请求；
  2. Rust 泵把排队请求交给异步侧并发执行（顺序规则见 4.1），完成后调用原生 `__timo_settle(id, ok, payloadJson)` 收敛 Promise；
  3. JS 线程在"跑完一轮 JS job 且没有新结果"时阻塞等结果（带 deadline/取消上界），再继续驱动 job。
- 全局（第一版）：`tools`、`text`、`console.*`、`return`、`exit`、`ALL_TOOLS`（`{name, description}`，`description` 即声明）、`describeTool(name)`（接受原名或消毒名）；`image`/`store`/`load`/`searchTools`/`describeNamespace`/`models` 不存在（D73~D76）。
- 输出：`text()` 按序；`console` 在全部文本之后合成一个块；多个文本项加 `==> text N/M <==`；`return` 值按 `text()` 规则追加。
- 失败：`Script failed\nWall time <s> seconds\nOutput:\n` + 已有输出 + `Script error:\n<错误>`；有调用记录时追加 `\n\nTool calls made before the failure (they are not undone): name (status), ...`。
- 中止/超时：`Script aborted: ...` / `Script timed out: ...`；沙箱创建失败：`Script sandbox failed: ...`。

### 4.4 线程与资源

- 每个脚本一条**独立 OS 线程**（`std::thread`，不用 `spawn_blocking`：runtime 关闭会等待卡死的阻塞任务），线程内建 `Runtime`：`set_memory_limit(256 MiB)` + 中断处理器（deadline 或父取消 → 中断）。Rust → JS 的 settle 回调在该线程内调用。脚本线程的完成经 oneshot 交回异步侧；中断后最多等 5 秒（`THREAD_STOP_GRACE`）兜底返回，避免中断处理器失效时调用无界挂起。内存/输出/worker 等限制做成结构体传入、缺省取蓝本值，测试用较小值，避免真分配 256 MiB。
- worker 准入：进程级 `Semaphore(2)`（`OnceLock`）；permit 随线程结束释放；等待上界 `min(timeout, 60s)`；拿不到时给出与蓝本同义的失败文案（"No codemode worker became free within 60 seconds..."）。
- 输出计数在 shim 里做（字符与调用次数），越界抛错；截断在 Rust 侧做（头 50% + 尾 50%），全文写 `std::env::temp_dir()` 下的 `timo-codemode-*.txt`，结果附 `[Full output: <path> (read with offset/limit)]`。
- 取消：父令牌 → 中断 JS + 取消所有子调用；未 settle 的 Promise 丢弃；已发生的工具调用不回滚。超时/结束后的顺序是"先取消子调用、最多等 2 秒让它们清理（例如杀进程树）、再 abort 仍未结束的任务"；子调用用独立派生令牌，超时不会被误报成父取消。`null` 错误值按"多半是沙箱内存上限"给出提示。

### 4.5 事件、可见性与用量

- 嵌套调用**不发** `ToolCallRequested/Started/Finished`（事件枚举无父字段，改它要动 CLI/桌面四处消费端）；codemode 在每次嵌套调用状态变化时经 `ctx.updates` 发一条部分 `ToolOutput`：`content` 喂一行进度文本，`details = {"calls":[{id,name,status,duration_ms,error?}]}`。桌面已有的 `ToolCallUpdate` 呈现可直接显示。
- 嵌套记录不落会话（`ToolResultMessage` 无字段）；只有**用量**经 `ToolOutput.usage` → `ToolResultMessage.usage` 进 D4 的 `Tools/summaries` 桶。
- 外层仍是普通工具：一次 codemode 调用 = 一条 tool result 消息；嵌套工具的文本不直接进模型上下文。

## 5. 批次切分（每批独立门禁 + 提交）

| 批 | 交付 | 关键测试 |
|---|---|---|
| A 接缝 | `timo-tools::nested`；`ToolCallCtx.nested`；`timo-agent` 管线抽出 + `run/nested.rs`（记录/顺序/取消/用量）；接缝文档 #8 | 钩子与审阅生效、阻止文案、id 前缀、串行/并行、取消、记录四条限、用量合并、catalog 过滤 |
| B 沙箱 | `timo-kernel/src/codemode/`（`mod.rs`/`tool.rs`/`describe.rs`/`sandbox.rs`/`format.rs`/`shim.js`）；注册与激活；描述渲染与预算；选项行；globals；输出格式/截断/临时文件；worker 2/60s；内存与输出上限 | 选项行三种形态、输出格式、截断落盘、值映射、未知名、命名消毒、描述预算、失败保留、exit、超时、取消、内存、worker 准入、激活面 |
| C 接线与验收 | CLI/桌面显示嵌套进度；`docs/codemode.md`（面向使用者的脚本手册，如需要）；真机 DeepSeek 两场景；全部变异；报告/看板/CHANGELOG | 真机：并行两命令 → 一次 codemode 结果、两行嵌套记录、用量合并；失败脚本保留部分输出；超时脚本有界结束 |

每批门禁四连不变：fmt / clippy `-D warnings` / workspace tests / `scripts/check-size`；真机密钥不打印。

批 A 已于 2026-10-27 落地：workspace **380 tests**（+10），**11 条变异全红**且逐条确认为断言失败；本批无真机读数（尚无脚本沙箱）。

批 B 已于 2026-10-28 落地：workspace **406 tests**（+26），**19 条变异全红**且逐条确认编译通过、断言失败、改后逐字节还原；本批无真机读数（脚本面接线与真机在批 C）。实现与本文的两处校正：脚本线程用 `std::thread`（见 §4.4）；原生函数名以 `__timo_post`/`__timo_settle`/`__timo_pending_count`/`__timo_take_output` 为准。

批 C 已于 2026-10-28 落地：CLI `run`/交互路径按行缩进打印 `ToolCallUpdate` 的文本块，桌面把同一部分结果按行累积进 trace（上限 400 字符）；`-p` text 模式仍只出终答（既有语义，JSON 模式早已转发事件）。真机 DeepSeek 三场景读数见报告；workspace **413 tests**（+7），批 B 的 19 条加本批 7 条共 **26/26 变异全红**，逐条确认编译通过、断言失败、改后逐字节还原。本批无新增用户手册：脚本语法已由 `codemode` 工具描述覆盖。真机还修掉三处缺陷：Windows 把 `System32\bash.exe`（WSL）误当原生 bash、`Promise.all` 续体未排空即判状态、嵌套超时直接 abort 绕过杀进程树（详见 §4.4 与报告）。

## 6. 验收

### 6.1 断言级（分批落，不合并成"大测试"）

| # | 断言 |
|---|---|
| T1 | 嵌套调用经过 before/after 钩子与审阅；被阻止时脚本收到 `blocked by extension: ...` |
| T2 | id 严格 `<parent>/<n>` 递增；`codemode` 不在脚本可调集合（`tools`/`ALL_TOOLS`/描述）里，`tools.codemode` 抛错 |
| T3 | 串行工具在 `Promise.all` 下仍逐个执行（并发计数恒 1）；并行工具能达到 ≥2 并发 |
| T4 | 父取消后嵌套调用与脚本都在有界时间内结束，结果标 cancelled |
| T5 | 记录：257 次调用只留 256 条且 `complete=false`；参数 8 KiB/32 KiB、错误 500 字符边界 |
| T6 | 用量逐次相加并入 `ToolOutput.usage`，最终进 tool result message |
| T7 | 选项行：首行合法生效；非首行报错且不执行；非法值报错 |
| T8 | 输出：多文本项编号、console 块在末尾、`return` 追加、`exit()` 保留已有输出 |
| T9 | 截断：超过 `max_output_tokens*4` 字符时头尾保留，临时文件内容等于全文 |
| T10 | 值映射：文本工具给字符串、结构化工具给对象、失败工具 throw、错误但带结构化值的工具给对象 |
| T11 | 脚本失败保留部分输出 + 调用清单 + `is_error=true`；成功时 `is_error=false` |
| T12 | 超时与内存越界都有界失败（外层 `tokio::time::timeout` 兜底），文案可区分 |
| T13 | 描述：Hidden 不进、deferred 进 ALL_TOOLS 不默认列出、预算超限的组被退出、`--tools codemode` 与 `+codemode` 均激活且暴露面仍是 ModelOnly |
| T14 | Phase 1/2 既有测试不改一字全绿（D7） |

### 6.2 真机（DeepSeek）

1. 让模型用 codemode 并行跑两条 `bash`（各自打印时间戳）并返回合并结果：断言会话里只有一条 codemode tool result、嵌套调用两条、两条时间戳几乎相同、`usage` 大于零、CLI 只打印一条顶层工具行。
2. 让脚本先 `text("before")` 再调用不存在的工具：结果 `Script failed`、保留 `before`、清单列出已发生的调用；退出码不受影响（工具错误不是进程错误）。
3. 选项行 `timeout_ms: 1000` + 长命令（`sleep 30`；模型拒绝写死循环，改用可确定复现的死等命令）：1 秒量级内返回 `Script timed out`，嵌套调用标记取消，前后 `tasklist` 均无 `sleep.exe` 残留。

### 6.3 变异（每条必须实测变红，且确认是断言失败）

`catalog` 不排除 codemode；Hidden 进 catalog；id 前缀丢失；用量不合并；记录上限失效；before 钩子跳过；审阅跳过；串行锁移除；选项行忽略；截断只留头；`is_error` 映射反转；输出计数上限移除；中断处理器恒假（超时用例由外层 timeout 兜底）；父取消不传递；名字消毒移除。

## 7. 体量

| 面 | 预算 |
|---|---|
| 新增文件 | `timo-tools/src/nested.rs`（~120 行）、`timo-agent/src/run/nested.rs`（~230）、`timo-kernel/src/codemode/{mod,tool,describe,sandbox,format}.rs`（各 150~300）、`shim.js`（~180） |
| 单文件 | 均 < 500 行；`kernel.rs` 只加注册调用，拆分点不够时按既有 `kernel/*.rs` 先例拆 |
| 依赖 | `timo-kernel` 增 `rquickjs`、`tempfile`（均已在 workspace）；`timo-agent`/`timo-tools` 不增第三方；无新 crate |
| 文档 | design 新增 1 篇（11）；本阶段设计稿 00~11 + report = 13 篇，超出 standards/08"每阶段新增 ≤12"1 篇，阶段收尾在报告登记豁免 |
| docs 总数 | 本篇新增 1 篇；工作树当前 81 篇（含并行会话新增），阶段收尾按报告口径复核（软上限 120） |

## 8. 偏离登记（接续 10 的 D71）

| # | 偏离 | 类型 | 说明 |
|---|---|---|---|
| D72 | `codemode` 是内核内置工具（`ModelOnly`、默认关），不是扩展 | b（装配点） | 需要同一条工具管线与专用沙箱线程；内核是唯一装配点；扩展宿主不适合跑任意脚本 |
| D73 | 不做 `models.*`（classifier/image）与 `image()` | d（范围收敛） | 目录没有可执行的分类器/图像模型面；不谎报能力，脚本手册不写这两项 |
| D74 | 不做 `store()`/`load()` 跨调用状态 | d（范围收敛） | 需要给工具开"读分支 + 写 custom entry"的新会话接缝，价值低于本轮核心；登记 backlog |
| D75 | 不做 `codemode.mode=only`、grammar/constrained sampling | d（范围收敛） | mode=only 改写全局声明面，属产品级开关；grammar 依赖供应商约束采样能力 |
| D76 | 不做 `searchTools`/`describeNamespace`；`ALL_TOOLS` 直接携带紧凑声明 | d（范围收敛） | 工具数有限，无 BM25 依赖；脚本可自行过滤名字，命名空间可由前缀得到 |
| D77 | 嵌套调用不发总线事件；记录不落会话，只合并用量 | c（事件/消息格式成本） | 事件枚举无父字段、消息无 nestedCalls；先经 `ToolCallUpdate.details` 呈现，避免动接缝 #1/#2 |
| D78 | 沙箱为"每脚本一条 OS 线程 + rquickjs"，worker 2 与预算为常量；不做 wasm worker / 环境变量覆盖 | a（运行时差异） | 与 timo-ext 宿主同型；预算取蓝本默认值，设置项留待需要时开 |
| D79 | 脚本里 `bash` 只拿到截断文本，`read` 的图像给占位文本 | d（能力缺口） | 内置工具没有 `output_schema`/`structured_content`；补结构化输出与图像通道另立题 |
