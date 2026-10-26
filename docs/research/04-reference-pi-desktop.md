# 研究 04：参考项目 PI-Desktop 分析（只借鉴，不抄码）

- 状态：定稿
- 日期：2026-10-26
- 调查对象：`F:\Warehouse\Github\PI-Desktop`（github.com/vastsa/PI-Desktop）
- **合规约束**：该项目为 **LGPL-3.0-or-later**。本项目**严禁引用其源码**；本文只记录其公开文档与结构所体现的架构思路。

## 1. 定位与形态

第三方社区项目（非 pi 官方），定位"pi 做 Agent Engine 的持久化桌面工作台"。技术栈：Electron 43 + React 19 + Rust host-core sidecar + Node pi agent sidecar，四进程模型。0.21.0-beta，3 个月 5376 commits，文档/ADR 驱动（spec 16 篇插件规范 + ADR 目录），README 自述开发耗 270 亿 token（重度 AI 辅助）。

## 2. 内核对接方式（与本项目路线的对照）

- 不 fork、不 spawn CLI，而是把 pi 三个 npm 包**锁 1.3.0 + pnpm patch**（改 dist 编译产物，补丁合计约 2,000 行）嵌入自己的 Node sidecar；配套依赖纪律 CI（版本钉死 + patch 契约 marker + 上游契约测试）。
- 边界原则：**"用 pi 的 agent loop，但不采用 CLI 的 session runtime，不让 JS 包拥有桌面持久化"**——持久化唯一所有者是 Rust host-core（SQLite，9.4 万行 / 715 个测试），Node 经 stdio NDJSON JSON-RPC 调用。
- 其架构 ADR **明确否决了 "Full Rust rewrite of agent loop"**（理由：太贵、失去 pi 上游杠杆）。→ 本项目选择重写（ADR-0001），代价已知，收益是消灭双运行时边界税（其 spec 中大量规则在填持久化所有权分裂、崩溃恢复语义的坑）与上游 patch 税。

## 3. 插件系统（自有，非 pi 格式）

- 形态：`manifest.json` + `main.js` + `renderer/`；插件跑在**专用 Node 插件进程**，界面跑在无 Node 的沙箱窗口。
- API（宿主注入全局 `pi`，走权限网关）：commands/ui/agent.registerTool/models/fs/clipboard/net.fetch/services/bus/settings/providers.oauth/audio 等。
- 权限：低/中/高三级声明 + 用户授予双闸；`manifest.fs` scope glob（**写/删禁止全树声明**）；`net.domains` 出网白名单 **fail-closed**（空=无网）；硬拒 `.env*`、SSH 凭证、`.git/**`；`agent.extension` 单列为高风险"信任等同 agent 本体"。
- 分发：`.piplug` ZIP 包 + devkit CLI + 官方 plugin center；**签名未实现**（当前信任 = SHA-256 + 人工审查）；插件进程**无 OS 沙箱**（项目自己承认是欠账）。
- **自家功能插件化验证扩展面**：其文件管理器就是一个 view 插件。

## 4. 对 pi 官方扩展的兼容策略（本案直接参照）

其 `docs/spec/07-plugins/16-trusted-extensions.md`（ADR 0214/0215/0244）**刻意兼容编排类子集**：

- "Import pi extension" 把扩展包装成 generated plugin；npm 依赖走受限安装（registry-only、禁 git 解析、禁 lifecycle scripts）。
- 加载：`jiti/static` + virtualModules；`pi-agent-core`/`pi-ai`/`typebox` 解析到宿主副本；**`pi-tui` 换成 inert Proxy stub**（可导入、调用即记诊断）；`pi-coding-agent` 换成只导出 `defineTool` + 类型守卫的 shim（`VERSION` 标 0.87.1 假版本以通过扩展检查）。
- 三分类矩阵：**Supported** = registerTool / registerCommand / registerProvider / on 事件 / exec / 工具开关 / setModel / setThinkingLevel / sendUserMessage；**Deferred v2** = sendMessage / appendEntry / sessionManager 读 / switchSession / registerShortcut / markdown transformer / editor 读写 / autocomplete；**Unsupported（全 TUI 面）** = ui.setWidget/setFooter/setHeader/setTitle/custom/overlay/onTerminalInput/setWorkingIndicator/pasteToEditor、四类 renderer 注册、navigateTree、shutdown——**成员存在但 no-op、每扩展每成员记一条诊断、永不 throw**。
- 钩子语义工程（真正的难点与工作量）：每 handler **30s 预算**、abort 即退休、payload **detach 拷贝**防 late mutation。

## 5. 借鉴与避坑清单

**借鉴（思路层面）**：三分类矩阵 + no-op + 诊断 + 永不 throw；VERSION 兼容标记与包版本解耦；权限双闸与 fs/net 作用域细则；"插件自有窗口"与"宿主窗口内 slot"二分法；renderer slot 的**白名单动作词汇表**接口收紧思路；强制命名空间前缀（`plugin_<id>_<tool>`）；spec/ADR 驱动 + 自家功能插件化自证扩展面完整。

**避坑**：双运行时边界税（本案用全 Rust 消除）；过渡依赖难摘除（其 pi-coding-agent"过渡性依赖"至今未移除 → 本案一步定义自有会话格式，ADR-0007）；patch 上游 dist 不可持续；插件沙箱欠账（本案用 NSH cap-std 从第一天解决）；其 renderer slot 依赖 Web DOM **不可平移到 GPUI**（本案用 NSH TSX 自建，ADR-0004）。
