# Phase 3.5 阶段进度报告（P0 / P1 完成）

- 日期：2026-10-27
- 本阶段目标（一句话）：把 pi 的"灵魂件"——分节系统提示、项目上下文、技能、统一命令面、用量统计、compaction、缓存、无头模式——装进内核，并让 CLI 与桌面共用同一份能力。
- 状态：**P0、P1 完成；D6 标尺与 P2 未开工**

## 完成了什么

- **提示词工程**：系统提示从一段字符串改为分节装配（身份/工具/规则/文档/项目上下文/技能/工作目录 + 自定义节）；`AGENTS.override.md` → `AGENTS.md` → `CLAUDE.md` 按"全局在前、祖先从根到当前目录"注入，支持 `SYSTEM.md` 整段替换与 `APPEND_SYSTEM.md` 追加；`/reload` 重载。
- **技能**：扫描五个根（用户、`.timo`、`.pi`、向上到仓库根的 `.agents`、设置显式路径），含 `SKILL.md` 的目录即根不再下钻，复用 `ignore` 规则跳过隐藏项与 `node_modules`；提示里只放名字/描述/路径，`/skill:name` 把正文展开进用户消息。
- **命令面**：内建命令三态（Ready/Planned/Unsupported）+ 技能命令 + 扩展命令合并成一份清单；CLI 分发顺序为扩展 → 内建 → `/skill:` → 普通提示。
- **用量**：从会话消息现算四项 token 与费用，assistant 按 `provider/response_model` 分桶，工具结果进 `Tools/summaries`；`/session` 与桌面状态栏可查。
- **P1 会话深度**：会话按 cwd 分区（旧平铺只读兼容）、`/export` 原子导出、compaction 检查点与恢复投影、无头 `-p`/`--mode json`、缓存 miss 统计与预热（详见 `design/phase-3.5/04~07`）。
- **桌面接线**：命令面板新增"技能"组（只列 Ready），点选把 `/skill:<name> ` 插入输入框；状态栏显示 `tokens · 费用 · 版本`；会话切换后刷新用量。

## 验收场景执行结果

| # | 结果 | 说明 |
|---|---|---|
| D1 项目上下文 | **通过（CLI + 桌面）** | 临时项目放 `AGENTS.md`（规则：先输出"暗号：青柚"）。CLI 与桌面都只回了该行，未复述其他内容。 |
| D2 skills | **通过（CLI + 桌面）** | 临时项目放 `.timo/skills/hello/SKILL.md`（要求三句以"柚"开头）。CLI `/skill:hello` 与桌面面板点选后都触发了技能，并遵循其内容。 |
| D3 compaction | **通过（自动 + 手动 + 重启）** | 自动：低 reserve 触发后会话文件出现一条 `session.compaction`（结构化摘要含「青柚」）；重启 `resume` 后仍答出代号。手动：`/compact` 读数 `1836 → 279`、保留 2 条，重启后同样答出代号。 |
| D4 用量与缓存 | **通过（P0 用量 + P1 缓存半场）** | 每轮 token 与 `cache_read` 可读；会话累计与分桶可查；`/session` 新增 `cache:` 行与 `warming:` 状态。 |
| D5 无头模式 | **通过（text + JSONL + 退出码）** | `-p` 只输出终答；`--mode json` 首行会话头、首/末事件 `agent_start`/`agent_end`，工具调用事件齐全；用法错误退出 2。Ctrl+C 130 未手工触发。 |
| D7 CLI 回归 | **通过** | workspace 294 tests 全绿；`phase-3.end..HEAD` 对既有测试文件零改动，唯一变更是新增 `crates/timo-kernel/src/prompt/skills/tests.rs`。四门禁全绿。 |

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

### D3 真机读数（DeepSeek，临时数据目录）

- 自动压缩：`reserve_tokens` 调到必触发后跑两轮，会话文件出现一条 `session.compaction`，摘要六节齐全、含项目代号「青柚」，`keep_recent_messages: 1`。
- 重启 `resume last --once "项目代号是什么？"` → `青柚`，说明投影从摘要恢复。
- 手动：两轮约 1800 token 的会话执行 `/compact 只保留项目代号` → `[compact] tokens: 1836 → 279 (kept 2 messages)`；随后 `/session` 为 3 条消息（摘要 + 保留尾部）。
- 修复：首版 `tokens_after` 沿用压缩前 assistant 的 usage 读数，出现 `1873 → 1873`；改为按新上下文全量估算后恢复 `1836 → 279`，并有单测钉住。

### D5 真机读数（DeepSeek）

- text：`timo -p "只回复两个字：青柚"` → stdout 只有 `青柚`，无 `[tool]`/`[usage]`/流式增量，退出 0。
- JSON：`--mode json -p "读取 README.md 第一行并原样回答"` 产出 50 行 JSONL——首行 `session_header`（id/cwd/model），第二行 `agent_start`，末行 `agent_end`；逐行 `serde_json` 解析通过；`message_update` 无 `partial`；`toolcall_start` 带 `id`/`tool_name`；工具 `bash` 读到 `TIMO_JSON_SMOKE`。
- 多条 `-p`：同一会话顺序执行，第二条答案引用了第一条给的暗号。
- 用法错误：`-p` 配子命令、无参启动均退出 2；Ctrl+C 的 130 语义已实现但未手工触发（Windows 控制台注入成本高，留人工抽检）。

### P1 缓存读数

- DeepSeek 会话 `/session`：`warming: inactive (…)`（模型目录无 `cache_ttl_seconds`），两轮请求均为缓存命中，未出现 miss 行。
- miss 判定/代价公式与 `cache_warm` 基线条目由 6 条单测覆盖；`usage(cache_warm)` 落盘与"恰好一次刷新"由假适配器集成测试覆盖（1 秒 TTL）。
- Anthropic 目录 17 个模型补 `cache_ttl_seconds: 300`；OpenAI 兼容目录不补。

### D6 批② 真机读数（DeepSeek，临时数据目录）

- `--mode json --no-session --tools read,bash --append-system-prompt @rule.md -p "跑 bash echo TIMO_FLAG_SMOKE，再说暗号"`：首行 `"session_id":null`；bash 工具真跑出 `TIMO_FLAG_SMOKE`；暗号答 `青柚李`（临时目录内会话文件数 0，`--no-session` 确实不落盘）。
- `--no-session --tools read,ghost -p "用 bash 执行…"`：模型只答 `NO_TOOL`（bash 未声明给模型，声明面收窄真生效）；stderr 出现 `[warn] D:\timo-pi: unknown tool \`ghost\``。
- 三条新闸门逐个变异验红（去掉 `!self.no_session`、把过滤换成空集、把 append 循环换成空文本），对应测试各自转红后逐字节还原并复核 sha。
- 本轮补的出口缺口：无头 `-p` 路径原先不调用资源告警打印，`--tools` 未命中在子代理场景里静默；现由 `print::run` 收尾统一出口（失败提前 return 也会打印）。

### D6 批③ 用量回报读数

- 接缝打通四处：`ToolOutput.usage` → `ToolResultMessage.usage`（`run/tools.rs`）→ `Tools/summaries` 桶（`usage.rs` 既有聚合，未改语义）；扩展侧 `ToolCallResult.usage` 走 serde 原样过 JSON 边界（shim 不裁字段），内建工具与 MCP 一律 `None`（现状不变）。
- 新增 2 条测试：agent 层 `a_tool_can_report_usage_into_its_result_message`（消息与 `ToolCallFinished` 事件两条通路都断到 input/total_tokens/cost.total）；ext 层 `tool_usage_crosses_the_json_boundary`（带 usage 解析成功 + 不带 usage 不得凭空造）。workspace 315 tests。
- 变异验证：把 `usage: output.usage` 换成 `None` → agent 那条测试转红（还原后 sha 一致）。
- **实测覆盖缺口（不谎报）**：把内核适配器 `crates/timo-kernel/src/extensions/events.rs` 的 `usage: result.usage` 换成 `None` 后，`timo-kernel` + `timo-ext` 全部测试仍绿——该环没有公开测试入口（`mod extensions` 私有、内核无单工具执行 API）。批④ 子代理真机验收要用 `/session` 的 `Tools` 桶读数把它端到端补上，否则不许算完成。

## 本轮发现并修复的问题

- **桌面的 `/skill:` 曾是字面量**：CLI 在分发层展开，桌面把输入框原文直接交给内核，技能不会触发。已把展开收进 `Kernel::run`（commit `85527d4`），CLI 与桌面共用一条路径；CLI 仍保留"未知技能命令"的报错。
- **测试文件越线**：`app_tests.rs` 因新增用例达到 512 行。按既有"窗口级测试按功能拆分"先例，面板相关 3 条用例移到 `features/palette_tests.rs`，原文件回到 500 行内。
- **构建缓存触硬限**：验收前 `target/` 25.4GiB，`check-size` FAIL；跑 `scripts/clean-build` 清到 14.0GiB（无 sccache，未全清）。

## 体量核对（standards/08）

- docs/：当前 76 篇（软上限 60，超 16）。P1 新增 `design/phase-3.5/04~07` 四篇；阶段设计稿与详设属硬流程交付物，登记豁免；整合评审仍待办（删除面需所有者确认）。
- .agents/：文件 51（不含 vendored 技能为 17，上限 40）；`CONTEXT.md` 57 行（上限 150）。
- 代码：最大文件 498 行（`timo-desktop/src/app.rs`），无超 500 行文件；最大 crate `timo-desktop` 5,661 行（上限 15,000 触发评审）。
- 依赖：无新增第三方依赖；`timo-cli` 增 `serde_json` 边、`timo-kernel` 增 `async-trait` dev-dep，均已在 workspace 中。
- 构建产物：P0 收尾 `clean-build` 到 14.0GiB；P1 开发后 20.5GiB（incremental 4.4 / deps 14.9），阶段收尾需再跑。
- 豁免登记：docs 总数软上限 +16。

## 已知问题与风险

- **项目技能无信任门控**（D33）：技能是文本指令，与上下文文件同级；本阶段靠审阅安全垫兜底，P2 随信任管理收紧。
- **工作树嵌套去重留 P2**（D36）：主仓与嵌套 worktree 各有一份 `AGENTS.md` 时可能重复注入。
- **每轮 usage 行只显示最后一次请求**（Phase 1 既有行为）：一轮里发生工具调用时，`[usage]` 是最后一条 assistant 消息的读数；整轮/会话合计以 `/session` 为准。本阶段未改这一语义。
- **费用依赖费率配置**：`models.json` 未写 `cost` 时费用恒为 0；不重新计价是设计选择，需在配置里给费率。
- **缓存统计只在 CLI `/session` 呈现**，桌面暂无 cache 行；桌面状态栏的费用已含缓存计价，不新增面板（设计 05 §3.3）。
- `target/` 无 sccache 时不做全清；当前 23.2GiB（incremental 5.2），阶段收尾需跑 `clean-build`。
- **无头 stderr 混入技能告警噪音**：`--no-session -p` 一次运行会打出项目 `.agents/skills/` 里 8 条 frontmatter 告警（README 与 ponytail 等非技能文件被扫）。资源告警本身是要出口的，但子代理场景里父进程会把 stderr 原文贴进结果，噪音挤掉有效信息。处置方向（未定）：`-p`/`--mode json` 下只出口与本次请求相关的告警，或把非技能 md 排除出扫描面——随 P2 信任与资源治理一起定。

## 下一步

1. D6 标尺：以 pi 官方 subagent 示例为行为蓝本，自研等价扩展（经无头模式跑子进程、.md 代理定义、用量回报）。
2. P2：prompt 模板、provider composer、codemode、信任门控。
3. 桌面视觉精修（并行线）：research/07 → 设计稿 → 批次实现；与内核零冲突。
4. 阶段收尾：D6 验收 → 本报告补全 → `clean-build` → tag `phase-3.5.end`。

## 文档更新清单

- `docs/design/phase-3.5/report.md`（本页）：P0 + P1 验收与读数。
