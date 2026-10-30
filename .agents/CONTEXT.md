# 工作上下文交接卡

- 更新：2026-10-30（会话主题：**Phase 4 验收补齐——D1~D5 通过、D6 基本通过，`phase-4.end` 待所有者确认**）。

## 当前阶段与状态

- **Phase 1 ✅**（tag `phase-1.end`）；**Phase 2 ✅**（tag `phase-2.end`）；**Phase 3 ✅**（tag `phase-3.end`）。
- **Phase 3.5 ✅**（tag `phase-3.5.end`）；**审查收债 ✅**（台账 `docs/roadmap/audit-debt.md`）。
- **Phase 4（GUI 插件系统）**：设计 00~08 定稿；P0、P1-a~d、P2 ✅。**验收完成（2026-10-30）：D1~D5 通过、D6 基本通过（无真实索引源）**；D4 无障碍崩溃已由 ADR-0018 本地补丁消除，看门狗/崩溃自动停用/退出/卸载均取得真机读数。报告 `docs/design/phase-4/report.md`。**`phase-4.end` 未打**（待所有者确认收口）。
- 验收由 AI 会话代理真机完成（所有者不在场）：临时数据目录 `%TEMP%\timo-accept-data`、临时签名密钥；真实数据未触碰，截图/日志只在本机临时目录。
- 看板：`docs/roadmap/00-roadmap.md` §1；远端 `github.com/qingbird1983/timo-pi`（私有），Phase 4 改动未提交/推送。

## 上一轮完成了什么（验收补齐）

- **D1 通过**：面板在场时安装 v1.0.6（build 8），侧栏立即出现「热更 v1.0.6」、插件行 `v1.0.6`；宿主 PID 17920 / 启动时间 11:00:41 未变 → 无重启即时替换。
- **D4 看门狗**：静默插件 v1.0.9（不注册 `__ping`）→ `E742 unresponsive: no __pong from the plugin`，面板 11:16:24 出现、11:16:31 前移除；主应用可交互。
- **D4 崩溃循环**：v1.0.10 与 v1.0.11 在 60s 内各崩一次 → 「插件 session-search 反复崩溃，已自动停用」、`plugins.json` `enabled=false`，重启后仍禁用。
- **D4 退出/卸载**：`WM_CLOSE` 后 65ms / 67ms 退出；运行中卸载后面板消失、目录删除、`plugins.json` 与授权清空、线程 59 → 45、无残留桩进程。
- **文档**：report、设计 05 注记、两份 roadmap（含 backlog）、CHANGELOG 0.1.119、P-023、`.gitignore`（vendor 构建产物）。

## 下一步（按优先级）

1. **打 `phase-4.end`**（所有者已确认收口，2026-10-30）；standards/09 四问复查已完成并写入报告。
2. **推送后核远端 CI**（Windows runner）；CI 绿即收口完成。
3. **上游 issue**：向 NSH/gpui-fast 报无障碍重复 child；上游修复后按 `PATCH.md` 移除补丁。
4. **桌面壳视觉精修**（所有者定调：Phase 4 收口后启动）：research/07 → 设计稿 → 批次实现。
5. docs 实测篇数与整合评审待办（删除面需所有者确认）。

> 施工顺序唯一来源：当前阶段设计文档；设计错误先改文档再写码。

## 未决问题

- **卸载残留**：`plugins/assets/<nsh_id>/<sha>/` 与 `nsh-trust.json` 的 `builds` 不随卸载清理，已入 roadmap backlog。
- 生产签名密钥分发策略未定。
- 市场无真实索引源；卡片安装只有代码路径 + 测试证据。
- 键盘能力缺口：NSH v1 无方向键/Esc（已降级为点击/回车/`×`）。
- 相对时间由插件本地格式化；`composerControl` 只校验不实例化。
- 健康门无首帧信号（1s 无错即接受）；`AppView::stop()` 无超时句柄。
- 内核忙时插件请求排队，过长撞 3s/30s 超时。
- ADR-0018 补丁只防崩，不修正 NSH 的无障碍树。

## 关键提醒

- **D4 补丁**：`vendor/accesskit_consumer/` 只经 `[patch]` 生效且排除门禁；升级该 crate 按 `PATCH.md` 重放补丁，并复验「面板 + UIA + 重绘」。`vendor` 的 `target/`、`Cargo.lock` 已忽略。
- **验收数据目录**：`TIMO_AGENT_DIR=%TEMP%\timo-accept-data`；永不读取/打印私钥，只传路径。
- **内核零 GUI**：`crates/` 不得出现 gpui-kit/NSH；NSH 类型只允许在 `apps/timo-desktop/src/features/plugins/nsh/`。
- **NSH app 目录 = 入口目录**（P-020）；**组件体只跑一次，条件写进 JSX**（P-021）；**a11y 焦点代理崩溃**（P-022）；**2s JS 预算先于看门狗**（P-023）。
- **NSH 信任流程**：先 `verify_package`、健康后 `accept_package`；两种信任别混（ADR-0015）；B 轨验签在桌面、落盘在内核；撤销 = 禁用 + E711。
- **数据目录分层**：`<data>/plugins/<id>/storage.json` 与 `<id>/nsh` 分开（P-018）。
- **宿主线程只入队**；UI 副作用走 `HostUiEvent`；运行期「失败即记住」；slot 数据只给本实例。
- **版本锁**：gpui-kit 0.8.0 / gpui-fast 0.1.10 / NSH 0.3.0；accesskit_consumer 0.38.0（**本地补丁**，ADR-0018）。
- **覆盖层快捷键**：Sheet/Dialog 焦点陷阱（P-009）；UI 测试禁用 `use gpui_kit::*`（P-007）、glob 遮蔽 `#[test]`（P-010）。
- **门禁四连**：fmt / clippy `-D warnings` / `cargo test --workspace` / `check-size`；本轮 **526 tests**；`target/` 13.0GiB（软 15 / 硬 25）。
- **收尾四件事**：重写本卡、归档 handoff、沉淀 pitfalls/skills、体量核对。
- **写中文文件后扫隐形字符**：`grep -nP '[\x{3040}-\x{30ff}\x{200b}-\x{200f}\x{feff}]'`。
- 踩坑索引：P-001 流事件静默回归；P-002 编辑重复行；P-003 Windows bash 是 WSL；P-004 上游摘要当合同；P-005 模块格式与运行时不符；P-006 依赖解析成功 ≠ API 兼容；P-007 gpui-kit 测试 glob 导入；P-008 `impl IntoElement` 生命周期捕获；P-009 sheet 焦点陷阱；P-010 gpui glob 遮蔽 `#[test]`；P-011 kill 后孙进程握着管道；P-012 会话半行拼接；P-013 Git Bash 改写 `/命令` 参数；P-015 主动中断的原因必须覆盖收尾错误；P-016 错误隔离会改变派发返回值；P-017 WSL 验 Linux 的假信号与去重缺陷；P-018 插件存储根与 NSH 数据目录别混用；P-019 解释器脚本重写源文件会先写后错；P-020 NSH 的 app 目录 = 入口文件所在目录；P-021 NSH 组件体只执行一次；P-022 NSH 无障碍焦点代理重复 child；P-023 NSH 2s JS 预算先于看门狗。
