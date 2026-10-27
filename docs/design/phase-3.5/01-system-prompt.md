# 设计 01：系统提示与项目上下文（P0）

- 状态：定稿（2026-10-27）
- 关联：00-overview K2/K3/K4/K9/D30/D35/D36；research/06 §2
- 落点：`crates/timo-kernel/src/prompt/`（现 `prompt.rs` 59 行，升级为目录）

## 1. 现状与目标

现状（Phase 1）：`prompt::build_system_prompt(cwd, registry)` 返回一段平铺字符串——身份行 + 环境 + "Available tools" + "Guidelines"，无分节、无项目上下文、无追加、无自定义替换。

目标：同一函数位置升级为**分节装配**，输出仍是整段字符串（K2），但内部按节建模，便于 Phase 4 扩展改写与 P1 的提示缓存诊断。

不做：不把 system 消息写进会话；不做按节补丁消息；不做提示缓存（P1）。

## 2. 数据模型

```rust
pub(crate) struct Section { pub name: String, pub content: String }

pub(crate) struct SystemPrompt {
    /// 按构建顺序保存；`preamble` 永远第一且不带标签。
    sections: Vec<Section>,
    /// `force_system_prompt` 生效时的整段替换（此时 sections 为空）。
    forced: Option<String>,
}

pub(crate) struct ContextFile { pub path: PathBuf, pub content: String }

pub(crate) struct PromptInputs<'a> {
    pub cwd: &'a Path,
    pub custom_prompt: Option<&'a str>,
    pub append_system_prompt: Option<&'a str>,
    pub tools: &'a [&'a ToolDefinition],   // 已按注册顺序过滤过 declared
    pub prompt_guidelines: &'a [String],  // 预留：扩展追加（P4）
    pub context_files: &'a [ContextFile],
    pub skills: &'a [Skill],              // 02 定义；P0 先接空切片
    pub docs_dir: Option<&'a Path>,       // 缺失则整节省略（D35）
    pub extra_sections: &'a [(String, String)],
}
```

`SystemPrompt` 对内核暴露两个只读方法：`render() -> String`、`sections() -> &[Section]`（诊断与测试用）。`forced` 只由 `before_agent_start` 钩子产生（见 §9）。

## 3. 构建算法（顺序即合同）

1. **整段替换**：`force_system_prompt` 非空 → `forced = Some(值)`，跳过其余全部步骤。
2. **preamble**：`custom_prompt` 非空 → 原样作为 preamble，且**不生成** `tools`/`rules`/`docs` 三节；否则写入默认身份行（本项目自写，语义对齐：编码助手 + 读写文件/执行命令/收尾报告）。
3. `tools`（仅默认）：`- name: prompt_snippet`，按注册顺序；`prompt_snippet` 为空的工具跳过；全空输出 `(none)`；末尾追加一句"项目可能另有自定义工具"。
4. `rules`（仅默认）：按序去重合并——
   - 选了 bash/powershell 且未选 grep/find/ls 时的兜底规则（两种 shell 都选时点名两者）；
   - 每个已选工具的 `prompt_guidelines`（按注册顺序）；
   - `prompt_guidelines` 参数（扩展追加，P0 为空）；
   - 固定两条：回复简洁、改文件时点明路径。
5. `docs`（仅默认）：`docs_dir` 存在时生成文档根 + 主题索引（扩展、技能、模板、MCP、会话格式、ADR、规范）；不存在则**整节省略**（D35）。
6. `addendum`：`append_system_prompt` 非空时原样写入。
7. `project_context`：非空时，先写引导句（本项目自写："Project instructions found for this workspace:"），再对每个文件输出 `<project_instructions path="…">` 块。
8. `skills`：技能非空且存在可读工具（read → bash → 间接）时生成（02 §4）。
9. `cwd`：工作目录；反斜杠统一成正斜杠。
10. `extra_sections`：按注册顺序；空内容跳过；名字校验失败 → `Err`（对齐 pi 的抛错行为）；内核发 `KernelEvent::Error` 并降级为身份行，不中止装配。

节名规则：`^[a-z][a-z0-9_-]*$`，`preamble` 保留字（不得自定义）；`tools/rules/docs/addendum/project_context/skills/cwd` 为内置名。

## 4. 渲染

- `preamble`：裸文本。
- 其余：`<name>\ncontent\n</name>`。
- 连接：节与节之间空一行（`\n\n`），首尾无多余空白；渲染结果逐字节确定（同输入 = 同输出，保证提示缓存等价）。
- 空节（内容仅空白）不渲染。

渲染示例（固定夹具，测试逐字断言，不用 `contains`）：

```text
You are Timo-pi, a coding agent ...

<tools>
- read(path, offset?, limit?): read a text or image file
- bash(command, timeout?): run a shell command

Other custom tools may be available ...
</tools>

<rules>
- Be concise in your responses.
- Name file paths explicitly when you touch them.
</rules>

<project_context>
Project instructions found for this workspace:

<project_instructions path="/work/AGENTS.md">
...
</project_instructions>
</project_context>

<cwd>
/work
</cwd>
```

## 5. 上下文文件发现

```rust
pub(crate) fn discover_context_files(
    cwd: &Path,
    data_dir: &Path,
) -> (Vec<ContextFile>, Vec<ResourceWarning>)
```

- 候选名（每目录取第一个存在的普通文件）：`AGENTS.override.md` → `AGENTS.md` → `AGENTS.MD` → `CLAUDE.md` → `CLAUDE.MD`。
- 全局：`data_dir` 下同规则，排第一。
- 祖先：从文件系统根到 `cwd` 逐层收集，输出**根在前、cwd 最后**（与 pi 一致；顺序错会改变模型行为）。
- 读取：UTF-8、去 BOM；读失败记警告并跳过（不中止装配）。
- 去重：按真实路径（`canonicalize`，Windows 下再小写比较）；符号链接共享一份。
- 不因项目信任被跳过（同 pi；技能的门控问题见 D33）。
- 工作树嵌套去重不在 P0（D36）。

## 6. 自定义与追加提示

| 文件 | 位置 | 作用 |
|---|---|---|
| `SYSTEM.md` | 全局 `<data_dir>/SYSTEM.md`；项目 `<cwd>/.timo/SYSTEM.md` | 替换 preamble（项目优先；两者都有时项目胜出） |
| `APPEND_SYSTEM.md` | 全局 `<data_dir>/APPEND_SYSTEM.md`；项目 `<cwd>/.timo/APPEND_SYSTEM.md` | 追加到 `addendum` 节（全局在前，`\n\n` 连接） |

文件缺失 → `None`；空文件视为缺失。不新增设置键（文件即配置，pi 同构）。

## 7. 构建时机与重载

- `ensure_ready()`：MCP 与扩展装载完、工具注册表冻结之后构建提示（顺序不能反，否则扩展工具的 snippet/guidelines 丢失）。
- `Kernel::reload_resources()`（新增，公开）：重新发现上下文与技能 → 重建提示；渲染结果与当前 agent 的不同 → 将 agent 置空，下次 `run`/`tools` 用新提示重建（旧 `AbortHandle` 随之失效，`abort()` 走内核方法不受影响）。
- CLI `/reload` 调它；桌面命令面板同一入口。
- `Kernel::system_prompt()`（只读，诊断/测试），返回当前渲染文本。

## 8. 设置新增

| 键（snake_case） | 类型 | 默认 | 用途 |
|---|---|---|---|
| `skills` | array | `[]` | 显式技能路径（文件或目录），02 使用 |
| `enable_skill_commands` | bool | `true` | 是否把技能列进命令发现（02） |

`prompts`（模板路径）在 P2 引入，键名现在不占位（未知键会被忽略，语义上不会误读）。

## 9. 与 `before_agent_start` 钩子的关系

- 钩子仍拿整段渲染文本；返回非空 → 本次 run 用它整段替换（`forced` 语义）。钩子不改内核缓存的提示。
- 因此 `SystemPrompt` 的 `forced` 只在 run 内有效，不写回内核字段。

## 10. 测试

| 用例 | 断言 |
|---|---|
| golden 提示 | 固定工具/上下文/追加，全文逐字相等（含空行） |
| 自定义提示 | 不出现 `tools`/`rules`/`docs` 节；preamble 等于输入 |
| 节名校验 | 大写、下划线开头、`preamble` → `Err` |
| 空节 | 内容只有空白 → 不渲染该节 |
| 上下文顺序 | 全局 → 根 → … → cwd；同名候选只取第一个 |
| BOM 与符号链接 | 去 BOM；两个路径指向同一真实文件只留一份 |
| 追加 | 全局与项目按序连接；单个缺失不影响另一个 |
| 重载 | 改 AGENTS.md 后 `reload_resources()` 的新渲染包含新内容 |
| 钩子替换 | 钩子返回整段 → 模型收到该文本，无节标签 |

## 11. 体量与风险

- 预计：`prompt/mod.rs` ≈ 220 行、`prompt/context.rs` ≈ 130 行，均在 500 行内。
- 风险 1：提示结构升级会让所有机器首次请求的缓存失效一次（预期，不设兼容层）。
- 风险 2：`extra_sections` 的非法名字会让装配失败（对齐 pi）；P0 无外部注册者，Phase 4 接入时再评估是否需要降级为警告。
