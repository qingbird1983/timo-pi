# 研究 05：参考项目 pi-flash 分析

- 状态：定稿
- 日期：2026-10-26
- 调查对象：`F:\Warehouse\Github\pi-flash`（github.com/danielbill/pi-flash）
- **合规约束**：该项目整体 **GPL-3.0-or-later**（含其中文设计文档）。与 PI-Desktop（LGPL）同级处理：**严禁复制/翻译/结构性照搬任何源码**；只吸收思路、架构与"陷阱事实"（版权不保护思路与事实，但不要逐句搬其文档文字）。一经复制代码，Timo-pi 整体被 GPL 传染。

## 1. 定位与形态

pi-flash 是 **Windows x64 专用的 pi 桌面 GUI 壳**：pi 本体（TS，锁 1.4.0）原样作为 vendored Node sidecar 经 `--mode rpc` 运行，作者用 **Rust + GPUI（Zed）+ gpui-component（longbridge）** 重写整个桌面前端。**它不是内核重写**——LLM 协议、agent 循环、工具系统全在 pi 里，壳只做 RPC 客户端（`crates/pi-link`，约 1.6 万行 typed JSONL/stdio 协议层）。

- 规模：Rust 约 11.2 万行（app 87k / pi-link 16k / 插件宿主等），**中文设计文档约 1.15 万行**（架构契约 + 55+ 编号模块设计）。
- 成熟度：1,234 commits（2026-09-24 ~ 10-25，约一个月，单人 + AI 高强度），已发 npm 与 GitHub Releases（v0.3.1），作者自用级真实产品；**无 push/PR 测试 CI**（质量靠发版 checklist）。

## 2. 对 Timo-pi 三条线的参考价值

### 2.1 内核线：参考价值低（它没有内核）

仅三个可吸收点（均为思路级）：

- `pi-link/src/sessions.rs`：会话**索引化扫描**（tail-seek + (mtime,size) 指纹索引 + 尾窗解析）——对 Phase 1 自研会话存储的快速加载有直接思路价值。
- `pi-link/src/protocol.rs` 注释沉淀的 **pi RPC 行为陷阱清单**（streaming_behavior 原子判定、Abort 不清队列、get_tree 深递归等）——若做编排兼容层，这是现成的坑清单。
- `extensions/pi-subagents`（1.9k 行 TS，GPL）：基于 pi 扩展 API 的 subagent 编排（spawn/fork、容量配额、深度限制）——编排兼容层的**行为规格级参考**（只看思路）。

### 2.2 桌面线（gpui/gpui-kit）：**参考价值最高**（用户判断正确）

- 极少数非 Zed 官方的**大型生产级 GPUI 应用**（87k 行）且全程 Windows 实战——Timo-pi Phase 3 要走的路它已走通一遍。
- `docs/agents/gpui.md`：**gpui 0.2.2 / gpui-component 0.2.0 陷阱全集**（滚动句柄跨帧复用、拖拽逆序派发、occlude 截断 hitbox、滚轮注册顺序、双帧 notify、亚像素抖动等）——吸收知识点无版权问题，Phase 3 开工必读。
- 工程方法论可直接照搬（思路）：gpui/gpui-component **vendored + `[patch.crates-io]`**、debug 保留行号表、热依赖单独 O3、`incremental=false`（26G target 膨胀教训）；大型 GPUI 应用的**状态所有权契约**与架构守卫（行数红线脚本）——与本仓库 standards/08 方向一致，可互相印证。
- ⚠️ **版本谱系注意**：pi-flash 用的是 longbridge **gpui-component 0.2.0，不是 gpui-kit**。gpui-kit 疑为 gpui-component 的后继发布形态；**Phase 3 开工前必须核对两者 API 延续性**，其 vendor 补丁（ScrollHandle、滚轮注册顺序等）说明 0.2.x 有实坑，选新版需复核。

### 2.3 插件系统线：参考价值高（与 NSH 能力面几乎同构）

`crates/pf-plugin-host` + `docs/模块设计/090-*` 实现了完整的"内嵌 QuickJS + 签名 + 热更分发"闭环，Phase 2/4 设计时值得精读其文档（只吸收思路）：

- **双载体同一契约**：JS 插件（rquickjs/quickjs-ng 跑 ESM，唯一能力面 `ctx`）与 wasm 插件（wasmtime component-model + WIT）共用同一份**组件树 JSON 渲染契约**，宿主统一解释。
- **分发安全链**：ed25519 宿主证明（fail-closed）、`main.js --qjsc--> plugin.qbc` **字节码落盘无明文源码**、安装即终态、卸载先撤运行态再删目录。
- **清单设计**：package.json 即清单，"一份包两个读者"（`contributes`/`engines` 给壳、`pi` 节透传）；npm/git/本地三源防双载。
- **线程模型实录**：rquickjs Runtime 非 Send → 固定 gpui 主线程、动作类入队——对 NSH 内嵌 QuickJS 的宿主集成是直接可参考的架构答案。

## 3. 合规与边界补充

- pi-flash 仓库内 vendored 的 gpui / gpui-component 上游本身是 Apache-2.0（Zed/longbridge）——但 Timo-pi 直接从上游获取即可，**不经过 pi-flash**。
- 其 README 有商标条款：不得以 "Pi-Flash" 名义再分发、不得指向其更新通道（与我们无冲突，知悉即可）。
- 它对 pi-web（MIT）"仅行为对齐、声明未复制源码"的做法，正是本仓库 research/02/04 既定方针的旁证。

## 4. 行动项

| 时点 | 动作 |
|---|---|
| Phase 3 开工前 | 核对 gpui-kit ↔ gpui-component 版本谱系与 API 延续性；通读其 `docs/agents/gpui.md` 吸收陷阱清单（只记结论不搬文字） |
| Phase 2 设计 | 读 `pi-link/src/protocol.rs` 陷阱注释、pi-subagents 行为规格（思路） |
| Phase 4 设计 | 读 `docs/模块设计/090-*` 插件系统三篇（思路）；对照 NSH 能力面取长补短 |
