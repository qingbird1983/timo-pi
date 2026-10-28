# Phase 3.5 阶段进度报告（P0 / P1 / D6 / P2 完成）

- 日期：2026-10-27
- 本阶段目标（一句话）：把 pi 的"灵魂件"——分节系统提示、项目上下文、技能、统一命令面、用量统计、compaction、缓存、无头模式——装进内核，并让 CLI 与桌面共用同一份能力。
- 状态：**P0、P1、D6、P2 完成；第四题（项目信任门控）已整体撤回（ADR-0015）**

## 完成了什么

- **提示词工程**：系统提示从一段字符串改为分节装配（身份/工具/规则/文档/项目上下文/技能/工作目录 + 自定义节）；`AGENTS.override.md` → `AGENTS.md` → `CLAUDE.md` 按"全局在前、祖先从根到当前目录"注入，支持 `SYSTEM.md` 整段替换与 `APPEND_SYSTEM.md` 追加；`/reload` 重载。
- **技能**：扫描五个根（用户、`.timo`、`.pi`、向上到仓库根的 `.agents`、设置显式路径），含 `SKILL.md` 的目录即根不再下钻，复用 `ignore` 规则跳过隐藏项与 `node_modules`；提示里只放名字/描述/路径，`/skill:name` 把正文展开进用户消息。
- **命令面**：内建命令三态（Ready/Planned/Unsupported）+ 技能命令 + 扩展命令合并成一份清单；CLI 分发顺序为扩展 → 内建 → `/skill:` → 普通提示。
- **用量**：从会话消息现算四项 token 与费用，assistant 按 `provider/response_model` 分桶，工具结果进 `Tools/summaries`；`/session` 与桌面状态栏可查。
- **P1 会话深度**：会话按 cwd 分区（旧平铺只读兼容）、`/export` 原子导出、compaction 检查点与恢复投影、无头 `-p`/`--mode json`、缓存 miss 统计与预热（详见 `design/phase-3.5/04~07`）。
- **桌面接线**：命令面板新增"技能"组（只列 Ready），点选把 `/skill:<name> ` 插入输入框；状态栏显示 `tokens · 费用 · 版本`；会话切换后刷新用量。
- **项目信任门控撤回**：曾按详设 12 实现批 A/B/C（内核决策链、扩展 `project_trust` 事件、CLI `--approve`、桌面五选项弹框）；因产品定位并非 VS Code 式工作区模型，整体撤回（ADR-0015），项目资源恢复无条件加载，启动不再询问。

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

### D6 批④ 真机读数（DeepSeek，临时数据目录，7 次主会话 / 9 次子进程）

- 装机链路真跑通：`ext add` 用 rolldown 把 TS 打成 `bundle.cjs` → `ext trust subagent` 授权 → 主会话模型调用 `plugin_subagent_subagent`。装机建议能力读数：`{"exec":true,"fs":{"read":["**"]}}`（`pi.readFile`/`pi.listDir` 现在会被探测到，此前只探 `pi.exec`）。
- **补上批③ 登记的适配器缺口**：主会话文件里 `tool_result`（`plugin_subagent_subagent`）带 `usage={input:4499,output:379,cache_read:2048,total_tokens:6926,cost.total:0.00073598}`，`/session` 打出 `Tools/summaries tokens=6926 $0.0007`、总计 `input=5274`——两个读数逐字对上，内核适配器那一行确认在生产里承重。
- single / parallel / chain 三模式真机各跑一轮：parallel 两条任务按提交序回填（宿主 `execParallel`）；chain 第二步 reviewer 原样征引第一步输出（`{previous}` 替换有实证）；失败即停用 `model: nosuchprovider/nosuchmodel` 验出——第一步 `FAILED exit 1: [error] model not found`，第二步未执行。
- `--no-session` 在子代理链路上的跨进程读数：sessions 目录恰好 7 个文件（= 7 次主会话），子进程跑了 9 次一个没落。
- **真机纠了两处偏差**（都已先改文档再改码）：① 子进程参数把 `-p` 当布尔开关用，实际我们的 `-p` 取值，子进程只打出用法错误、主会话拿到 `0 turn(s) 0 tokens`；② `--tools` 早期实现只做减法，示例 scout 声明的 `grep/find` 拿不到，子代理回答"没有 grep/find 工具可用"——按 pi 的正向 enable 语义改为"列出的即最终允许集合"（复跑后 scout 的 steps 真出现 `grep → find`）。
- 顺带把 `pi.exec` 与 `pi.execParallel` 合并到同一条执行路径：原先单条 exec 用 `Command::output()`，既不认 `timeoutMs` 也 kill 不掉，取消/超时验收过不去。
- 未做（如实）：交互 Ctrl+C 触发的真机 abort（无头管道收不到信号）——用超时通道与进程内 abort 单测等价覆盖，读数见设计 08 §6.4。

### P2 批 A 真机读数（prompt 模板，2026-10-27）

- **模板机制落地**：`.md` 即 `/命令`，`$1`/`$@`/`${2:-默认}`/`${@:2:3}` 全套占位与 bash 分词；目录约定与技能共用一层（`prompt/resources.rs`）。gate：fmt / clippy `-D warnings` / workspace **338 tests**（较上轮 +17）/ `check-size` OK。
- **变异 9/9 承重**（每条都看到 `Compiling timo-kernel`）：摘掉模板展开、切片右端不夹长度、同名遮蔽失效、未闭合 frontmatter 不跳过、默认目录开关恒关、关默认时连显式路径一起丢、共享层不认 `.pi`、清单来源标注被抹平、settings 默认值翻成 false——分别打到对应的那条测试。另补 `dir_scope` 的 2 条窄变异。
- **一处无测试入口，实测证明**：CLI 兜底放行（`if kernel.has_template(raw)`）改成 `if false` 后 `timo-cli` 13 条测试仍全绿——该行只能靠真机覆盖（见下 T7b 的放行/报错对照）。
- **真机六场景**（DeepSeek，临时数据目录，沙盒工作区）：
  - 无头 `-p "/implement 在 notes.md 末尾加一行 '- item two'"`：模板在 `run` 里展开，chain 落地，沙盒 `notes.md` 真出现 `- item two`。
  - 交互 REPL（`timo resume last` 喂 stdin）`/scout-and-plan`：**被放行**，chain 跑 14 946 ms，`/session` 打出 `Tools/summaries tokens=55310 $0.0027`。
  - 负对照 `--no-prompt-templates`：同一行 → `[error] unknown command \`/scout-and-plan\``。
  - 显式路径（关默认 + `--prompt-template examples/prompts`）：同一行可用，chain 跑 10 731 ms。
  - 会话文件 2 个（= 2 次主会话，REPL 复用不新增），遗留 timo 进程 0。
  - 坏模板（未闭合 frontmatter）不产生命令：`/broken` → `unknown command`。
- **真机抓到一个真实缺口并修掉（G8）**：`fs.read` 的 `**` 按 cwd 解析，而用户级 `<data_dir>/agents` 几乎总在 cwd 之外 ⇒ 首次真机 `E403 … outside the declared fs.read scopes`，subagent 一个代理都发现不了。装机时补 `<data_dir>/**` 绝对作用域（`timo_ext::dir_scope`，与 `key()` 同一套归一化，`is_protected` 仍先拦 `.git/.ssh/.env`）。修复后同一场景通过——没有它，D56 的 workflow 预设在真实目录布局下不可用。
- **如实登记两处噪音/未做**：① 扩展诊断表里每次都会留一行 `E303 cannot access …/.timo/agents`（扩展对"项目级 agents 目录不存在"的探测，功能无影响，与已登记的 stderr 噪音同类）；② REPL 在跑过一次 prompt 之前不打印资源告警（`WARNED` 首次打印的既有行为），所以坏模板的 warning 要等首轮对话才可见；③ 扩展自带资源目录（`resources_discover`）按 §7 留本题第二批；④ 桌面命令面板暂不列模板（D63）。

### P2 第二题·批 A 读数：provider 配置面（2026-10-27）

- **落地**：`models.json` 新增 `auth_header`（供应商级，不写 = 照旧发认证头）、模型级 `headers`、模型级 `compat`（`max_tokens_field` / `supports_usage_in_streaming` / `supports_finish_reason`，未写的项沿用默认）。头合并统一到一个 `merge_headers`（同名大小写不敏感、后者覆盖），顺带修掉"供应商级与模型级同名头会被发两遍"的旧行为。设计见 [10](10-provider-composer.md) §4.1。
- gate：fmt / clippy `-D warnings` / workspace **344 tests**（+5）/ `check-size` OK。变异 5/5 全红（模型级头不参与合并、`auth_header` 缺省翻成不发、未声明 compat 项翻成 false、同名头只追加不覆盖、openai 认证头开关反转），每条都确认 `Compiling` 行。
- **一处无测试入口，实测证明**：把 openai adapter 里 `let headers = base_headers(...)` 换成空表，`timo-ai` 38 条测试仍全绿——`base_headers` 函数本身有单测，**调用点没有**。真机回归覆盖：带供应商级 + 模型级同名自定义头、`compat.max_tokens_field=true`、`auth_header` 缺省，DeepSeek 正常回答（RC=0）。`auth_header=false` 的"不发认证头"只有单测（无自建网关可抓请求），如实登记。
- **未做**：`oauth` / `refreshModels` / `thinkingLevelMap` / `samplingParams`（D68，另一条能力线）；虚拟模型 B2/B3（B1 见下）。

### P2 第二题·批 B1 读数：虚拟模型目录条目（2026-10-27）

- **落地**：`ApiKind::Virtual`（规范名 `pi-virtual`）+ `Model::is_virtual()`（判据与蓝本 `isVirtualModel` 同源，不另设 kind 字段）+ `Model::virtual_entry`。`api_with_transport(Virtual)` 返回拒绝式适配器：流内 `Start` → 终态 `Error`，文案对齐蓝本 `Virtual model <provider>/<id> must be routed before streaming`，**从不触碰传输层**。解析层显式选中虚拟条目即报同名错误（`KernelError::ModelMustBeRouted`），自动兜底只从物理模型里挑（目录序里虚拟条目排在前面也不会被选中）。`models.json` 可声明 `"api": "pi-virtual"` 条目，`--model` 与桌面供应商清单都能列出（桌面清单本就按 models.json 全量列 id）。设计见 [10](10-provider-composer.md) §4.2。
- gate：fmt / clippy `-D warnings` / workspace **348 tests**（+4）/ `check-size` OK（target 16.0GiB 超软限，与上批同型，阶段收尾统一 `clean-build`）。
- **变异 6/6 承重**（每条都确认 `Compiling` 行与 FAILED 命中）：`is_virtual` 恒假、显式选择跳过拒绝、兜底失去虚拟过滤、虚拟条目接到真实 adapter（这条同时让"传输层计数为 0"报红）、流内终态丢 `Error`、`parse` 忘掉 `pi-virtual`。
- **真机**（临时数据目录 + models.json 声明 `router/auto`）：`timo run "hi" --provider router --model auto` 与裸 `--model auto` 都 RC=1，stderr 一行即蓝本文案；数据目录里除 models.json 外一个文件都没多——失败发生在解析期，早于会话创建与任何请求。如实登记：真机侧只能证明"解析期失败"，"不发网络"由单测的计数传输层（断言 0 次）证明。
- **留给 B2 的两个实现约束**（本批只读摸底，未改）：① 循环里已有 `LoopHooks::route_model`（D23，同 provider），但它直接改写 `self.model`——虚拟路由要求"选中模型（可为虚拟）"与"本轮请求模型（物理）"分开，否则会话记录会丢掉虚拟名；② `LoopConfig.api` 在装配期固定，路由到物理模型后必须按请求模型的 api 取 adapter，否则虚拟条目会被送进上面的拒绝式适配器。

### P2 第二题·批 B2 读数：路由契约与扩展注册（2026-10-27）

- **落地**：`LoopHooks::route_model` 改为 `Result<Option<RouteChoice>, String>`；请求携带 `RouteReason`（user/continuation/retry/direct）、虚拟档位、`previous`/`failed`（模型身份）与分支 `state`。循环拆成"选中模型（可为虚拟，run 内不变）/ 本轮请求模型（物理）/ adapter+凭证"；`LoopConfig.dispatch` 由内核的 `CatalogDispatch` 提供（按 `provider/id` 核对目录、按协议缓存适配器、按供应商解析凭证），无 dispatch 时沿用装配期单适配器（Phase 1/2 语义不变）。reason 判定与蓝本同源；压缩阈值改用本轮物理模型的窗口，压缩摘要按 `Direct` 单独路由且不传/不存 state；未路由的虚拟选中跳过凭证解析，交给拒绝式适配器，原文案不变。扩展面实装 `registerVirtualModel`/`unregisterVirtualModel`、`__timo_route_virtual` 与 `ctx.modelRegistry.find/getModels/getAllModels`（目录快照经宿主 config 下发）。虚拟条目仍由 `models.json` 声明（D69），`thinkingLevels`/limits/input 只做注册期校验（D70）。
- gate：fmt / clippy `-D warnings` / workspace **361 tests**（+13，既有测试只补契约字段，断言语义未改）/ `check-size` OK（拆分后 `run/mod.rs` 381 行、`kernel.rs` 500 行；target 20.6GiB 超软限，阶段收尾 `clean-build`）。
- **变异 13/13 承重**（拆分后重跑，逐条确认 `test result: FAILED` 且编译通过、文件按 sha 还原）：reason 二选一、重试原因、目录核对、跳过虚拟凭证、state 回存、previous 快照、终态档位回填、只路由已注册虚拟、注销生效、物理 id 冲突、虚拟不能作路由目标、按模型取 adapter、`modelRegistry.find`。
- **真机 DeepSeek**（临时数据目录；`router/auto` 由 models.json 声明，扩展注册路由）：同一 run 内用户轮 `provider_request deepseek/deepseek-chat`、工具续轮 `deepseek/deepseek-reasoner`，终答 `VM_ROUTER_OK`，exit 0，`--no-session` 下零会话文件；`--no-extensions` 再选同一虚拟模型 → `Virtual model router/auto must be routed before streaming`、exit 1、零网络。
- **真机顺带发现（未修，登记）**：把续轮目标换成 `deepseek-flash` 时 400——"The `reasoning_content` in the thinking mode must be passed back to the API"。同一历史改投 `deepseek-reasoner` 正常；说明该模型的思考模式在工具调用回放时要求 assistant 消息带 `reasoning_content`，我们的 openai 适配器未回传。与 B2 路由无关，见「已知问题」。


### P2 第二题·批 B3 读数：状态与分支选择（2026-10-27）

- **落地**：新 entry `session.virtual_model_state {provider, model_id, state}`（只增不改；`Direct` 不写；状态在请求发出前落盘，失败也保留；同值不重复写，D71）。路由状态跨 run 用共享槽传递；进程重启从会话分支读回。恢复选择：显式 `--provider/--model` 优先；否则最后一条 `model.change` 指向仍注册且目录可解析的虚拟模型时保持它，否则退回最后一条成功响应的物理模型。虚拟选择在新建会话与显式覆盖时写 `model.change`（响应只记物理模型）；恢复出另一个选择后重建 agent（首次装配用的是命令行/设置的选择）。
- gate：fmt / clippy `-D warnings` / workspace **370 tests**（+9）/ `check-size` OK（`kernel.rs` 493 行；`open_new_session` 移入 `kernel/sessions.rs`；target 21.3GiB 超软限，阶段收尾 `clean-build`）。
- **变异 12/12 承重**（逐条确认 `test result: FAILED`、编译通过、文件按 sha 还原）：Direct 状态落盘、同值重复写、状态不落盘、共享槽不更新、跨 run 不读槽、恢复选择、显式优先、恢复状态、虚拟必须已注册、跳过失败响应、新会话写选择、取最后一条状态。
- **真机 DeepSeek**（两个独立会话各跑一次首轮，再分别恢复）：
  - 首轮两请求：`deepseek-chat` thinking=low（用户轮）→ `deepseek-reasoner` thinking=high（工具续轮），会话里写了两条 `session.virtual_model_state`（seen 1 → 2）与一条 `model.change router/auto`。
  - 带扩展恢复：助手消息 `deepseek-reasoner` thinking=**high**——说明选择恢复成虚拟模型，且 `request.state {seen:2}` 被读回并驱动路由；状态值未变化，未重复落盘（仍两条）。
  - 不带扩展恢复：助手消息 `deepseek-reasoner` thinking=**off**——虚拟模型未注册，选择退回最后响应的物理模型，且用命令行档位；无新增状态 entry。若 agent 没按恢复后的选择重建，默认选择会落到 `deepseek-chat`。


### P2 第三题·批 A 读数：嵌套工具接缝（2026-10-27）

- **落地**：`timo_tools::nested`（`CallableTool`/`NestedCall`/`NestedToolHost`/记录与用量合并）+ `ToolCallCtx.nested`；`timo-agent/src/run/nested.rs` 复用同一条钩子/审阅/管线（不发总线事件），id `<父>/<n>`、顺序队列、取消、四条记录上限、用量并回父结果；接缝清单 #8 同步。
- gate：fmt / clippy `-D warnings` / workspace **380 tests**（+10）/ `check-size` OK（target 22.0GiB 软限，阶段收尾统一 `clean-build`）。
- **变异 11/11 承重**：before/after 钩子、id 前缀、串行锁、用量合并、记录条数与参数字节上限、取消检查、Hidden 目录过滤、审阅传递、错误状态映射；每条都确认是断言失败而非编译错误，改后逐字节还原。
- 如实登记：本批只做接缝，**没有脚本沙箱**，因此没有真机读数；codemode 工具与脚本面在批 B。
- 体量：新增 `timo-tools/src/nested.rs` 232 行、`timo-agent/src/run/nested.rs` 278 行；测试拆 `tests/nested.rs`（248）+ `tests/nested_limits.rs`（222）+ `tests/common/mod.rs`（267），无超 500 行文件。

### P2 第三题·批 B 读数：codemode 沙箱与工具面（2026-10-28）

- **落地**：`timo-kernel/src/codemode/{mod,tool,describe,format,sandbox}.rs` + `shim.js`；`codemode` 内核内置（`ModelOnly`，默认关；`+codemode` 或 `--tools codemode` 激活；占位注册先于 allowlist、最终按注册表重建）。脚本全局 `tools/text/console/return/exit/ALL_TOOLS/describeTool`；选项行（`max_output_tokens`/`timeout_ms`）；输出编号/console 块/截断落盘；`Deferred` 进 `ALL_TOOLS` 但默认不占描述预算；worker 进程级 2、等待上界 60 秒；256 MiB 内存、16 MiB/10 万次输出上限。
- gate：fmt / clippy `-D warnings` / workspace **406 tests**（+26）/ `check-size` OK（target 24.2GiB 软限 WARN，阶段收尾统一 `clean-build`）。
- **变异 19/19 承重**（每条确认断言失败、编译通过、改后逐字节还原）：自过滤、Hidden 目录过滤、选项行、截断头尾、`is_error` 映射、字符与调用次数上限、中断处理器（恒假时 5.1 秒有界红，由 5 秒兜底收尾）、名字消毒、worker 上限、`--tools`/设置激活、默认隐藏、描述预算、失败清单、落盘路径、`exit` 成功语义、进度详情、`Deferred` 不列。
- **实现取舍**：脚本线程用 `std::thread` 而非 `spawn_blocking`（runtime 关闭会等待卡死的阻塞任务）；完成经 oneshot 交回异步侧，中断后最多等 5 秒。参数声明按名字排序，不依赖 JSON Map 遍历顺序。
- 如实登记：真机 DeepSeek 三场景（并行 bash、失败脚本、超时）留批 C；CLI/桌面嵌套进度呈现也在批 C。
- 体量：`describe.rs` 418、`format.rs` 476、`sandbox.rs` 498、`tests.rs` 454、`tool_tests.rs` 82、`mod.rs` 75、`tool.rs` 45；`kernel.rs` 500 行（激活逻辑移到 `kernel/codemode_api.rs`），无超 500 行文件。

### P2 第三题·批 C 读数：进度呈现、外壳修正与真机验收（2026-10-28）

- **落地**：CLI `Printer` 把 `ToolCallUpdate` 的文本行缩进打印（`run`/交互路径）；桌面把同一部分结果按行累积进 trace（换行连接，超 400 字符截断）。`-p` text 模式仍只出终答（既有语义）；JSON 模式本就逐事件转发。
- **真机 DeepSeek（临时数据目录 `target/codemode-accept/`，密钥只用环境变量）**：
  - 场景 1（`Promise.all` 跑两条 `sleep 0.8; date +%s%3N`）：会话只有一条 `codemode` tool result；脚本墙钟 999~1103 ms（串行约 1.6 s），两个时间戳相差 1~2 ms；`run` 输出 4 行嵌套进度（两条 `→ tools.bash`、两条 `← bash ok`）、`codemode done (995~1103 ms)`，随后 assistant usage>0。
  - 场景 2（`text("before")` + 未知名工具）：`codemode failed`（120 ms），结果保留 `before`，嵌套 `bash` 先成功、随后报 `Unknown tool "definitely_missing"`；进程 exit=0，工具错误不升级成进程错误。
  - 场景 3（`// @options: {"timeout_ms":1000}` + `sleep 30`）：1171 ms 返回 `Script timed out`，嵌套调用标 `bash error`（cancelled）；运行前后 `tasklist` 均无 `sleep.exe` 残留。
- gate：fmt / clippy `-D warnings` / workspace **413 tests**（+7）/ `check-size` OK（`clean-build` 后 14.0 GiB，软限 15 GiB）。
- **变异 26/26 承重**：批 B 的 19 条复跑 + 本批 7 条（CLI 进度行被丢弃、桌面覆盖而非累积、WSL 启动器未跳过、Windows 原生 bash 被忽略、settle 续体未排空就判状态、子调用先 abort 不给清理窗口、OOM 提示丢失），每条确认编译通过、断言失败、改后逐字节还原。
- **实现取舍**：超时/结束后先取消子令牌并最多等 2 秒让嵌套调用清理（例如杀进程树），再 abort 剩余任务；子调用用独立派生令牌，超时不会误报成父取消。
- 体量：新增 `sandbox/worker.rs` 304、`tests/limits.rs` 136、`shell_kind.rs` 115；`sandbox.rs` 280；无超 500 行文件。

## 本轮发现并修复的问题

- **Windows 上 `bash` 静默变成 WSL**：`Command::new("bash")` 命中 `System32\bash.exe`，一次 echo 要 13~20 秒且行为不同。改为按 PATH 解析绝对候选、跳过 System32/WindowsApps、用 `uname -s` 确认 MINGW/MSYS/CYGWIN，未命中再回退 PowerShell；echo 耗时回到 0.3 s。
- **`Promise.all` 续体被误报"未 settle"**：多个 settle 在同一轮到达后，Promise 续体 job 还没执行就判了状态。改为收完结果先清空 job 队列再判状态，并补一条确定性 worker 单元测试。
- **嵌套超时留下孤儿进程**：结束时直接 `abort_all` 绕过了 shell 自己的杀进程树。改为子令牌取消 + 最多 2 秒排空，再 abort；真机 `sleep 30` 用例前后均无残留。

- **桌面的 `/skill:` 曾是字面量**：CLI 在分发层展开，桌面把输入框原文直接交给内核，技能不会触发。已把展开收进 `Kernel::run`（commit `85527d4`），CLI 与桌面共用一条路径；CLI 仍保留"未知技能命令"的报错。
- **测试文件越线**：`app_tests.rs` 因新增用例达到 512 行。按既有"窗口级测试按功能拆分"先例，面板相关 3 条用例移到 `features/palette_tests.rs`，原文件回到 500 行内。
- **构建缓存触硬限**：验收前 `target/` 25.4GiB，`check-size` FAIL；跑 `scripts/clean-build` 清到 14.0GiB（无 sccache，未全清）。

## 体量核对（standards/08）

- docs/：当前 82 篇（软上限 120，ADR-0014）。阶段设计稿与详设属硬流程交付物；整合评审仍待办（删除面需所有者确认）。
- .agents/：文件 55（不含 vendored 技能为 21，上限 40）；`CONTEXT.md` 74 行（上限 150）。
- 代码：最大文件 496 行（`timo-agent/tests/routing.rs`），无超 500 行文件（批 B 拆出 `shim/runtime_dispatch.js`，批 C 拆出 `app_events.rs`）；最大 crate `timo-desktop` 5,658 行（含测试；上限 15,000 触发评审）。
- 依赖：无新增第三方 crate；`codemode` 使用 workspace 既有的 `rquickjs`、`tempfile`。
- 构建产物：本批前 25.2 GiB（incremental 3.0 / deps 21.0），`clean-build` 后 14.0 GiB（incremental 3.0 / deps 9.8）；批 B 收口 20.7 GiB（incremental 3.2 / deps 16.1），批 C 收口 23.6 GiB（incremental 4.2 / deps 18.1）；信任门控撤回后当前 24.3 GiB（incremental 4.6 / deps 18.4，软限内、incremental 触 WARN）；阶段收尾再跑。

## 已知问题与风险

- **codemode 超时/取消用例在 GitHub Windows runner 偶发**（2026-10-28 两次 CI 分别命中）：`timeout_and_cancel_interrupt_a_busy_script` 报 `Script sandbox failed: Exception generated by QuickJS`，`timeout_lets_nested_calls_clean_up_before_returning` 报 `Error: cancelled` 顶掉 timeout 文案。根因两处：① 慢机器上 SHIM 安装期撞上 deadline，中断错误被原样当成沙箱失败；② 主循环先按 deadline 收尾并取消子调用，脚本线程随后报的 `cancelled` 覆盖了 timeout 原因。已修：安装期失败按 deadline/取消重述；主循环主动中断时强制用中断原因覆盖脚本收尾错误。本机 20 次重复全绿，CI 复跑验证。

- **工作树嵌套去重留 P2**（D36）：主仓与嵌套 worktree 各有一份 `AGENTS.md` 时可能重复注入。
- **每轮 usage 行只显示最后一次请求**（Phase 1 既有行为）：一轮里发生工具调用时，`[usage]` 是最后一条 assistant 消息的读数；整轮/会话合计以 `/session` 为准。本阶段未改这一语义。
- **费用依赖费率配置**：`models.json` 未写 `cost` 时费用恒为 0；不重新计价是设计选择，需在配置里给费率。
- **缓存统计只在 CLI `/session` 呈现**，桌面暂无 cache 行；桌面状态栏的费用已含缓存计价，不新增面板（设计 05 §3.3）。
- `target/` 无 sccache 时不做全清；当前 24.3GiB（incremental 4.6），阶段收尾需跑 `clean-build`。
- **DeepSeek 思考模式 + 工具调用的 `reasoning_content` 回放**（B2 真机发现）：从非思考模型切到 `deepseek-flash` 时，含工具调用的 assistant 历史被拒（400，要求回传 `reasoning_content`）；同一历史改投 `deepseek-reasoner` 正常。我们的 openai 适配器未把思考内容写回请求；属供应商兼容项，未在本批修，随 provider composer/codemode 的思考链处理一起定。
- **无头 stderr 混入技能告警噪音**：`--no-session -p` 一次运行会打出项目 `.agents/skills/` 里 8 条 frontmatter 告警（README 与 ponytail 等非技能文件被扫）。资源告警本身是要出口的，但子代理场景里父进程会把 stderr 原文贴进结果，噪音挤掉有效信息。处置方向（未定）：`-p`/`--mode json` 下只出口与本次请求相关的告警，或把非技能 md 排除出扫描面——随资源治理一起定。

## 下一步

1. 桌面视觉精修（并行线）：research/07 → 设计稿 → 批次实现；与内核零冲突。
2. 阶段收尾：D1~D7 与 P2 各题验收 → 本报告补全 → `clean-build` → tag `phase-3.5.end`。

## 文档更新清单

- `docs/design/phase-3.5/report.md`（本页）：P0 + P1 验收与读数。
