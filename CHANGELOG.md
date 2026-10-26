# 更新日志（CHANGELOG）

本文件是**面向非技术读者的进展流水**，每次文档或阶段变更都必须在此登记一条（格式规范见 `docs/standards/03-documentation.md`）。技术读者另见 git 历史。

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)；版本号在本项目同时用于标记「文档/阶段里程碑」，与未来代码发布版本共用一个序列。

---

## [0.1.17] - 2026-10-26

### Phase 2 实现：循环钩子、供应商钩子与事件桥

- **扩展能影响主循环**：系统提示可被替换；每次请求前的消息可被改写；工具调用前可改参数或直接否决（否决会以"错误结果"写给模型，界面仍能看到这次调用）；工具结果返回前可按字段覆盖。
- **扩展能改网络请求**：发送前可替换请求体、增删请求头；成功响应到达时收到通知（只读）。
- **统一的桥**：内核只认识两个抽象接口，不知道"扩展"存在；事件载荷超过 8 MiB 不派发并留下诊断；取消后不再派发新事件。
- **探针结论**：钩子超时中断后，同一个 QuickJS 上下文仍能继续执行下一条脚本（已写成测试）；无需按最坏情况重建上下文。
- 新增桥接测试 6 个、循环钩子测试 6 个、供应商钩子测试 1 个；`timo-agent` 的循环按 500 行上限拆为 `run/mod.rs` + `run/tools.rs`；全仓门禁四连通过。

### 当前状态

- **Phase 2 进行中**：提交序列第 5 步（钩子）完成。下一步：权限执行面（第 6 步）。

---

## [0.1.16] - 2026-10-26

### Phase 2 实现：安装期打包 + QuickJS 宿主

- **安装时打包**：rolldown/oxc 把扩展源码打成单个 `bundle.cjs`，同时生成 manifest（源码指纹、建议能力）；缺少源码目录、入口越界、引入 Node 内建模块、动态求值、顶层 await 都会给出对应错误码，不静默放过。
- **运行期托管**：每扩展开独立 QuickJS 上下文，由一条专用线程串行执行；扩展只能在宿主提供的桥上调工具、命令、事件与用户交互，自身没有系统能力。
- **pi 兼容面**：`VERSION="1.4.0"`、工具/命令强制 `plugin_<id>_` 前缀、惰性 Proxy 降级（TUI 等未支持成员不报错但留诊断）；`tool_call` 可改参数或被否决、`tool_result` 合并、`before_agent_start` 的 `systemPrompt` 最后生效。
- **稳定性**：钩子 30 秒预算到点即中断并报 E301；单个处理器抛错只留诊断、不拖垮其他扩展；错误文案保留原始原因，不只给堆栈。
- **格式说明**：产物采用 CJS（`bundle.cjs`），因为内嵌 QuickJS 不加载 ESM 模块；设计文档同步登记（D27），顶层 await 明确记为不支持（E106）。
- 新增 1 个打包测试文件、1 个宿主测试文件（合计 17 个测试）；`timo-ext` 现有 37 个测试全绿，门禁四连通过。

### 当前状态

- **Phase 2 进行中**：提交序列第 3 步（打包）与第 4 步（shim + 宿主）完成。下一步：接入 `LoopHooks`/`ProviderHooks` 与事件桥（第 5 步）。

---

## [0.1.15] - 2026-10-26

### Phase 2 实现：扩展宿主骨架（发现 / manifest / 诊断）

- **能认出扩展**：扫描项目 `.timo/extensions/`、pi 兼容目录 `.pi/extensions/`、用户目录与显式路径；支持 pi 的三种入口写法（单文件、目录 `index.ts`、`package.json` 的 `pi.extensions`），跳过隐藏项与 `node_modules`；同名扩展按"显式 > 项目 > 项目 pi > 用户"取舍，冲突可见。
- **能校验声明**：manifest 的 schema、id、相对路径、源码指纹逐项检查；源码目录生成稳定 sha256 指纹，为后续"源码一变、授权作废"做准备。
- **能说清问题**：28 个错误码（打包/加载/运行/权限四段）+ JSON 报告 + 人读表格；每条错误带码、原因、定位。
- **规范对齐**：按代码规范禁止扩展代码动态求值（`eval`/`new Function`），设计文档同步登记（D26）。
- 新增 `timo-ext` crate（5 个源文件、2 个集成测试文件；18 个测试），全量门禁四连通过。

### 当前状态

- **Phase 2 进行中**：设计定稿；提交序列第 2 步（骨架）完成。下一步：安装期打包模块。

---

## [0.1.14] - 2026-10-26

### 里程碑：Phase 2 设计定稿（实现未开始）

- **扩展怎么进来**：扩展保持 pi 的 `.ts` 写法；安装时一次性打包成单文件，运行期由内嵌 QuickJS 执行，**日常使用不需要安装 Node**。遇到 `node:fs` 这类不可用能力会明确报错，不静默。
- **扩展能做什么**：注册工具/命令、订阅生命周期与请求钩子；每一项能力分成「支持 / 缓做 / 不支持」并写进报告，用户能看到哪个成员没生效、为什么。
- **权限怎么管**：扩展先声明需要什么（命令执行 / 网络域名 / 文件范围），用户批准后才生效；默认全拒，网络按域名白名单，撤销后立即失效，源码变化自动重新询问。
- **稳定性底线**：单个钩子最多占用 30 秒（用户交互与命令运行时间不计），超时只切断该钩子、会话继续；扩展改事件数据默认不影响内核；取消后钩子立即「退休」，不再收到事件。
- **校准**：按 pi 1.4.0 实际 API 逐项核对了 41 个事件——pi 没有 `error` 事件，压缩事件名也与早期文档不同；修正记录在架构 04 与设计 05。
- **验收样本**：确定 hello、permission-gate、protected-paths 三个官方样本（工具、钩子否决、对话框/通知四类面）；todo 与 auto-commit 依赖尚未实现的会话读取接口，本阶段不选。
- 新增设计文档 6 篇（单篇 107~222 行）；docs 总数 53 篇（上限 60），`.agents/` 项目文件 12 个（上限 40）。

### 当前状态

- **Phase 2 进行中（设计定稿）**。下一步：建立 timo-ext、按提交序列实现；实现前不再新增设计文档。

---

## [0.1.13] - 2026-10-26

### 治理修正：`.agent/` 并入 `.agents/`

- **修正目录命名笔误**：AI 知识库的唯一目录是 `.agents/`。原 `.agent/` 的 CONTEXT、handoff、pitfalls、skills 已用 `git mv` 并入，git 历史可追溯；旧目录与占位 `.gitkeep` 删除。
- **更新全部活跃引用**（12 个文件）：AGENTS.md、docs/README.md、standards 01/03/07/08/09、roadmap、Phase 1 报告、`.agents/README.md`、`.agents/skills/README.md`、H-001。CHANGELOG 中已发布的历史记录保留原文，不改写。
- **明确 `skills/` 双重身份**：vendored 上游技能子目录（gpui-kit 等，不计预算）与项目自创 `S-NNN-*.md` 同目录共存；预算口径同步写入 `.agents/README.md` §3 与 standards/08 §3。
- 纯文档/治理变更，无代码行为变化；`.agents/` 项目自创文件 9 个（上限 40）。

### 当前状态

- **Phase 1 完成**。下一阶段：Phase 2 扩展宿主（未开始，待开工通知）。

---

## [0.1.12] - 2026-10-26

### 里程碑：Phase 1 完成（CI 全绿，tag `phase-1.end`）

- **GitHub Actions 全绿**（run 40308144023）：fmt / clippy `-D warnings` / test（122 个）/ check-size 全部成功。
- **修复首次 CI 暴露的测试辅助函数问题**：shell 测试直接调用 Bash、绕过了产品的原生 bash 探测；GitHub runner 的 `bash.exe` 是空 WSL，导致 `echo`/超时测试失败。测试改为复用同一判定。
- **验收 A1~A6 全部通过**：真实任务、会话恢复、强杀、MCP 工具调用、CI、七条接缝；进度报告转定稿。
- 仓库已推送至 `github.com/qingbird1983/timo-pi`（私有）。

### 当前状态

- **Phase 1 完成**。下一阶段：Phase 2 扩展宿主（未开始，待开工通知）。

---

## [0.1.11] - 2026-10-26

### 里程碑：真实模型验收通过（DeepSeek），修复两个真实缺陷

- **A1~A4 真实执行通过**：用 DeepSeek `deepseek-flash` 完成"读文件→改文件→跑命令验证→汇报"、会话列表与恢复续聊、运行中强杀后会话文件完好、模型发现并调用本地 MCP 工具（`mcp__echo__echo`）。A5 本地门禁全绿（122 个测试），远端 CI 待推送确认；A6 接缝复核通过。
- **修复 `edit` 精确替换缺陷**：替换单行时会把后续行重复、并插入空行（首个真实任务即暴露，模型自己发现异常）。改为按字节区间替换，保留同行前后缀，并加单行/行内/跨行/多编辑/CRLF+BOM 精确内容回归。
- **修复 Windows `bash` 落到 WSL 的问题**：仅凭 PATH 有 `bash.exe` 就使用，实际可能是 WSL 启动器——它不认 Windows 工作目录。改为仅接受原生 MSYS/Cygwin bash，否则回退 PowerShell。
- 真实调用使用本机环境变量 `DEEPSEEK_API_KEY`；临时测试数据目录未入库。

### 当前状态

- Phase 1 验收完成；剩 A5 远端 CI 确认与 tag `phase-1.end`。

---

## [0.1.10] - 2026-10-26

### 里程碑：Phase 1 内核与 CLI 全部落地，进入人工验收

- **八个 crate 全部实现并可编译**：timo-config、timo-ai、timo-session、timo-tools、timo-mcp、timo-agent、timo-kernel 门面、timo-cli 冒烟壳；全量测试 117 个通过，fmt/clippy/check-size 门禁全绿。
- **两种协议可流式输出**：新增可注入传输层 `SseTransport`（默认 reqwest 实现），协议夹具（`fixtures/protocol/`）回放覆盖正常流、工具调用、错误、取消四类路径；修复 OpenAI 协议从不产出流式增量事件的问题（此前 CLI 只看得到最终结果）。
- **会话与 MCP 夹具**：`fixtures/session/` 四个 v1 格式夹具（线性/工具/未知类型/半行截断）进测试；`fixtures/mcp/echo_server.py` + 集成测试验证 MCP 工具发现、命名空间与统一管线。
- **内核门面收尾**：MCP 改为首次 run/tools 才连接（失败只通告不阻断）；补齐 `tools()`、`wait_idle()`、中止句柄、无凭证可用的会话列表入口；CLI `run`/`resume`/`sessions` 三命令与 Ctrl+C 两段语义落地。
- **阶段进度报告**（`docs/design/phase-1/report.md`，进行中）：A4/A5 本地通过；A1~A3 需真实凭证与人工操作，待所有者授权后执行；A6 七条接缝核对通过。

### 当前状态

- Phase 1 进入人工验收：真实任务执行（A1~A3）与 CI 首次运行待完成，完成后报告转正、看板更新、打 tag `phase-1.end`。

---

## [0.1.9] - 2026-10-26

### 里程碑：Phase 1 开工（所有者已下达开工通知）

- **Phase 1 实现级详设定稿**（`docs/design/phase-1/`，8 篇）：00 总览与跨 crate 决策（含与 pi 的偏离登记表 D1~D10，逐项挂 ADR-0011 白名单）、01 协议层（消息/流事件/协议 trait/两协议映射/模型目录/认证/两层重试）、02 agent 循环与事件总线、03 工具元模型与内置工具契约、04 会话格式实现设计、05 配置与模型解析、06 MCP/门面/CLI、07 测试与 CI。行为语义全部以 pi 为蓝本，详设内逐篇标注 pi 参照路径。
- **会话格式定稿**（architecture/05 草稿→定稿）：四个待决问题全部决议——entry id 用 ULID+进程内单调工厂（解决时钟回拨）；单文件不轮转；大 payload 一律内联；数据目录走平台惯例（Windows `%APPDATA%\timo-pi`）。相对草案两处修订：四种消息类 entry 合并为单一 `message` 型（与 pi 同构，防同一事实两处表达）、取消 `system.notice` 型（错误/取消由 assistant 终态消息承载）。
- **依赖准入表初版**（standards/04 新增附录 A，18 项登记版本线/用途/备选否决理由）。同时修正 §5 笔误：原引用不存在的 `04-dependencies.md`（与既有 04 编号冲突）——standards/09"矛盾即 bug 当轮修"的首例，按笔误级处置。
- phase-1 §6 开工前置条件全部达成并勾选；本提交打 tag `phase-1.start`。

### 当前状态

- **Phase 1 实现进行中**：按 workspace 骨架 → timo-config → timo-ai → timo-session → timo-tools → timo-mcp → timo-agent → timo-kernel → timo-cli 顺序实现，每 crate 一个提交、含测试；收尾出《阶段进度报告》（含接缝核对、体量核对、规范复查）。

---

### 新增（所有者要求：给制度装上"制度的制度"）

- **规范 09：规范复查与制度进化**（`docs/standards/09`）——本项目靠制度管理而非人治，所以制度自身也会出 bug（漏洞/过严/过松/过时），必须有"复查—修正—进化"闭环：
  - **例行复查**：每阶段收尾固定动作，按"四问"过一遍全部规范——有漏洞吗 / 过严吗 / 过松吗 / 有冗余矛盾吗；结论写入《阶段进度报告》新增小节（standards/03 模板已更新）。无发现如实写"无发现"，不强行产出。
  - **触发复查**：五种信号出现即当轮处置——违规漏拦（漏洞）、同一规则被豁免 ≥2 次（过严）、同类问题重复 ≥2 次靠 AI 自觉兜底（过松）、上游变化致规则失效、**所有者读错文档视为文档 bug**（所有者是文档唯一验收人）。
  - **修正分级**：笔误直接改；规则级走 ADR；机制级（废/立规范）ADR + CHANGELOG 置顶向所有者说明。修订须同步全部引用处，防止"改了正文漏了引用"。
  - **自约束**：复查输出有界（报告一节、不新增文档）、当轮闭环、不做预防性全盘重审。
- 联动：standards/03（模板 + 规范类型行）、standards/07（收尾职责）、standards/08（豁免 ≥2 次触发复查）、AGENTS.md 与 docs/README.md（登记为 9 篇）。

### 当前状态

- **等待所有者审阅文档后下达开工通知**；Phase 1 开工前置条件不变（phase-1 §6）。

---

## [0.1.7] - 2026-10-26

### 新增（写码纪律）

- **ponytail 技能 vendored** 至 `.agents/skills/ponytail/`——GitHub `DietrichGebert/ponytail`（176k stars，MIT，commit `09c434a`），字节一致复制 SKILL.md + LICENSE。"最懒资深工程师"纪律：用最少的新代码完整解决问题，优先复用既有结构与依赖，不造没人要的抽象；**绝不削减**边界校验、错误处理、安全与必要测试（与防膨胀规范 standards/08 同向）。
- 联动：`.agents/skills/README.md`（收录清单/来源与许可/同步策略扩为两类）、AGENTS.md 文档索引新增"写码纪律"入口、standards/04 新增 §8（冲突时以本规范为准；`shortcut:` 标记按 §7 落地为 `TODO`）。

### 当前状态

- **等待所有者审阅文档后下达开工通知**；Phase 1 开工前置条件不变（phase-1 §6）。

---

## [0.1.6] - 2026-10-26

### 新增（所有者硬约束）

- **ADR-0011：内核阶段严格对齐 pi，忠实复刻；创新延后至项目稳定**——Phase 1~2 行为语义以 pi 为唯一蓝本（语义对齐而非逐行照抄），允许偏离仅限三类白名单（语言必然差异 / 既有 ADR 载体替换 / standards 硬要求）；创新冲动一律记录不执行，解禁需 Phase 4 验收通过 + 所有者同意。
- 联动：AGENTS.md 铁律新增"方向纪律"（第 6 条）、standards/07 §6、roadmap/phase-1 与 phase-2 各加方向纪律注记、roadmap/00 §3 Backlog 升格为创新提案唯一通道。

### 当前状态

- **等待所有者审阅文档后下达开工通知**；Phase 1 开工前置条件不变（phase-1 §6）。

---

## [0.1.5] - 2026-10-26

### 新增（standards/08 §7：构建产物治理）

- 针对"Rust 仓库膨胀到几十上百 G"的老大难，定下治理方案：**单 workspace 单 target/ 红线**、瘦身 profile（`debug = "line-tables-only"` 等，预期砍 60~80% 体积）、按龄清扫不全清（cargo sweep + sccache 兜底）、target/ 15G 软上限纳入阶段核对、git 禁止构建产物与 >1MB 二进制。
- Phase 1 交付物补充：`.cargo/config.toml` 瘦身 profile + `scripts/clean-stale` + `check-size` 盘点 target/。

---

## [0.1.4] - 2026-10-26

### 新增

- **`.agents/skills/`**：vendored 官方工具技能——gpui-kit 框架技能（组件/编码规范/GPUI 机制 22 篇）+ gpui-kit 设计规范总纲，共 29 个文件，自用户级安装复制、逐一比对一致。Phase 3（桌面壳）核心参考。
- 其 `README.md` 登记来源、同步策略（视同依赖升级，Phase 3 开工前重同步）与预算豁免声明；与项目自创的 `.agent/skills/`（知识沉淀）明确区分。

### 联动更新

- standards/01 目录布局新增 `.agents/skills/`；AGENTS.md 文档索引新增桌面开发入口。

---

## [0.1.3] - 2026-10-26

### 新增

- **调研 05：pi-flash 分析**（`docs/research/05`）——Windows 专用 pi 桌面 GUI 壳（Rust + GPUI + gpui-component，GPL-3.0）。结论：内核线无参考价值（它复用 pi 无内核）；**gpui 桌面线价值最高**（8.7 万行生产级 GPUI 应用 + 陷阱全集 + vendor/性能/架构守卫方法论）；插件系统线次之但对口（QuickJS + qjsc 字节码 + ed25519 签名闭环）。

### 联动更新

- AGENTS.md 铁律 6 与 standards/07 §5：外部代码合规范围扩大——pi-flash（GPL-3.0）与 PI-Desktop（LGPL）同级，只读思路、严禁抄码。
- roadmap/phase-3 风险注记：开工前需核对 **gpui-kit 与 gpui-component（0.2.x）的版本谱系与 API 延续性**。

---

## [0.1.2] - 2026-10-26

### 变更（仓库根入口重组）

- **README 改为纯项目说明，出中英双份**：`README.md`（英文）+ `README.zh-CN.md`（中文），内容一致、互链；进度看板从 README 移除。
- **进度看板收拢**：`docs/roadmap/00-roadmap.md` 的阶段总览表成为**唯一看板**（消除原先 README 与 roadmap 两份重复）。
- **AGENTS.md 重构为薄索引入口**：只保留导航表 + 铁律链接；明确禁止记录状态/进度/阶段指针等流水账；原"当前阶段指针"职责移交 `.agent/CONTEXT.md`。

### 规范修订

- `standards/01`：根文件清单与 README/AGENTS 定位约束。
- `standards/08`：§2 新增"入口文件定位约束"（README 双语一致性、AGENTS 薄索引、状态唯一来源）。
- `docs/README.md`：所有者与 AI 的阅读路径更新。

---

## [0.1.1] - 2026-10-26

### 决策落定（ADR-0010）

- 产品正式命名 **Timo-pi**；项目**开源，许可证 MIT**（新增根目录 LICENSE）；**Windows 优先**（Phase 3 主攻，macOS/Linux 保证可构建）。

### 新增

- **`.agent/` 知识库与交接中枢**：当前上下文交接卡（CONTEXT.md）+ handoff 归档 + pitfalls 踩坑沉淀 + skills 技术沉淀；自带硬性体量预算（防无限膨胀）。
- **规范 08：体量与防膨胀管理**（`docs/standards/08`）——覆盖四类"只增不减"集合：docs 文档（流水账禁令、单篇行数上限、孤儿清零）、.agent 知识库、代码（单文件 ≤500 行、crate 拆分评审、死代码零容忍）、依赖。

### 联动更新

- README（标题/许可证/决策表）、AGENTS.md（.agent 纳入开工必读与硬规则 9）、standards/01（目录布局）、standards/03（进度报告模板新增体量核对）、standards/07（新增 §7 交接沉淀流程）、roadmap/phase-3（Windows 优先）、docs/README（登记 08）。

---

## [0.1.0] - 2026-10-26

### 新增（Phase 0：仓库初始化与文档定稿）

- 初始化 git 仓库（`main` 分支）。
- **顶层**：README（含进度看板与待决策事项）、AGENTS.md（AI 协作入口）、CHANGELOG、.gitignore。
- **调研沉淀 4 篇**（`docs/research/`）：pi 内核规模与架构分析、pi 扩展系统分析、gpui-kit + NSH 技术栈评估、参考项目 PI-Desktop 分析。
- **决策记录 9 篇**（`docs/adr/`）：Rust 全量自研、桌面栈选型、插件编排类兼容策略、GUI 插件 TSX 自建、rmcp、QuickJS 扩展宿主、自研会话格式、范围排除、内核即库。
- **架构设计 5 篇**（`docs/architecture/`）：总体架构、内核设计、关键接缝清单、插件系统（双轨制）、会话格式草案。
- **路线图 6 篇**（`docs/roadmap/`）：总路线图 + Phase 0~4 阶段文档（各含验收场景）。
- **仓库规范 7 篇**（`docs/standards/`）：仓库结构、Git 工作流、文档规范、代码规范、测试规范、安全规范、AI 协作规范。
- **术语表**（`docs/glossary.md`）。

### 待决策（不阻塞 Phase 1）

- 开源许可证、正式产品名、目标平台优先级（见 README「待所有者决策的事项」）。
