# Phase 3.5 / 08：subagent 扩展（D6 标尺）

- 状态：定稿（2026-10-27）
- 关联：roadmap/phase-3.5 §5 D6；ADR-0009/0013；design/phase-2/01（shim 面）、design/phase-2/03（能力与强制点）、design/phase-3.5/06（无头线协议）
- 行为蓝本：pi 1.4.0 `examples/extensions/subagent`（MIT，只读；铁律 7 不复制代码）

## 1. 目标

用我们自己的接缝跑通"子代理"：主会话的一次工具调用 → 起一个 `timo --mode json -p` 子进程 → 子进程有独立上下文、受限工具面与指定模型 → 结果与用量回到主会话。pi 官方 README 明言不内置 sub-agents，它是**扩展示例**；所以 D6 考的不是"内核有没有子代理"，而是**内核的接缝够不够写出一个子代理扩展**。

## 2. 蓝本行为（只读摘录，不含实现代码）

- **代理定义**：`<name>.md` + YAML frontmatter（`name`/`description`/`tools`/`model`），正文即系统提示。用户级目录总是加载；项目级（`.pi/agents/`）只在信任作用域加载，同名覆盖用户级。
- **子进程调用**：`--mode json -p --no-session`，按需追加 `--model`、`--thinking`、`--tools <逗号列表>`、`--append-system-prompt <临时文件>`，任务文本以 `Task: <task>` 作为 prompt。
- **事件消费**：逐行解析 JSONL；每条 assistant 终态消息 `turns += 1`，累加 `input/output/cache_read/cache_write/cost.total`，`contextTokens` 取最后一次 `total_tokens`；工具结果按序记入过程。
- **三种模式**：single `{agent, task}`；parallel `{tasks[]}`（≤8 任务、4 并发）；chain `{steps[]}`，步骤文本里的 `{previous}` 用上一结果替换。
- **失败语义**：子进程退出码非 0 → 工具失败并带 stderr；`stopReason = error/aborted` 原样传播；chain 在第一个失败步停止并指明哪步。
- **取消**：父侧 abort → kill 子进程（TERM，5 秒后 KILL）。
- TUI 展示（折叠/展开、实时进度、主题）不移植（ADR-0002）。

## 3. 缺口与补齐（本批实现范围）

| # | 现状（已核对） | 补齐 |
|---|---|---|
| 1 | 扩展读不了文件：`node:*` 打包期禁（E104），`fs.*` 只有契约没有宿主 API（phase-2/03 §2） | 新增 `pi.readFile(path)` / `pi.listDir(path)`，强制点接 `fs.read` 作用域 |
| 2 | CLI 无 `--no-session` / `--tools` / `--append-system-prompt`（phase-3.5/06 §2 只定了通用参数） | `KernelOptions` 三字段 + CLI flag + 内核装配生效 |
| 3 | 工具结果 `usage` 恒 `None`（`timo-agent/src/run/tools.rs`），子代理用量回不到主会话 | `ToolOutput.usage: Option<Usage>` → `ToolResultMessage.usage`，扩展结果可带 `usage` |
| 4 | `pi.exec` 同步阻塞宿主线程（`host/services.rs` 用 `Command::output()`），JS 里 `Promise.all` 不会真并发 | 新增宿主 `pi.execParallel(requests)`：一次提交多命令，Rust 侧 ≤4 并发 |

登记偏离：

| # | 偏离 | 类型 | 说明 |
|---|---|---|---|
| D51 | 代理定义读取走宿主 `pi.readFile`/`pi.listDir`（`fs.read` 能力），不提供 `node:fs` | a（我们的沙箱没有 OS 面） | phase-2/03 的预留契约在本批落地，强制点唯一 |
| D52 | 系统提示以命令行文本传入（`--append-system-prompt <text>`，`@path` 前缀表示读文件），不写临时文件 | b（免开 `fs.write` 面） | 代理正文已由 `readFile` 拿到，无需宿主写盘 |
| D53 | 子代理实时进度不落地：`pi.exec` 返回才有结果 | c（宿主 API 无流式） | GUI/审阅批次再补 `onUpdate` 流式；不谎报能力 |
| D54 | parallel 由宿主 `execParallel` 实现（≤4 并发），JS 侧不引入并发事件循环 | a（QuickJS 单线程） | 语义与 pi 一致：一次提交、按序回填结果 |
| D55 | 子代理用量计入主会话 `Tools/summaries` 桶（`ToolResultMessage.usage`） | b（pi 只在 TUI 展示） | 用量设计（phase-3.5/03）本就为嵌套调用预留合并入口 |
| D56 | workflow 预设（`/implement` 等 prompt templates）不在本批 | d（排期） | 随 P2 prompt-templates 落；本批只做工具与代理定义 |
| D57 | 子进程可执行文件与超时经 `--ext-flag bin=` / `--ext-flag timeout=` 传入，不读 `TIMO_BIN` 环境变量 | a（QuickJS 沙箱没有 `process.env`） | 蓝本用 env 传二进制路径；我们只有宿主 flag 通道，缺省按 PATH 找 `timo`、超时 600 秒 |
| D58 | 宿主新增 `pi.getAgentDir()`（config 带 `agentDir`）给扩展定位用户级 `agents/` | a（沙箱没有 `os.homedir`/`CONFIG_DIR_NAME`） | 蓝本从 pi 宿主导入 `getAgentDir()`；我们把同一个值经 `HostServices::config()` 交给 JS，缺省空串 |

## 4. 接口契约

### 4.1 宿主文件 API（`fs.read`）

```js
await pi.readFile(path)  // → string（UTF-8）；超过 1 MiB 截断并追加 "\n[truncated]"
await pi.listDir(path)   // → [{ name, isDir }]，不递归，不排除隐藏项
```

- 路径解析按 phase-2/03 §2.2：相对工作目录、绝对路径不得越界、`~` 展开、符号链接解析、Windows 大小写折叠。
- 拒绝码：未声明/未授予 → `E401`；越界或不在声明作用域 → `E403`；硬拒路径（`.env*`、`.ssh/**`、`.git/**`、凭据文件）→ `E404`。三类都产生诊断（capability=`fs.read`，callSite=`readFile`/`listDir`）并在 JS 侧抛 `PermissionError`。
- 其它 IO 失败（不存在、OS 权限）：JS 抛 `Error`，`code = "E303"`，message 带路径；不记权限诊断（不是安全事件）。
- 声明 `"fs": {"read": ["./agents"]}` 等价 `"./agents/**"`；用户级代理目录（`<dataDir>/agents`）需在声明里给绝对路径，越界检查以工作目录为根的规则对**读**放开为"声明即可"（读不写，风险中）。

### 4.2 CLI 新参数

```text
--no-session                      本次运行不落会话文件
--tools a,b,c                     只把这些工具声明给模型（内置/扩展/MCP 名）
--append-system-prompt <text|@p>  追加一段系统提示；@path 表示读取文件内容
```

- `--no-session`：不建会话文件，`session_id` 为 None；JSON 首行 `"session_id": null`，其余事件不变；与 `/export`、`resume` 互斥（同时给出 → 用法错误 2）。
- `--tools`：列出的名字就是**最终允许集合**——命中即声明给模型（原本被 `default_tools` 置 Hidden 的也恢复为可声明，对齐 pi 的正向 enable 语义），未命中的置 `Exposure::Hidden`；注册表不变，只改声明面。未知名 → stderr 逐条 `[warn] unknown tool \`x\``，不影响其余；`--tools ""` → 用法错误 2。
  - 真机依据（批④）：示例 `scout.md` 声明 `tools: read, grep, find, ls`，早期"只做减法"的实现里 grep/find 仍是 Hidden，子代理回答"没有 grep/find 工具可用"并拒绝取证——只减不增的口径与蓝本不符，故按 enable 语义修正。
- `--append-system-prompt`：文本作为最后的附加节接在系统提示尾部（与 `APPEND_SYSTEM.md` 同一节实现）；`@path` 读不到 → stderr 报错退出 2；空文本 → 忽略并警告。
- 资源告警在无头路径也必须出口（`-p`/`--mode json` 收尾打印，失败提前返回同样打印）：父代理只能看到子进程的 stderr，静默等于把缺陷藏起来。

### 4.3 工具用量回报

- `ToolOutput` 增 `usage: Option<timo_ai::Usage>`；`run/tools.rs` 写入 `ToolResultMessage.usage`。
- 扩展 bundle 的工具结果可带 `usage`（字段与 `timo_ai::Usage` 的 serde 形态一致：`input`/`output`/`cache_read`/`cache_write`/`cost.total`/`total_tokens`）；缺省 `None`。
- 归属：走 phase-3.5/03 的 `Tools/summaries` 桶 → 主会话 `/session` totals 含子代理 token（D55）。内建工具与 MCP 不带 usage（现状不变）。

### 4.4 宿主并发执行

```js
await pi.execParallel([{ command, args, timeoutMs }, ...])
// → [{ stdout, stderr, code }, ...]，顺序与入参一致
```

- 能力仍是 `exec`（不新增能力位）；未授予 → 整批拒绝并诊断 E401。
- 任务上限 8、并发上限 4（与 pi parallel 模式同值）；超限 → JS 抛 `Error("too many parallel commands")`。
- 取消：内核 abort 后未启动的不再启动，已启动的按 `pi.exec` 既有中断路径 kill；单个子进程失败不影响同批其它任务（结果里带非 0 code）。

## 5. 扩展本体（`examples/extensions/subagent`）

- **注册工具** `subagent`（`label: "Subagent"`），参数：
  `{ agent?: string, task?: string, tasks?: [{agent, task}], chain?: [{agent, task}] }`。
- **发现**：`<dataDir>/agents/*.md` 先、`<cwd>/.timo/agents/*.md` 后；同名后者覆盖前者。`dataDir` 经宿主 `pi.getAgentDir()` 取得（D58，缺省空串则跳过用户级）。frontmatter 取 `name`/`description`/`tools`（逗号串或数组）/`model`；正文 = 系统提示；缺 `name` 用文件名。坏文件跳过并 `ctx.ui.notify` 一次（我们没给扩展 `pi.ui`）。
- **模型解析**：`provider/id` 拆成 `--provider` + `--model`；裸 `id` 只给 `--model`，唯一性与歧义判定收在内核一处（歧义时子进程非 0 退出并在 stderr 列出候选，扩展原样转述）——扩展自己不再数一遍候选，避免两处口径。
- **调用**：`pi.exec(bin, ["--mode","json","--no-session","--thinking",<档位>, ...(tools?["--tools",tools.join(",")]:[]), ...(model?(hasSlash?["--provider",p,"--model",id]:["--model",id]):[]), "--append-system-prompt", text, "-p", `Task: ${task}`], { timeoutMs })`。
  - `-p` 在我们的 CLI 里是**取值**参数（可重复），任务文本必须紧跟它，不能像蓝本那样放末尾位置——批④ 真机踩过：拼错的子进程只打出用法错误，主会话拿到 `0 turn(s), 0 tokens`。
  - `bin` 取 `--ext-flag bin=`（D57），缺省按 PATH 找 `timo`；单次超时取 `--ext-flag timeout=`，缺省 600000ms。
- **解析**：逐行 JSON。`type=="message" && message.role=="assistant"` → `turns+=1`、累加 usage、记 `contextTokens`；`type=="tool_execution_end"` → 记一步过程；`type=="agent_end"` → 取最后一条 assistant 文本为 output；`code!=0` → 失败并附 stderr 尾部 4 KB。
- **返回**：`content` = 文本摘要（代理名、turns、答案，超 24 KB 截断）；`details` = 结构化结果（agent/source/model/exitCode/usage/steps，留给 GUI）；`usage` = 聚合值（§4.3）。
- **模式**：single 直接跑；parallel 用 `execParallel`（每个任务一条子进程命令）；chain 顺序跑并替换 `{previous}`，首个失败即停。
- **示例代理**：`examples/agents/scout.md`（`tools: read, grep, find, ls`）、`examples/agents/reviewer.md`（再加 `bash`），仅作验收夹具与文档样例，不进内核仓库的运行时路径。

## 6. 验收（D6）

1. 单测：`readFile`/`listDir` 的三条拒绝码 + 正常读；`execParallel` 顺序与上限；`ToolOutput.usage` 进 `ToolResultMessage` 与 `Tools/summaries` 桶；`--tools` allowlist 后模型可见工具集；`--append-system-prompt` 文本出现在系统提示尾部；`--no-session` 不落文件且 JSON 首行 `session_id: null`。
2. 集成：打包并安装示例扩展（`timo ext`），fixture SSE 驱动一次 single 调用 → 工具结果含 usage，`/session` 的 `Tools/summaries` 出现子代理 token。
3. 真机（DeepSeek，临时数据目录）：
   - 放一个 `scout.md`（`tools: read, grep, find, ls`，model 指向 deepseek-flash），主会话里让模型调用 `subagent` 完成一次侦察；
   - 断言：子进程独立会话（临时目录里不产生子会话文件）、工具面受 `--tools` 约束、返回文本含答案、`/session` totals 比调用前增加了子代理 token；
   - chain 模式跑 `scout → reviewer`，确认 `{previous}` 替换与失败即停。
4. 取消：主会话 abort 后子进程消失（`tasklist`/`pgrep` 读数为 0），工具结果报 aborted 而非挂住。
   - 实测形态（批④）：无头进程在管道里收不到 Ctrl+C，`taskkill` 走的是 TerminateProcess 不是 abort 路径，所以这一项用**同一套 kill 代码**的超时通道验：`--ext-flag timeout=1500` 让 scout 子进程超时 → 工具结果 `FAILED exit -1: timeout`，跑完 `Get-Process timo` 读数为 0（无孤儿）。abort 通道另有进程内单测 `a_raised_abort_flag_cancels_pi_exec`（置位后 kill 并报 `cancelled by host`）。桌面/交互 Ctrl+C 的真机触发留 Phase 4 与 `Ctrl+C 130` 一起补。

## 7. 体量

- `timo-ext`：宿主 fs API + `execParallel` ≈ 180 行 + 测试；shim 面 +20 行。
- `timo-kernel`：`KernelOptions` 三字段与装配 ≈ 60 行；`timo-tools`/`timo-agent` 各 +5 行。
- `apps/timo-cli`：三个 flag ≈ 30 行。
- `examples/extensions/subagent`：index.ts ≈ 250 行（自研，不进 workspace 构建；验收脚本装载）。
- 新增文档 1 篇（本文件），docs 总数随批次登记豁免。
