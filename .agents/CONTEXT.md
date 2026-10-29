# 工作上下文交接卡

- 更新：2026-10-29（会话主题：**Phase 4 P1-c**——slot 运行时、TSX 宿主 API v1、健康门/看门狗与市场安装接线）。

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`，报告 `docs/design/phase-3/report.md`）。
- **Phase 3.5 ✅**（tag `phase-3.5.end`）；**审查收债 ✅**（台账 `docs/roadmap/audit-debt.md`）。
- **Phase 4（GUI 插件系统）**：设计 00~08 定稿；**P0 ✅**、**P1-a ✅**（安装账本）、**P1-b ✅**（NSH 常驻 + 真签名包）、**P1-c ✅**（本轮的运行时与宿主 API v1）。剩余 **P1-d**：slot 容器渲染（侧栏 panel、工具卡、会话条目附加区）与诊断面。
- 三个设计默认值：市场索引地址默认留空（首次配置，不伪造官方地址）；评论只外链；实验性组 v1 为空。
- 看板：`docs/roadmap/00-roadmap.md` §1；桌面壳已知限制见 `docs/roadmap/phase-3-desktop-shell.md` §6。
- 远端：`github.com/qingbird1983/timo-pi`（私有）。

## 上一轮完成了什么

- **宿主 API v1 接线**：每个实例一个 `HostChannel` + 独立宿主线程；查询/动作按「名字 → 声明 → 参数」fail-closed（E740/E741），挡位裁决在内核注册表侧完成（E722），查询 3s / `runTool` 30s 超时（E742）。`models.list` 与 `runTool` 经桥接通道回内核，走同一审阅/取消/审计管线。
- **存储与主题**：每插件 `storage.json`（单值 64KB / 总 1MB，E744，临时文件 + rename）；`theme.update` 只收合法 token，越界键产生 E743 诊断；主题补丁覆盖到实例与 `theme.changed`。
- **运行时**：一个 `panel` 一个 `AppView`，最多 5 个（E751）；排序后装载；1s 健康门通过才 `accept_package`；`__ping/__pong` 看门狗（2s 一轮、2 次失联停）；崩溃记 E742 并**不再自动重载**，60s 内 2 次自动禁用；`sessions.changed` 500ms 节流；退出统一 `stop()`。
- **市场安装接线**：市场卡片「安装」不再禁用——下载 → 验签（`verify_dir`）→ 挡位确认卡 → 内核安装账本；UI 测试断言点击确实发出 `InstallMarketPlugin`。
- **体量**：`host_api.rs` 拆出 `host_worker.rs`，`runtime.rs` 拆成 `runtime/{mod,load,events}.rs`，内核 `host/mod.rs` 拆出 `path.rs`，全部 ≤500 行。

## 下一步（按优先级）

1. **Phase 4 P1-d**：slot 容器渲染与诊断面——主窗口承载 `AppView`（侧栏 panel 起步），插件详情展示 E7xx 诊断；容器出现前不扩大 slot 种类。
2. **桌面壳视觉精修**（所有者定调：先功能入口，视觉后置）：research/07 → 设计稿 → 批次实现；Phase 4 收口后启动。
3. **推送与远端核验**：P0~P1-c 的本地提交尚未 push，推送后必须核远端 CI 为绿（Windows runner 需 C 工具链）。
4. docs 实测篇数与整合评审待办（删除面需所有者确认）。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- **健康门无首帧信号**：NSH 0.3.0 无公开首帧回调，当前是 `run_package` 后 1s 无 `on_app_error` 即接受（已写进设计 05 §3 实现注记）；平台提供首帧信号后收紧。
- **线程回收未做有界等待**：设计 05 §7 的「stop 后 3s 未退出标 Leaked」尚未实现；随 P1-d 容器/诊断面一起补。
- **内核忙时插件请求排队**：agent run 期间 `models.list`/`runTool` 等到 run 结束才执行，过长会撞 3s/30s 超时（设计 04 §3 实现注记）；并发执行需内核改造，留后续评估。
- NSH 渲染/热更为人工验收（设计 03 §8 允许），CI 只跑「未签名拒绝」。
- 生产签名密钥分发策略未定，留 Phase 4 ADR。
- 桌面壳遗留：模型热切换、设置·扩展页、会话搜索、超大 diff 外部编辑器、trace 全文入口、审阅开关不重放（roadmap phase-3 §6）。
- 无头 stderr 噪音（`.agents/skills/` frontmatter 告警）留资源治理批次。

## 关键提醒

- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；桌面只 import `timo-kernel`。
- **NSH 类型只允许出现在** `apps/timo-desktop/src/features/plugins/nsh/`。
- **NSH 信任流程**：先 `verify_package`、健康后再 `accept_package`；不要用 `load_package`。
- **两种信任别混**：扩展能力授权与 NSH 包信任互相独立（ADR-0015）。
- **B 轨验签在桌面、落盘在内核**：内核只吃 `PackageReceipt`；「撤销授权」= 已禁用 + E711，不要标 `Error`。
- **数据目录分层**：插件存储固定 `<data>/plugins/<id>/storage.json`（`PluginStorage` 吃内核数据根）；NSH 私有状态在 `<data>/plugins/<id>/nsh`。别把两者当一个字段传（P-018）。
- **宿主线程只入队**：`HostChannel` handler 在引擎线程，查询/存储/工具在独立线程；UI 副作用一律 `HostUiEvent` → 渲染帧应用。
- **运行期“失败即记住”**：崩溃/看门狗失败写 `failed`，同版本不再重载；用户关/开插件才清（设计 05 §2「不自动重启」）。
- **版本只认 registry 源码签名**：锁 gpui-kit 0.8.0 / gpui-fast 0.1.10 / NSH 0.3.0；升级前重跑编译探针。
- **覆盖层快捷键**：Sheet/Dialog 的焦点陷阱会让 App 根级 action 失效（P-009）。
- **UI 测试禁用 `use gpui_kit::*`**（P-007）；子模块普通 `#[test]` 不要被 glob 带进 `test` 宏（P-010）。
- **门禁四连**：fmt / clippy `-D warnings` / `cargo test --workspace` / `check-size`；全量测试慢，用 `CARGO_BUILD_JOBS=1`。
- **门禁**：workspace **495 tests** 全绿（P1-c 新增：宿主主题消毒、panel 排序与上限、看门狗状态机、市场安装按钮命令断言等）。
- **构建缓存按容量治理**：`target/` 15GiB 软 / 25GiB 硬；incremental >4GiB 先删；阶段收尾跑 `scripts/clean-build`；无 sccache 不全清。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱；P-010 gpui glob 遮蔽 `#[test]`；P-011 kill 后孙进程握着管道；P-012 会话半行拼接；P-013 Git Bash 改写 `/命令` 参数；P-015 主动中断的原因必须覆盖收尾错误；P-016 错误隔离会改变派发返回值；P-017 WSL 验 Linux 的假信号与去重缺陷；P-018 插件存储根与 NSH 数据目录别混用。
