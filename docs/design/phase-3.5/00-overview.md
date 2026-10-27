# 设计 00：Phase 3.5 总览与跨模块决策

- 状态：定稿（2026-10-27，与 P0 详设 01~03 同批；P1 详设 04~07 于同日补定稿；D6 标尺详设 08 于同日定稿）
- 关联：roadmap/phase-3.5；ADR-0011/0013；research/06；design/phase-1/02~05
- 行为蓝本：pi 1.4.0（本地只读，不复制；铁律 7）

## 1. 定位

本目录是 Phase 3.5「内核深化」的实现级详设：模块边界、接口契约、加载时序、默认值与验收断言。范围以阶段文档 §3 为准，本目录不扩范围；"更好的做法"只进 backlog（ADR-0011）。

与 Phase 2 的差别：Phase 2 新增一个 crate（`timo-ext`）；本阶段**不新建 crate**——提示词与资源属于内核装配层，落在 `timo-kernel` 内部模块（见 K1）。

## 2. 一句话方案

把系统提示从一段内嵌字符串升级为**分节装配物**：上下文文件、skills、命令注册面、用量聚合由内核统一发现与组装，CLI 与桌面只消费结果；P1/P2 的压缩、缓存、无头模式、模板与模型路由随后按批落，每一批先定稿对应详设再写码。

## 3. 模块布局

| 位置 | 职责 | 批次 | 详设 |
|---|---|---|---|
| `timo-kernel/src/prompt/mod.rs` | 分节模型、渲染、节名校验、整段替换 | P0 | 01 |
| `timo-kernel/src/prompt/context.rs` | 上下文文件发现、顺序、去重 | P0 | 01 |
| `timo-kernel/src/prompt/skills.rs` | 技能发现、校验、提示格式、`/skill:` 展开 | P0 | 02 |
| `timo-kernel/src/commands.rs` | 内建 + 模板 + 技能 + 扩展命令合并 | P0（模板 P2 接） | 02 |
| `timo-kernel/src/usage.rs` | 用量与成本聚合、分桶、嵌套合并入口 | P0 | 03 |
| `timo-config/src/settings.rs` | `skills` / `prompts` / `enableSkillCommands` 键 | P0 | 01/02 |
| `timo-agent` | 不改：系统提示仍是整段字符串（K2） | — | — |
| `apps/timo-cli` / `apps/timo-desktop` | 消费命令清单与用量读数 | P0 末 | 02/03 |
| `timo-agent/src/compaction/` | 估算、切点、序列化、摘要请求；循环内请求前接入 | P1 | 04 |
| `timo-session`（投影/导出/分区） | 压缩 entry 投影、会话目录编码、JSONL 导出 | P1 | 04/07 |
| `timo-kernel/src/cache_stats.rs`、`warmer.rs` | 缓存 miss 统计、按 TTL 预热与用量记录 | P1 | 05 |
| `apps/timo-cli/src/json_mode.rs` | `-p` / `--mode json` 无头输出 | P1 | 06 |
| `timo-ext`（宿主 fs API + `execParallel`）+ `examples/extensions/subagent` | D6 标尺：子代理扩展、无头子进程、用量回报 | D6 | 08 |
| `timo-tools/src/nested.rs` + `timo-agent/src/run/nested.rs` + `timo-kernel/src/codemode/` | 嵌套工具执行接缝、codemode 沙箱与脚本工具面 | P2 | 11 |

后续批次（P2）：`prompt_templates`、`provider-composer`、`nested-tool-calls`、`trust`。各自详设编号在开工前补，不在本文预设签名。

## 4. 批次与提交序列

1. **本批**：00 总览 + P0 详设 01~03 定稿；阶段文档/看板/CHANGELOG 同步。
2. 提示分节 + 上下文文件（单测：顺序、候选名、去 BOM、工作树去重）。
3. skills + 命令注册面（单测：发现规则、校验告警、冲突先到先得、隐藏技能、`/skill:` 展开）。
4. 用量聚合（单测：分桶键、成本、缺失字段、工具/摘要桶）。
5. CLI 与桌面接线（命令菜单列出技能；状态区显示用量；D1 的桌面半场可验）。
6. P1 详设 04~07 定稿 → 实现（压缩、缓存统计与预热、无头模式、会话分区与导出）。
7. P2 详设定稿 → 实现（模板、路由、codemode、信任；09/10/11 已定稿，12 待写）。
8. 验收 D1~D7 → `report.md` → 看板/CHANGELOG → tag `phase-3.5.end`。

> 每批门禁四连不变（fmt / clippy / workspace tests / check-size）；新增提示输出用快照式断言（固定输入 → 逐字期望），不只 `contains`。

## 5. 关键实现决策

| # | 决策 | 理由 / 约束 |
|---|---|---|
| K1 | 不新建 crate，模块落在 `timo-kernel`；单文件超 500 行就拆文件 | 内核是唯一装配点；新增 crate 会带来依赖边与版本负担；skills/命令只有内核与壳两个消费者 |
| K2 | 系统提示**不写入会话**；装配时（或 `/reload`）按节构建一次并渲染为整段字符串；提示变化时重建 agent | 现有 wire 只接受整段 system；相同输入渲染出相同字节，提示缓存等价；省去"按节补丁"的持久化面（偏离 D30） |
| K3 | 分节规则逐条对齐 pi：`preamble` 裸文本、其余 `<节名>` 包裹、节名正则 `^[a-z][a-z0-9_-]*$`、空节跳过、`forceSystemPrompt` 整段替换 | 扩展改写面（Phase 4）依赖同一套名字与标签；无节名规则会直接拒绝 |
| K4 | 上下文文件候选名、全局优先、根到 cwd 顺序、去 BOM 按 pi；工作树嵌套去重留 P2（D36） | 属于"提示词工程"复刻范围（ADR-0013）；顺序错会改变模型行为 |
| K5 | 技能目录：`<data_dir>/skills`、项目 `.timo/skills`、`.pi/skills`、`.agents/skills`（cwd→仓库根）、`settings.skills` 显式路径；同名先到先得 | 兼容 pi 生态位置，另加本项目 `.timo`；先到先得与告警对齐 pi（偏离 D31 仅目录集） |
| K6 | 技能正文不进提示；`/skill:name` 把正文展开进**用户消息**，参数追加在后 | 与 pi 一致；未知技能原样透传，不报错 |
| K7 | 命令注册面在内核：内建名 + 模板 + 技能 + 扩展命令合并，扩展仍带 `plugin_<id>_` 前缀 | 单一来源，CLI 与桌面消费同一清单；P0 只要求内建命令的**语义**，不要求 `share/bug/import/llama` 实现（D32） |
| K8 | 用量从会话消息推导（assistant 消息的 `usage`；工具结果的 `usage` 进 `Tools/summaries` 桶），按 `provider/model` 分桶；`usage`/摘要 entry 在 P1 落地后再接入 | 与 pi 的 `usage-totals` 对齐；不新增落盘格式；P0 不改写会话 |
| K9 | 设置键沿用本项目既有的 snake_case 约定（`default_tools` 等）；pi 的多词键转 snake_case（`enable_skill_commands`、`default_project_trust`），单词键同名（`skills`、`prompts`、`compaction`） | 与现有 settings.json 一致，避免两套命名；对照关系写进 01 |

## 6. 与 pi 的偏离登记（接续 Phase 2 的 D29）

> 类别：a=语言/运行时必然差异；b=既有 ADR 已定载体；c=本仓库规范硬要求；d=范围收敛（验收前必须登记）。

| # | 偏离 | 类别 | 说明 |
|---|---|---|---|
| D30 | 系统提示不落会话、不走"按节补丁" | b（自有会话格式，ADR-0007） | 装配与 `/reload` 时重建整段；内容等价，缓存等价；代价是改动后首轮缓存冷启动 |
| D31 | 项目技能目录增加 `.timo/skills`，并保留 `.pi/skills`、`.agents/skills` | c（本项目配置目录） | 发现顺序：用户 → 项目 `.timo` → 项目 `.pi` → `.agents`（近到远）→ 显式路径 |
| D32 | `share`/`bug`/`import`/`llama`/`scoped-models` 等命令不实现 | d（范围收敛） | 命令注册面如实标注"未实现"，不静默消失；语义对齐只覆盖可执行命令 |
| D33 | 项目技能在 P0 无信任门控（pi 门控 `.pi/skills` 与项目 `.agents/skills`） | c（无信任管理器） | 与技术相关风险：技能是文本指令，与不受门控的上下文文件同级；本项目的审阅接缝是安全垫；信任门控在 P2 补齐后收紧 |
| D34 | 用量分桶暂不含工具结果的内嵌用量 | d（嵌套调用在 P2） | P0 统计 assistant 消息与工具结果自带的 `usage`（当前工具不产出，桶多为空）；P2 嵌套调用接线后补真实合并 |
| D35 | 提示中的文档索引指向本仓库文档结构，而非 pi 安装路径 | a（自有文档） | 结构同 pi（主文档/附加文档/示例 + 主题索引），内容按本项目；运行时找不到文档目录则整节省略，不编造路径 |
| D36 | 工作树嵌套去重留 P2 | d（范围收敛） | P0 只按真实路径去重（符号链接共享一份）；主仓与嵌套 worktree 各有一份 `AGENTS.md` 时可能重复注入，登记为已知偏差 |
| D37 | 技能发现不读 `.fdignore` | d（依赖能力） | 复用工作区既有 `ignore` crate：读 `.gitignore` 与 `.ignore`，跳过隐藏项与 `node_modules`；`.fdignore` 不在其能力内 |
| D38 | 技能 frontmatter 只解析单层标量 | a（无 YAML 依赖） | 只取 `name`/`description`/`disable-model-invocation`；嵌套、多行块、锚点忽略并告警；描述含冒号按首个冒号切分 |
| D39 | 压缩只在 user 消息处切分，不做 turn-prefix 摘要 | d（范围收敛） | 保留量可能略多于预算；上下文永远从完整轮次开始（04） |
| D40 | 摘要请求仍写 prompt cache | a（无 retention 开关） | 一次性摘要会付一次缓存写费用（04） |
| D41 | 缓存统计从会话 entry 现算 | b（只增不改） | 不新增落盘字段；压缩/预热 entry 参与扫描（05） |
| D42 | 预热需模型声明 `cache_ttl_seconds` | c（目录数据不全） | 只给 Anthropic 目录补 300 秒；自动缓存网关不预热（05） |
| D43 | thinking 非 Off 时不预热 | a（适配器约束） | `max_tokens=1` 与 thinking 预算冲突（05） |
| D44 | 预热与摘要的用量暂不计入 `/session` 总量 | d（范围收敛） | entry 已落盘，P2 统一并入（05） |
| D45 | 会话按 cwd 分区但列表仍跨分区 | c（桌面既有体验） | 只改落盘位置，不改侧栏范围（07） |
| D46 | 旧平铺会话不迁移 | b（只增不改） | 读取兼容，新写入进分区（07） |
| D47 | 导出保留原始 entry 与 parent_id | a（自有会话格式） | 线性链原样输出（07） |
| D48 | 压缩 entry 用 `keep_recent_messages` 计数而非 `first_kept_entry_id` | b（线性链实现更简） | 免去"消息 → entry id"映射；投影时在链上倒数 N 条（04） |
| D49 | 无头模式先调 `Kernel::start_session()` 预建会话 | b（缺口补齐） | 会话头必须在首行而 run 内部才建会话；预建后 run 复用，仅此一处新增内核入口（06 §8） |
| D50 | 预热只在 run 收尾排定（streaming 续一次、idle 窗内重复） | d（范围收敛） | 循环内无空闲点回调；退到 run 级挂点，经济账与安全窗不变（05 §2） |
| D51 | subagent 读代理定义走宿主 `pi.readFile`/`pi.listDir`（`fs.read` 能力），不给 `node:fs` | a（沙箱无 OS 面） | phase-2/03 预留契约落地（08 §4.1） |
| D52 | 子代理系统提示经 `--append-system-prompt <text|@file>` 传入，不写临时文件 | b（免开 `fs.write`） | 08 §4.2 |
| D53 | 子代理实时进度不落地（`pi.exec` 返回才有结果） | c（宿主无流式） | GUI/审阅批次再补 `onUpdate`（08 §3） |
| D54 | parallel 由宿主 `pi.execParallel` 实现（≤8 任务、4 并发） | a（QuickJS 单线程） | 语义仍是"一次提交、按序回填"（08 §4.4） |
| D55 | 子代理用量计入主会话 `Tools/summaries` 桶 | b（pi 仅 TUI 展示） | 经 `ToolOutput.usage` → `ToolResultMessage.usage`（08 §4.3） |
| D56 | workflow 预设（`/implement` 等）随 P2 prompt-templates 落 | d（排期） | 本批只做工具与代理定义（08 §5）；预设与模板机制见 09 §5 |
| D57 | 子进程可执行文件与超时经 `--ext-flag bin=` / `--ext-flag timeout=` 传入，不读 `TIMO_BIN` 环境变量 | a（QuickJS 沙箱没有 `process.env`） | 蓝本用 env 传二进制路径；我们只有宿主 flag 通道，缺省按 PATH 找 `timo`、超时 600 秒（08 §5） |
| D58 | 宿主新增 `pi.getAgentDir()`（config 带 `agentDir`）给扩展定位用户级 `agents/` | a（沙箱没有 `os.homedir`/`CONFIG_DIR_NAME`） | 同一个值经 `HostServices::config()` 交给 JS，缺省空串（08 §5） |
| D59 | 模板根除 `<data_dir>/prompts`、`.pi/prompts` 外，还认 `.timo/prompts` 与向上每层 `.agents/prompts` | a（与技能同一套根逻辑） | 复用 K5/D31 的目录约定，避免同一属性两个出处（09 §4.1） |
| D60 | 扩展自带资源目录（`resources_discover`）不在 prompt-templates 批 A | d（范围收敛） | 批 B 落；批 A 走显式路径与目录约定（09 §7） |
| D61 | 模板 `description` 缺省截断按字符计数，不按 UTF-16 码元 | a（Rust 无码元语义） | 中文首行不被切成半个字，长度语义仍是 60（09 §2） |
| D62 | 不做 `expandPromptTemplates` 程序化开关 | b（无二次展开面） | `expand_input` 仅 `Kernel::run` 与 CLI `/skill:` 两处调用，扩展注入不经此路（09 §4.3） |
| D63 | 桌面命令面板暂不列模板 | d（范围收敛） | 面板只有「技能」组，模板并入语义不对、另起组要动中英 i18n，留给桌面壳精修批次（09 §6 T6） |

## 7. 接缝核对（architecture/03）

| 接缝 | 本阶段落点 |
|---|---|
| #2 事件总线 | skills/命令/用量都不进 agent 循环；内核装配完毕后作为普通数据暴露 |
| #4 工具元模型 | 提示 loadout 只读 `ToolDefinition` 的 `exposure`/`prompt_snippet`/`prompt_guidelines`，不引入第二套工具模型 |
| #5 JSON Schema | 无变化 |
| #6 富流事件 | 用量来自既有 `KernelEvent` 与 `AssistantStreamEvent`，不新增裸字符串流 |
| #7 取消贯穿 | 技能/上下文加载是启动期同步 I/O，不参与取消；`/skill:` 展开在 run 前置阶段完成 |

## 8. 验收映射

| 场景 | 本阶段落点 | 批次 |
|---|---|---|
| D1 项目上下文 | `prompt/context.rs` + 提示装配；CLI 与桌面各验一次 | P0 |
| D2 skills | `prompt/skills.rs` + 命令注册面 | P0 |
| D3 compaction | 独立详设（P1） | P1 |
| D4 用量与缓存 | `usage.rs`（用量）；缓存统计 P1 | P0 / P1 |
| D5 无头模式 | 独立详设（P1） | P1 |
| D6 subagent 式扩展 | 依赖 D5 + 模板 + 扩展工具注册 | P2 |
| D7 CLI 回归 | Phase 1/2 测试一字不改全绿 | 每批 |

## 9. 体量预算

- 新增代码文件遵守 500 行上限；`prompt/` 按职责拆三个文件，单测与实现同文件或独立 `tests/`。
- docs 总数已超软限（当前 73 > 60）：本阶段新增设计稿属于硬流程交付物，按 standards/08 登记豁免；**整合评审**仍待办，先出候选清单再动删除（删除面需所有者确认）。
- 无新增依赖预期；若 `skills` 需要 glob/ignore 语义，优先用标准库手写的最小匹配（与 ext discovery 同风格），不引入 `ignore` crate 除非必要。

## 10. 未决问题

- **docs 整合评审**：需要先给候选清单（合并/归档/删除），删除面待确认后才动。
- **项目技能信任**：P0 按 D33 无门控；P2 的 trust-manager 定稿时回调本决策。
- **命令注册面的模板接缝**：P0 先留接口，模板与技能共用"名称 → 展开"两步；P2 的实现不得改这两步的顺序。
- **提示缓存冷启动**：提示结构升级会让每台机器首轮缓存失效一次，属预期影响，不设兼容层。
- **缓存预热元数据**：`cache_ttl_seconds` 目前只给 Anthropic 内置目录（300 秒）；自定义模型需用户在 `models.json` 写；P1 实现时不猜默认值。
- **无头 JSON 线协议**：字段名跟随本项目 `KernelEvent` 的 snake_case，不承诺与 pi 逐字段一致（D5 只要求可解析的事件流）；D6 的扩展按本协议编写。
