# Phase 3.5 P0 阶段进度报告（草稿）

- 日期：2026-10-27
- 本阶段目标（一句话）：把 pi 的"灵魂件"——分节系统提示、项目上下文、技能、统一命令面、用量统计——装进内核，并让 CLI 与桌面共用同一份能力。
- 状态：**P0 完成，阶段继续**（P1 compaction/缓存/无头、P2 模板/trust 尚未开工）

## 完成了什么

- **提示词工程**：系统提示从一段字符串改为分节装配（身份/工具/规则/文档/项目上下文/技能/工作目录 + 自定义节）；`AGENTS.override.md` → `AGENTS.md` → `CLAUDE.md` 按"全局在前、祖先从根到当前目录"注入，支持 `SYSTEM.md` 整段替换与 `APPEND_SYSTEM.md` 追加；`/reload` 重载。
- **技能**：扫描五个根（用户、`.timo`、`.pi`、向上到仓库根的 `.agents`、设置显式路径），含 `SKILL.md` 的目录即根不再下钻，复用 `ignore` 规则跳过隐藏项与 `node_modules`；提示里只放名字/描述/路径，`/skill:name` 把正文展开进用户消息。
- **命令面**：内建命令三态（Ready/Planned/Unsupported）+ 技能命令 + 扩展命令合并成一份清单；CLI 分发顺序为扩展 → 内建 → `/skill:` → 普通提示。
- **用量**：从会话消息现算四项 token 与费用，assistant 按 `provider/response_model` 分桶，工具结果进 `Tools/summaries`；`/session` 与桌面状态栏可查。
- **桌面接线**：命令面板新增"技能"组（只列 Ready），点选把 `/skill:<name> ` 插入输入框；状态栏显示 `tokens · 费用 · 版本`；会话切换后刷新用量。

## 验收场景执行结果

| # | 结果 | 说明 |
|---|---|---|
| D1 项目上下文 | **通过（CLI + 桌面）** | 临时项目放 `AGENTS.md`（规则：先输出"暗号：青柚"）。CLI 与桌面都只回了该行，未复述其他内容。 |
| D2 skills | **通过（CLI + 桌面）** | 临时项目放 `.timo/skills/hello/SKILL.md`（要求三句以"柚"开头）。CLI `/skill:hello` 与桌面面板点选后都触发了技能，并遵循其内容。 |
| D4 用量与缓存 | **通过（P0 范围）** | 每轮请求的 token 与 `cache_read` 可读；会话累计与分桶可查。桌面状态栏显示累计 tokens 与费用。缓存统计 UI 属 P1。 |
| D7 CLI 回归 | **通过** | workspace 263 tests 全绿；`phase-3.end..HEAD` 对既有测试文件零改动，唯一变更是新增 `crates/timo-kernel/src/prompt/skills/tests.rs`。四门禁全绿。 |

### D1 真机读数（DeepSeek，临时数据目录）

- CLI：`暗号：青柚`，随后 `[usage] input=1509 output=5 cache_read=0 cache_write=0`。
- 桌面：对话区只有 `暗号：青柚`；右上 `1514 tokens`；状态栏 `1514 tok · $0.0000 · v0.1.0`。
- 第二轮提问同样先输出暗号，说明规则每轮生效，不是首轮幻觉。

### D2 真机读数

- 命令面板"技能"组列出 `/skill:hello`（以及用户级 `.agents/skills` 下的两个技能，来自向上发现规则）。
- CLI：秒开技能后模型先 `read` 了 `<项目>/.timo/skills/hello/SKILL.md`，再输出 `暗号：青柚` + 三句以"柚"开头的短句。
- 桌面：点选 `/skill:hello` 后输入框出现该文本（光标处插入，不覆盖草稿），回车后同样输出暗号与三句"柚"字短句，工具过程显示 1 步。
- 技能列表与 `/skill:` 展开走同一份内核发现结果，桌面不再需要自己解析。

### D4 真机读数

- CLI 第二次请求：`[usage] input=232 output=28 cache_read=1536 cache_write=0`（缓存命中可读）。
- CLI `/session`：`6 messages · input=1963 output=101 cache_read=2944 · $0.0000`；分桶 `deepseek/deepseek-flash tokens=5008`。
- 桌面：技能轮后右上 `1802 tokens`，状态栏累计 `5014 tok · $0.0000`。
- 费用为 0 的原因：验收夹具的 `models.json` 未配费率卡（`cost` 全缺省 0），不是聚合错误；分桶与加法由 6 条单测覆盖。分桶键是供应商返回的 `response_model`（请求 `deepseek-chat`、上游回 `deepseek-flash`），与 K8 设计一致。

## 本轮发现并修复的问题

- **桌面的 `/skill:` 曾是字面量**：CLI 在分发层展开，桌面把输入框原文直接交给内核，技能不会触发。已把展开收进 `Kernel::run`（commit `85527d4`），CLI 与桌面共用一条路径；CLI 仍保留"未知技能命令"的报错。
- **测试文件越线**：`app_tests.rs` 因新增用例达到 512 行。按既有"窗口级测试按功能拆分"先例，面板相关 3 条用例移到 `features/palette_tests.rs`，原文件回到 500 行内。
- **构建缓存触硬限**：验收前 `target/` 25.4GiB，`check-size` FAIL；跑 `scripts/clean-build` 清到 14.0GiB（无 sccache，未全清）。

## 体量核对（standards/08）

- docs/：当前 69 篇（软上限 60，超 9）。本阶段新增 `design/phase-3.5/00~03` 四篇（`research/06` 由草稿定稿、`research/07` 为并行视觉工作的修订，均未新增篇数）。阶段设计稿属硬流程交付物，登记豁免；整合评审仍待办（删除面需所有者确认）。
- .agents/：文件 51（不含 vendored 技能为 17，上限 40）；`CONTEXT.md` 50 行（上限 150）。
- 代码：最大文件 500 行（`timo-kernel/src/kernel.rs`），无超 500 行文件；最大 crate `timo-ext` 4,843 行（上限 15,000 触发评审）。
- 依赖：无新增第三方依赖；`timo-kernel` 新增 `ignore` 的 crate edge，该依赖已在 workspace 中。
- 构建产物：`clean-build` 25.4 → 14.0GiB（释放 11.4GiB）；验收重建后 15.0GiB（incremental 2.6 / deps 11.2），处于软上限。
- 豁免登记：docs 总数软上限 +9。

## 已知问题与风险

- **项目技能无信任门控**（D33）：技能是文本指令，与上下文文件同级；本阶段靠审阅安全垫兜底，P2 随信任管理收紧。
- **工作树嵌套去重留 P2**（D36）：主仓与嵌套 worktree 各有一份 `AGENTS.md` 时可能重复注入。
- **每轮 usage 行只显示最后一次请求**（Phase 1 既有行为）：一轮里发生工具调用时，`[usage]` 是最后一条 assistant 消息的读数；整轮/会话合计以 `/session` 为准。本阶段未改这一语义。
- **费用依赖费率配置**：`models.json` 未写 `cost` 时费用恒为 0；不重新计价是设计选择，需在配置里给费率。
- **缓存命中只在 CLI 文本中体现**，桌面 P0 不展示 cache 字段；P1 缓存统计接入。
- `target/` 无 sccache 时不做全清；当前 15.0GiB 贴着软限，后续批次会再涨，阶段收尾需再跑 `clean-build`。

## 下一步

1. P1 详设与实现：compaction、缓存统计、会话 warmer、会话导出、headless/print 模式（D3/D4 缓存半场/D5）。
2. P2：prompt 模板、provider composer、codemode、信任门控（D6 标尺）。
3. 桌面视觉精修（并行线）：research/07 → 设计稿 → 批次实现；与内核零冲突。
4. 阶段收尾：D3/D5/D6 验收 → 本报告补全 → tag `phase-3.5.end`。

## 文档更新清单

- 新增 `docs/design/phase-3.5/report.md`（本页，P0 草稿）。
