# 设计 00：Phase 1 总览与跨 crate 决策

- 状态：定稿
- 日期：2026-10-28（2026-10-26 定稿；2026-10-28 补依赖列修订注记）
- 关联：roadmap/phase-1；architecture/01~05；ADR-0001/0005/0007/0009/0011；本目录 01~07

## 1. 定位

本目录是 Phase 1 的**实现级详设**：数据类型定稿、trait 签名、行为契约、错误类型表。行为语义的**唯一蓝本是 pi**（对照入口 research/01 §5；本目录各篇逐项标注 pi 参照路径，形如 `pi: packages/agent/src/agent-loop.ts`）。方向约束见 ADR-0011；写码纪律见 standards/04 §8（ponytail）。

## 2. Phase 1 crate 布局与实现顺序

| 序 | crate | 详设 | 依赖（内部） |
|---|---|---|---|
| 1 | timo-config | 05 | 无 |
| 2 | timo-ai | 01 | config（模型目录/凭证引用） |
| 3 | timo-session | 04 | 无 |
| 4 | timo-tools | 03 | 无（schema 校验用 jsonschema） |
| 5 | timo-mcp | 06 | tools（并入统一注册表） |
| 6 | timo-agent | 02 | ai + tools + session |
| 7 | timo-kernel | 06 | 以上全部（门面） |
| 8 | apps/timo-cli | 06 | 仅 timo-kernel |

- **timo-ext 不在 Phase 1 建立**（Phase 2 首个提交建立）——空壳 crate 违反防膨胀纪律（standards/08 §4 死代码零容忍）。standards/01 §1 目录树是终态布局，非 day-1 清单。
- crate lib 名一律 `timo_xxx`（连字符转下划线，Rust 惯例）。

> **2026-10-28 修订（依赖列按实现校正，原表保留）**：`timo-session` 实际依赖 `timo-ai` + `timo-config`（entry 复用消息表示、读写设置与数据目录）；`timo-tools` 实际依赖 `timo-ai`（工具输出复用 `Content`）。上文「无」是 Phase 1 开工时的计划值。

## 3. pi ↔ timo 对应总表（crate 级）

| timo | pi 参照（packages/…） | 详设篇 |
|---|---|---|
| timo-ai | ai/src（types.ts、api/anthropic-messages.ts、api/openai-completions.ts、utils/retry.ts） | 01 |
| timo-agent | agent/src（agent.ts、agent-loop.ts、types.ts） | 02 |
| timo-tools | coding-agent/src/core/tools/ | 03 |
| timo-session | coding-agent/src/core/session-manager.ts（语义参照；格式为自有，ADR-0007） | 04 |
| timo-mcp | coding-agent/src/extensions/mcp/（协议栈替换为 rmcp，ADR-0005） | 06 |
| timo-config | coding-agent/src/core/settings-*.ts、model-resolver.ts、config.ts | 05 |
| timo-kernel | coding-agent/src/core/agent-session.ts + sdk.ts | 06 |
| timo-cli | coding-agent/src/cli.ts（非交互冒烟壳，仅验收载体） | 06 |

## 4. 跨 crate 统一决策

1. **错误处理**：各库 crate 用 thiserror 定义 `XxxError`；错误链保留来源（`#[from]`）；一切运行期错误经事件总线 `error` 事件外泄，库层不 panic、不 unwrap（standards/04 §2，clippy `unwrap_used` = deny）。
2. **取消**：`tokio_util::sync::CancellationToken` 从入口贯穿 HTTP 流、子进程、工具执行；`select!` 一律带取消臂（standards/04 §3）。语义对齐 pi 的 AbortSignal 检查点（详设 02 §6）。
3. **时间与 id 注入**：`Clock`（now_ms）与 `IdGen`（ulid）作为可注入依赖传入，保证会话/日志可复现（standards/05 §3）。
4. **JSON Schema 通货**（接缝 #5）：工具参数 schema 以 `serde_json::Value` 手写（对应 pi 的 TypeBox 对象字面量）；校验统一走 `jsonschema` crate（draft 2020-12）；全仓库不引入第二套 schema 体系。
5. **文件预算落地**：单源文件 ≤500 行（standards/08 §4）。协议适配器按 `request / convert / stream` 三模块拆分；工具按一具一文件。
6. **事件总线**（接缝 #2）：`tokio::sync::broadcast`；payload 为不可变快照（Arc）；详见 02 §5。

## 5. 与 pi 的偏离登记（ADR-0011 §3 白名单逐项）

| # | 偏离 | 类别 | 说明 |
|---|---|---|---|
| D1 | grep/find 用 `ignore`+`regex` 原生实现，不下载 rg/fd 二进制 | a 语言/运行时 | pi 依赖外部二进制自动下载（TS 生态做法）；`ignore` 即 ripgrep 的目录遍历库、`regex` 即其正则引擎，**方言与输出格式不变**，且免除二进制供应链面 |
| D2 | HTTP+SSE 用 reqwest + 自研 SSE 解码器（~100 行） | a 语言/运行时 | pi 走官方 TS SDK；Rust 侧无对等官方 SDK 选型（ADR-0003/0005 既定），自研解码器小而可测，fixture 全覆盖 |
| D3 | 消息流的 `partial` 快照按值克隆传递，不共享可变对象 | a 语言/运行时 | pi 靠 JS 对象同一性让 partial 即终态；Rust 以"每次事件携带最新快照 + 消费方以末条为准"达到同一可观察行为 |
| D4 | 无 `ls` 内置工具 | 范围 | roadmap Phase 1 工具清单未列；pi 有但默认集（read/bash/edit/write）也不含。登记 backlog，Phase 2+ 补齐 pi 全集 |
| D5 | 会话格式自有（非 pi session v3） | b（ADR-0007） | 语义对齐：树形/追加/崩溃安全/可恢复；载体不同，见 04 |
| D6 | MCP 协议栈用 rmcp | b（ADR-0005） | 工具命名/结果转换/超时语义仍对齐 pi |
| D7 | 配置字段 snake_case、数据目录走平台惯例 | a 语言/运行时 | pi 为 camelCase + `~/.pi`；结构字段一一对应，见 05 |
| D8 | read 工具图像缩放用 `image` crate（pi 用 sharp） | a 语言/运行时 | 缩放策略与默认值对齐 |
| D9 | MCP 工具 Phase 1 默认 exposure=direct | 必要差异 | pi 默认 codemode（依赖 Phase 2 的 codemode/tool-search）；Phase 1 无该通路，默认 direct 才可被模型调用（A4 场景要求）。per-tool override 照常生效 |
| D10 | 不实现：OAuth（枚举占位）、compaction、会话树用户功能、扩展钩子执行、deferred 工具 | 范围 | roadmap §2 不做清单；结构占位处见各篇 |

其余一切"想换更好的做法"→ roadmap/00 §3 backlog 记录不执行。

## 6. 提交序列（本阶段）

1. 本批：详设定稿 + architecture/05 定稿 + 开工登记（tag `phase-1.start`）。
2. workspace 骨架 + 构建卫生（.cargo/config.toml、scripts、CI）。
3. 起依赖序实现：config → ai → session → tools → mcp → agent → kernel → cli；每 crate 一个提交，含测试。
4. 中期与收尾：进度报告（含接缝核对、体量核对、规范复查 standards/09）→ 看板 → CHANGELOG → tag `phase-1.end`。
