# 设计 12：项目信任门控（project trust）

- 状态：定稿（2026-10-28）；实现：批 A、批 B 已落地（2026-10-28，438 → 446 tests / 批 B 8 组变异 / DeepSeek 对照含 T10），批 C 桌面接线与 CLI 矩阵已落地（451 tests / 批 C 4 组变异；桌面实机待人工），T9 实机待补
- 关联：roadmap/phase-3.5 §3（P2 第四题）；[00-overview](00-overview.md) D33/§10；[research/06](../../research/06-pi-interaction-logic.md) §6；[02-skills](02-skills-and-commands.md) §10；[08-subagent](08-subagent.md) §2/§5
- 行为蓝本：pi 1.4.0 `core/project-trust.ts`、`core/trust-manager.ts`、`core/resource-loader.ts`、`docs/security.md`（只读，不复制；铁律 7）
- 范围：只做 **P2 集成的信任门控**；不做审批档位（backlog）、不做桌面信任管理全页（精修批次）、不做项目包安装器（本仓无包系统）

## 1. 目标与非目标

**目标**：在工作目录出现"项目级可执行/可注入资源"时，内核在装配前询问并持久化一次决定；决定为"不信任"时这些资源一律不加载，用户级资源与上下文文件不受影响。行为、决策顺序与存储格式对齐 pi。

**非目标**：

- 不把信任当安全边界。照 pi 的口径写进 README 与告警：信任只决定"加载哪些项目资源"，不限制工具能读写什么、不替代审阅与原子的 OS 隔离。
- 不做每次工具调用的审批（内核零审批不变，ADR-0012 的审阅接缝是桌面外挂）。
- 不做项目包/主题（本仓无此二类资源）；不做 `sessionDir` 预读（见 D83）。

## 2. 蓝本行为（已核对源码，逐条为合同）

### 2.1 触发条件

`hasTrustRequiringProjectResources(cwd)`：扫描 `<cwd>/.pi` 下固定清单 `settings.json`、`mcp.json`、`extensions`、`skills`、`prompts`、`themes`、`SYSTEM.md`、`APPEND_SYSTEM.md`，以及 cwd 与**每一层祖先**的 `.agents/skills`（排除 `$HOME/.agents/skills`）。任一存在即需要决定；空 `.pi` 目录不算；无任何项目资源时直接视为信任，**不问也不写**。

### 2.2 决策顺序（唯一顺序，不得交换）

1. 命令行覆盖 `--approve` / `--no-approve`：只作用于本次进程，不落盘。
2. 若无需门控资源 → 信任。
3. 扩展 `project_trust` 事件：用户级与命令行扩展逐个询问，**第一个回答 yes 或 no 的拥有决定**；结果可带 `remember`，带了才写存储。
4. 存储：canonical 路径逐级向上找最近的 true/false，最近者生效（父目录决定可覆盖子目录）。
5. 全局设置 `defaultProjectTrust`：`always` → 信任；`never` → 拒绝；`ask` → 继续。
6. 有交互界面 → 询问并按选择落盘（"仅本次"两项不落盘）；没有界面（print/json/rpc）→ 拒绝。

### 2.3 交互选项与存储

交互选项五项：信任 / 信任父目录（写父目录 true 并清除本目录条目）/ 仅本次信任（不落盘）/ 不信任 / 仅本次不信任。

存储为 `<agentDir>/trust.json`：`{ "<canonical path>": true | false }`，键按字典序、两空格缩进、末尾换行；写入带文件锁；`null` 表示清除条目（读时忽略）。文件损坏是**硬错误**（读取抛错，启动中止）。

### 2.4 生效时机

- `/trust` 只保存决定并提示"重启生效"；`/reload` **不重新解析**信任（`resourceLoader.reload()` 不带 `resolveProjectTrust`，沿用当前会话的信任态）。
- 以"隐式信任"启动（当时无项目资源）的会话，若期间出现了待门控资源，`/reload` 成功后把信任固化进 `trust.json`（`maybeSaveImplicitProjectTrustAfterReload`）。

### 2.5 门控面

不信任时跳过：项目设置、项目 `mcp.json`、项目扩展与项目包、项目技能/模板/主题、`.pi/SYSTEM.md` 与 `.pi/APPEND_SYSTEM.md`、项目 `.agents/skills`。**上下文文件（AGENTS.md/CLAUDE.md 等）始终加载**。

### 2.6 蓝本自述缺口

`docs/security.md` 明确：项目 `sessionDir` 在信任解析之前被读取，拒绝信任不能撤销这次读取。本仓不复刻该缺口（D83）。

## 3. 本仓落点

### 3.1 模块布局

| 位置 | 职责 | 体量预估 |
|---|---|---|
| `crates/timo-kernel/src/trust/mod.rs` | 类型（`ProjectTrust`/`TrustSource`/`ProjectResource`）、决策链、`resolve` | ~170 行 |
| `crates/timo-kernel/src/trust/gate.rs` | 资源清单、触发检测、`TrustScope` 开关 | ~130 行 |
| `crates/timo-kernel/src/trust/store.rs` | `trust.json` 读取、最近祖先查找、原子写 | ~150 行 |
| `crates/timo-config/src/settings.rs` | 新增 `load_global_settings` 与 `default_project_trust` 键 | +40 行 |
| `crates/timo-kernel/src/kernel.rs` | `Kernel::new` 前插信任判定；`KernelOptions.trust_override` / `trust_interactive` / `trust_prompt` | +60 行 |

公开门面（`timo_kernel::trust`）：

```rust
pub struct ProjectTrust {
    pub trusted: bool,
    pub source: TrustSource,          // Override/NoResources/Extension/Store/Default/Prompt/NoUi
    pub skipped: Vec<ProjectResource>, // 不信任时被跳过的已存在资源
}

pub enum TrustSource { Override, NoResources, Extension, Store, Default, Prompt, NoUi }

/// 一条被门控的项目资源：种类 + 路径（`gate.rs` 的清单产出）。
pub struct ProjectResource { pub kind: ResourceKind, pub path: PathBuf }

pub trait ProjectTrustPrompt: Send + Sync {
    fn ask<'a>(&'a self, request: TrustRequest) -> Pin<Box<dyn Future<Output = Option<TrustChoice>> + Send + 'a>>;
}

pub struct TrustRequest { pub cwd: PathBuf, pub resources: Vec<ProjectResource>, pub saved: Option<TrustStoreEntry> }
pub struct TrustChoice { pub trusted: bool, pub scope: TrustScopeChoice } // Directory/Parent/Session
```

`Kernel::project_trust() -> &ProjectTrust` 供 CLI/桌面读取；`Kernel::is_project_trusted() -> bool` 供扩展桥的 `isProjectTrusted()`（批 B）。

### 3.2 为什么判定必须在 `Kernel::new` 里、且在加载之前

`Kernel::new` 当前顺序（`kernel.rs`）：解析 data_dir/work_dir → `load_settings(global, project)` → 模型解析 → MCP 配置 → 模板装载。项目设置本身受门控，所以必须：

```text
data_dir / work_dir / sessions 默认值
  → trust::resolve(work_dir, data_dir, global_settings, override, prompt)   ← 新增
  → load_settings(global, project: trusted 时否则 None)
  → 模型/目录/注册表/MCP（项目文件仅 trusted）
  → load_templates()（项目模板根仅 trusted）
  → … ensure_ready：扩展（项目目录仅 trusted）、提示装配（项目 SYSTEM/APPEND/技能根仅 trusted）
```

`default_project_trust` 读**全局层**：用被门控的项目设置决定是否门控会形成循环。这不是偏离——pi 的 `getDefaultProjectTrust()` 同样只读 `globalSettings`。

### 3.3 门控面清单（本仓，唯一出处 `gate.rs`）

| 资源 | 项目路径 | 现加载点 | 不信任时 |
|---|---|---|---|
| 项目设置 | `.timo/settings.json` | `kernel.rs` `load_settings` | 项目层不合并 |
| 项目 MCP | `.timo/mcp.json` | `kernel.rs` `load_server_registry`；`timo-mcp/src/config.rs` | 参数传 `None`，等价文件缺失 |
| 项目扩展 | `.timo/extensions/`、`.pi/extensions/` | `timo-ext/src/discovery.rs` | `DiscoveryOptions.include_project = false` |
| 技能 | `.timo/skills`、`.pi/skills`、祖先 `.agents/skills` | `prompt/resources.rs` `resource_roots` | 根不加入（显式命令行路径仍加入） |
| 模板 | `.timo/prompts`、`.pi/prompts`、祖先 `.agents/prompts` | 同上 | 同上 |
| 系统提示 | `.timo/SYSTEM.md` | `prompt/context.rs` `custom_prompt` | 跳过，回退 `<data_dir>/SYSTEM.md` |
| 追加提示 | `.timo/APPEND_SYSTEM.md` | `prompt/context.rs` `append_system_prompt` | 跳过，保留全局追加 |
| 子代理定义 | `.timo/agents/` | `examples/extensions/subagent`（扩展自读） | 扩展查 `pi.isProjectTrusted()` 后跳过（批 B） |
| 上下文文件 | `AGENTS*.md`/`CLAUDE*.md` | `prompt/context.rs` `discover_context_files` | **不受门控** |
| 用户级资源 | `<data_dir>/{skills,prompts,extensions}`、`~/.agents/{skills,prompts}` | 各加载点 | 不受门控 |
| 命令行显式 | `--extension`、`--prompt-template` | 各加载点 | 不受门控（用户亲手指定本次运行） |

实现约定：

- `gate::PROJECT_ENTRIES` 是**唯一**清单（`.timo` 下八项 + 祖先 `.agents/{skills,prompts}`），触发检测与逐面门控都从这里派生，禁止在加载器里各写一份。
- 祖先扫描按 pi：cwd 起逐级到根，`canonical_key(path) == canonical_key(home/.agents/<dir>)` 时视为用户级，不算触发也不算门控。
- `resource_roots` 增加 `project: bool` 形参；`false` 时只返回 `data_dir/<dir>` 与显式 `extra`。显式 `extra` 来自全局设置或命令行；项目设置里的 `skills`/`prompts` 随项目设置一起被丢弃（不会走到这里）。
- `timo_config::load_settings` 保持既有签名（测试与调用点多），新增 `load_settings_with(global, Option<&Path>)`；不信任时第二参传 `None`，既有调用点零改动。
- `context::custom_prompt` / `append_system_prompt` 增加 `trusted: bool`。
- `timo_mcp::load_server_registry` 的第二参数改为 `Option<&Path>`。
- `DiscoveryOptions` 增加 `include_project: bool`（信任维度：false 时 `.timo/extensions` 与 `.pi/extensions` 都不扫）；`include_project_pi` 保留为兼容维度（是否认 `.pi` 目录），两个开关独立组合、由 `Kernel` 统一给出，loader 内部不再判信任。
- 模板的 `template_roots` 已有 `include_defaults`（`--no-prompt-templates`）；信任是第二个维度，实现上取 `include_defaults && trusted`，不新增第三个开关。

### 3.4 交互通道

- **CLI 交互**（`timo resume` 的 REPL，命令分发处）：沿用 `extensions_interactive` 的 TTY 探测；命中且无 `--no-approve` 时在 stderr 打印资源清单与五行菜单，读一行；空行/EOF = 不信任。与扩展授权（`extensions/mod.rs::approve`）同一风格。
- **一次性路径**（`timo run`、`-p`、`--mode json`）：一律不询问（对齐 pi 的 print/json 模式）；`ask` 按拒绝、`always` 按信任。stderr 只出结果行，JSON 流不加字段。
- **桌面**：桥不做预判定；内核在 `Kernel::new` 内通过 `KernelOptions.trust_prompt` 回调发 `BridgeEvent::Trust { request, respond: oneshot<Option<TrustChoice>> }`，UI 选择后回传；窗口关闭/通道断开按"不信任（本次）"。复用现有 Review 对话框的事件形态。
- **`/trust`**：内建命令从 `Unsupported` 改为 `Ready`，只在 REPL 命令分发处执行（保存决定 + 输出 `Saved trust decision: trusted|untrusted. Restart for it to take effect.`；不自动重载，与 pi 一致）。一次性路径下斜杠文本按既有兜底当普通提示发出，不产生信任动作——此类场景用 `--approve`/`--no-approve`。

### 3.5 可观测与告警

- `ProjectTrust` 在装配完成后可用（`Kernel::project_trust()`）；CLI 在首个 run 前打印一行（一次性与 REPL 同）：
  - 不信任：`[trust] project not trusted (4 project resources skipped; run with --approve or /trust)`
  - 覆盖/存储来源：`[trust] project trusted (source: saved decision)`（`--approve` 时不打印噪音）
- 新增 `WarningKind::UntrustedProject` 与 `WarningKind::TrustStore`，并入既有 `print_resource_warnings` 统一出口（D6 批② 已建立的无头出口）。
- 桌面：`BridgeEvent::Trust(TrustReport)` 在 `Ready` 之前发出；状态徽标与设置页留精修批次，本轮只保证事件与文案。
- 会话**不记录**信任状态（pi 同）；`resume` 在新进程重新判定。

## 4. 信任存储

- 路径：`<data_dir>/trust.json`（`KernelOptions.data_dir` 覆盖时同根）。
- 键：canonical 绝对路径（原样写盘，保可读）；查找时对双方做 `canonical_key`（canonical + lowercase）比较，Windows 大小写不敏感。
- 值：仅 `true`/`false`。读到时忽略 `null`（等价"无条目"）；写入时不产 `null`，清除即删除键。
- 写：读 → 改 → 写临时文件（同目录 `.trust.json.tmp`）→ 原子替换；键排序、两空格、末尾换行。不引入锁依赖；并发写入为"后写胜"，登记 D81。
- 读：文件不存在 = 无条目；JSON 非法或结构非法 → 返回错误，由调用方降级为"无决策 + `TrustStore` 告警"，**不覆盖原文件**（D82）。
- 查找：从 cwd 的 canonical 路径逐级向上，命中第一个布尔值即返回；到根为止。

## 5. 设置与命令面

- `settings.json` 新增 `default_project_trust: "ask" | "always" | "never"`，缺省 `ask`；**只认全局层**（新增 `timo_config::load_global_settings`，签名不破坏既有 `load_settings`）。
- CLI 新增 `--approve` / `--no-approve`（长选项），`Common` 与 `print::PrintArgs` 都加（无头路径也要能覆盖），映射到 `KernelOptions.trust_override`；两者同时给出 → 退出码 2（参数冲突）。
- 内建命令表：`/trust` 改 `Ready`（本批）；`/reload` 语义不变（不重解析信任）。

## 6. 与 D33、D6 的衔接

- **D33 关闭**：项目技能、项目模板、项目 `.timo/agents` 与项目扩展一样受门控；不信任时 `/skill:<项目技能>` 原样透传（既有"未知技能"语义），并在资源告警中列出被跳过的目录。
- **子代理（D6）**：子进程是独立进程，会重新判定。示例扩展开子进程时必须透传主进程决定：`pi.isProjectTrusted()` 为真加 `--approve`，为假加 `--no-approve`（批 B）；否则 stdin 非 TTY 会把 `ask` 变成拒绝，子代理静默看不到项目资源。
- **上下文文件不门控**：AGENTS.md 属输入内容；04/06 research 已定，保持。
- **扩展能力授权独立**：`permissions.json` 管"扩展能做什么"，信任管"扩展是否被加载"；两层都要过。
- **审阅接缝独立**：信任不替代 `tool_review`；不安全项目仍可在桌面开审阅。

## 7. 偏离登记（接续 11 的 D79）

| # | 偏离 | 类别 | 说明 |
|---|---|---|---|
| D80 | `--approve`/`--no-approve` 仅长选项，不提供 `-a`/`-na` 短别名 | a（clap 单字符短选项） | 与 `--no-extensions` 等既有长选项风格一致；帮助文本给出等价说明 |
| D81 | `trust.json` 不引入文件锁依赖，采用"读-改-写 + 原子替换" | a（无等价依赖） | 多进程并发写为后写胜；原子替换杜绝半截文件；单用户桌面场景可接受 |
| D82 | 存储损坏降级为"无决策 + 告警"，不改写文件；pi 直接抛错中止启动 | c（可用性） | 失败方向仍是 fail-closed（`ask` → 无 UI 拒绝）；损坏证据留在原文件 |
| D83 | 不复刻 pi 的 `sessionDir` 预读缺口 | c（安全收紧） | 项目设置不信任即不合并，`session_dir` 不生效；会话落全局/默认目录 |
| D84 | 不做"隐式信任固化"（pi 的 `autoTrustOnReloadCwd`） | d（范围收敛） | `/reload` 不重解析信任；会话中出现的新项目资源在下次启动重新询问，无安全损失 |
| D85 | 桌面信任提示是原生确认框（Review 同型），不是 pi 的 TUI 选择器；选项语义与五项一致 | a（GUI 载体差异） | 视觉细节留桌面壳精修批次；事件与文案本轮定型 |
| D86 | 门控清单增加 `.timo/prompts`、`.agents/prompts`、`.timo/agents`（pi 清单无此三项） | c（本仓资源面） | 模板与子代理定义在本仓是项目资源；同属"可注入指令"，必须与技能同级门控 |
| D87 | 扩展 `project_trust` 事件与 `isProjectTrusted` 真值在批 B 落地，批 A 期间该决策源缺位 | d（排期） | 批 A 的决策链为 覆盖 → 资源扫描 → 存储 → 默认 → 交互；批 B 插入扩展源并补断言 |

## 8. 施工批次

**批 A（内核核心 + CLI）**

1. `timo-config`：`default_project_trust` 键 + `load_global_settings`。
2. `trust/{mod,gate,store}.rs`：决策链、资源清单、存储；`KernelOptions.trust_override`。
3. `Kernel::new` 顺序改造；逐面门控（设置/MCP/扩展/技能/模板/SYSTEM/APPEND）。
4. CLI `--approve`/`--no-approve`、启动告警、`/trust` 命令。
5. 单测 + 变异（见 §9），门禁四连。

**批 B（扩展接面，已落地 2026-10-28）**

1. `ExtensionOptions.include_project`；`isProjectTrusted` 改为真实值。
2. pre-trust 扩展装载：只加载用户级与显式扩展（项目设置按未信任加载），用于 `project_trust` 事件；随后按决定重载完整集合（对齐 pi 的两遍装载）。
3. shim `pi.on("project_trust")` 往返（宿主新增请求通道，形态同 `RouteVirtual` 反向）；`remember` 落盘。
4. subagent 示例透传 `--approve`/`--no-approve`；项目 `.timo/agents` 按信任读取。

落地补充（真机纠正两处接线）：① 扩展宿主 `BasicServices` 的信任活值必须在**两处构建点**都透传（宿主级 base 与逐扩展 services），否则 `ctx.isProjectTrusted()` 恒为默认 `true`；② pre-trust 装载与最终装载必须共享**同一个** `Arc<AtomicBool>`，否则决定后复用宿主仍读到旧值。两处均已修并各配一条内核级断言（见 §9.1 末条）。

**批 C（桌面 + 真机，桌面接线已落地 2026-10-28；桌面实机待人工）**

1. 桥接 `TrustPrompt` 回调与 `BridgeEvent::Trust`；对话框与状态行。
2. 真机夹具（见 §9.3）在 CLI 与桌面各跑一次；读数进 `report.md`。

落地补充：桥接新增 `BridgeEvent::Trust { request, respond }`（询问）与 `BridgeEvent::TrustReport(TrustReport)`（结论，`Ready` 之前发出）；`TrustBroker` 实现 `ProjectTrustPrompt`，通道断开返回 `None`（内核按不信任处理）。桌面新增 `features/trust.rs`（五项选择的 sheet，Esc/遮罩 = 仅本次不信任），未信任结论写进现有 notice 行；状态徽标留精修批次。桌面实机启动被当前权限模式拦下（GUI 启动需人工），T9 的弹框/点击行为由 gpui 测试覆盖，实机读数待补。

## 9. 测试、变异与验收

### 9.1 单元测试（无网络）

- 决策链：覆盖优先；无资源不问；存储最近祖先；默认三值；无 UI 的 `ask` 拒绝；`always` 放行。
- 触发检测：空 `.timo` 不算；清单内任一存在即触发；`$HOME/.agents/skills` 不算；祖先 `.agents/skills` 触发。
- 存储：排序写、原子替换、缺失文件、非法 JSON 降级不覆盖、`null` 忽略、大小写归一。
- 门控逐面：不信任时设置不合并（项目键不生效）、项目 MCP 不读、项目扩展不扫、技能/模板根不含项目、SYSTEM/APPEND 回退全局、上下文文件仍加载。
- CLI：两参数互斥、映射到 options、告警出口（含无头路径）。
- 扩展接面（批 B）：首决定胜出、`undecided` 不拥有决定、`remember` 才落盘、pre-trust 宿主复用不重复装载、`ctx.isProjectTrusted()` 读活值（覆盖与 pre-trust 决定两条路径）。
- 无单测入口：subagent 扩展的 `--approve`/`--no-approve` 透传与项目 `.timo/agents` 门控在 TS 示例里，由 §9.3 T10 真机读数覆盖（包装脚本记录子进程 argv）。

### 9.2 变异（逐条确认编译通过、断言失败、逐字节还原）

每道门控删判据各一条（八面至少覆盖设置/扩展/技能/模板/SYSTEM/MCP 六面）、决策顺序交换一条、home 特判删除一条、存储损坏静默覆盖一条、`--approve` 不生效一条。

批 B 追加 8 组（2026-10-28 全部验红并逐字节还原）：首决定改为"最后决定胜出"、`undecided` 在 JS/宿主两层同时被当成决定、`is_project_trusted` 恒 `true`、`needs_extension_decision` 恒 `false`、删除复用宿主的 `already` 跳过、逐扩展 services 去掉信任透传、pre-trust 装载换回独立 `Arc`、shim `isProjectTrusted` 恒 `true`。

批 C 追加 4 组（2026-10-28 全部验红并逐字节还原）：Esc/关窗改写为"保存不信任"（scope 由 Session 改 Directory）、桥接忽略 UI 选择恒回 `None`、render 不再自动弹 sheet、`spawn` 不接 `trust_prompt`（真实内核不再发问）。前两次尝试用 `blocking_recv()` 断言导致"不弹 sheet"变异变成挂死，改为 `try_recv()` 后才得到干净断言失败——变异用例本身也要防挂。

### 9.3 真机验收（DeepSeek，临时数据目录）

夹具：项目内放 `.timo/settings.json`（故意覆盖一个可观察键，如 `default_thinking_level`）、`.timo/skills/demo/SKILL.md`、`.timo/prompts/demo.md`、`.timo/SYSTEM.md`（暗号）、`.timo/mcp.json`（覆盖一个全局 server）、`.timo/extensions/echo`（启动时打印标记）、`AGENTS.md`（另一暗号）。

| 场景 | 通过标准 |
|---|---|
| T1 无项目资源 | 不询问、无告警；`project_trust().source == NoResources` |
| T2 不信任启动 | 项目设置/技能/模板/SYSTEM/MCP/扩展全部不生效；AGENTS.md 暗号仍生效；stderr 一行给出跳过数，`resource_warnings()` 有完整清单 |
| T3 `--approve` | 上述项目资源全部生效；不写 `trust.json` |
| T4 保存信任后重启 | 项目资源生效；`trust.json` 有本目录条目 |
| T5 保存不信任后重启 | 同 T2，且不再询问 |
| T6 祖先决策 | 在父目录保存信任，子目录启动直接放行 |
| T7 无头 `-p` + `ask` | 项目资源不生效、退出码 0、stderr 一行告警 |
| T8 `/trust` | 保存后提示重启；重启后按新决定生效；`/reload` 不改变当前信任态 |
| T9 桌面 | 弹框选择"信任"→ 项目资源生效；选择"仅本次"→ 下次启动再问 |
| T10 子代理（批 B） | 主进程信任时子代理能看到项目技能/代理；`pi.isProjectTrusted()` 为真 |

批 B 真机读数（2026-10-28，临时数据目录 `%TEMP%/timo-trust-b`，DeepSeek `deepseek-chat`）：

- 用户级扩展 `trustor` 回答 `yes`：stderr `[trust] project trusted (source: extension decision)`；其 `probe_trust` 工具返回 `TRUST=true`（证明 pre-trust 复用宿主读到决定后的活值）。
- 同一扩展回答 `no`：`project not trusted (3 resource(s) skipped)`、`TRUST=false`；改回 `yes` 后恢复。
- `remember: true` 写入 `{"<work 绝对路径>": true}`；再次启动来源变为 `source: saved decision`、`TRUST=true`。
- T10：主进程信任时项目 `.timo/agents/probe.md` 被发现并跑通（子代理回 `SUBAGENT_PROJECT_OK`）；包装 exe 记录的子进程 argv 含 `--approve`。主进程 `--no-approve` 时项目代理不可见（`unknown subagent`），用户级代理仍可用，包装记录 argv 含 `--no-approve`。

批 C 真机读数（2026-10-28，临时目录 `%TEMP%/timo-trust-c`）：

| 场景 | 读数 |
|---|---|
| T1 无项目资源 | 空目录 `-p` 无任何 `[trust]` 行，正常答 `C_OK`（source = NoResources） |
| T2/T3 | 批 A 已跑（不信任跳过 / `--approve` 全生效、不落盘） |
| T4 | 批 B 已跑（`remember` 写 `trust.json`，重启 source = saved decision） |
| T5 保存不信任 | 手写 `trust.json` = false 后启动：`project not trusted (1 resource(s) skipped)` |
| T6 祖先决策 | 父目录 true、子目录启动：`project trusted (source: saved decision)`、无告警 |
| T7 无头 `ask` | 项目资源存在、无保存决策：`-p` 退出码 0，stderr 一行 `1 resource(s) skipped` |
| T8 `/trust` | 管道喂 stdin 时 REPL 缓冲吞掉选项，落盘为默认"不信任"（安全方向）；TTY 下的五项菜单属批 A 手工验收，本轮未重复 |
| T9 桌面 | 自动化被权限模式拦截（GUI 启动需人工）：弹框/点击/回传由 gpui 测试覆盖（`trust_sheet_accept_returns_directory_choice`、`escape_denies_trust_for_session_only`），实机截图待所有者本地跑 |
| T10 子代理 | 批 B 已跑（argv 含 `--approve`/`--no-approve`） |

## 10. 风险与未决

- **扩展事件往返**（批 B）是最大不确定项：需要宿主在资源装配前启动用户级扩展并等待回答；若宿主启动失败，按"无扩展决定"继续，不阻断开机。
- **Windows canonical 路径**：UNC、长路径、大小写；`canonical_key` 已在资源去重处使用，沿用同一实现。
- **用户预期**：信任 ≠ 安全边界，启动告警与 README 用 pi 的措辞；不承诺"信任后安全"。
- **无头 stderr 噪音已有先例**（D6 批②）：信任告警只出一行摘要，明细进 `resource_warnings()` 供 `/session` 或诊断查看。
- **未决**：桌面信任提示的视觉形态（随视觉精修批次）；是否需要 `/untrust` 快捷命令（暂不做，`/trust` 菜单含"不信任"）。
