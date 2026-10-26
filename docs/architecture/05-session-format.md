# 架构 05：会话磁盘格式（草案）

- 状态：**草稿**（Phase 1 内定稿；定稿前禁止写实现代码——文档先行）
- 日期：2026-10-26
- 关联：ADR-0007；architecture/02/03

## 1. 设计目标（按最终形态，一步到位）

1. **版本化**：每条 entry 与每个会话文件都带格式版本。
2. **树形**：entry 以 `parentId` 链接，天然支持分叉/多分支（MVP 只写线性链，结构不省略）。
3. **可扩展 payload**：entry 类型可增；**读取端遇到未知 entry 类型必须跳过且不报错**（向前兼容铁律）。
4. **append-only + 崩溃安全**：只追加写入；半条写入（断电/kill -9）在下次读取时丢弃，不损坏已写入内容。
5. **人类可读**：JSON Lines（每行一个 entry），文本编辑器可直接打开检查。

## 2. 存储布局（草案）

```
<数据目录>/sessions/<session-id>.jsonl      会话主体（append-only entry 流）
<数据目录>/sessions/<session-id>.meta.json  会话元信息（标题/时间/模型/统计，可整文件重写）
<数据目录>/sessions/index.json              会话列表索引（可重建，非权威）
```

- 数据目录默认遵循平台惯例（如 Windows `%APPDATA%`；Phase 1 定稿时确认）。
- `index.json` 丢失可由 `.jsonl` 全量重建——单一权威来源是 entry 流本身。

## 3. Entry 信封（草案）

```json
{
  "v": 1,
  "id": "e_01J9...",
  "ts": "2026-10-26T03:21:00.512Z",
  "parentId": "e_01J8... | null",
  "type": "message.user",
  "payload": { }
}
```

- `type` 命名空间：`message.*`、`tool.*`、`session.*`、`custom.<plugin-namespace>.*`、`system.*`。
- `custom.*` 即 pi `appendEntry` 的对应物（A 轨 v2 成员落点，结构先占位）。

## 4. v1 必备 entry 类型（Phase 1 定稿清单）

| type | payload 要点 |
|---|---|
| session.header | 每文件首条：格式版本、创建时间、agent/模型信息 |
| message.user / message.assistant | 文本、thinking（如有）、usage 快照 |
| tool.call | 名称（含命名空间）、参数、调用 id |
| tool.result | 调用 id、状态、输出（截断策略定稿时定）、时长 |
| system.notice | 错误、取消、降级等非对话事件 |

## 5. 演进规则（ADR-0007 的执行细则）

- **只加不改**：新需求新增 entry 类型或新可选字段；禁改既有字段语义。
- 破坏性变更 = 新 `v` 大版本 + 迁移器 + ADR；同一时刻读取端至少兼容最近两个大版本。
- compaction（Phase 5+）落点：新增 `session.compaction` entry 类型，引用被摘要的 entry 区间——原区间不删除，天然可回溯。
- 兼容性测试：`fixtures/session/` 存放历史版本样例文件；CI 断言"旧样例可读且语义不变"（格式发布后生效）。

## 6. 定稿前待决问题（Phase 1 处理）

- entry id 格式与时钟回拨策略；单文件大小与轮转策略；大 payload（如文件快照）外置存储与否；跨平台数据目录与迁移。
