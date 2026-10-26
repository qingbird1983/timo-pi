# 设计 01：窗口信息架构与界面状态流

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0009/0012；architecture/03；设计 00、04；gpui-kit coding/design guides

## 1. 主任务与对象

一句话：**打开窗口，选一个会话，向模型交代工作，看清它做了什么，并对文件改动做决定。**

用户心智模型里的稳定对象是**会话**；消息、工具调用、文件变更都是会话内的内容。界面围绕“当前会话”组织，不围绕内部 crate 或事件类型组织。

主路径（必须无鼠标可完成）：

1. 新建会话 → 输入任务 → Enter 发送；
2. 看流式回答与工具卡片；
3. 出现文件改动 → 打开审阅 → 接受或拒绝；
4. 取消正在跑的 run；
5. 关掉应用，重开后从侧栏恢复会话。

## 2. 窗口骨架

```text
┌──────────────────────────────────────────────────────────────────────┐
│ TitleBar：Timo · 当前会话标题                    [模型] [状态] [取消] │
├──────────────┬───────────────────────────────────────┬───────────────┤
│ Sidebar      │ Chat                                  │ Review sheet  │
│ 260 (240~360)│ flex_1, min_w_0                       │ 440 (360~640) │
│              │                                       │ 仅审阅时出现  │
│ [＋ 新建会话] │ 消息流（MessageScroller）             │ 文件列表      │
│ [搜索会话]   │  · 用户消息                           │ 统一 diff     │
│ 今天         │  · 助手 Markdown/流式                 │ [拒绝] [接受] │
│  · 会话 A    │  · 工具卡片                           │               │
│ 昨天         │  ...                                  │ 变更摘要      │
│  · 会话 B    │                                       │               │
│              │ ┌ Composer ──────────────────────────┐│               │
│ [设置] [模型]│ │ 输入任务…            [发送/停止]   ││               │
└──────────────┴───────────────────────────────────────┴───────────────┘
```

- 默认窗口 1280×800；最小 960×600。窗口缩到最小时：审阅 sheet 覆盖聊天区右侧（仍可 Esc 关闭），侧栏可折叠为 56 图标栏。
- 侧栏宽度 240~360、默认 260；审阅宽度 360~640、默认 440。窗口宽度 <1200 时审阅改为覆盖聊天区（保留顶部运行控制栏，不挤压正文），拖动后的宽度存桌面设置（不是内核设置）。
- 所有区域高度铺满：`h_flex().items_stretch()`，列内再用 `v_flex`；可滚动区各自拥有滚动条。
- 不使用多级卡片嵌套；工具卡片与消息在同一列，靠背景/边线/间距分层。

## 3. 组件映射（库优先）

| 界面 | gpui-kit 组件 | 用法约束 |
|---|---|---|
| 窗口/根 | `Root`（`open_window` 自动包） | 唯一 Root；overlay/通知都走它 |
| 标题栏 | `TitleBar`、`Button`、`Badge` | 状态用文字 + Badge，不靠颜色 alone |
| 会话导航 | `Sidebar`、`SidebarMenu`、`SidebarMenuItem` | `active` 表示当前会话；删除走 `ContextMenu` + AlertDialog |
| 会话搜索 | `Input`/`InputState` | 防抖 120ms，本地过滤，不调用内核 |
| 消息流 | `MessageScroller` + `MessageScrollerState` | 仅在用户位于尾部时自动跟随；离开尾部出现“跳到最新” |
| 消息内容 | `Message`、`MessageContent`、`Text`/`TextView::markdown` | 助手正文 Markdown；用户消息纯文本 |
| 流式提示 | `ShimmerText` 或 `Spinner` | 只在真实等待/流式中出现 |
| 工具卡片 | 应用组件（`ToolCard`，基于 `div` + `Button` + `Badge` + `Collapsible`） | 结构固定：图标/名称/状态/时长/摘要/展开 |
| 工具输出 | `TextView`（代码块）或 `Collapsible` | 默认折叠；长输出截断并给“打开审阅/查看全文” |
| diff 审阅 | `Diff` + `DiffState` + `DiffFile::parse` | 只读展示；接受/拒绝由应用按钮完成 |
| 审阅容器 | `Sheet`（右侧）或内联面板 | 桌面优先 Sheet，窄窗覆盖 |
| 输入框 | `Textarea`/`TextareaState`（或 `Editor`） | Enter 发送、Shift+Enter 换行；禁用时保留草稿 |
| 发送/停止 | `Button`（primary/ghost） | 运行中同一个位置切换为“停止” |
| 命令面板 | `Command` + `CommandState` + `CommandGroup` | Ctrl/Cmd+K；搜索 + 键盘确认/取消 |
| 设置 | `Settings` + `SettingPage` + `SettingGroup` + 字段组件 | 页面在聊天区切换，不做大弹窗 |
| 通知/错误 | `Alert`（内联）+ `push_notification`（异步结果） | 错误必须给原因与下一步 |
| 确认 | `AlertDialog` | 删除会话、移除密钥 |
| 加载骨架 | `Skeleton` | 仅在真实加载时；已知即时内容不摆骨架 |

不新增自研原语，除非库组件无法表达且影响面评估通过（memory：library-first）。工具卡片是**应用组件**，因为它承载产品语义；内部仍复用库控件。

## 4. 状态所有权

根视图 `DesktopApp`（`Entity`）持有：

| 状态 | 所有者 | 说明 |
|---|---|---|
| `sessions: Vec<SessionInfo>` | `sessions::Model` | 启动/结束后扫描；按 `modified_ms` 倒序 |
| `active: ActiveSession` | `chat::SessionState` | id、标题、消息、流式缓冲、运行态、usage |
| `scroller: Entity<MessageScrollerState>` | `ChatView` | 行数变化时 append/splice；不每帧重建 |
| `composer: Entity<TextareaState>` | `ChatView` | 草稿跨会话保留 |
| `reviews: ReviewQueue` | `review::Model` | 待审请求、当前索引、排队数 |
| `settings: DesktopSettings` | `settings::Model` | 主题、语言、审阅策略、模型选择 |
| `palette: Entity<CommandState>` | `CommandPalette` | 查询、选中项 |
| `nsh: Option<Entity<AppView>>` | `nsh_poc` | feature 门控；离开页面可保留或 drop |
| `kernel: KernelBridge` | 根 | 命令发送端 + UI 事件接收任务 |

- `render` 只读状态并组合元素；domain 操作放命名方法（gpui-kit coding guide）。
- 重复元素用 domain id：`("session", id)`、`("tool", tool_call_id)`；禁止用列表 index。
- 每次内核对齐的状态变化 `notify` 一次；流式增量按 16ms 合并。

## 5. 状态流

### 5.1 启动

1. 打开窗口，立即渲染侧栏骨架 + 空聊天欢迎态。
2. `KernelBridge` 在 tokio 线程装配 `Kernel`（读配置、模型、扩展）；成功发 `Ready`，失败发 `BootFailed{message}`。
3. `Ready` 后扫描 `list_sessions()`；若上次关闭前有活动会话且仍存在，自动 `resume`；否则显示欢迎页。
4. `resume` 成功后 `history()` 填充消息流，滚动到底部（不触发自动发送）。

### 5.2 新建 / 切换 / 删除

- **新建**：`new_session()` 清空当前上下文，进入“草稿”态；侧栏暂不出现条目，首次 run 成功创建文件后再刷新列表。
- **切换**：运行中先弹确认（“停止当前任务并切换？”）；确认后 `abort` → 等待 `AgentFinished`/idle → `resume(id)` → `history()` 重填。
- **删除**：仅非当前会话可直接删；当前会话需先切换到欢迎页。AlertDialog 标题“删除会话「标题」？”，正文写“磁盘上的会话文件将被删除，无法恢复。”，按钮“取消 / 删除”。
- 删除成功后刷新列表；若删除的是最后一条，显示欢迎页。

### 5.3 一次 run

1. 用户 Enter → UI 追加本地用户消息（临时 id `pending-user-N`）→ 禁用发送、切换为停止 → 发送 `Run(text)`。
2. `AgentStarted` → 运行态 `Running`，清空流式缓冲。
3. `StreamEvent`：
   - `Start`：新建 assistant 临时消息；
   - `Delta`：按 block index 追加文本/thinking，16ms 合并；
   - `BlockEnd`：封块；
   - `Done`：等待持久化消息替换；
   - `Error`：标红，保留已生成内容。
4. `MessageAdded`：用户消息用临时消息替换（按首次出现顺序）；assistant 消息替换临时流式消息；工具结果不单独变成气泡，而是归属工具卡片。
5. `ToolCallStarted/Update/Finished`：更新卡片（§6）。
6. `ProviderResponse`：更新 header 的 token 用量；`Error`：内联 Alert。
7. `AgentFinished`：运行态 `Idle`，恢复发送按钮；刷新会话列表（标题/时间）；若最后一条是错误终态，保持错误可见。

### 5.4 关闭与恢复

- 关闭：发送 `Shutdown` → 取消 run → 有界等待（3s）→ drop NSH → 退出。
- 强杀：append-only 会话文件保证已落盘消息可读；重开扫描时截断最后一条不完整行（Phase 1 已实现）。
- 恢复后不自动重发；未完成的工具调用在界面上标记“上次运行中断”，不伪造结果。

## 6. 工具卡片

固定结构（从左到右）：

```text
[icon] 工具名 · 摘要（路径/命令）        [状态 Badge] [时长] [展开/收起]
        └ 参数摘要（单行，省略号）
        └ 结果/错误（折叠；代码等宽）
        └ [审阅改动]（仅等待审阅时）
```

状态机：

| 状态 | 触发 | 展示 |
|---|---|---|
| `Queued` | `ToolCallRequested` | 中性 Badge“等待” |
| `Review` | 审阅请求入队 | 警示 Badge“待确认”，按钮“审阅改动” |
| `Running` | `ToolCallStarted` | Spinner + “运行中” |
| `Partial` | `ToolCallUpdate` | 运行中 + 最新一行输出 |
| `Done` | `ToolCallFinished` 且非错误 | 成功 Badge + 时长 |
| `Failed` | `ToolCallFinished` 且 `is_error` | 危险 Badge + 错误首行；“展开”看全文 |
| `Blocked` | 审阅拒绝/扩展否决 | “已拒绝” + 原因；不显示成功 |
| `Interrupted` | 恢复会话时发现无配对结果 | “上次运行中断” |

- 工具名用 `label`（缺失回退 `name`）；MCP/扩展工具保留命名空间但不把前缀当标题。
- 参数摘要只显示关键字段（path/command/query），完整 JSON 在展开区。
- 输出超过 200 行或 32 KiB：显示“已截断，查看全文”，全文进审阅 sheet 或系统临时文件；不在卡片里无限增高。
- 时长来自 `duration_ms`；等待审阅的时间单独显示为“等待确认 Ns”，不计入工具执行时长。

## 7. diff 审阅

- 触发：`ReviewRequested`（04 §4）。审阅队列**串行**展示；并行工具可能同时入队，header 显示“待审 2”。
- Sheet 内容：
  1. 标题“审阅文件改动”；
  2. 文件头：路径（相对 `work_dir`）、新增/删除行数；
  3. `Diff`：unified 视图，默认上下文 3 行，可展开；
  4. 底部：拒绝（outline/ghost，点击后可填原因）与接受（primary）。
- 只读：`Diff` 本身不支持编辑；接受/拒绝是应用动作。禁止在 diff 上做“行内编辑后接受”，本阶段不做。
- 键盘：`Esc` 关闭 = 拒绝（需确认？不：关闭即拒绝，并在卡片显示原因“未审阅”）；`Ctrl/Cmd+Enter` 接受；`Ctrl/Cmd+Backspace` 拒绝。
- 大 diff：超过 2 MiB 或 5,000 行，显示文件摘要与“打开外部编辑器”按钮；接受/拒绝仍可用（04 §5）。
- 拒绝后工具返回错误结果 `Rejected by user: {reason}`，模型可据此调整；界面卡片状态 `Blocked`。
- 多文件：`DiffState` 支持一个 state 多文件；本阶段工具一次只改一个文件，但结构按多文件设计。

## 8. 命令面板

- 打开：`Ctrl/Cmd+K`；关闭：`Esc`；确认：Enter。
- 分组与命令（全部调用同一 action/owner 方法）：
  - 会话：新建会话、切换到会话…、删除当前会话…
  - 运行：取消当前任务、重新发送上一条、聚焦输入框
  - 界面：设置、切换语言、切换主题、打开插件预览（仅 `nsh-poc`）
  - 模型：选择模型…（空闲时重建内核）
- 空结果：说明“没有匹配的命令”，不显示假条目。
- 最近使用不持久化（避免第二个状态源）。

## 9. 设置

设置页在聊天区打开，左侧分组、右侧内容，使用 `Settings` 组件：

| 页 | 内容 |
|---|---|
| 常规 | 语言（中文/English）、主题（跟随系统/浅色/深色）、界面字号 |
| 模型 | 当前 provider/model、思考档位；只列已配置且凭证可用的模型 |
| 凭证 | 每个 provider 一行：状态（钥匙串/环境变量/未配置）、设置/替换/移除；输入不回显 |
| 工具 | “执行命令前询问”（默认开）、“文件改动前审阅”（默认开，C2）、“本会话始终允许某工具”清空 |
| 扩展 | 已装载/降级/失败列表、诊断码、信任按钮（调用 `trust_extension` 前确认） |
| 关于 | 版本、内核版本、NSH PoC 入口（feature 开启时） |

- 修改立即生效的项（主题/语言/审阅开关）落桌面设置文件；模型与凭证修改需要重建内核/写钥匙串，显示成功/失败结果。
- 设置文件只存引用与非敏感项；任何明文 key 不允许写入（02 §3）。

## 10. 空、加载、错误、权限态

| 状态 | 界面 |
|---|---|
| 无会话 | 欢迎页：一句产品说明 + “新建会话” primary + 3 条示例任务（点击填入输入框） |
| 无模型/凭证 | 欢迎页替换为“配置模型”引导，按钮打开设置·模型/凭证；发送禁用并说明原因 |
| 内核装配中 | 侧栏骨架、聊天区 Spinner；超过 5s 显示“仍在加载扩展…” |
| 会话扫描中 | 侧栏骨架；扫描失败显示重试按钮 |
| 运行中 | 停止按钮、工具卡片进度；输入框禁用但保留草稿 |
| 网络/协议错误 | 内联 Alert（原因 + 重试/打开设置）；不丢已生成内容 |
| 扩展 degraded/failed | 设置·扩展红点；不阻塞对话；诊断可展开 |
| 审阅拒绝 | 卡片 Blocked + 原因；run 继续，模型收到拒绝结果 |
| 应用恢复中断 | 顶部一次性 Alert“上次运行被中断”，会话内容完整 |

错误文案规则：说清“发生了什么 + 下一步”，不贴堆栈；原始错误放可展开详情（脱敏）。

## 11. 键盘与焦点

| 快捷键 | 动作 |
|---|---|
| `Ctrl/Cmd+N` | 新建会话 |
| `Ctrl/Cmd+K` | 命令面板 |
| `Ctrl/Cmd+,` | 设置 |
| `Ctrl/Cmd+Enter` | 接受当前审阅 |
| `Ctrl/Cmd+Backspace` | 拒绝当前审阅 |
| `Esc` | 关闭最上层覆盖；审阅关闭=拒绝 |
| `Enter` / `Shift+Enter` | 发送 / 换行（输入框聚焦时） |
| `Ctrl/Cmd+.` | 取消当前 run |

- 打开覆盖层时把焦点移入，关闭后回到触发控件；`Root` 负责模态焦点陷阱。
- 所有图标按钮有可访问名称与 tooltip；工具卡片按钮不用 hover 才出现。
- 焦点环不被 `overflow_hidden` 裁掉（审阅 sheet 留出内距）。

## 12. 语言、主题与可访问性

- 语言：`rust_i18n`，`locales/zh-CN.yml` 与 `en.yml`；默认 `zh-CN`，切换立即重绘并持久化。技术标识（工具名、路径、命令）不翻译。
- 主题：只用 `cx.theme()` 语义 token；明/暗/跟随系统；不写裸十六进制。界面字号走主题 base font，rem 相对尺寸。
- 对比度：状态同时用文字 + 图形，不只靠颜色；错误/危险色只表达语义。
- 动效：流式 shimmer、sheet 进出可短过渡；尊重减少动效；不用动画解释状态。
- 窗口缩窄：先折叠侧栏文字、再让审阅覆盖；不隐藏主操作（发送/停止/接受/拒绝始终可达）。

## 13. UI 集成测试清单

用 `#[gpui_kit::test]` 覆盖（无真实模型，假内核桥）：

1. 新建会话 → 发送 → 流式文本出现在消息流 → AgentFinished 后按钮恢复。
2. 工具卡片状态：Queued → Review → Running → Done；Failed 显示错误。
3. 审阅：Accept 发送 allow；Reject 发送 reject；Esc 等于拒绝；队列第二条接着出现。
4. 取消 run：点击停止 → abort 调用一次；迟到事件不复活运行态。
5. 会话切换/删除：当前会话删除需先离开；AlertDialog 文案含对象名。
6. 命令面板：Ctrl+K 打开、查询过滤、Enter 执行、Esc 关闭并还焦点。
7. 设置：语言切换后可见文案变化；审阅开关改变后下一工具请求带对应策略。
8. 空态/无凭证态：发送禁用且说明原因。

视觉与像素级核对在真实窗口里做；测试不声称覆盖 OS IME、系统剪贴板与像素。

## 14. 未纳入本阶段

会话重命名/置顶/分叉；消息内联编辑与重发；多选会话批量操作；代码块折叠/复制按钮的完整交互；富附件；语音输入；通知中心。
