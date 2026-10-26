# 设计 04：timo-session —— 会话格式定稿与实现

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0007；architecture/05（定稿）；architecture/03 接缝 #1；pi 参照: coding-agent/src/core/session-manager.ts（语义）

## 1. 存储布局（对草案 §2 的定稿修订）

```
<数据目录>/sessions/<session-id>.jsonl     唯一权威（append-only entry 流）
```

- **删除草案中的 `<id>.meta.json` 与 `index.json`**：会话元信息并入首条 `session.header` entry；列表 = 扫描目录内 `.jsonl` 文件头（单文件读取上限 1MB，防超大文件拖慢）。单一权威 = entry 流本身，无第二份可失步状态（防膨胀 + 防不一致，ponytail）。
- 数据目录：`dirs::data_dir()/timo-pi`（Windows `%APPDATA%\timo-pi`；详见设计 05 §1）；`TIMO_AGENT_DIR` / settings `session_dir` 可覆盖。目录名即会话 id（ULID，见 §3）。

## 2. Entry 信封（每行一个 JSON 对象）

```json
{"v":1,"id":"01J9XYZ...","ts":"2026-10-26T03:21:00.512Z","parent_id":null,
 "type":"message","payload":{ }}
```

| 字段 | 定稿 |
|---|---|
| `v` | 格式版本，常量 1；破坏性变更→v2+迁移器（architecture/05 §5） |
| `id` | **ULID**（26 字符 Crockford base32）：字典序=时间序，天然排序；进程内单调工厂（同一毫秒自增随机段）**同时解决时钟回拨**（§6 Q1） |
| `ts` | RFC3339 毫秒 UTC（可注入 Clock） |
| `parent_id` | 父 entry id；根（含 header）为 null——树形结构（MVP 只写线性链） |
| `type` | 命名空间：`session.*` / `message` / `model.*` / `custom.*`（Phase 2 扩展落点） |
| `payload` | 各 type 自定；未知 type **读取端必须跳过不报错**（向前兼容铁律） |

## 3. v1 entry 类型（定稿清单；对草案 §4 的修订说明见括号）

| type | payload | 对应 pi |
|---|---|---|
| `session.header` | `{format_version:1, session_id, created_ts, cwd, parent_session_id?, initial_model?}` | pi header 行（首条，parent_id=null） |
| `message` | 完整消息对象（role 判别：user / assistant / tool_result / system），schema 即 timo-ai `Message` | pi `message` entry（**合并草案的 message.user/assistant 与 tool.call/tool.result 四型为一型**：assistant 的 content 已含 ToolCall 块、tool_result 是独立 role——分离型会造成同一事实两处表达，且与恢复投影冲突） |
| `model_change` | `{provider, model_id}` | pi model_change |
| `thinking_level_change` | `{level}` | pi thinking_level_change |
| `usage` | `{kind, provider, model, usage}`（轮次记账快照，非 LLM 口径） | pi usage |
| `custom` | `{custom_type, data?}`（Phase 2 扩展私有状态；永入 LLM 上下文） | pi custom |
| `custom_message` | `{custom_type, content, display}`（Phase 2；**会**入 LLM 上下文） | pi custom_message |

- 草案的 `system.notice` 型**取消**：错误/取消已由 assistant 终态消息承载（stop_reason/error_message），不另设非对话事件型（防同一事实两处写）。Phase 2 若扩展确需再以只加不改增补。
- `session.compaction` 型按 architecture/05 §5 预留说明保留（Phase 5+ 实现）。

## 4. 读写 API（timo-session 公共面）

```rust
pub struct SessionWriter { file: BufWriter<File>, id_gen, clock, leaf: Ulid, path: PathBuf }
impl SessionWriter {
    pub fn create(dir, cwd, clock, id_gen) -> Result<Self>;       // 写 header
    pub fn open(path) -> Result<Self>;                            // 恢复追加（校验 header）
    pub fn append(&mut self, entry: Entry) -> Result<Ulid>;       // 单行序列化 + "\n" + flush
    pub fn leaf(&self) -> Ulid;
}
pub fn read_entries(path) -> Result<Vec<Entry>>;   // 逐行解析；非法 JSON 行→错误；见 §5 容忍规则
pub fn project_context(entries) -> Vec<Message>;   // 沿 leaf 链回溯 parent_id → 顺序投影消息
pub fn list_sessions(dir) -> Vec<SessionInfo>;     // 扫描 *.jsonl；读首条 header（≤1MB cap）
pub struct SessionInfo { id, path, cwd, created, modified, message_count, first_message }
```

## 5. 崩溃安全（验收 A3 的依据）

- 写入：每 entry 一次 `write!` + `flush()`（行完整即落 OS 缓冲；**不做每行 fsync**——pi 同构，进程被杀不丢已 flush 行，断电尾部丢失可接受）。
- 读取容忍：**末行不完整**（无换行结尾或 JSON 截断）→ 丢弃该行且不报错（kill -9 半条写入的恢复路径）；**中间行**非法 → 报错（真损坏，不静默吞）。
- 追加恢复：`open()` 定位文件末尾追加；不重写既有内容（append-only 铁律）。
- 自动化测试：截断末行 / 中间注入垃圾行 / 空 文件 / 仅 header 四类样例（fixtures/session/，standards/05 §2.2 自 Phase 1 起留存）。

## 6. architecture/05 §6 待决问题决议（逐项）

| 问题 | 决议 |
|---|---|
| Q1 entry id 格式与时钟回拨 | ULID + 进程内单调工厂（同毫秒随机段自增；回拨时仍单调不重复）；无中心分配器 |
| Q2 单文件大小与轮转 | **不轮转**。理由：会话体为对话文本，量级 KB~MB；compaction（Phase 5+）按格式预留的 summary 型处理上下文而非物理切分；轮转会破坏 append-only 单文件不变量。若未来出现真实膨胀案例→ 以 v2+迁移器处理（记录 backlog 触发条件：单文件 >100MB 场景） |
| Q3 大 payload 外置 | **v1 一律内联**（含图像 base64）。理由：pi 同构、实现面最小；外置 = 新 entry 可选字段（只加不改允许后续增补 `payload_ref` 型字段），先不定 |
| Q4 跨平台数据目录与迁移 | Windows `%APPDATA%\timo-pi`、macOS `~/Library/Application Support/Timo-pi`、Linux `$XDG_DATA_HOME/timo-pi`（dirs crate）；env `TIMO_AGENT_DIR` 最高优先。**无迁移**：v1 为首个发布格式（设计 05 §1） |

## 7. 兼容性义务（接缝 #1 检查点）

- `fixtures/session/v1-*.jsonl` 自 Phase 1 起留存：线性对话样例、含工具调用、含未知 type 行（`custom.demo`）、截断末行样例。
- CI 断言：各样例可读、投影结果稳定（快照断言）；未知 type 跳过不报错。
- 格式演进从 v1 起遵循 architecture/05 §5（只加不改；读取端至少兼容最近两个大版本）。
