# 设计 05：B 轨运行时与隔离

- 状态：草稿（2026-10-28）
- 关联：roadmap/phase-4；ADR-0004；design/phase-3/03 §4/§9；design/phase-4/01、02、04
- 事实：`AppView::stop()` 会中断 JS；`on_app_error` 回调可捕获应用级错误（Phase 3 PoC 已用）

## 1. 实例模型

- **一个 slot 实例 = 一个 `AppView`**（独立引擎线程）。同一插件声明 `panel` 与 `toolCard` 时各有一个实例。
- 同一插件的多个实例不共享 JS 状态；共享数据只经宿主查询（04）读取。
- 单插件实例上限 4 个（`panel` 最多 1、`toolCard` 每个工具一个、`entryExtra` 每会话一个）；超出 E751。
- 每个实例有独立 `HostChannel`；宿主在 channel 上注册该插件的查询/动作 handler，卸载时移除。

## 2. 生命周期

```text
NotLoaded → Loading → Running → Stopping → Stopped
                 │         │
                 └─ Error ←┘（崩溃/看门狗/健康检查失败）
```

| 阶段 | 行为 |
|---|---|
| Loading | `Bundle::from_package` → `AppOptions`（host、theme、policy）→ `AppView::new` |
| Running | 首帧 + 1s 内无 `on_app_error`；容器显示；看门狗启动 |
| Error | 记录诊断、移除容器、保留目录与授权；不自动重启 |
| Stopping | `stop()` → drop → 有界回收（3s）→ 标记 Stopped |

- 更新时新实例先 Loading，健康后与旧实例原子交换（02 §6）；失败保留旧实例。
- 禁用 = Stopping；启用 = 重新 Loading。撤销授权 = Stopping + 删 Grant。
- 主应用退出：先对所有实例 `stop()`，再关窗；不等引擎线程无限期。

## 3. 健康检查与崩溃

- 启动健康检查：首帧回调 + 1s 内无 `on_app_error`（沿用 Phase 3 PoC）。
- `on_app_error`：记录 `{message, kind}`（脱敏，不含堆栈默认展示）；执行 `stop()`；实例进入 Error。
- 同一插件 60s 内 Error ≥ 2 次：自动禁用并提示「插件反复崩溃，已停用」。
- 加载失败（`Bundle::from_package`/`AppView::new`）：不创建容器，旧实例不变。

## 4. 看门狗（死循环与无响应）

- SDK 在每个实例注册 `__ping` 事件：收到后立即 `host.post("__pong", {t})`。
- 宿主每 2s 发一次 `__ping`，等待 1s；连续 2 次无 pong → `stop()` → Error。
- `stop()` 后 3s 内实例下线；若引擎线程仍未退出，标记 `Leaked` 并继续（UI 不等待），下次加载新实例。
- 看门狗只针对插件主线程；工具调用等宿主侧等待单独超时（04 §3）。
- 插件在 `host.on` 回调里长时间同步计算也会被判定无响应——这是预期，SDK 文档要求回调快速返回。

## 5. 资源与限制

- 实例数与单次查询限额见 01/04；存储 1MB；单次 `runTool` 30s。
- NSH 引擎不提供内存/CPU 硬配额；本阶段以「单实例 + 看门狗 + 事件节流」控制影响面。
- 进程级隔离（独立进程/沙箱）不在 Phase 4 范围；若后续需要，先立 ADR 再改运行时。
- 事件 `sessions.changed` 宿主侧节流 500ms；插件不得轮询（v1 无定时器 API 承诺）。

## 6. 主题、焦点与 IME

- 宿主切主题：`set_appearance` + `set_theme`，同时 `host.send("theme.changed")`；Phase 3 已验证跟随。
- 窗口 `inactive_frame_interval: None`（NSH 要求，Phase 3 已设）。
- 插件输入框获得焦点时主应用快捷键不响应（预期）；`Esc` 先由插件处理，未处理则宿主收回焦点。
- AppView 只画在自己的 bounds 内；主应用 Dialog/Sheet 仍在其上方（Phase 3 已验证）。

## 7. 卸载与残留

```text
stop() → drop(AppView) → 有界等待引擎线程 3s → 删目录 → 删 state/Grant
```

- 3s 内线程未退出：目录仍删除，线程标记 Leaked 并记录（进程退出时回收）。
- 卸载后检查：无该插件 id 的实例、channel handler、容器、存储文件。
- 安装/更新暂存目录失败时清理 `.staging`/`.trash`；启动时清扫超过 24h 的暂存。

## 8. 验收断言

- 插件启动即抛错：1s 内回退，主应用可用；诊断含错误消息（无堆栈）。
- `while(true){}`：3 次看门狗（≤5s）内容器移除，插件进入 Error，主应用交互正常。
- 连续两次崩溃：插件自动禁用，重启后仍为禁用。
- 卸载正在运行的插件：目录/授权/状态清空，实例消失；30s 内进程无该插件线程。
- 更新失败：旧实例继续显示，诊断给出 E730/E711，授权不变。
- 关闭主窗口：所有实例 stop，进程 5s 内退出。
