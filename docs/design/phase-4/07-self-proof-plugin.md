# 设计 07：自证插件（会话搜索）

- 状态：定稿（2026-10-28）
- 关联：roadmap/phase-4 D2；design/phase-4/00 K11、03、04、05
- 选型：**会话搜索面板**（`read-only` 挡位）。若要换 todo/工具卡片，只替换本篇，00~06 不变。

## 1. 目标

把「搜索历史会话」做成一个 B 轨插件，与内置功能体验一致，并且**不依赖任何内置特权**：只用 v1 公共的 `panel` slot、只读查询与 `openSession` 动作。

证明三件事：

1. panel slot 能承载真实交互面板；
2. 只读查询足够做出实用功能；
3. 插件被禁用/卸载后入口与数据彻底消失，主应用行为不变。

## 2. 交互设计

入口：侧栏会话列表下方的 `panel`（标题「搜索会话」，随插件启用出现）。

```text
┌ 搜索会话 ───────────────┐
│ [ 输入关键词…        ]  │
│ ─────────────────────  │
│ Phase 4 插件设计        │
│ 刚刚 · 12 条 · timo-pi  │
│ 审查收债                │
│ 今天 · 48 条 · timo-pi  │
│ …                       │
└─────────────────────────┘
```

- 输入 ≥1 字符后 200ms 防抖调用 `sessions.search({query, limit: 20})`。
- 结果按 `updatedAt` 降序；显示标题、相对时间、消息数、`cwdLabel`。
- 键盘：↑/↓ 移动、Enter `actions.openSession`、Esc 清空并回到会话列表。
- 状态：空查询显示最近会话（`sessions.list({limit:10})`）；无结果显示「没有匹配的会话」；错误显示 E74x 与重试。
- `sessions.changed` 到达时，若有查询则静默重跑；结果列表保持滚动位置。
- 不做：全文正文搜索、跨工作区筛选、会话删除/重命名（超出只读范围）。

> 实现注记（2026-10-29）：NSH v1 没有方向键/Esc 事件（`View`/`Input` 只有 `onPress`/`onSubmit`），键盘只保留 **Enter 打开首条结果**；逐条移动改为点击行，Esc 清空改为输入框右侧的「×」。相对时间由插件本地格式化（宿主只返回 `updatedAt` 毫秒时间戳）；与内置会话行同一文案层级，但不共享函数——要完全一致需在宿主 API 增加格式化查询，留给后续版本。

## 3. 权限与清单

```json
{
  "schema": 2,
  "id": "session-search",
  "kind": "gui",
  "name": "会话搜索",
  "version": "1.0.0",
  "nsh_id": "com.timo.session-search",
  "entry": "App.tsx",
  "bundle": "plugin.nshpkg",
  "source_hash": "sha256:<64hex>",
  "bundler": "timo-devkit/0.1",
  "api": 1,
  "tier": "read-only",
  "slots": [{ "kind": "panel", "title": "搜索会话", "icon": "search", "order": 40 }],
  "queries": ["sessions.list", "sessions.search"],
  "actions": ["openSession"],
  "tools": [],
  "capabilities": { "exec": false, "net": { "domains": [] }, "fs": { "read": [], "write": [], "delete": [] } }
}
```

- 安装向导只允许 `read-only`（申请即上限）。
- 运行时未声明任何 fs/net/exec；工具列表为空。

## 4. 数据与内核支持

- 新增内核查询：`Kernel::search_sessions(query: &str, limit: usize) -> Vec<SessionInfo>`。
  - 数据源：`Kernel::list_sessions_at` 的 `SessionInfo`（id、cwd、modified_ms、message_count、first_message）。
  - 匹配：标题/首条消息的大小写不敏感子串；结果截断到 `limit`。
  - 不读消息正文、不跨进程、不改会话文件。
- B 轨 handler 在 `plugins` 模块把 `sessions.search` 映射到该查询；返回 04 §2 的脱敏字段。
- `openSession` 由桌面路由处理：切换到会话 id，关闭当前覆盖层。

## 5. 与内置体验的一致性

| 维度 | 要求 |
|---|---|
| 视觉 | 使用宿主主题 token；字体/间距与侧栏会话行一致；不自定义滚动条 |
| 交互 | 键盘可用；输入框焦点与主应用一致；Esc 退出 |
| 性能 | 1 万条会话元数据搜索 < 50ms（内存过滤）；结果首屏 < 100ms |
| 空态/错误 | 与内置列表同一套文案层级；错误可重试 |
| 禁用 | 面板立即消失，侧栏恢复原状；无残留线程/存储 |

## 6. 验收（D2/D3）

- 按 06 的 devkit 流程从源码打包、签名、安装、授权、显示、热更，全程可复现。
- 自证插件与内置侧栏在相同主题/尺寸下并排截图，行高、字号、间距无肉眼差异。
- 未声明查询被拒：把 manifest 的 `sessions.search` 删掉后，调用返回 E740，插件显示错误态。
- 篡改包/未签名包无法安装；撤销授权后面板 1s 内消失。
- 卸载后 `plugins/session-search/` 与授权记录均不存在；重启后入口不出现。

## 7. 风险

- 会话量大时 `list_sessions` 全量扫描：先在内存过滤，P2 若超 50ms 再加索引（不改协议）。
- 相对时间与主应用格式不一致：复用同一格式化函数（不引入第二套）。
- 面板与内置侧栏争空间：`panel` 放在会话列表下方，可折叠；默认展开宽度与侧栏一致。
