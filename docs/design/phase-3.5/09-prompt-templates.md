# Phase 3.5 / 09：prompt 模板（P2 第一题，落 D56）

- 范围：pi `core/prompt-templates.ts` 的等价接缝——`.md` 模板即 `/命令`，提交时展开成提示文本。
- 蓝本（只读，MIT 不得复制）：`packages/coding-agent/src/core/prompt-templates.ts`（325 行）、`core/agent-session.ts` 的 `prompt()` 分派段、`cli/args.ts` 的 `--prompt-template` / `--no-prompt-templates`、`examples/extensions/subagent/prompts/*.md`。
- 前置：02（技能与命令清单）、08（subagent 工具与代理定义）。D56 欠的 workflow 预设在本题落地。

## 1. 目标（一句话）

让用户在 `.md` 文件里写一段带 `$1 / $@` 占位的提示词，就能用 `/模板名 参数` 直接提交——把 pi 的"模板即命令"接缝补齐，并使 subagent 示例的 `/implement`、`/scout-and-plan` 这类工作流预设可用。

## 2. 蓝本行为（只读摘录，不含实现代码）

| 面 | pi 的行为 |
|---|---|
| 模板身份 | `name` = 文件名去掉 `.md`；`content` = 去掉 frontmatter 的正文；`description` = frontmatter `description`，缺失则取正文首个非空行前 60 字符（超长加 `...`）；`argument-hint` 原样透传，仅用于命令展示。 |
| 发现顺序 | 全局 `<agentDir>/prompts` → 项目 `<cwd>/.pi/prompts` → 显式路径（`--prompt-template`，文件或目录）。目录只取**非递归**的 `.md`。查找按名字命中第一个 ⇒ **先注册者优先**，全局遮蔽项目、项目遮蔽显式路径。 |
| 开关 | `--no-prompt-templates`（`-np`）只关掉两个默认目录；显式路径**仍然加载**。 |
| 坏文件 | 读不出、frontmatter 解析失败 → 记一条 warning 诊断并跳过该文件，不中止加载，也不影响其它模板。 |
| 展开触发 | 提交文本以 `/` 开头，且匹配 `/<名字>[空格剩余...]`，名字命中模板才替换；未命中**原样发出**。 |
| 参数分词 | bash 风格：空白切分，双/单引号内不切分，`""` 保留为一个空参数，未闭合引号走到字符串末尾结束。 |
| 占位符 | `$1 $2 …`（越界→空串）、`$@` 与 `$ARGUMENTS`（全部参数空格连接）、`${N:-默认}`、`${@:-默认}` / `${ARGUMENTS:-默认}`（取值为空即用默认）、`${@:N}`（第 N 个起，1 起始，0 视同 1）、`${@:N:L}`（从第 N 个取 L 个）。 |
| 替换范围 | 只对模板正文做一次替换；参数值与默认值里出现的 `$1`、`$@` **不递归展开**。 |
| 分派顺序 | 扩展命令（立即执行，不发提示）→ 扩展输入拦截 → `/skill:name` 展开 → 模板展开 → 发出。 |
| 程序化开关 | `prompt(text, { expandPromptTemplates })` 默认 `true`；扩展以 `sendMessage` 注入时默认 `false`，避免二次展开。 |

## 3. 缺口与补齐（本题实现范围）

| # | 缺口 | 处置 |
|---|---|---|
| G1 | 内核没有模板加载器 | 新增 `crates/timo-kernel/src/prompt/templates/`（`mod.rs` + `tests.rs`）：`load(roots)` → 模板表 + 告警。技能的 frontmatter 解析与目录约定原本长在 `prompt/skills/` 里，模板要共用，因此抽一层 `prompt/resources.rs`（`resource_roots` + `canonical_key` + `parse_frontmatter`），skills 与 templates 都从它取——不留第二个出处。装载时机见 T9（装配期一次 + `/reload`）。 |
| G2 | 提交时不展开模板 | `Kernel::expand_input()` 在 `/skill:` 之后接模板展开；`run()` 与桌面提交自动生效。 |
| G3 | 命令清单没有模板这一类 | `commands::merge()` 末尾追加模板项（新 `CommandSource::Template`，`status = Ready`，`argument_hint` 用 frontmatter 值）；CLI 补全与桌面命令面板随之可见。 |
| G4 | 无路径与开关入口 | `Settings` 加 `prompts: Vec<String>` 与 `enable_prompt_templates: bool`（默认 `true`）；CLI 加 `--prompt-template <path>`（可重复）与 `--no-prompt-templates`。 |
| G5 | CLI 把未知 `/xxx` 全部拦死 | `handle_command()` 的兜底分支之前先问内核"这是模板吗"，命中则按普通提示走 `run()`；未命中保持现有 `unknown command` 报错。内建命令优先，不受影响。 |
| G6 | D56 欠的 workflow 预设 | 移植 `examples/prompts/` 三个模板（`implement`、`implement-and-review`、`scout-and-plan`，自研文案，语义对齐），并补 `examples/agents/planner.md`、`worker.md`——现只有 scout/reviewer，模板引用的另两个代理不存在。 |
| G7 | 扩展自带的资源目录无入口 | pi 有 `resources_discover` 事件让扩展贡献 prompts/skills 路径，我们完全没有。本题**第二批**做（见 §7）。第一批的 `examples/prompts` 经 `--prompt-template examples/prompts` 或复制进 `<data_dir>/prompts` 生效。 |
| G8 | 装机授权的 `fs.read` 只有 cwd 相对的 `**`，用户级 `<data_dir>/agents`、`<data_dir>/prompts` 读不到 | 真机暴露：`/scout-and-plan` 派 subagent 时 `E403 … is outside the declared fs.read scopes`，代理一个都发现不了。装机时补一条 `<data_dir>/**` 绝对作用域（`timo_ext::dir_scope`，与 `key()` 同一套归一化）；`is_protected` 仍然先拦 `.git/.ssh/.env`。没有它，D56 的 workflow 预设在真实目录布局下不可用。 |

## 4. 接口契约

### 4.1 目录与优先级

发现根按顺序（与 02 的 skill 根同构，`.pi` 目录一并认，便于 pi 资产直接搬）：

1. `<data_dir>/prompts`（装机全局）
2. `<cwd>/.timo/prompts`（本项目，Timo 约定）
3. `<cwd>/.pi/prompts`（pi 兼容）
4. 自 `<cwd>` 向上每层的 `.agents/prompts`，遇到 `.git` 停止
5. `Settings.prompts` + `--prompt-template`（相对路径按 `cwd` 解析，可为文件或目录）

同名模板**先注册者生效**，后来的不进清单（§2 的 pi 语义）。`--no-prompt-templates` 关掉 1~4，5 仍加载。

> 注意与 08 §5 的代理定义方向相反：`agents/` 是"后读的项目级覆盖先读的用户级"，那是 pi subagent 示例自己的写法；模板走 pi 内核 `prompt-templates.ts` 的"先到先得"。两处各自忠实于自己的蓝本，不强行统一（铁律 6）。

### 4.2 数据结构

```rust
pub struct Template {
    pub name: String,          // 文件名去 .md
    pub description: String,   // frontmatter description，缺失取首行截断
    pub argument_hint: Option<String>,
    pub content: String,       // 去 frontmatter 的正文
    pub file_path: PathBuf,
}
```

加载返回 `TemplateSet { templates, warnings }`，`warnings` 并入 `prompt_warnings`，走 `/reload` 与无头 stderr 的既有告警出口（08 §4.2 修的那条）。

### 4.3 展开与分派

- 展开入口唯一：`Kernel::expand_input()`。顺序 `/skill:name` → 模板 → 原样。
- 匹配式：`^/([^\s]+)(?:\s+([\s\S]*))?$`。名字未命中 → 原文返回（不报错）。
- 分词与占位符逐条照 §2 实现，纯函数 `substitute_args(content, args)`，便于单测。
- 程序化路径无需额外开关：我们只有用户输入走 `run()`，扩展注入不经 `expand_input`（已核 `Kernel::run` 与 `apps/timo-cli/src/commands.rs` 两个调用点），不存在二次展开面。

### 4.4 命令行

| 参数 | 语义 | 失败 |
|---|---|---|
| `--prompt-template <path>` | 追加显式模板文件/目录，可重复 | 路径不存在 → 静默跳过（对齐 pi：`existsSync` 不过就 continue），坏文件出 warning |
| `--no-prompt-templates` | 停用 1~4 号默认目录 | — |

## 5. workflow 预设（D56 收口）

`examples/prompts/*.md` 三个模板都是"用 subagent 工具的 chain 参数按序驱动代理"的指令文本，正文用 `$@` 承接用户参数；配套的 `planner.md` / `worker.md` 代理定义与既有 `scout.md` / `reviewer.md` 放同一目录，`examples/README.md` 增一节说明装法（复制进 `<data_dir>/prompts`，或 `--prompt-template` 指过去）。

## 6. 验收

| # | 场景 | 通过标准 |
|---|---|---|
| T1 | 目录发现与遮蔽 | 全局与项目放同名模板，清单只出一条、展开用全局那份；`--no-prompt-templates` 后显式路径仍加载 |
| T2 | 坏文件 | 不可读 + frontmatter 未闭合两个文件 → 只出 2 条 warning，同目录好文件仍可用 |
| T3 | 参数语法 | 单测覆盖 `$1` 越界、`$@`、`${2:-默认}`、`${@:2}`、`${@:1:2}`、`""` 空参数、引号内空白、未闭合引号、参数值里的 `$1` 不递归 |
| T4 | description 缺省 | 无 frontmatter 时取首个非空行，超长截断加 `...` |
| T5 | 分派顺序 | `/skill:` 命中优先于同名模板；内建命令不被模板抢走；未知 `/xxx` 仍报 `unknown command`；`expand_input` 未命中返回原文 |
| T6 | 清单接线 | `slash_commands()` 出现 `CommandSource::Template` 项、带 `argument_hint`。桌面命令面板的分组接线不在本批：面板现在只有「技能」组（`source == Skill`），模板并进该组语义不对、另起组要动中英 i18n，统一留给桌面壳精修批次（D63） |
| T7 | 真机（DeepSeek，临时数据目录） | 无头 `-p "/implement …"` 在 `run` 的 `expand_input` 里展开，驱动一次 subagent chain（worker 真改沙盒文件）；`/session` 的 Tools 桶有该次用量 |
| T7b | 真机交互分派 | `handle_command` 只在 `timo resume` 的 REPL 里被调用（裸 `timo` 无子命令会直接报"nothing to run"，`-p` 走 `print::run` 不经分派）——管道喂 `/scout-and-plan …` 应被放行并跑通 chain；加 `--no-prompt-templates` 后同一行应报 `unknown command`（G5 的跨进程证明，该行无单测入口） |
| T8 | 回归 | 既有 workspace 测试一字不改全绿 |
| T9 | `/reload` 刷新 | 新增/改名模板文件后 `/reload` 即生效，不必重启进程（模板装载点：装配期一次 + `/reload`；`/模板名` 在首轮输入就要能用，所以不能等首次 `run`） |

## 7. 第二批（同题内，批 B）

扩展资源贡献事件 `resources_discover`（pi 的 `promptPaths` / `skillPaths`）：宿主收集扩展返回的路径，并入本批的显式路径通道，使 `examples/extensions/subagent/prompts/` 这类随扩展打包的资产自动生效。单列一批是因为它同时改 shim、宿主服务与内核装配三处，与本批的内核/CLI 面正交，混做会让回归面说不清。

## 8. 体量

| 项 | 预算 → 实际 |
|---|---|
| 模板模块 | 预算 ≤250 行 → 实现 337 行（`templates/mod.rs`）+ 用例 211 行另置 `templates/tests.rs`；共享层 `prompt/resources.rs` 192 行（含 4 条用例），`prompt/skills/mod.rs` 随之从 482 行降到 370 行。均在 500 行硬限内，未用豁免。 |
| `commands.rs` 合并面 | +1 源枚举、+1 参数，不新增分支 |
| CLI `Common` | +2 flag，不新增子命令；`main.rs` 492 行（拆出 `printer.rs`/`sessions_cmd.rs` 后仍在限内） |
| 文档 | 本文件 + `00-overview` 偏离表若干行 + `examples/README` 一节 |

## 9. 偏离登记（接续 08 的 D58）

| # | 偏离 | 类别 | 说明 |
|---|---|---|---|
| D59 | 模板根除全局 `<data_dir>/prompts`、项目 `.pi/prompts` 外，还认 `.timo/prompts` 与向上每层 `.agents/prompts` | a（与技能同一套根逻辑） | 复用 K5/D31 已登记的目录约定，避免同一属性两个出处（§4.1） |
| D60 | 扩展自带资源目录（`resources_discover`）不在批 A | d（范围收敛） | 批 B 落；批 A 用显式路径/目录约定，功能面不缺失（§7） |
| D61 | `description` 缺省截断按字符计数，不按 UTF-16 码元 | a（Rust 无码元语义） | 中文模板首行不会被切成半个字；长度语义仍是 60（§2） |
| D62 | 不做 `expandPromptTemplates` 程序化开关 | b（无二次展开面） | `expand_input` 仅 `Kernel::run` 与 CLI `/skill:` 两处调用，扩展注入不经此路（§4.3 已核） |
| D63 | 桌面命令面板暂不列模板 | d（范围收敛） | 面板只有「技能」组（`source == Skill`）：模板并进去语义不对，另起组要同时动中英 i18n 与分组渲染，留给桌面壳精修批次（§6 T6） |

