# H-018 Phase 4 验收补齐：热更、看门狗、崩溃自动停用、退出与卸载

- 日期：2026-10-30
- 主题：补齐 Phase 4 剩余的验收证据，D1~D5 通过、D6 基本通过；`phase-4.end` 待所有者确认。

## 本轮做了什么

- **D1 热更**：面板在场时安装 v1.0.6（build 8，侧栏新增「热更 v1.0.6」标记）；面板立即显示新标记、插件页行 `v1.0.6`，宿主 PID 17920 / 启动时间 11:00:41 未变——无重启即时替换。
- **D4 看门狗**：静默插件 v1.0.9（不注册 `__ping`）→ `E742 unresponsive: no __pong from the plugin`，面板 11:16:24 出现、11:16:31 前移除。发现并登记：`for(;;){}` 回调会先撞 NSH 的 2s JS 调用预算（同为 E742，消息 `Error: interrupted`），看门狗只能由「不回 `__pong`」触发（P-023）。
- **D4 崩溃循环**：v1.0.10 与 v1.0.11 在 60s 内各崩一次 → 提示「插件 session-search 反复崩溃，已自动停用」、`plugins.json` `enabled=false`，重启后仍禁用。
- **D4 退出/卸载**：`WM_CLOSE` 后 65ms / 67ms 退出；运行中卸载后面板消失、目录删除、`plugins.json` 与授权清空、线程 59 → 45、无残留桩进程。残留 `plugins/assets/<id>/<sha>/` 与 `nsh-trust.json` 的 `builds` 已记 backlog。
- **文档**：report、设计 05 注记、两份 roadmap（含 backlog）、CHANGELOG 0.1.119、P-023、`vendor` 构建产物加入 `.gitignore`。

## 遗留与下一步

1. 所有者确认收口 → 打 `phase-4.end`，并做 standards/09 四问逐篇复查。
2. 提交并推送 Phase 4 实现与文档（含 `vendor/accesskit_consumer`），推后核远端 CI（Windows runner）。
3. 上游 NSH/gpui-fast 报 a11y issue；修复后按 `PATCH.md` 移除补丁。
4. 桌面壳视觉精修（所有者定调：Phase 4 收口后启动）。

## 已提取的沉淀

- P-023（NSH 2s JS 预算先于看门狗）；P-022 a11y 补丁沿用。
