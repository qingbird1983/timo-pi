# Phase 3 阶段进度报告（写给非技术读者）

- 日期：2026-10-27
- 本阶段目标（一句话）：把内核装进一个日常可用的桌面窗口——会话、流式对话、工具过程、改动审阅全部可见，并验证 NSH 插件界面可以嵌入。
- 状态：**完成**（tag `phase-3.end`）

## 完成了什么

- **一个能每天用的窗口**：左侧会话、中间对话、底部状态栏；新建/切换/删除会话，流式回答，工具调用显示成卡片（状态、参数、结果、耗时），任务可取消。
- **改动先审后做**：模型要写文件或执行命令时，右侧滑出审阅面板，显示 red/green diff 或完整命令；接受才落盘/执行，拒绝则什么都不做，并把原因回给模型。
- **快捷键与命令面板**：Ctrl+K 命令面板（会话/运行/界面/模型四组，可过滤）；Ctrl+N、Ctrl+,、Ctrl+.、Ctrl+Enter、Ctrl+Backspace、Esc；设置页可改语言、主题、字号，主题与窗口尺寸重启后保持。
- **密钥进系统钥匙串**：设置里粘贴 API key 只写 Windows 凭据管理器，配置文件只留 `keychain:timo-pi/<provider>` 引用，界面、配置、日志都不出现明文。
- **NSH 嵌入 PoC**：一个 TSX 面板画在 gpui-kit 窗口内；换成 v2 签名包后内容与版本号在 1 秒内热更；未签名、篡改、降级、启动崩溃的包全部被拒，原面板保持可用。

## 验收场景执行结果

| # | 结果 | 说明 |
|---|---|---|
| C1 日常使用清单 | **通过** | 真实 DeepSeek：新建会话 → 流式回答 → 工具卡片 → 文件审阅接受/拒绝 → 重启后会话与历史完整恢复。流式逐字沿用第 4/5 批真机证据。 |
| C2 权限/信任确认 | **通过** | 写 `proof.txt` 前出现 diff 审阅（`+1 -0 · 新文件`），接受后文件内容为 `timo phase3 accept`；写 `rejected.txt` 时填原因后拒绝，文件不存在，模型回复“操作被拒绝，原因是验收规则：拒绝写入”。命令拒绝沿用 H-008 证据（`cmd-out.txt` 未创建、未 spawn）。 |
| C3 密钥安全 | **通过** | 沿用 H-008 钥匙串真机证据（保存/移除、`deepseek.timo-pi` 条目、数据目录无 `sk-`）；本轮回扫临时数据目录与运行日志：`sk-` 命中 0，配置只有 `$DEEPSEEK_API_KEY` 引用。 |
| C4 NSH PoC | **通过** | v1 面板渲染并计数 0→1；v2 热更（版本 2.0.0、标题变橙、sha 变化、不重启）；未签名/篡改/降级/启动崩溃四类拒绝且原面板不变；深色主题跟随；最大化到 1745×768 面板跟随；单窗口，无第二套事件循环。 |
| C5 崩溃恢复 | **通过** | 会话完成后 `taskkill /F` 强杀进程，重开应用：侧栏出现原会话，打开后两轮消息、两份工具 trace 与拒绝原因都在；会话文件 9 行可读，`proof.txt` 仍在、`rejected.txt` 不存在。 |
| C6 接缝 + CI | **通过** | 七条接缝逐条核对（见下）；桌面 11 条 `#[gpui_kit::test]` 核心交互 + 1 条 UI 集成测试；workspace 228 tests 全绿；`nsh-poc` feature 下 23 条含“未签名字节被拒”单元测试。 |

### C4 关键文本（真机截图附件未入库，按 standards/08 的 1MB 二进制资产规则）

- v1 接受：`已接受 v1.0.0（sha256 d51a6e1b…）`，面板 `Timo · NSH 预览` / `已加载签名包 v1.0.0`。
- v2 热更：`当前包 com.timo.poc v2.0.0 (build 2)` / `sha256 cd64faa7` / `已接受 v2.0.0`。
- 未签名：`poc-unsigned.bin 被拒绝：not a valid nsh package: bad magic`。
- 篡改：`poc-tampered.nshpkg 被拒绝：the signature does not verify`。
- 降级：`签名校验失败：app 'com.timo.poc' build 1 is older than build 2, which was accepted before`。
- 崩溃隔离：`新包启动失败，已保留原面板：Error: poc boom: startup failure`。

## 关键接缝核对（architecture/03 七条）

1. **会话磁盘格式**：architecture/05 定稿；v1 fixture 覆盖未知 entry 保留不投影、末行截断丢弃；强杀后文件可读（C5 实测）。
2. **内部事件总线**：`crates/timo-agent` 无 `if extension`/扩展分支（grep 0 命中）；桌面 GUI 只作为订阅者接 `KernelEvent`，新增订阅者未改循环。
3. **协议 trait + 数据驱动目录**：`ProviderApi` 一套 trait，Anthropic/OpenAI 两个实现；模型目录仍是 `assets/models/*.json` 数据文件。
4. **工具元模型**：`ToolDefinition` 带 `exposure`/`namespace`/`annotations`；MCP 工具经 `mcp__<server>__<tool>` 同一注册表，内置/MCP/扩展来源仅前缀不同。
5. **JSON Schema 通货**：`timo-tools::definition::validate_params` 用 `jsonschema` 统一校验，无第二套 schema。
6. **富流事件枚举**：`AssistantStreamEvent`（Start/BlockStart/Delta/BlockEnd/Done/Error）与 `KernelEvent`（13 变体）均为强类型枚举，无裸字符串流。
7. **取消贯穿**：`CancellationToken` 从工具执行入口传到审阅等待、编辑写入与子进程；取消/关窗时审阅回传拒绝。

## 体量核对（standards/08）

- docs/：新增 1 篇（本报告）；总数 65（软上限 60，已超 5 篇；阶段报告是硬流程，整合评审登记为 Phase 3.5 开工前必做）。
- .agents/：文件 53（含 vendored 技能；不计技能 19，上限 40）；`CONTEXT.md` 60 行（上限 150）。
- 代码：最大文件 499 行（`timo-kernel/src/kernel.rs`），本批无 >500 行文件；无新增依赖。
- 构建产物：`target/` 约 38GiB（`check-size` 口径；字节折算 40.6 GB，其中 `debug/incremental` 12.7 GB、`debug/deps` 26.3 GB）。`cargo-sweep 0.8.0` 已安装并运行 `scripts/clean-stale`（`-t 30`），清理 0——全部产物新于 30 天。软上限仍超；无 sccache 时不做全清，留给 Phase 3.5 或安装 sccache 后处理。
- 豁免登记：docs 软上限 +5（阶段报告与既有设计稿）；target 软上限（NSH 构建面）。

## 规范复查（standards/09 四问，逐篇结论）

- **01 仓库**：无漏洞；新增 `fixtures/nsh-poc/panel.tsx` 与 `scripts/build-nsh-poc` 均属既定目录，无新根文件。
- **02 提交**：无漏洞；第 6 批独立提交，版本未动。
- **03 文档**：执行到位；设计定稿后写码；本报告按模板落盘。
- **04 代码**：无过严/过松；NSH 隔离层单文件曾 667 行，已按 500 行上限拆成 `mod.rs`（409）+`page.rs`（268）。
- **05 测试**：需补一处——NSH 渲染/热更为人工验收（按设计 03 §8 允许），自动化只覆盖未签名拒绝。
- **06 安全**：无漏洞；开发密钥、签名包均在用户数据目录，不入库；无个人路径写进仓库。
- **07 协作**：无漏洞；交接卡与 handoff 同步。
- **08 体量**：两处软上限超限已登记（docs 总数、target 大小），不构成硬失败。
- **09 复查**：无需立即改规范；建议下阶段处理 docs 总数整合与 `cargo-sweep` 工具就位。

## 已知问题与风险

- 模型页仍只读，运行期切换模型需重建内核（后续批次）。
- 设置缺“扩展”页；会话搜索框、超大 diff 外部编辑器、trace 全文入口未做（详见 roadmap phase-3 §6）。
- NSH 渲染/热更证据为人工截图与文本，未进 CI（NSH 默认不编译，符合设计 03 §8）。
- `target/` 约 38GiB 超软上限；`cargo-sweep 0.8.0` 已装，30 天按龄清扫 0 释放（产物均新于 30 天）；需 sccache + `cargo clean` 才能降到 15GB 以下。
- 移除钥匙串条目后配置引用保留，需重新配置（设计 02 §8 语义）。

## 对下阶段的影响

- Phase 3 验收通过，Phase 3.5（内核深化）可开工；桌面壳视觉精修与 3.5 并行。
- 插件系统（Phase 4）可复用 NSH 隔离层与签名包信任链，但生产分发密钥策略仍需 ADR。

## 文档更新清单

- 新增 `docs/design/phase-3/report.md`（本页）。
- 更新 `docs/roadmap/00-roadmap.md`（Phase 3 完成）。
- 更新 `docs/roadmap/phase-3-desktop-shell.md`（状态与已知限制）。
- 更新 `CHANGELOG.md`（0.1.28）。
- 更新 `docs/README.md`（登记本报告）。
- 更新 `.agents/CONTEXT.md`、新增 `.agents/handoff/H-009-phase-3-nsh-c1-c6.md`、新增 `.agents/pitfalls/P-010-gpui-glob-shadows-test.md`。
