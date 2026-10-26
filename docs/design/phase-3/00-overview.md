# 设计 00：Phase 3 总览与跨层决策

- 状态：定稿
- 日期：2026-10-26
- 关联：roadmap/phase-3-desktop-shell；architecture/01~03；ADR-0002/0009；ADR-0012；本目录 01~04

## 1. 定位

本目录是 Phase 3「gpui-kit 桌面壳」的实现级详设。目标不是做一个展示页，而是**日常可用的编码代理桌面应用**：会话可恢复，对话逐字流式，工具过程可见、可审，文件改动能接受或拒绝，设置与密钥安全，停掉应用后会话无损；同时用最小面积验证 NSH 可嵌入。

三条不可越过的边界：

1. **内核仍是纯库**：`apps/timo-desktop` 只 import `timo-kernel` 门面；GPUI/NSH 依赖不得进入 `crates/`（architecture/01 §2）。
2. **GUI 只消费事件与显式 API**：内核不得知道窗口、组件或对话框存在；审阅是显式请求-响应接缝，不是内核里的 UI 分支（ADR-0009、ADR-0012）。
3. **不照搬参考项目**：pi-flash 的 GPUI 0.2.x 经验只吸收事实与陷阱，不复用其代码/补丁（铁律 7，research/05 §2.2）。

## 2. 范围

### 做

- 窗口骨架：会话侧栏、聊天流、工具卡片、diff 审阅、命令面板、设置页、取消与错误展示。
- 内核事件订阅层：GUI 作为事件总线消费者的参考实现。
- 工具审阅接缝：`write`/`edit` 执行前查看 diff 并接受/拒绝；`bash` 执行前确认；默认不改变 CLI 行为（ADR-0012）。
- 密钥：设置页写入系统钥匙串，配置只存引用；日志/界面不回显明文。
- NSH PoC：AppView 嵌入静态 TSX 面板；签名包替换后热更；未签名包被拒；仅 `nsh-poc` feature 启用。
- Windows 优先打磨；macOS/Linux 保证可构建并记录未验证面。

### 不做

B 轨插件系统与 marketplace；多窗口/多标签工作区；主题编辑器；自动更新/安装器；模型 OAuth；语音；遥测；跨设备同步。

## 3. 平台与版本边界

| 依赖 | 锁定版本 | 用途 | 边界 |
|---|---|---|---|
| gpui-kit | 0.8.0 | GPUI 门面 + 组件 + 主题 + 测试 | 只出现在 `apps/timo-desktop` |
| gpui-component | 0.8.0（由 gpui-kit 引入） | 消息流、Diff、命令面板、Settings、Sidebar | 不直接写依赖 |
| gpui-fast / gpui-fast-platform | 0.1.10（版本钉） | NSH 要求的 GPUI 后端 | 应用直接钉 `=0.1.10`（仅版本约束，不 import）；升级必须重跑 NSH 探针 |
| nsh-platform-gpui / nsh-engine / nsh-package | 0.3.0 | AppView 嵌入、签名包验证 | `default-features = false, features = ["js"]`，不进内核 |
| nsh-cli | 0.3.0（仅开发期） | `keygen` / `certify` / `publish` / `verify` | 不进入运行时依赖；版本写入脚本与报告 |
| keyring | 3.6.3 | 系统钥匙串读写 | Windows/macOS/Linux 三后端；4.x 升级另立窗口 |
| similar | 2.7.0 | 由 before/after 生成 unified diff 供 `DiffFile::parse` | 只在桌面壳使用 |
| rust-i18n | 4.2.x | 中英界面文案 | 只在桌面壳使用 |

**版本谱系核对（2026-10-26）**：

- gpui-kit 是 `gpui-base` + `gpui-component` + assets 的打包门面；pi-flash 用的是更早的 `gpui-component 0.2.0`，其 ScrollHandle/vendored 补丁**不能直接套用**。本阶段以 0.8.x 源码与技能文档为准。
- **仓外编译探针已通过**：`gpui-kit 0.8.0` 家族 + `gpui-fast 0.1.10` + `nsh-platform-gpui 0.3.0` 的 `cargo check` 成功（临时 crate 同时实例化 `AppView::new` 与 `Button`）。
- 失败路径（已实测）：解析到 gpui-kit 0.8.1 / gpui-fast 0.1.12 时，NSH 0.2.0 与 0.3.0 都在 `chrome.rs` 报 E0716（`KeyBinding::new` 在 0.1.12 要求 `'static`）。把 gpui-fast 钉到 0.1.10 后，Cargo 解析器自动选择 0.8.0 家族，三者共存。
- 因此**锁 gpui-kit 0.8.0 家族 + gpui-fast/gpui-fast-platform =0.1.10 + NSH 0.3.0**；升级任一依赖都必须重跑该编译探针。
- 探针形态（仓外临时 crate，不入库）：`gpui-kit = { version = "=0.8.0", features = ["gpui-fast"] }`、`gpui-fast = "=0.1.10"`、`gpui-fast-platform = "=0.1.10"`、`nsh-platform-gpui = { version = "=0.3.0", default-features = false, features = ["js"] }`；`cargo check` 退出码 0。
- 选用 NSH 0.3.0 而非 0.2.0：0.3 发布了 `nsh-cli`（keygen/certify/publish/verify），PoC 不需要自研 TSX 工具链；AppView 嵌入接口保持同名同形。
- NSH 默认 feature 含 `webview`；嵌入只用 `js`，避免无谓的 WebView 依赖进入 Windows 构建。

## 4. 模块布局

```
apps/timo-desktop/
  src/main.rs                 入口：application / init / open_window
  src/app.rs                  DesktopApp 根视图与窗口级动作
  src/kernel_bridge.rs        专用 tokio 线程、命令通道、事件转发、审阅通道
  src/theme.rs                gpui-kit 主题与明暗切换
  src/i18n.rs                 rust_i18n 初始化与语言切换
  src/features/
    sessions/                 侧栏列表、新建/切换/删除
    chat/                     消息流、流式拼装、工具卡片、输入框
    review/                   审阅队列、diff sheet、与内核审阅接缝对接
    settings/                 常规/模型/凭证/工具/扩展/关于
    command/                  命令面板
    nsh_poc/                  feature 门控的 AppView 实验页
```

- 单文件 ≤500 行；模块按能力聚合，不建全局 `views/`、`models/`、`modals/`（gpui-kit coding guide）。
- 先单 crate；当某能力出现独立生命周期/公共接缝或应用超过 15,000 行时再拆 feature crate（standards/08 §4）。
- 测试：模型/状态机用普通 `#[test]`；核心交互用 `#[gpui_kit::test]` 无头窗口；NSH 渲染为人工验收。

## 5. 内核前置改动（K0~K3）

Phase 3 发现内核缺少桌面所需的最小面。全部为**附加**或**修缺陷**，不改既有公共语义：

| # | 改动 | 理由与证据 |
|---|---|---|
| K0 | run 后保留完整上下文 | 现 `Kernel::run` 在 `kernel.rs:235` 取走 `history`，`run_agent_loop` 在 `run/mod.rs:183` 只返回 **new_messages**；第二次 `run` 从空历史开始，REPL/GUI 多轮会丢上下文。改为内部返回“新消息 + 完整上下文”，`AgentHandle::run` 公共返回值不变，Kernel 保存完整上下文 |
| K1 | 会话 API：`new_session()`、`history()`、`delete_session(id)` | `open_new_session` 现为私有（`kernel.rs:343`）；GUI 需要“新建草稿”“读取恢复后的历史”“删除会话”。新建草稿不落盘，首次 run 才创建（避免空文件） |
| K2 | `KernelOptions.tool_review: Option<Arc<dyn ToolReview>>` | C1/C2 的执行前审阅；`None` 时与 Phase 1/2 行为逐字节等价（ADR-0012） |
| K3 | 扩展信任辅助保持门面导出 | GUI 设置页调用 `trust_extension`/`summarize`；不新增权限路径 |

`history()` 返回**当前会话投影上下文**（resume 后为磁盘历史，run 后为完整上下文）；GUI 首次打开时用它填充消息流，之后以事件增量维护。

模型切换：Phase 3 不做 `set_model`。设置在**没有运行中的 run** 时重建 Kernel（保留 `data_dir`/`work_dir`，随后 `resume(当前 id)`）；有 run 时禁用并提示先取消。理由：模型解析、凭证、系统提示与扩展宿主都在装配期确定，重建是最小且可验证的路径。

## 6. 关键决策

| # | 决策 | 说明 |
|---|---|---|
| P3-D1 | 桌面壳用 gpui-kit 0.8.0 家族 + `gpui-fast 0.1.10` | NSH 嵌入要求同一 GPUI 后端；gpui-kit 0.7 无该 feature |
| P3-D2 | NSH 锁 0.3.0、独立 feature、只做 PoC | 版本年轻；Phase 4 才做插件系统 |
| P3-D3 | 审阅在工具执行**前**，不是事后撤销 | 拒绝必须无副作用；diff 用 before/after 精确生成（04） |
| P3-D4 | CLI 审阅默认关闭 | 保持 Phase 1/2 自动执行语义；桌面显式注入审阅器 |
| P3-D5 | 钥匙串引用 `keychain:<service>/<account>` | 配置只存引用；GUI 不写明文（02） |
| P3-D6 | 中英双语，默认中文 | 遵循所有者偏好；所有可见文案入 locale 文件 |
| P3-D7 | 单窗口、侧栏 + 聊天 + 审阅 sheet | 侧栏 260（240~360），聊天弹性；审阅 440（360~640），窗口 <1200 时改为覆盖聊天右侧 |
| P3-D8 | 流式增量 16ms 合并刷新 | 避免每个 token 触发整树渲染；尾随滚动只在跟随尾部时 |
| P3-D9 | 会话删除用 AlertDialog，当前运行中禁止 | 名称对象 + 后果，不做“确定吗”式空话 |
| P3-D10 | 崩溃恢复依赖既有 append-only 会话 | GUI 不另建草稿持久化；重开扫描会话目录 |
| P3-D11 | 主题只用语义 token，无裸色值 | 明/暗/跟随系统；原型中的颜色只是方向 |
| P3-D12 | 模型切换=空闲期重建 Kernel | 不新增运行期热切模型路径 |

## 7. 线程与生命周期

- **UI 线程**：GPUI 主线程；只做状态与渲染，不做阻塞 IO。
- **内核线程**：专用 tokio runtime（单线程即可），独占 `Kernel`（`run` 需要 `&mut`）。命令经 `mpsc` 进入；`KernelEvent` 与审阅请求经 `mpsc` 回到 UI；UI 用 `cx.spawn` 接收并 `cx.notify()`。
- **审阅等待**：工具任务在 tokio 线程等待 `oneshot`；不占用 UI 线程，不占用扩展宿主线程。取消/关窗 → drop sender → 工具收到拒绝。
- **NSH**：每个 AppView 自带引擎线程；只由 `nsh_poc` 模块持有，drop 即停。
- **关闭顺序**：取消当前 run → 等待内核任务退出（有界 3 s）→ drop AppView → 退出。不等待则直接进程退出，会话仍由 append-only 保证可读。

## 8. 提交序列

1. 本批：设计 00~04 + 原型 + ADR-0012 + 文档登记（tag `phase-3.start`）。
2. 最小窗口（空壳可运行）+ 版本锁：gpui-kit 0.8.0 / gpui-fast 0.1.10 / NSH 0.3.0 / keyring / similar。
3. K0/K1 内核修复与测试；K2 审阅接缝与 CLI 不变性测试。
4. 会话侧栏 + 聊天流 + 工具卡片 + 输入/取消（可日常对话）。
5. diff 审阅 sheet + 命令面板 + 设置/钥匙串。
6. NSH PoC（feature 门控）+ 签名包热更/拒绝路径。
7. C1~C6 验收 → `report.md` → 看板/CHANGELOG → tag `phase-3.end`。

每步四道门禁：fmt / clippy / test / check-size；UI 步骤另跑 `#[gpui_kit::test]` 核心交互。

## 9. 验收映射

| # | 验收 | 设计落点 | 证据形态 |
|---|---|---|---|
| C1 | 日常使用清单 | 01 §4~§6、04 §5 | 真实 DeepSeek 会话录屏/截图 + 重启恢复 |
| C2 | 权限/信任确认 | 04 §3~§6 | 拒绝写文件、拒绝命令；副作用不存在 |
| C3 | 密钥安全 | 02 §3~§5 | 钥匙串条目、配置文件/日志扫描无明文 |
| C4 | NSH PoC | 03 §3~§6 | 面板截图、v1→v2 热更、未签名拒绝 |
| C5 | 崩溃恢复 | 01 §5.4 | 强杀重开，会话条数与内容不变 |
| C6 | 接缝 + CI | 00 §5、01 §9 | 内核零 GUI 依赖、`#[gpui_kit::test]` 通过 |

真实模型验收沿用 Phase 2 方式：`DEEPSEEK_API_KEY` 只从环境变量读取，不打印、不入库；以 `keychain:timo-pi/deepseek` 路径另验 C3。

## 10. 风险与对策

| 风险 | 等级 | 对策 |
|---|---|---|
| gpui-fast 0.1.12 / gpui-kit 0.8.1 与 NSH 不兼容 | 高 | 已实测 0.1.12 触发 E0716；锁 0.8.0 家族 + 0.1.10，升级前重跑编译探针；失败则 C4 降级为“独立 NSH 窗口 + 记录阻塞”，不伪造嵌入 |
| gpui-kit 0.8 API 与技能文档偏差 | 中 | 只以 registry 源码签名为准；组件用法先写最小用例 |
| 审阅等待导致 run 无法取消 | 高 | 审阅等待同时 select 取消臂；关窗 drop sender；测试“取消后无写入” |
| K0 改动影响事件/会话语义 | 高 | 既有测试一字不改全绿 + 新增多轮上下文测试 + 会话文件逐行对照 |
| 钥匙串在无 Secret Service 的 Linux 失败 | 中 | 明确错误页 + 允许 `$ENV` 引用；不静默回退明文 |
| 工具卡片/流式渲染导致重绘风暴 | 中 | 16ms 合并；只 notify 一次；虚拟列表 + 稳定 ElementId |
| diff 过大拖慢 UI | 中 | 2 MiB 上限；超限显示摘要与接受/拒绝，不内联全文 |
| NSH 包供应链 | 中 | 只信本地 TrustStore 根；未签名/过期/降级包拒绝；PoC 密钥不入库 |

## 11. 开放问题

- 审阅“本会话始终允许某工具”是否跨会话持久化：本阶段只做会话内，持久化留给 Phase 4 权限 UI。
- 助手 Markdown 的代码块复制/折叠：先用 `TextView::markdown`，真实长文体验后再补。
- 会话标题是否允许重命名：本阶段用首条用户消息；重命名进 backlog。
- NSH 面板与主应用的主题同步粒度：PoC 只同步明暗与基础色，不做完整 token 映射。

## 12. 文档与体量

- 新增 `docs/design/phase-3/` 5 篇 md（00~04）+ 1 个原型 HTML；ADR-0012。
- docs 总数 60（软上限 60，已达上限；下一阶段前触发整合评审，不新增孤儿文档）；单篇 ≤500；ADR ≤100。
- 本阶段不写实现代码；`phase-3.start` 标签只代表设计定稿。
