# 架构 05：会话磁盘格式（定稿）

- 状态：**定稿**（2026-10-26；实现级细节与测试计划见 design/phase-1/04、07）
- 日期：2026-10-26
- 关联：ADR-0007；architecture/02/03；design/phase-1/04

## 1. 设计目标（最终形态，一步到位）

1. **版本化**：每条 entry（`v`）与会话首条 header（`format_version`）都带格式版本。
2. **树形**：entry 以 `parent_id` 链接，天然支持分叉/多分支（MVP 只写线性链，结构不省略）。
3. **可扩展 payload**：entry 类型可增；**读取端遇到未知 entry 类型必须跳过且不报错**（向前兼容铁律）。
4. **append-only + 崩溃安全**：只追加写入；半条写入（kill -9）在下次读取时丢弃，不损坏已写入内容。
5. **人类可读**：JSON Lines，文本编辑器可直接打开检查。

## 2. 存储布局（定稿）

```
<数据目录>/sessions/<session-id>.jsonl     会话主体 = 唯一权威（append-only entry 流）
```

- 数据目录：Windows `%APPDATA%\timo-pi` / macOS `~/Library/Application Support/Timo-pi` / Linux `$XDG_DATA_HOME/timo-pi`；`TIMO_AGENT_DIR` 与 settings `session_dir` 可覆盖。
- 草案中的 `<id>.meta.json` 与 `index.json` **删除**：元信息并入首条 `session.header`；会话列表 = 扫描 `.jsonl` 文件头（单文件读取上限 1MB）。单一权威 = entry 流本身。

## 3. Entry 信封（定稿）

```json
{"v":1,"id":"01J9XYZ...","ts":"2026-10-26T03:21:00.512Z","parent_id":null,
 "type":"message","payload":{ }}
```

- `id`：ULID（字典序=时间序；进程内单调工厂同时解决时钟回拨）。
- `parent_id`：父 entry；根为 null。
- `type` 命名空间：`session.*`、`message`、`model.*`、`custom.*`。
- `custom.*` 即 pi `appendEntry` 的对应物（A 轨 v2 成员落点，结构先占位）。

## 4. v1 entry 类型（定稿清单）

| type | payload 要点 |
|---|---|
| session.header | 首条：format_version、session_id、created_ts、cwd、parent_session_id?、initial_model? |
| message | 完整消息（role 判别 user/assistant/tool_result/system；assistant content 含 ToolCall 块；tool_result 为独立 role）——同一事实单处表达 |
| model_change | provider、model_id |
| thinking_level_change | level |
| usage | 轮次记账快照（kind、provider、model、usage） |
| custom | 扩展私有状态（不入 LLM 上下文）；custom_message 变体入上下文（Phase 2 起用） |

- 相对草案的修订：草案 `message.user/assistant/tool.call/tool.result` 四型**合并**为单一 `message` 型（pi 同构，防同一事实两处表达）；草案 `system.notice` **取消**（错误/取消由 assistant 终态消息承载）。
- compaction（Phase 5+）落点：新增 `session.compaction` 型，引用被摘要区间——原区间不删除，天然可回溯。

## 5. 演进规则（ADR-0007 的执行细则）

- **只加不改**：新需求新增 entry 类型或新可选字段；禁改既有字段语义。
- 破坏性变更 = 新 `v` 大版本 + 迁移器 + ADR；同一时刻读取端至少兼容最近两个大版本。
- 兼容性测试：`fixtures/session/v1-*.jsonl` 自 Phase 1 起留存（线性/含工具/含未知型/截断半行）；CI 断言"样例可读且语义不变"。

## 6. 原 §6 待决问题决议（2026-10-26，全部关闭）

| 问题 | 决议 |
|---|---|
| entry id 格式与时钟回拨 | ULID + 进程内单调工厂（同毫秒随机段自增；回拨仍单调不重复） |
| 单文件大小与轮转 | 不轮转（会话为对话文本量级；compaction 处理上下文不物理切分）；单文件 >100MB 场景出现再议 v2 |
| 大 payload 外置 | v1 一律内联（含图像 base64，pi 同构）；后续仅可增补可选 `payload_ref` 型字段 |
| 跨平台数据目录与迁移 | 见 §2 数据目录表；v1 为首个发布格式，无迁移 |
