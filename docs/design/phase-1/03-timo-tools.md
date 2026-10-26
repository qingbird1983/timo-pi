# 设计 03：timo-tools —— 工具元模型与内置工具

- 状态：定稿
- 日期：2026-10-26
- 关联：architecture/02 §2/§4、03 接缝 #4/#5；ADR-0003；pi: packages/coding-agent/src/core/tools/

## 1. ToolDefinition（元模型，接缝 #4 一步到位）

```rust
pub struct ToolDefinition {
    pub name: String,                    // LLM 调用名；内置工具裸名（read/bash/…）
    pub label: String,                   // 人读标签（UI/日志）
    pub description: String,             // LLM 描述
    pub prompt_snippet: Option<String>,  // 系统提示"Available tools"一行
    pub prompt_guidelines: Vec<String>,  // 系统提示 Guidelines 追加条目
    pub parameters: Value,               // JSON Schema（接缝 #5 唯一通货）
    pub output_schema: Option<Value>,    // structuredContent 的 schema
    pub exposure: Exposure,              // Direct(默认)/ModelOnly/Codemode/Deferred/Hidden
    pub namespace: Option<Namespace>,    // {name, description}（mcp server 组）
    pub annotations: Annotations,        // read_only/destructive/idempotent/open_world 提示
    pub execution_mode: ExecutionMode,   // Parallel(默认)/Sequential
    pub execute: Arc<ToolExecute>,       // async (tool_call_id, Value, CancellationToken,
}                                        //      update_cb) -> ToolOutput
pub struct ToolOutput { content: Vec<Content>, details: Value, structured_content: Option<Value>,
                        is_error: bool, terminate: bool }
```

- 工具**抛错语义**：execute 返回 `Err(AgentError)` → 循环转错误 result（is_error=true）；`is_error:true` 的 Ok = 报告失败但保留 details/structured_content（pi 同构）。on_update 在 settle 后到达的更新被丢弃（含排队冲刷，pi 同构）。
- **统一注册表**（接缝 #4 检查点）：`ToolRegistry` 持 `HashMap<String, RegisteredTool>`；内置/MCP/（Phase 2）扩展工具同表存放，仅名字前缀不同（`mcp__<server>__<tool>`）；注册表对来源不可区分。
- 命名空间前缀规则：MCP 工具名清洗 `[^A-Za-z0-9_]→_`；超 64 字符或冲突 → `name[0:56]_` + `sha256("server\0tool")[0:8]`（pi 同构）。内置工具默认激活集 = `["read","bash","edit","write"]`（pi DEFAULT_TOOL_NAMES），其余经 settings `default_tools`（`+name/-name`）增删。

## 2. 截断常量（pi truncate.ts，全工具共用）

| 常量 | 值 | 用途 |
|---|---|---|
| MAX_LINES | 2000 | 行数上限 |
| MAX_BYTES | 51200（50KB） | 字节上限（先到先赢） |
| GREP_MAX_LINE | 500 | grep 匹配行截断 |

- 头截断（keep start，不切半行）：read/grep/find；尾截断（keep end，允许半行）：bash；中截断（head+tail+`…N chars truncated…`）：MCP 结果。
- 头截断尾注：`\n\n[Showing lines A-B of T. Use offset=C to continue.]`（字节触发达加 "(50KB limit)"）。

## 3. 内置工具契约（pi 参照：各同名 .ts；除 D1/D4 外行为逐条对齐）

### read
- 参数：`{path: string, offset?: u64(1 起始行), limit?: u64(最大行数)}`。
- 文本：utf-8 读入按 `\n` 分；`offset` 越界→错误 `Offset N is beyond end of file (M lines total)`；头截断（2000 行/50KB）；超长单行（>50KB）→不返回内容，给 sed 提示文案。
- 图像（jpg/png/gif/webp/bmp 按魔数嗅探）：base64 ImageContent + 说明文本；默认自动缩放（image crate；上限 2000×2000、jpeg 质量 85——pi 默认 profile），关闭经 settings `images.auto_resize=false`。
- 错误：`Could not read file: X`（含 OS 错误码）。

### write
- 参数：`{path, content}`；父目录递归创建；覆盖写；成功 `Successfully wrote to <path>`。
- 与 edit 共享**每绝对路径互斥队列**（串行化同文件变更）。

### edit
- 参数：`{path, edits: [{old_text, new_text}]}`（一次多编辑）。
- 精确优先：先原样 `indexOf`；未中→**模糊匹配**（NFKC；逐行 trimEnd；智能引号→ASCII；Unicode 破折号族→`-`；NBSP 族→空格）——模糊命中时仅回写受影响行。
- 校验（抛错）：old_text 空 / 未找到（"must match exactly including all whitespace"）/ 命中 N>1（must be unique）/ 编辑区间重叠 / 无变化（no-op）。
- 匹配一律对**原文件**一次性完成、按偏移倒序应用；BOM 剥离后还原；CRLF 检出并保留。
- 成功：`Successfully replaced N block(s) in <path>.`；details 含 diff（+/− 行号格式）与 unified patch。

### bash
- 参数：`{command: string, timeout?: u64 秒（**无默认超时**，上限 2^31−1 ms）}`。
- 执行：经配置 shell（`settings.shell_path`；Windows 未配置时探测 bash→PowerShell 顺序）；进程组杀树（超时/取消）。
- 输出：**尾截断**（最后 2000 行/50KB，允许半行）；空输出→`(no output)`；截断→`[Showing lines A-B of T. Full output: <tempfile>]`（临时文件前缀 `timo-bash`）。
- 退出码非 0→is_error + `\n\nCommand exited with code N`；超时→错误含已捕获输出 + `Command timed out after N seconds`；取消→`Command aborted`。
- 子进程环境：默认注入 `TIMO_SESSION_ID/TIMO_PROVIDER/TIMO_MODEL`（对齐 pi PI_*，命名随项目）；透传父环境。

### powershell
- 与 bash 同一执行机（同参数 schema）；固定 PowerShell + `[Console]::OutputEncoding=UTF8` 前导；临时文件前缀 `timo-powershell`。（Windows 支持的正式通道；Phase 1 注册但不在默认激活集——pi 同构。）

### grep（pi grep.ts；D1：ignore+regex 原生实现）
- 参数：`{pattern, path?(默认 cwd), glob?, ignore_case?=false, literal?=false(关则 regex), context?=0, limit?=100}`。
- 遍历：`ignore` walker（尊重 .gitignore 与隐藏文件规则、`--hidden` 语义对齐）；正则方言 = Rust regex（ripgrep 同引擎）。
- 输出：`relpath:LINE: text`（上下文行 `relpath:LINE- text`，组间 `-` 分隔）；匹配行超 500 字符截断 + `... [truncated]`；总输出 50KB 头截断；达到 limit → `[N matches limit reached. Use limit=2N for more, or refine pattern]`；无匹配→`No matches found`。

### find（pi find.ts；即 roadmap 所称"glob"；D1 同上）
- 参数：`{pattern(glob), path?, limit?=1000}`；pattern 含 `/`→全路径匹配（`**/` 前缀；Windows 路径分隔归一）。
- 输出：posix 相对路径逐行；50KB 截断；limit 注记同 grep；空→`No files found matching pattern`。

### 显式不做
`ls`（D4，backlog）；read 的 outputSchema 结构化输出、bash 的 outputSchema（structuredContent）——结构占位延后（D10；契约行为不受影响）。

## 4. 系统提示组装

Phase 1 固定骨架：身份行（Timo-pi agent）+ 环境节（cwd/平台/shell）+ `Available tools`（各工具 prompt_snippet）+ Guidelines（各工具 prompt_guidelines）。文案**逐条对齐 pi** 对应工具的 snippet/guidelines（移植其文本，MIT）。pi 的 sections 机制 Phase 2 增补。

## 5. 错误类型表

| 变体 | 触发 |
|---|---|
| ToolNotFound{name} | 注册表未命中 |
| InvalidParams{reason} | JSON Schema 校验失败 |
| Io{path, source} | 文件系统（read/write/edit/grep/find） |
| Subprocess{kind, source} | bash/powershell spawn/等待失败 |
| Timeout{seconds} | bash/powershell 超时 |
| Cancelled | token 触发（转 "Command aborted" 等） |

错误统一以 `Err` 返回给循环（转错误 result）；`is_error=true` 的 Ok 仅用于模型可读失败报告。
