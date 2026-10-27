# 设计 02：技能与命令注册面（P0）

- 状态：定稿（2026-10-27）
- 关联：00-overview K5/K6/K7/D31/D32/D33/D37/D38；design 01 §3-§8；research/06 §2.4/§3
- 落点：`crates/timo-kernel/src/prompt/skills.rs`、`crates/timo-kernel/src/commands.rs`、`crates/timo-kernel/src/settings`（键）

## 1. 目标与范围

- 技能（SKILL.md）：发现、校验、提示注入、`/skill:name` 展开、诊断告警。
- 命令注册面：内建命令清单 + 技能命令 + 扩展命令合并为一个只读清单，CLI 与桌面共用。
- 不做：模板展开（P2）、技能脚本执行、技能的权限模型（信任门控在 P2，D33）。

## 2. 技能数据模型

```rust
pub(crate) struct Skill {
    pub name: String,
    pub description: String,
    pub file_path: PathBuf,
    pub base_dir: PathBuf,           // SKILL.md 所在目录
    pub disable_model_invocation: bool,
}

pub struct ResourceWarning {          // 内核再导出；壳层只读
    pub kind: WarningKind,            // Io | InvalidName | InvalidDescription | Parse | Collision
    pub message: String,
    pub path: PathBuf,
}
```

告警不中止装配；内核缓存并通过 `Kernel::resource_warnings()` 暴露（CLI 启动时写 stderr，`/reload` 重新打印；桌面后续批次消费）。

## 3. 发现

### 3.1 根目录与顺序（先到先得）

| 序 | 根 | 说明 |
|---:|---|---|
| 1 | `<data_dir>/skills` | 用户级（`TIMO_AGENT_DIR` 或平台数据目录） |
| 2 | `<cwd>/.timo/skills` | 项目（本项目配置目录） |
| 3 | `<cwd>/.pi/skills` | 项目（pi 兼容位置） |
| 4 | `.agents/skills`：从 `cwd` 向上直到仓库根（最近的在前） | 仓库根 = 最近含 `.git` 的祖先；没有则到文件系统根 |
| 5 | `settings.skills` 的每项（相对路径按 `cwd` 解析） | 文件或目录；不存在 → 告警并跳过 |

- 每个根内部用 `ignore::WalkBuilder` 遍历（工作区已有 `ignore` 依赖）：读 `.gitignore`/`.ignore`、跳过隐藏项、跳过 `node_modules`、跟随符号链接（环路由底层报错跳过）；不读 `.fdignore`（D37）。
- **技能根**：目录含 `SKILL.md` → 只加载它，不下钻（后代的 `SKILL.md` 一律忽略）。
- 根目录没有 `SKILL.md` 时，直接子级的 `*.md` 视为独立技能（必须带描述）。
- 文件去重：`canonicalize` 后相同 → 静默跳过（Windows 下小写比较）。
- 同名冲突：**先发现的胜出**，后发现的记 `Collision` 告警。

### 3.2 frontmatter（D38）

- 只解析文件首部 `---` 与下一行 `---` 之间的单层标量：`name`、`description`、`disable-model-invocation`。
- 行格式 `key: value`，按**首个**冒号切分；值去首尾空白，成对引号去掉。
- 出现嵌套/列表/多行块 → 忽略该字段并记告警；解析失败 → 该文件跳过（`SKILL.md` 记告警）。
- 名字缺省取父目录名；缺描述 → 不加载。

### 3.3 校验（违规只告警）

| 字段 | 规则 |
|---|---|
| name | ≤64 字符；仅小写 `a-z0-9-`；首尾无 `-`；无连续 `--` |
| description | 非空且 ≤1024 字符 |

## 4. 提示注入（接 01 §3 第 8 步）

- 过滤 `disable_model_invocation = true` 的技能。
- 可读工具提示：已选工具含 `read` → 提示用 `read`；否则含 `bash` → 提示用 `bash`；否则不点名工具。
- 内容（本项目自写文案，结构对齐）：

```text
Skills listed below provide task-specific instructions.
When a task matches a description, load that skill's file with the read tool.
Relative paths inside a skill resolve against its directory (next to SKILL.md); use that absolute path in tool commands.

<available_skills>
  <skill>
    <name>pdf-tools</name>
    <description>Extract text and tables ...</description>
    <location>/home/u/.timo/skills/pdf-tools/SKILL.md</location>
  </skill>
</available_skills>
```

- XML 转义：`& < > " '` 五个字符。
- 技能为空或全部隐藏 → 整节省略。

## 5. `/skill:name` 展开

```rust
pub(crate) fn expand_skill_command(text: &str, skills: &[Skill]) -> Option<String>
```

- 触发：文本以 `/skill:` 开头；名字到首个空白为止。
- 命中：读取 `SKILL.md`、去掉 frontmatter、构造块并拼接参数（参数去首尾空白）：

```text
<skill name="pdf-tools" location="/home/u/.timo/skills/pdf-tools/SKILL.md">
Relative paths resolve against /home/u/.timo/skills/pdf-tools.

<body>
</skill>

<args>
```

- 未命中：返回 `None`（调用方原样透传，不报错）。
- 读失败：同样返回 `None`（只读路径不引入可变告警状态；发现阶段已经报过读错误）。
- 隐藏技能（`disable_model_invocation`）仍可被显式展开（pi 同义）。

## 6. 命令注册面

```rust
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub source: CommandSource,   // Builtin | Skill | Extension | Prompt(P2)
    pub status: CommandStatus,   // Ready | Planned(&'static str) | Unsupported
}
```

合并顺序：内建（固定表序）→ 技能（`skill:<name>`，按发现顺序）→ 扩展命令（`plugin_<id>_*`，Phase 2 现有清单）。同名先到先得并告警。

### 6.1 内建命令（P0 盘点）

| 命令 | P0 状态 | 说明 |
|---|---|---|
| `/new` | Ready | 清空为草稿会话（`Kernel::new_session`） |
| `/resume [id]` | Ready | 有 id 直接恢复；无 id 列出会话（选择器留桌面） |
| `/session` | Ready | 打印会话 id、模型、轮数与用量（用量见 03） |
| `/reload` | Ready | 重新发现上下文/技能并重建提示（01 §7） |
| `/quit` | Ready | 退出（CLI） |
| `/abort`、`/ext`、`/sessions` | Ready（本项目命令） | 现有 CLI 行为，不属于 pi 清单；`source=Builtin` 且文档标注 |
| `/compact` | Planned(P1) | 压缩 |
| `/model`、`/thinking`、`/settings`、`/login`、`/logout` | Planned(后续) | 运行期切换模型需重建内核（Phase 3 已知限制） |
| `/tree`、`/fork`、`/clone` | Unsupported | GUI 轨 backlog |
| `/export` | Planned(P1) | 会话导出 |
| `/import`、`/share`、`/bug`、`/llama`、`/scoped-models`、`/copy`、`/name`、`/changelog`、`/hotkeys`、`/trust` | Unsupported | D32；`/trust` 随 P2 的信任管理器再议 |

`Unsupported` 与 `Planned` 的差异：前者本阶段范围外，后者有明确批次。CLI 对两者都打印一行说明，**不静默当普通提示发给模型**。

### 6.2 输入分发（CLI 与桌面同一顺序）

1. 文本不以 `/` 开头 → 普通提示。
2. 扩展命令（Phase 2 现有 `split_command` + 前缀匹配）→ 执行，结束。
3. 内建命令 → `Ready` 执行；`Planned/Unsupported` 打印说明，结束。
4. `/skill:` / 模板（P2）→ `expand_input` 展开成用户消息，进入 run。
5. 其它 `/xxx` → 未知命令错误，**不**当提示发送（与 pi 一致）。

> 桌面命令面板（Phase 3）只消费 `slash_commands()`；输入框里的 `/` 补全走同一清单。P0 不改 GUI 交互结构。

## 7. 内核 API

```rust
impl Kernel {
    pub fn slash_commands(&self) -> Vec<SlashCommand>;
    pub fn expand_input(&self, text: &str) -> String;      // 技能（P0）；模板（P2）
    pub fn resource_warnings(&self) -> &[ResourceWarning];
    pub fn reload_resources(&mut self);                    // 01 §7
}
```

- `expand_input` 只做"技能块 + 参数"，不做命令分发（分发在壳层，见 §6.2）。
- `reload_resources` 重新发现技能与上下文；`enable_skill_commands=false` 只影响清单与补全，不影响显式 `/skill:`（pi 同义）。

## 8. 设置

| 键 | 类型 | 默认 | 语义 |
|---|---|---|---|
| `skills` | array | `[]` | 显式技能路径（文件/目录，相对 `cwd`） |
| `enable_skill_commands` | bool | `true` | 技能是否进入命令发现 |

## 9. 测试

| 用例 | 断言 |
|---|---|
| 技能根截断 | 根有 `SKILL.md` 时，子目录里的 `SKILL.md` 不加载 |
| 独立 md | 根级带描述的 `.md` 加载；无描述跳过 |
| 校验告警 | 大写名字 → 仍加载 + `InvalidName`；空描述 → 不加载 |
| 冲突 | 两个同名 → 先到者生效 + `Collision` |
| ignore | `.gitignore` 命中的目录不加载；隐藏目录与 `node_modules` 不加载 |
| 隐藏技能 | 不进 `available_skills`，但 `/skill:` 可展开 |
| XML 转义 | 描述含 `<&>` → 提示中是转义实体 |
| 展开 | 未知技能原样返回；参数追加、frontmatter 去除、路径基准行正确 |
| 命令清单 | 内建固定表 + `skill:` 项 + 扩展项；`enable_skill_commands=false` 时无 `skill:` 项 |
| 分发 | `Planned/Unsupported` 不产生 run；未知 `/xxx` 报错 |
| 重载 | 新增技能文件后 `reload_resources` 的清单与提示都更新 |

## 10. 体量与风险

- 预计：`skills.rs` ≈ 260 行、`commands.rs` ≈ 200 行、frontmatter 解析并入 `skills.rs`；均低于 500 行。
- 依赖：`ignore` 从工作区既有依赖引入 `timo-kernel`（`Cargo.lock` 无新增条目，standards/04 准入表不新增）。
- 风险 1：`ignore` crate 的跟随符号链接在极端目录结构下可能变慢；遍历有天然上限（技能目录通常很小），不另设深度上限。
- 风险 2：项目技能无信任门控（D33）；P2 的 trust-manager 接入时必须先改本节再改代码。
