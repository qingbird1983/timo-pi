# H-009：Phase 3 第 6 批——NSH PoC 与 C1~C6 验收

- 日期：2026-10-27
- 状态：完成（四道门禁绿，C1~C6 真机通过）
- 上一批：`H-008-phase-3-review-settings.md`

## 本批交付

1. **NSH 隔离层**（`features/nsh_poc/`，`nsh-poc` feature 门控）：`NshPreview` 独占 `nsh_platform_gpui`；`verify_package` → 新 `Entity<AppView>` 暂存 → 1 秒健康检查 → `accept_package` 后交换；失败保留旧面板。
2. **信任根**：`<agent_dir>/nsh-poc/trust/` 放 `*.pub`；`TrustStore::open` 持久化已接受构建，降级包被拒；“重置信任”删除 `nsh-trust.json`。
3. **页面**：侧栏“插件预览（实验）”入口；加载 v1/v2/崩溃/未签名/篡改、重置信任、停止；显示当前 id/version/build/sha 与最近状态；`inactive_frame_interval: None`。
4. **打包脚本**：`scripts/build-nsh-poc` 只调用官方 `nsh 0.3.0 (76fa8a7)` 的 `keygen → certify → publish → verify`；v1/v2 共用 `fixtures/nsh-poc/panel.tsx`，v2 只改版本常量与标题色；另生成崩溃包、篡改包、未签名字节。密钥与包只在数据目录。
5. **测试**：`unsigned_bytes_are_rejected`（feature 下运行）；桌面 23 tests（含该条）全绿；默认 workspace 228 tests 全绿。

## 真机验收（DeepSeek + NSH，临时 `TIMO_AGENT_DIR`）

- **C1**：新建会话 → 真实模型回复 → 工具卡片；写 `proof.txt` 前审阅接受，文件内容 `timo phase3 accept`；第二轮拒绝写 `rejected.txt`，文件不存在、模型收到原因。
- **C2**：审阅面板显示 `+1 -0 · 新文件` 与 diff；接受/拒绝均无错误副作用。
- **C3**：沿用 H-008 钥匙串证据；本轮 `sk-` 扫描临时数据目录与运行日志 0 命中。
- **C4**：v1 渲染、计数 0→1；v2 热更到 2.0.0（标题变橙、sha 变化、无重启）；未签名/篡改/降级/崩溃四类拒绝且面板保持；深色主题跟随；最大化 1745×768 面板跟随。
- **C5**：`taskkill /F` 强杀后重开，原会话、两轮消息、工具 trace 与拒绝原因完整；会话文件 9 行可读。
- **C6**：七条接缝逐条核对通过；桌面 11 条 `#[gpui_kit::test]` + 1 条 UI 集成测试。

## 验收中发现并修复

- `use gpui_kit::*` 把 gpui 的 `test` 属性宏带进子模块，普通 `#[test]` 自递归、抬高 recursion_limit 也无穷 → 改为显式导入（P-010）。
- `nsh_poc.rs` 单文件 667 行超 500 → 拆 `mod.rs`（409）+ `page.rs`（268）。
- 首次真机启动报 “no provider with usable credential”：临时目录需放 `models.json`（`base_url` 字段是下划线命名，`api_key` 用 `$DEEPSEEK_API_KEY`），不是代码缺陷。

## 已知限制

- NSH 渲染/热更为人工验收；CI 只跑“未签名拒绝”单元测试（设计 03 §8 允许）。
- 开发签名密钥留在用户数据目录；生产分发密钥策略留 Phase 4 ADR。
- `target/` 约 38GiB；`cargo-sweep 0.8.0` 已装，30 天按龄清扫 0 释放（产物均新于 30 天），需 sccache + `cargo clean`。
- 其余桌面壳遗留见 roadmap phase-3 §6。

## 下一步

- 打 tag `phase-3.end`；Phase 3.5 设计定稿后开工；视觉精修与 3.5 并行。
