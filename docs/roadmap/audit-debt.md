# 审查收债：代码—文档一致性与复杂度清偿

- 状态：已完成（2026-10-28；A/B/C/D 全部清零，workspace 419 tests 全绿）
- 日期：2026-10-27（同日两轮：三路一致性审查 + 全仓复杂度审计）
- 关联：architecture/01~05、architecture/03-critical-seams；standards/03/04/07/08；`.agents/handoff/H-010-audit-debt.md`（启动）、`H-012-audit-debt.md`（完成）；`.agents/pitfalls/P-012-session-half-line-append.md`、`P-016-hook-error-isolation-changes-dispatch-result.md`

## 1. 背景

2026-10-27 对 `crates/` 与 `apps/` 做了三路只读审查（依赖分层 / 事件总线与工具管线 / 会话与取消传播），逐条比对 `docs/architecture/01~05` 的书面承诺。结论：

- **主体一致**：事件总线、工具元模型、会话格式骨架、QuickJS 扩展宿主均为真实实现，不是骨架。
- **1 项数据级缺陷**：崩溃恢复存在半行拼接路径（A1），可导致会话文件永久不可读。
- **9 项代码缺口**：文档承诺的能力未落地或只落一半（B 组）。
- **10 项文档滞后**：文档写旧了或写满了（C 组；本轮已回填 8 项，余 2 项待收债）。

**同日第二轮：全仓复杂度审计（2026-10-27）**。范围 `crates/` + `apps/`（150 个 `.rs`、29.5k 行，跳过 vendored 与构建产物）。结论：

- **主体不臃肿**：无 TODO/FIXME，`#[allow(dead_code)]` 仅 1 处（有注释归属），依赖表基本零冗余。问题集中在"接口齐、语义无"的预留面与手写标准库，合计可删约 **330 行 + 1 个未用依赖**，归入 **D 组**（§3）。
- **与既有组重叠项不重复登记**：`ToolCallCtx.updates` 空管道见 B6，`settings.session_dir` 无消费者见 B7。
- **不在删除面**：扩展对 JS 的公开契约（`HostServices` trait、`pi.registerProvider` 等 no-op + `E202` 诊断、manifest/permissions 校验链）是书面承诺，一律保留——即使本阶段未实现，也必须"no-op + 诊断"而非静默成功。

本清单是**清偿台账**：每项给出证据位置与可执行验收标准。全部清零即本批次完成。

## 2. 范围

- 做：逐条清偿 A/B/C/D；补最小回归测试；文档与实现重新对齐；D 组删除面按"先改文档、再删代码"的顺序执行。
- 不做：新功能、架构重构、依赖升级、UI 改动；不借 D 组之机重写模块（只删不重设计）。任何超出清单的改动须先登记 backlog 或 ADR（standards/07 §范围纪律）。

## 3. 清单

### A 组 —— 数据安全（P0，优先于一切）

| # | 问题 | 证据 | 验收标准 |
|---|---|---|---|
| A1 ✅ | `SessionWriter::open` 以追加模式续写，不清除文件末尾残留半行。崩溃留下的半行会被新 entry 拼成「带换行的非法中间行」，此后 `read_entries` 恒报 `CorruptEntry`，该会话永久不可读 | `crates/timo-session/src/writer.rs:115-134`；读取端容忍见 `reader.rs:32-39` | 打开续写前截断末尾非完整行；新增测试：写半行 → open → 追加 → 全量可读（2026-10-28 已修） |

### B 组 —— 代码缺口

| # | 问题 | 证据 | 验收标准 |
|---|---|---|---|
| B1 ✅ | 桌面壳直接依赖 `timo-config`，绕过门面（违反 architecture/01 §2 原则 2） | `apps/timo-desktop/Cargo.toml:19`；调用点 13 处（`bridge.rs`、`bridge_providers.rs`、`settings.rs`、`features/nsh_poc/mod.rs`） | 门面补齐凭证/路径 API；`apps/timo-desktop` 的 `[dependencies]` 不再出现 `timo-config`；或按 ADR 正式豁免并回填 architecture/01 §2（2026-10-28 已修：新增 `timo_kernel::config`，路径改走 `data_dir()`） |
| B2 ✅ | 命名空间前缀无校验；`builtin_` 前缀不存在 | `crates/timo-tools/src/registry.rs:36-38`（仅 `insert`）；内置工具裸名见 `tools/read.rs:36` 等 | 已修订 architecture/02 §2/§4 与 04 §3：前缀是**注册期约定**（对齐 pi），不做强制校验；`builtin_` 不存在（2026-10-28） |
| B3 ✅ | 「权限/信任检查」不是管线环节 | `crates/timo-agent/src/run/tools.rs:55-145`；`timo-tools/src/review.rs:3` 自述不参与权限 | 已修订 architecture/02 §4：审阅器只在写类工具内部触发（`read`/`grep`/`find` 不经过），不另设管线检查点（对齐 pi，2026-10-28） |
| B4 ✅ | 取消未贯穿：MCP 工具执行不接 `ctx.cancel`；扩展工具用 `AtomicBool`；非 Windows 无进程组清理 | 原 `crates/timo-mcp/src/convert.rs:158-174`；`crates/timo-kernel/src/extensions/events.rs:40-65`；`crates/timo-tools/src/tools/shell.rs:278-290` | 已接：MCP 等待 `select!` 受 `ctx.cancel` 约束；扩展工具 per-call 标志交给 QuickJS 中断处理器（宿主层 E301，内核层返回 `Cancelled`）；Unix 子进程 `process_group(0)` + 整组 `kill`。测试：MCP/内核取消均立即返回（`convert.rs`、`extensions/events.rs`、`tests/cancel.rs`）。Unix 分支无 Windows CI 覆盖，随平台打包实测（2026-10-28） |
| B5 ✅ | 扩展钩子错误隔离不达设计：单个 handler 出错即中断同事件后续 handler | 原 `crates/timo-ext/src/host/runner.rs:228-241`；设计要求 `docs/design/phase-2/04-hook-semantics.md` §8 | 已改为记诊断后 `continue`；新增跨扩展测试：前一个扩展派发失败（E302）时后一个仍生效；原两个预算超时测试改为断言 E301 诊断（派发不再整体报错）（2026-10-28） |
| B6 ✅ | 进度回调空管道：`ToolCallUpdate` 无任何生产者 | 全仓无向 `ToolCallCtx.updates` 发送的实现 | codemode 已在 P2 批 C 产出嵌套调用进度（`→ tools.x` / `← x ok`），接口保留（2026-10-28 核实） |
| B7 ✅ | `settings.session_dir` 字段无消费者 | `crates/timo-config/src/settings.rs`；`crates/timo-kernel/src/kernel.rs` 只用 `options.session_dir` | 已删字段；目录覆盖只走 `TIMO_AGENT_DIR` / CLI `--session-dir`，docs 与 architecture/05 同步（2026-10-28） |
| B8 ✅ | ULID 生成器为「每写入器一个」，非文档承诺的「进程内单调」 | `crates/timo-session/src/writer.rs`；`kernel/sessions.rs` 各自 `new` | 新增 `IdGen::shared()` 进程级单例，内核两处写入器改用它（2026-10-28） |
| B9 ✅ | 事件总线错误事件覆盖窄：供应商/认证/流错误不发 `Error` | 仅 `run/mod.rs:296`、`kernel.rs:270`、`kernel/prompt_api.rs:53` 三处 | 已修订 architecture/02 §5：error 事件仅循环级，供应商/认证/流错误走 `stop_reason=Error` 终态消息（与 §3 一致，2026-10-28） |

### C 组 —— 文档回填

| # | 文档 | 偏差 | 处置 |
|---|---|---|---|
| C1 | architecture/01 §1 图 | `tools→ext`、`agent→mcp` 两条边不存在；`ext` 层级错位；缺 `config` 相关边 | ✅ 2026-10-27 已修订 |
| C2 | architecture/02 §4 | 管线次序（Schema 校验在 veto 之后）与缺失环节 | ✅ 2026-10-27 已修订 |
| C3 | architecture/02 §3 | `compaction_event` 已成 `Compacted` 且已实现；veto 走钩子不走事件 | ✅ 2026-10-27 已修订 |
| C4 | architecture/04 §3 | `mcp_<server>_<name>` → 实际 `mcp__<server>__<tool>`；`builtin_` 不存在 | ✅ 2026-10-27 已修订 |
| C5 | architecture/04 §1.1 | `registerProvider`/`registerMcpServer`/`registerVirtualModel`/`events` 列为真实实现，实为 no-op + 诊断 | ✅ 2026-10-27 已修订 |
| C6 | architecture/05 §3/§4 | 类型名 `model.change`/`model.thinking_level_change`；字段 `created_ms` | ✅ 2026-10-27 已修订 |
| C7 | architecture/05 §2 | 布局已按 cwd 分区（phase-3.5/07 D45/D46） | ✅ 2026-10-27 已修订 |
| C8 | architecture/05 §4 | `usage` 实为 cache_warm 记账，非「轮次记账快照」 | ✅ 2026-10-27 已修订 |
| C9 ✅ | design/phase-1/00 §2 | `timo-session`/`timo-tools` 记为「无内部依赖」，实为 `{ai,config}` / `{ai}` | 已加修订注记，原表保留（2026-10-28） |
| C10 ✅ | standards/04 附录 A | `time` 依赖已引入但未登记 | 已补 `time` 准入行，并同步补登同缺口 `globset`、`unicode-normalization`（2026-10-28） |

### D 组 —— 复杂度与过度工程（2026-10-27 全仓审计，10 条发现去重后 9 项）

> 处置原则：**纯死码直接删**；**pi 对齐项先改设计文档/ADR 再接线或删**（铁律 6 / ADR-0011）；**换实现项须先证明行为等价**（安全边界不得削弱）。`ToolCallCtx.updates` 空管道已在 B6、`settings.session_dir` 已在 B7，此处不重复。

| # | 问题 | 证据 | 验收标准 |
|---|---|---|---|
| D1 ✅ | 两个未接线的执行模式：`ToolExecutionMode::Sequential`、`QueueMode::All`，以及无消费者的 `NestedToolHost::is_sequential()` | `timo-agent/src/run/{mod,tools,nested}.rs`、`queue.rs`；`timo-tools/src/nested.rs` | 已删枚举、字段、分支与 trait 方法；设计 02 标注批量级 Sequential / steeringMode 留 backlog（2026-10-28） |
| D2 ✅ | 三处未消费配置：`images.auto_resize`、`shell_path`、`shell_command_prefix` | `timo-config/src/settings.rs`；`timo-tools/src/tools/read.rs` | 已删字段与恒真分支；设计 03/05 回填修订说明，旧配置键按未知字段忽略（2026-10-28） |
| D3 ✅ | `timo-ai` 零调用 API：`AuthMethod` 及 `resolve`、`catalog_load_error`、`AssistantMessage::tool_calls`（`Catalog::all` 已被 codemode/扩展目录消费，保留） | `timo-ai/src/auth.rs`、`catalog.rs`、`types.rs` | 已删除；只留 `AuthResolver`；OAuth 占位随枚举删除，Phase 5 需要时按新 ADR 加回（2026-10-28） |
| D4 ✅ | 宿主内部零消费面：`BasicServices::with_model`、`ExtensionHost::diagnostics()`、`ExtensionRuntime.loaded`、`DiagnosticEntry::with_detail` | `timo-ext/src/host/services.rs`、`host.rs`、`diag.rs`；`timo-kernel/src/extensions/mod.rs` | 已删除；诊断统一走 `diagnostics_for`；`HostServices` 契约与 `pi.*` no-op 诊断不动（2026-10-28） |
| D5 ✅ | `timo-tools` 小项：`read.rs` 复制 `str_param`、`sha2` 依赖零引用（`ToolOutput::error_text` 已被 nested 使用、`ToolRegistry::all()` 已被 codemode 使用，保留） | `tools/read.rs` vs `tools/write.rs`；`Cargo.toml:21` | 已删复制函数与依赖；`read` 改用 `crate::tools::write::str_param`（2026-10-28） |
| D6 ✅ | `Kernel` 五个零生产消费者的查询 API：`usage_totals`、`usage_breakdown`、`mcp_servers`、`retry_settings`、`extension_report_json` | `timo-kernel/src/kernel/{usage_api,ext_api}.rs`、`kernel.rs` | 已删除冗余查询，只留 `usage_summary` 与 `extension_report`；设计 03 已同步（2026-10-28） |
| D7 ✅ | `Reader::Indirect` 死枚举变体：生产只返回 `Read` / `Bash`，仅测试构造 | `timo-kernel/src/prompt/skills/mod.rs` | 已删变体、match 臂与测试断言，不留无主 `#[allow(dead_code)]`（2026-10-28） |
| D8 ✅ | `timo sessions` 手写 29 行 Hinnant 公历换算，而 `time` 0.3 已在 `timo-session` 做同样换算 | 原 `apps/timo-cli/src/sessions_cmd.rs:46-75` | 已删 `format_ms` + `civil_from_days`；格式化下沉 `timo-session::format_utc_minutes` 并经内核门面重导出，CLI 零日期依赖（2026-10-28） |
| D9 ✅ | `fs_scope` 手写 36 行 glob 引擎（`**`/`*`/`?`），工作区已有 `ignore`/globset 可用而本 crate 未依赖 | 原 `timo-ext/src/fs_scope.rs:125-160` | 已换 `globset`（`literal_separator` + `backslash_escape`）；对照测试锁定 `**` 吞 0 段、单段 `*`、`?` 与尾 `/**` 命中目录本身；`[]`/`{}` 转义为字面量防授权面扩大；权限/E403 测试全保留（2026-10-28） |

## 4. 排序与前置

1. **A1 最先**：唯一会丢数据的项；单独提交 + 单独测试，不与其它改动混。
2. **B1 次之**：`apps/timo-desktop/src/settings.rs:146` 用的是 `timo_config::agent_dir()`，门面已有等价的 `timo_kernel::data_dir()`，可零成本先消一处；余下等门面 API 补齐。
3. **D 组与 B6/B7/B8 同批清**（都是「接口有、语义无」的小口子）：
   - 先做纯死码 **D3/D4/D5/D6/D7**（零消费者、最低风险，可一次提交，靠 `cargo test --workspace` 兜底）；
   - 再做 **D1/D2**——先在设计文档标注「本阶段不接线」，再删或接线（pi 对齐义务，不能默默删）；
   - **D8/D9** 单独提交，D9 必须先补语义等价对照（权限边界）。
4. **B2/B3/B4/B5** 涉及公共契约，各自独立提交，可能引出 ADR。
5. **C9/C10** 随收债文档批次一起做。

**前置**：Phase 3.5 收口（D6 四批已落地并真机验收，见 `.agents/CONTEXT.md`；余 P2 定稿与实现），避免与在跑的会话存储/压缩改动撞车。C 组已于 2026-10-27 提前回填（纯文档，无代码冲突）。

## 5. 验收

- A/B/D 组全部清零；`cargo test --workspace` 全绿；新增测试至少覆盖 A1。
- D 组清零判据：删除后无新增 `#[allow(dead_code)]` 豁免；`scripts/check-size` 通过；被删除的公共 API 在 docs 中的引用已先行改掉（先文档后代码）。
- C 组全部回填；`docs/README.md` 清单状态同步。
- **完成记录（2026-10-28）**：fmt / clippy `-D warnings` / workspace **419 tests** / check-size 全绿；新增测试覆盖 A1、B4（MCP/扩展/内核三层取消）、B5（跨扩展错误隔离）；`clean-build` 后 `target/` 14.0GiB（incremental 1.1 / deps 11.7）。
- 《阶段进度报告》按 standards/03 附录模板填写，含接缝核对与体量核对（体量核对里记 D 组净减行数）。

## 6. 风险

- B3/B4 触及公共契约，可能引出 ADR；若选择「修订文档而非改代码」，须在 ADR 或本清单登记豁免理由（standards/08 §6 豁免流程）。
- **D 组的最大风险是「把 pi 对齐面当垃圾删」**：D1/D2/D3 涉及 pi 的 `toolExecution`/`steeringMode`/settings 字段与 OAuth 占位。删前必须先在设计文档写明「本阶段不做、Phase N 再接线」，否则等于悄悄丢掉行为承诺（铁律 6 / ADR-0011）。
- **D9 触碰权限边界**：换 glob 实现后 E403 行为可能改变，现有 `fs_scope.rs:185-192` 测试必须逐条通过；不得以「简化」为名放宽 `**` 段匹配。安全边界不适用 ponytail 精简（standards/06）。
- 本条目**不新增阶段编号**，作为 Phase 3.5 与 Phase 4 之间的**清偿批次**执行；范围不得膨胀为新功能（standards/07 范围纪律）。
