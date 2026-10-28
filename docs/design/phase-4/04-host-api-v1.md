# 设计 04：TSX 宿主 API v1

- 状态：草稿（2026-10-28）
- 关联：roadmap/phase-4；ADR-0004；design/phase-4/02、03、05
- 通道事实：NSH `nsh/host`（`host.params` / `host.on` / `host.call` / `host.post`），Phase 3 PoC 已验证 `AppView` 可嵌入

## 1. 入口约定

插件 TSX 默认导出一个根组件；宿主为**每个 slot 实例**创建一个 `AppView`，启动参数：

```ts
export default function App() {
  const { plugin, slot, api } = host.params;
  // slot.kind: "panel" | "toolCard" | ...
  // slot.data: 该槽位的只读上下文
  return <Panel />;
}
```

- `host.params` 在启动时同步可读：`{ api, plugin: {id,version}, slot: {kind, instanceId, data}, theme, locale }`。
- 插件不能选择自己的 slot；实例由宿主按 manifest 创建。
- 事件：`host.on("theme.changed", ...)`、`host.on("sessions.changed", ...)`。
- 调用：`await host.call("sessions.search", { query, limit })`；错误以 rejected Promise 返回 `{code, message}`。

## 2. 查询（只读，需在 `queries` 声明）

| 名称 | 参数 | 返回（脱敏） | 限时 |
|---|---|---|---|
| `sessions.list` | `{limit≤100, offset?}` | `[{id,title,updatedAt,cwdLabel,messageCount}]` | 3s |
| `sessions.search` | `{query,limit≤50}` | 同上（按标题/首条消息匹配） | 3s |
| `sessions.get` | `{id}` | `{id,title,updatedAt,cwdLabel,messageCount}`，不含消息正文 | 3s |
| `models.list` | — | `[{provider,id,label,contextWindow}]`，不含密钥 | 3s |
| `theme.get` | — | 当前主题 token（只读） | 1s |
| `plugin.info` | — | `{id,version,tier,slots}` | 1s |
| `storage.get` | `{key}` | `{value}` 或 `null` | 1s |

- 返回一律深拷贝；不含绝对路径（`cwdLabel` 只是最后一级目录名）、系统提示、密钥、完整消息正文。
- 未声明 → E740；挡位不足 → E722；参数 schema 失败 → E741；超时 → E742。

## 3. 动作（写操作，需在 `actions` 声明）

完整词汇表与挡位见 03 §3。`actions.runTool` 额外要求 manifest `tools` 列出工具名，且：

```ts
await host.call("actions.runTool", {
  name: "read",
  arguments: { path: "src/main.rs" },
});
```

- 宿主代理执行：参数校验 → 权限（声明 ∩ 挡位 ∩ 注册表）→ 内核工具管线（取消/审计）→ 返回 `{content, isError}`。
- 写类工具默认不逐次审阅；全局审阅开关开启时才弹确认（01 §8）。
- 插件卸载/禁用时，进行中的 `runTool` 收到取消（E742），宿主侧不再等待。

## 4. 存储

- `storage.get` / `storage.set`：按插件 id 隔离的 KV，存 `<data_dir>/plugins/<id>/storage.json`。
- 键：`[a-zA-Z0-9._-]{1,64}`；值：JSON，单条 ≤ 64KB，总量 ≤ 1MB；超限 E744。
- 不提供枚举/删除（v1 只读写）；卸载时随目录删除。

## 5. 事件

| 事件 | 负载 | 触发 |
|---|---|---|
| `theme.changed` | `{appearance, tokens}` | 主应用切换明暗/主题 |
| `sessions.changed` | `{ids}` | 会话新建/删除/重命名（节流 500ms） |

- v1 不推送 token 流、工具结果或消息正文；需要时由插件主动查询。
- 事件回调在引擎线程执行，必须快速返回；阻塞会触发看门狗（05 §4）。

## 6. 权限与错误

- 每次 call 在宿主侧做三重检查：`queries/actions` 声明、挡位上限、参数 schema。
- 错误码：E740 未声明/未知调用；E741 参数非法；E742 超时或取消；E743 主题键越界（警告）；E744 存储超限；E722 挡位不足。
- 错误不默认弹宿主对话框；插件负责在自己的 UI 里展示。连续 10 次相同错误只记一条诊断。

## 7. 版本契约

- `api: 1` 为主版本；新增查询/动作/事件用次版本（manifest `api` 仍为 1，宿主按能力表暴露）。
- 破坏性变更升主版本；旧主版本至少保留两个次版本周期，并在插件详情提示「即将弃用」。
- 宿主能力表（`plugin.info.capabilities`）让插件在运行期探测可用查询/动作。
- 弃用动作先返回警告字段 `{deprecated: "..."}`，再在下一主版本移除。

## 8. SDK 与脚手架

- `@timo/plugin-sdk`：只含类型定义与 `host.call` 薄封装，编译期打包进插件，不进入运行时依赖。
- `timo-devkit new <id>` 生成 TSX 模板、`timo-plugin.json` 与打包脚本（06 详设）。
- 示例：`examples/plugins/session-search` 使用 `sessions.search` + `panel`，作为 v1 的活文档。

## 9. 验收断言

- 未声明 `sessions.search` 的插件调用被拒（E740），插件捕获后仍可渲染。
- `sessions.list` 返回不含消息正文、密钥或绝对路径；`cwdLabel` 只有最后一级目录名。
- `runTool("write")` 在 `read-only` 挡位被拒（E722）；在 `trusted` 挡位成功并产生审计记录。
- 主应用切主题后 500ms 内插件收到 `theme.changed` 且渲染更新。
- `storage.set` 超过 1MB 返回 E744，旧值不变。
- SDK 类型与宿主能力表一致（CI 用一处常量生成并比对）。
