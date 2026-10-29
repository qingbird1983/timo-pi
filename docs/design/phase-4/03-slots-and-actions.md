# 设计 03：slot 与动作词汇表（v1）

- 状态：定稿（2026-10-28）
- 关联：roadmap/phase-4；ADR-0004；architecture/04 §4；design/phase-4/02、04
- 原则：宿主只暴露**有限槽位**与**有限动作**；未声明、未授予、未知一律拒绝（fail-closed）。

## 1. slot 白名单（v1）

| slot | 位置 | 输入（宿主注入） | 插件输出 | v1 状态 |
|---|---|---|---|---|
| `panel` | 侧栏停靠面板 | `panelId`、`width`、主题 token | 根组件树 | 实现 |
| `toolCard` | 聊天内工具调用卡片 | `toolName`、`status`、`input`、`output`（已脱敏） | 卡片内容 | 实现 |
| `composerTrigger` | 输入区上方按钮/菜单项 | `text`（当前输入快照，需 `composer.text` 查询） | 按钮/菜单项 | 实现 |
| `composerControl` | 输入区右侧控件 | `text`、`model` | 单行控件 | 仅声明，v1 不渲染 |
| `entryExtra` | 会话条目附加区 | `sessionId`、`title` | 徽标/短文本 | 实现 |
| `theme` | design-token 覆盖 | 当前主题 token | token 补丁表 | 实现（白名单消毒） |

- 每个 slot 实例独立挂载一个 NSH `AppView`；同一插件可声明多个 slot。
- `panel` 最多同时显示 5 个（按 `order` 升序），超出拒绝并记 E751。
- slot 容器由宿主绘制：尺寸、滚动、边框、标题栏归宿主；插件只画内容。
- 禁用/卸载/撤销授权时，宿主先卸载对应 AppView，再移除容器。

## 2. 主题 token 消毒

`theme` 只允许覆盖以下类别的 token，其余键忽略并记诊断：

- 颜色：`background`、`foreground`、`card`、`border`、`primary`、`muted`、`accent`、`sidebar*`
- 圆角：`radius-sm/md/lg`
- 字号：`text-xs/sm/base/lg`
- 间距：`space-1..6`

禁止：布局（`display`/`position`/`grid-*`）、动画时长、阴影、字体族、任何 `url(...)` 或选择器注入。值必须是合法颜色/数字，字符串长度上限 64。

## 3. 动作词汇表（v1）

| 动作 | 参数 | 最低挡位 | 说明 |
|---|---|---|---|
| `notify` | `level`（info/warn/error）、`message` | ReadOnly | 应用内通知；不弹系统通知 |
| `openSession` | `sessionId` | ReadOnly | 切换到已有会话 |
| `openPath` | `path` | Workspace | 用系统默认应用打开；限制在授权读作用域内 |
| `sendPrompt` | `text`、`submit=false` | Workspace | 默认只插入输入框；`submit=true` 需 Trusted |
| `runTool` | `name`、`args` | 见 01 §8 | 宿主代理执行；受 `tools` 声明与注册表约束 |
| `openPanel` | `panelId` | ReadOnly | 打开插件自己的面板 |
| `clipboardWrite` | `text` | ReadOnly | 写剪贴板，长度 ≤ 64KB |
| `storage.set` | `key`、`value` | ReadOnly | 写插件私有存储；限额见 04 §4 |

- `storage.get` 在查询面（04 §2），`storage.set` 在这里：读写分属两个白名单，插件要同时声明两项。
- `theme.update` 不是 call，而是插件用 `host.post("theme.update", tokens)` 提交主题补丁；宿主按 §2 消毒后应用，越界键记 E743。

- 动作必须在 manifest `actions` 中声明；`runTool` 还必须列在 `tools` 中。
- 未声明 → E740；挡位不足 → E722；未知动作名 → E740；参数非法 → E741。
- 写类工具（write/edit/bash）经 `runTool` 时仍走内核注册表与取消令牌；全局审阅开关开启时才弹确认（01 §8）。
- 动作调用串行排队，单次超时：`runTool` 30s，其余 3s；超时返回 E742，不重试。

## 4. 渲染约束

- 插件 UI 只能使用 NSH 的基础元素与宿主注入的组件词汇；不允许 WebView、外链脚本、`eval`。
- 图片/资源只能引用包内 `asset:/` 路径；网络图片需 `net` 能力且域名在白名单。
- 文本输入受宿主 IME/焦点管理；插件不能抢全局快捷键。
- 工具卡片内容高度上限由宿主裁剪（超出滚动），插件不得修改聊天行布局。

## 5. 越界与诊断

| 情况 | 结果 | 诊断码 |
|---|---|---|
| slot 不在白名单 | 拒绝挂载 | E750 |
| 同 slot 超出数量 | 拒绝该实例 | E751 |
| 动作未声明/未知 | 抛错给插件 | E740 |
| 动作参数非法 | 抛错给插件 | E741 |
| 动作超时 | 抛错给插件 | E742 |
| 主题键越界 | 忽略该键 | E743（警告） |

- 诊断进入插件详情的「诊断」卡，保留最近 50 条；不写日志文件、不默认展示堆栈。
- 同一错误连续 10 次只保留首条与计数，避免刷屏。

## 6. 验收断言

- 声明 `panel` 的插件只出现在侧栏，不占用聊天区。
- `toolCard` 实例收到的是脱敏后的工具结果，拿不到系统提示/密钥。
- 未声明 `notify` 的插件调用通知 → E740，插件捕获后界面可继续使用。
- `theme` 提交 `display:none` 或 `url(...)` → 被忽略并产生 E743，主界面样式不变。
- 5 个 panel 之外的第 6 个 → E751，前 5 个正常。
- 禁用插件后，所有 slot 容器与 AppView 在 1s 内消失，宿主无残留线程。
