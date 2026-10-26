# H-008：Phase 3 第 5 批——审阅 sheet、命令面板、设置与钥匙串

- 日期：2026-10-27
- 状态：完成（四道门禁绿，真机验收通过）
- 上一批：`H-007-phase-3-chat-bridge.md`

## 本批交付

1. **凭证链**（`crates/timo-config`）：`ValueRef::Keychain { service, account }`、`keychain:<service>/<account>` 语法；`KEYCHAIN_SERVICE = "timo-pi"`、account = provider id；Debug 脱敏；`set_provider_api_key` 原子改写 `models.json`（保留未知字段）。钥匙串后端 keyring 3.6.3（Windows 原生）。
2. **diff 审阅**（`features/review.rs`）：`similar` 生成 unified diff → `DiffFile::parse` → `DiffState` → `Diff`；上限 2 MiB / 5,000 行，二进制只显示摘要；串行队列与“本会话允许”缓存；接受/拒绝回传 `ReviewDecision`，拒绝原因写进 trace；取消 run 时 pending 审阅按拒绝处理。
3. **审阅接缝注入**：桌面 `ReviewBroker` 实现 `ToolReview`，按设置 `review.files/commands` 放行或弹 sheet；`KernelOptions.tool_review` 注入；CLI 仍为 `None`。
4. **命令面板**（`features/palette.rs`）：Ctrl+K 开关；分组（会话/运行/界面/模型）；本地过滤、Enter 执行、Esc 关闭；打开时清空上次查询。
5. **设置页**（`features/settings_view.rs` + `settings_credentials.rs`）：常规（语言/主题/字号）、模型（只读列表）、凭证（钥匙串/环境变量状态、设置、移除确认）、工具（两个审阅开关、清空本会话允许）、关于；修改即时持久化到 `desktop.json`。
6. **桌面设置**（`src/settings.rs`）：`desktop.json`（schema/locale/theme/font_scale/review/window），原子写；启动恢复，退出保存窗口尺寸。
7. **快捷键**（设计 01 §11）：Ctrl+N、Ctrl+K、Ctrl+,、Ctrl+.、Ctrl+Enter（接受审阅）、Ctrl+Backspace（拒绝审阅）、Esc（关面板/设置；审阅由 sheet 处理）。
8. **测试**：桌面 22 条（新增审阅接受/拒绝/快捷键、面板开关与查询清空、设置快捷键）；workspace 全绿。

## 真机验收（DeepSeek 真实模型，临时 `TIMO_AGENT_DIR`）

- **写文件审阅**：请求写 `notes.txt` → sheet 显示 `+1 -0 · 新文件` 与 diff；接受后文件落盘，内容为 `timo review ok`，模型回读确认。
- **拒绝写入**：请求写 `notes2.txt` → 拒绝后文件不存在，模型收到拒绝结果并说明未创建。
- **拒绝命令**：请求 `echo hello > cmd-out.txt` → sheet 显示 `powershell · 工作目录` 与命令；拒绝后文件未创建，命令未执行。
- **钥匙串**：设置页保存 `DEEPSEEK_API_KEY`（掩码输入）→ `models.json` 只保留 `keychain:timo-pi/deepseek`，扫描数据目录无 `sk-` 明文，Windows 凭据管理器出现 `deepseek.timo-pi`；移除后条目消失、状态变“未配置”、页面内显示成功反馈。
- **命令面板**：Ctrl+K 打开；输入“设置”过滤；执行“打开设置”；再次打开时查询已清空；快捷键提示显示 Ctrl+N / Ctrl+,。
- **设置页**：Ctrl+, 打开；标题与分组顶部对齐；语言/主题/字号按钮即时生效；主题写入 `desktop.json` 并在重启后恢复。
- **会话侧栏**：次要信息由裸消息数改为相对时间（如“36 分钟前”）。

## 验收中发现并修复

- 面板再次打开残留上次查询 → 打开时 `set_query("")` 并聚焦。
- 审阅快捷键不生效 → App 根级 action 不在 sheet 覆盖层的焦点路径上；改为 sheet body 自持焦点 + key context + `on_action`，打开后延后一帧聚焦（P-009）。
- 关闭面板/设置后焦点丢失，全局快捷键失效 → 关闭时把焦点还给输入框。
- 设置页内容垂直居中、上方大片空白 → 内层 `h_flex` 补 `items_start`，页面 `h_full`。
- 钥匙串已移除但仍显示“系统钥匙串” → 读取供应商列表时校验条目存在，缺失显示“未配置”；设置页内显示保存/移除反馈。

## 已知限制（已登记 backlog / 后续批次）

- 模型页只读：运行期切换模型需重建内核，未做。
- 设置缺少“扩展”页；扩展诊断目前只写 `extensions-diagnostics.json`。
- 侧栏会话搜索框（设计 01 §5.2 的 120ms 本地过滤）未做。
- 超过 2 MiB / 5,000 行的 diff 只显示摘要，未提供“打开外部编辑器”。
- trace “查看全文”仍无入口，全文保存在 `TraceEntry.output`。
- 移除钥匙串后配置引用保留（按设计 02 §8：解析报错、提示重新配置），未引导“改用环境变量”。
- desktop 设置里的审阅开关只影响后续请求；运行中的请求不重放。

## 下一步

- 第 6 批：NSH PoC（`nsh-poc` feature 门控）+ C1~C6 验收 → `report.md` → 看板/CHANGELOG → tag `phase-3.end`。
