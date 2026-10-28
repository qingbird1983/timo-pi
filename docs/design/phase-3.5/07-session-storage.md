# Phase 3.5 / 07：会话存储按项目分区与导出

- 状态：定稿（2026-10-27）
- 关联：architecture/05（会话格式）；ADR-0007；pi `core/session-manager.ts`、`core/session-export.ts`（只读参照）

## 1. 目标

- 会话文件按工作目录分区，避免所有项目的会话堆在 `sessions/` 根下；
- 提供会话导出（当前分支 → 单个 JSONL），可被脚本/归档复用。

## 2. 范围与偏离

| # | 偏离 | 类型 | 说明 |
|---|---|---|---|
| D45 | 新会话按 cwd 分区，但**列表仍跨分区** | c（现有桌面体验） | pi 默认只列当前 cwd 的会话。我们的侧栏一直显示全部会话；P1 只改落盘位置，不改列表范围，避免 GUI 行为突变。 |
| D46 | 旧平铺文件不迁移 | b（只增不改） | 读取时兼容根目录文件；新写入一律进分区目录。迁移会改写用户文件，不做。 |
| D47 | 导出保留原始 entry 与 parent_id | a（自有会话格式） | pi 导出当前分支时会按顺序重写 `parentId`；我们的 MVP 是线性链，原样输出即可，不制造差异。 |

## 3. 目录编码

对齐 pi：

```text
sessions/--<encoded-cwd>--/<session-id>.jsonl
```

编码规则：

1. cwd 取绝对路径字符串（`Kernel::work_dir`）；
2. 去掉开头的路径分隔符（`/` 或 `\`）；
3. 把 `/`、`\`、`:` 全部替换成 `-`；
4. 前后各加 `--`。

例子：

| cwd | 目录名 |
|---|---|
| `C:\work\demo` | `--C--work-demo--` |
| `D:\a\b-c` | `--D--a-b-c--` |
| `/home/me/proj` | `--home-me-proj--` |

已知碰撞：`C:\a-b` 与 `C:\a\b` 编码相同。与 pi 相同的限制，登记不修；ULID 文件名保证会话本身不撞。

## 4. 写入、查找与列表

- `SessionWriter::create(dir, …)`：调用方传入的 `dir` 现在是分区目录（内核计算）。目录不存在则创建（沿用现有行为）。
- `SessionWriter::open`：续写前修整末尾——末行不是合法 JSON 则截断到最后一个换行；末行合法但缺 `\n` 则补一个换行，保证新 entry 不会拼到旧行上。
- 查找（`resume` / `delete` / 导出当前会话）：
  1. `sessions/<id>.jsonl`（旧平铺）；
  2. `sessions/<encoded-cwd>/<id>.jsonl`（当前项目）；
  3. 其他一级子目录 `<子目录>/<id>.jsonl`（跨项目恢复）。
  id 仍按现有规则校验（字母数字、`-`、`_`），不接受路径分隔符。
- 列表 `list_sessions(root)`：扫描根目录 `*.jsonl` **与**一级子目录内的 `*.jsonl`；按 id 去重（同一 id 多份取修改时间最新的一份并告警）；按修改时间倒序。
- `sessions` 命令的 `--cwd` 过滤、桌面侧栏行为不变。

## 5. 导出

- 内核 API：

```rust
pub fn export_session(&self, path: Option<&Path>) -> Result<PathBuf>;
```

  - 缺省路径：`<work_dir>/session-<YYYYMMDD-HHMMSS>.jsonl`（UTC，秒级；同一秒冲突时追加 `-1`…`-9`）；
  - 已存在同名文件 → 报错，不覆盖；
  - 内容：`session.header` + 当前 leaf 链上的全部 entry（原样序列化，行尾 `\n`），包含 `session.compaction` 与 `usage`；
  - 原子性：先写 `<目标>.tmp`，成功后 `rename`；失败删除 tmp。
- CLI：`/export [path]` → 打印导出后的绝对路径；草稿会话报错。命令面 `CommandStatus` 从 Planned 改为 Ready。
- 桌面：P1 不接按钮（命令面板不显示 `/export`，输入框里的 `/export` 仍按普通文本发送）；GUI 批次再接。

## 6. 测试

1. 编码表：三种 cwd 的目录名逐字匹配；尾部分隔符被去掉；纯根 `C:\` → `--C--`。
2. 新建会话落在 `sessions/<encoded>/<id>.jsonl`；header 的 `cwd` 仍是原始路径。
3. 列表：根目录旧文件 + 两个分区目录 → 全部出现、按修改时间排序；同 id 重复只出现一次。
4. 恢复：分区内会话 `resume(id)` 成功，历史一致；`delete(id)` 删除分区文件；跨项目 id 也能恢复。
5. 导出：字节内容 = header + 链上 entry（逐行 `serde_json` 可解析）；已存在路径报错且原文件不变；`.tmp` 不残留。
6. CLI：`/export` 在草稿上报错、在已保存会话上打印路径；导出的文件可被 `resume --session-dir` 的目录重新扫描到。

## 7. 体量

- `timo-config`/`timo-session` 路径编码与扫描 ≈ 80 行；
- 导出 ≈ 80 行（`timo-session`），内核入口 ≈ 30 行；
- CLI `/export` ≈ 20 行。
