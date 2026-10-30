# Phase 4 阶段进度报告（写给非技术读者）

- 日期：2026-10-30
- 本阶段目标（一句话）：让第三方用 TSX 写带界面的插件，经签名包分发、授权后在主窗口指定槽位运行，并用一个自家功能插件自证扩展面。
- 状态：**D1~D5 通过、D6 基本通过；`phase-4.end` 待所有者确认后打**。D4 原先的崩溃（插件面板在场 + 无障碍激活 + 重绘 panic）已由本地补丁（ADR-0018）消除；本轮补齐看门狗 E742、60s 崩溃循环自动停用、关闭窗口退出与卸载残留的真机证据，也录到 D1「无重启即时替换」的过程。

## 验收方式与环境

- 所有者不在电脑旁，本轮由 AI 会话代理真机操作，所有截图与日志只留在本机临时目录，不随仓库分发。
- 桌面数据用**临时目录**（`%TEMP%\timo-accept-data`），未触碰所有者真实会话与插件数据；签名用临时测试密钥，私钥只以路径传给 `nsh`，未读取或打印。
- 平台：Windows（10.0.26300，x64）；调试构建；窗口 1296×808（物理像素 2800×1280 截图）。
- 被测包：`examples/plugins/session-search`，经 `timo-devkit` 打包签名的 `1.0.5` build 7（sha256 `054cdc57…`），`read-only` 挡位、`panel` 槽位。D4 补充用例为同 id 的 v1.0.6~v1.0.12（build 8~14），仍用同一临时测试证书；v1.0.9 是「不回 `__pong`」的静默变体，v1.0.7/8/10/11 带故意死循环的入口。

## 完成了什么（P0~P2 摘要）

- **统一插件管理器 + 市场**：两轨共用清单与生命周期；market 页签、分类/搜索、收藏/屏蔽、离线缓存；卡片安装与本地安装共用验签与挡位流程。
- **安装账本**：schema 2 清单、ed25519 包验签、回执落盘、版本替换与回滚、撤销、暂存清扫；防降级由内核账本兜底（E712）。
- **运行时与宿主 API v1**：每实例独立宿主线程；查询/动作白名单与参数校验（E740/E741）、挡位不足（E722）、超时（E742）；存储、主题、`sessions.changed`；1s 健康门、2s 看门狗、60s 内两次崩溃自动停用。
- **四种 slot 容器**：侧栏面板、工具卡、输入区触发器、会话附加区；诊断卡记录 E7xx。
- **devkit 与自证插件**：`new/check/pack/verify/keygen`；会话搜索插件只用 `sessions.list`/`sessions.search`/`openSession`。

## 验收场景执行结果

| # | 结果 | 说明 |
|---|---|---|
| D1 示例插件全链路 | **通过** | devkit 全链、安装、渲染、搜索、打开会话、防降级均通过；热更录到一轮无崩溃干扰的即时替换：面板在场时安装 v1.0.6，侧栏立即出现新标记、进程 PID 与启动时间未变。 |
| D2 自家功能插件化 | **通过** | 内置侧栏与插件面板并排显示同样的 3 条会话，搜索结果一致；插件只用公开查询/动作，清单无 fs/net/exec。 |
| D3 安全专项 | **通过**（真机 + 自动化） | 篡改包、降级包、越挡申请、未声明调用均被拒；撤销后拒绝重装/启用；沙箱逃逸以能力面封闭 + 拒绝路径测试覆盖，未做 JS 层对抗性逃逸用例。 |
| D4 稳定性 | **通过**（本地补丁） | 无障碍崩溃由 ADR-0018 消除；看门狗 E742、60s 内两次崩溃自动停用、关闭窗口 5s 内退出、卸载后无实例/进程残留均在真机触发；卸载后资产缓存与信任记录残留登记为 backlog。 |
| D5 接缝 + 契约 | **通过**（本地） | 接缝逐条核对；API v1 有版本与弃用承诺；四道门禁全绿。远端 CI 因未推送未运行。 |
| D6 市场发现 | **基本通过** | 页签、空索引态、刷新按钮、离线保留缓存、收藏/屏蔽持久化有真机或测试证据；默认索引地址为空，无真实索引源，卡片安装未真机走通。 |

## D1 关键证据

- **打包签名链**：临时目录/临时密钥上 `new → check → keygen → certify → pack → verify` 全通；包内 id/版本/build 复核通过；篡改包 `verify` 退出码 2；build 回退拒打包；同源码重复打包指纹不变。
- **安装**：插件页 → 添加插件 → 原生选目录 → 页内向导显示“签名校验已通过：选择运行挡位，挡位不能高于清单申请挡位”；`read-only` 可选，`workspace`/`trusted` 置灰；落盘后已安装行显示 `会话搜索 · B 轨 · 只读 · v1.0.5`，账本 build 7。
- **渲染与交互**：侧栏出现“搜索会话”面板，空查询列出 3 条会话；输入 `parser` 只留 `Refactor the parser module`；输入 `zzz` 显示“没有匹配的会话”；点击行或用回车打开会话；`×` 清空。
- **热更（无重启即时替换）**：面板在场时安装 v1.0.6（build 8，侧栏顶部新增「热更 v1.0.6」标记）：面板立即显示新标记，插件页行显示 `v1.0.6`，宿主进程 PID 17920 与启动时间 11:00:41 未变——是旧实例被新实例替换，不是重启应用。此前 v1.0.3 → v1.0.4 修掉「永远载入中」的首帧条件渲染问题见 P-021。
- **防降级**：build 2 包在 build 6 已接受后被拒（E712），运行中的面板与账本保持不变；最终装回 build 7。

## D2 关键证据

- 内置侧栏与插件面板同时可见，均列出同一组会话（`检查桌面插件的验收用例`、`Fix the session reader...`、`Refactor the parser module`）；搜索 `parser` 两边结果一致。
- 插件清单只申请 `sessions.list`/`sessions.search`/`openSession` 与 `theme.update`，无存储、fs、net、exec；宿主未给它任何内置特权入口。
- 体验差异（已登记为设计偏离）：NSH v1 无方向键/Esc 事件，逐条移动改为点击、Esc 改为 `×`；相对时间由插件本地格式化。

## D3 关键证据

- **篡改**：在包尾追加 1 字节后安装，向导显示“签名校验失败: the signature does not verify”，未落盘。
- **未签名**：`unsigned_bytes_are_rejected`（桌面隔离层）覆盖；`not a valid nsh package: bad magic`。
- **降级**：`refuses_a_downgrade_and_keeps_the_running_build`（E712）+ 真机拒绝 build 2。
- **挡位**：`read_only_tier_rejects_exec_and_network`、`workspace_tier_rejects_exec`、`refuses_a_tier_above_what_the_manifest_declares`；真机向导中越挡按钮置灰。
- **调用越界**：未知/未声明 E740、参数非法 E741、挡位不足 E722；被拒调用写入插件诊断卡（运行期同码同文案合并计数）。
- **撤销**：`a_revoked_plugin_refuses_reinstall_and_enable`（E711）；运行时把已撤销插件视为禁用并给 E711，不标 `Error`。
- **沙箱**：插件能力面只有清单声明过的查询/动作，NSH 不提供 Node fs/net；未做“插件代码直接摸系统 API”的对抗性用例。

## D4 无障碍激活崩溃（已修复：ADR-0018）

**复现（任选其一，均在插件已安装、面板在场时）**

1. 焦点落在面板输入框（清单开启自动聚焦）→ 打开原生“选择插件目录”对话框 → 再触发任意重绘（导航、点“详情”）；
2. 焦点落在面板输入框 → 连接/断开 UIA 自动化客户端 → 再触发任意重绘。

**现象**：进程在绘制阶段 panic，窗口卡死（内存约 260MB），需 `taskkill`；部分轮次残留一个约 40K 的桩进程。

```text
thread 'main' panicked at accesskit_consumer-0.38.0/src/tree.rs:225:21:
TreeUpdate includes duplicate child #10705834630348379779
  accesskit_consumer::tree::State::update
  accesskit_windows::adapter::Adapter::update_if_active
  gpui_fast_windows::window::a11y_tree_update
  gpui::window::Window::draw_roots
```

本轮共 5 次复现（导航、选目录、焦点后连 UIA、安装后点详情、再次点详情），重复 id 每次不同。

**根因分析**：NSH 0.3.0 在无障碍激活且 NSH 元素有焦点时会预置焦点代理节点（`nsh-a11y-focus`）：先用 `take_subtree` 移走焦点元素子树，把代理顶到父节点首位并替换焦点节点 id，代理节点再承载焦点元素的原 children。`accesskit_consumer` 0.38.0 对整包更新里**任何**重复 child id 直接 panic（`tree.rs:225`）。

本轮用 `target/` 下的临时插桩副本（NSH `access.rs` 打印合成树、gpui-fast `repair_tree_update` 打印重复 child 的父节点；验证后已删除、`[patch]` 已回退）做了收敛：

- 重复 id 全部命中 NSH 合成节点 id（`hash(("nsh", surface_key, view_id))`）：`#10705834630348379779`=key1/view18、`#7223334741684448574`=key1/view66、`#5220590099689921369`=key1/view72、`#8742214482047128939`=key2/view42，都是列表行/条目一类的会随数据移动的节点（此前「遍历 NSH 节点 id 未命中」的判断有误，已更正）。
- `accesskit_consumer` 的判重集合 `seen_child_ids` 只遍历当次 `update.nodes`（读源码确认），所以重复**就在这一帧的 TreeUpdate 内部**；`synthetic` 打印只覆盖 NSH 面板子树，看不到窗口级其他元素，故当时误判为「持久树残留」——复验探针在整份更新层面记录到同一帧 3 个重复引用，已更正。
- 触发条件与焦点/数据同时移动有关：面板输入框聚焦、查询变化导致列表行重排（行节点被重新挂接），焦点代理同时重建时，同一 child 被两个父节点同时引用。

**上游版本核查（2026-10-30，本机索引缓存）**：`nsh-platform-gpui` 最新 0.3.0（即当前锁定版本）；`gpui-fast` 最新 0.1.12 的 a11y 代码与 0.1.10 一致，且仍依赖 `accesskit ^0.24` / `accesskit_windows 0.34`（对应 `accesskit_consumer 0.38.0`），`accesskit_windows 0.35` 要求 consumer 0.39，受 gpui-fast 的 caret 约束无法单独升级。**不存在「升级依赖即修复」的路径。**

**影响（修复前）**：D4 失败；**读屏/Narrator 用户打开插件面板后，只要发生重绘就会崩溃**，自动化测试工具也无法在面板在场时操作。这是产品级无障碍缺陷，不是测试环境特例。

**处置与实施**：所有者决定打本地补丁（2026-10-30），决策见 [ADR-0018](../../adr/ADR-0018-vendored-accesskit-consumer-patch.md)。实施：

- `vendor/accesskit_consumer/`（0.38.0 原样副本 + 补丁，`PATCH.md` 记录出处与改动），根 `Cargo.toml` 用 `[patch.crates-io]` 指向它；`workspace.exclude` 使其不参与 fmt/clippy/test 门禁。
- 补丁点：`State::update` 收到 `TreeUpdate` 后先裁掉重复 child 引用（保留首个父节点）并 `log::warn!`，替代 `accesskit_consumer` 的原 panic；附单元测试 `strip_duplicate_children_keeps_first_parent`（`cargo test --manifest-path vendor/accesskit_consumer/Cargo.toml`：**206 passed**）。
- 许可证：上游 MIT OR Apache-2.0，本副本按 MIT 保留 `LICENSE-MIT`。

**复验（真机）**：面板输入框聚焦 + UIA 反复连接/断开 + 查询串逐字变化（`p→pa→…→parser`、`zzz`、`fix`）+ 最小化/还原后重绘。复验用探针版构建记录到 `TIMOA11Y stripped 3 duplicate child reference(s) from a TreeUpdate`——同一份更新在旧版必 panic；补丁版进程存活、无 `panicked`、面板与输入焦点正常。探针已移除，正式补丁只保留 `log::warn!`。

**残余**：补丁只防崩，不修正 NSH 生成的无障碍树（重复帧里重复 child 只保留在首个父节点下，下一帧重建）；仍建议向 NSH/gpui-fast 上游报 issue，上游修复后按 `PATCH.md` 移除本补丁。


**D4 其余子项（本轮真机补齐）**

- **看门狗 E742**：装载不注册 `__ping` 的「静默」插件 v1.0.9（build 11）——面板 11:16:24 出现，容器 11:16:31 前被移除，诊断卡为 `E742 unresponsive: no __pong from the plugin`，主应用照常可交互。
- **实现注记（重要）**：`host.on("__ping")` 里写 `for(;;){}` 验不到看门狗——NSH 0.3.0 对一次 JS 调用有 2s 预算，会先以 `the script was interrupted: a call into JS exceeded its budget of 2000 ms` 中断（同样落 E742，但走的是引擎中断而非看门狗）。看门狗只能由「从不回 `__pong`」的插件触发，验收/回归测试应按此构造。
- **60s 内两次崩溃自动停用**：v1.0.10 与 v1.0.11 在 60s 窗口内各崩溃一次，宿主弹出「插件 session-search 反复崩溃，已自动停用」，插件行变为 `v1.0.11 · 已禁用`，`plugins.json` 中该插件 `enabled=false`；重启应用后仍为「已禁用」、面板不再出现。
- **关闭主窗口**：两轮 `WM_CLOSE` 后进程分别在 **65 ms / 67 ms** 退出（设计 05 §8 要求 5s 内），stderr 无 panic。
- **卸载**：运行中卸载 → 确认框 → 面板消失、已安装列表清空、插件目录删除、`plugins.json` 为 `{}`、`permissions.json` 的 `grants` 为 `{}`；进程线程数 **59 → 45**，无残留桩进程。
- **卸载残留（backlog）**：`plugins/assets/com.timo.session-search/<sha>/`（按版本的内容寻址解包缓存）与 `nsh-trust.json` 的 `builds` 记录在卸载后保留，`sweep_staging` 只清 `.staging`/`.trash`。不影响功能与安全（缓存无实例引用，授权已撤销），但属于残留，登记 backlog。

## D5 接缝与版本契约

- **事件总线**：GUI 只订阅 `KernelEvent`，内核循环无插件分支；B 轨实例事件经 `HostUiEvent` 回 UI。
- **工具元模型**：`toolCard` 只能绑定清单声明过的工具，宿主代理 `runTool` 仍走内核注册表与取消/审计，无第二套执行权。
- **嵌套执行**：插件工具调用与内置调用共用同一注册表与超时；未新增旁路。
- **统一入口**：市场与已安装管理共用内核清单、挡位与生命周期，不按轨分家。
- **API v1**：`docs/design/phase-4/04-host-api-v1.md` 定稿，带版本号与弃用流程；清单 `api` 字段与宿主能力表对齐。
- **隔离**：NSH 类型只出现在 `apps/timo-desktop/src/features/plugins/nsh/`；`crates/` 无 gpui-kit/NSH。
- **门禁**：`cargo fmt --all`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（**526 passed / 0 failed**）、`scripts/check-size` 全绿。远端 CI 未运行（本地改动未推送）。

## D6 市场证据

- 市场页五个页签（发现/主题/收藏/已安装/已屏蔽/高级）、筛选 chip、空态“没有可显示的插件。刷新索引地址”与“刷新索引”按钮均在真机可见。
- 索引默认地址为空（设计默认值）；离线拉取失败保留缓存（`offline_refresh_keeps_the_cache`），缓存往返与损坏回退（`cache_roundtrip_and_corruption`），收藏/屏蔽持久化（`favorites_and_blocked_persist`）有测试。
- 卡片安装：`InstallMarketPlugin` 下载后走与本地安装同一个 `verify_dir` 与挡位向导（代码路径核对 + UI 集成测试），因无真实索引源未真机走通。

## 体量核对（standards/08）

- docs/：新增本篇；总数仍在 ADR-0014 的 120 篇软上限内。
- .agents/：`CONTEXT.md`、pitfalls 均在预算内；handoff 滚动保留最近 5 份。
- 代码：新增 `timo-devkit` 与 slot/运行时子模块；单文件 ≤500 行或已登记豁免；无新增外部依赖。
- 第三方修补副本：`vendor/accesskit_consumer/` 约 412K / 9.9k 行（0.38.0 原样 + 补丁），按 ADR-0018 豁免 500 行与 `check-size` 扫描（脚本只扫 `crates/`、`apps/`），并已在 `workspace.exclude` 排除门禁。
- 构建产物：收尾运行 `scripts/clean-build`，`target/` **18.7GiB → 13.0GiB**（释放 5.6GiB，全为 incremental；deps 11.4GiB 保留）；验收补齐一轮后为 **14.0GiB**（incremental 2.8GiB / deps 9.7GiB），仍低于 15GiB 软上限。

## 规范复查（standards/09）

- 验收项已收口（D1~D5 通过、D6 基本通过）；四问逐篇复查留到所有者确认收口、打 `phase-4.end` 的那一轮，本轮只做门禁、体量与交接核对。

## 已知问题与风险

- **D4 已全部通过**：无障碍崩溃由 ADR-0018 本地补丁消除；看门狗、崩溃循环自动停用、关闭窗口退出、卸载残留均有真机读数。补丁只防崩不修正 NSH 的无障碍树，建议向 NSH/gpui-fast 上游报 issue，上游修复后按 `PATCH.md` 移除。
- **卸载残留**：`plugins/assets/<nsh_id>/<sha>/` 与 `nsh-trust.json` 的 `builds` 记录不随卸载清理，已登记 backlog。
- **看门狗与 JS 调用预算**：2s 预算会先于看门狗中断死循环回调；两者都落 E742，验收应按「不回 `__pong`」构造看门狗用例。
- 生产签名密钥分发策略未定（Phase 4 ADR 待办）。
- 键盘能力缺口（方向键/Esc）与相对时间格式差异，已在设计 07 登记为实现注记。
- 市场无真实索引源；卡片安装只到代码路径与测试级证据。
- 健康门无首帧信号（1s 无错即接受）、`AppView::stop()` 无超时句柄，均为既有实现注记。

## 对下阶段的影响

- 无障碍崩溃已修复（ADR-0018），D4 全部子项取得真机读数；Phase 4 待所有者确认后收口、打 `phase-4.end`。
- 桌面壳视觉精修仍按既定顺序：Phase 4 收口后启动。
- 插件生态与市场在收口前不对外承诺可用性。

## 文档更新清单

- 新增 `docs/design/phase-4/report.md`（本页）、`docs/adr/ADR-0018-vendored-accesskit-consumer-patch.md`。
- 更新 `docs/roadmap/00-roadmap.md`、`docs/roadmap/phase-4-gui-plugins.md`（验收状态）。
- 更新 `CHANGELOG.md`、`docs/README.md`（登记本报告与 ADR）。
- 更新 `.agents/CONTEXT.md`，新增 handoff 与两条 pitfalls（Solid 式条件渲染、NSH 无障碍焦点代理）。
