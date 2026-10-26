# 规范 01：仓库结构与命名

- 状态：定稿
- 日期：2026-10-26

## 1. 目录布局（含 Phase 1 起的预留）

```
/                     仓库根：README.md（英）+ README.zh-CN.md（中）/ AGENTS.md / CHANGELOG.md / LICENSE / .gitignore
.agents/               AI 开发知识库与交接中枢（规范见其 README；预算受 standards/08 约束）
.agents/skills/       vendored 上游技能（gpui-kit 等）+ 项目自创沉淀（S-NNN；来源/预算规则见其 README）
docs/                 全部文档（唯一事实来源；结构见 docs/README.md）
crates/               Rust 内核库（Phase 1 建立）
  timo-ai / timo-agent / timo-tools / timo-session / timo-mcp
  timo-ext / timo-config / timo-kernel
apps/                 可执行薄壳（Phase 1: timo-cli；Phase 3: timo-desktop）
packages/             （预留）JS/TS 资产：扩展兼容 shim 类型、B 轨 TSX 宿主 API 声明
fixtures/             测试夹具（协议录制、会话格式历史样例）
scripts/              构建/检查脚本（CI 用）
```

- cargo workspace 根清单在仓库根；`fixtures/`、`scripts/` 可在 Phase 1 随需要建立，但**命名与位置以本规范为准**。
- crate 划分属"预留命名"（architecture/02），Phase 1 定稿可微调，微调即 ADR。

## 2. 命名约定

| 对象 | 规则 | 示例 |
|---|---|---|
| crate / app | `timo-` 前缀 + kebab-case | `timo-session` |
| Rust 文件/模块 | snake_case | `tool_registry.rs` |
| 类型 | UpperCamelCase；Kind 后缀表枚举类别 | `StreamEvent`、`AuthMethodKind` |
| 文档文件 | 两位序号 + kebab-case（adr 用 ADR-XXXX 前缀） | `03-critical-seams.md` |
| 分支 | `feat|fix|docs|chore/<短横线描述>` | `feat/session-writer` |
| tag | `phase-N.start` / `phase-N.end`（annotated）；发布用 `vX.Y.Z` | `phase-1.end` |

## 3. 内容纪律

- 仓库内**禁止**出现：密钥/凭证、个人绝对路径（如 `F:\`、`C:\Users\<名>`）——文档引用外部项目用仓库相对描述或 GitHub URL（standards/06 §2）。
- 根目录只放本规范列出的文件（README.md、README.zh-CN.md、AGENTS.md、CHANGELOG.md、LICENSE、.gitignore、.agents/）；新增根文件须先修订本规范。
- 根 `README.md`（英）与 `README.zh-CN.md`（中）：**只做项目说明与导航**，两份内容保持一致；禁止放进度看板、状态或流水账（看板唯一在 `docs/roadmap/00-roadmap.md`，流水唯一在 CHANGELOG）。
- 根 `AGENTS.md`：**薄索引入口**（导航 + 铁律链接），禁止记录状态/进度/阶段指针；AI 的"当前状态"唯一来源是 `.agents/CONTEXT.md`。
- 空目录不进 git——目录随首批内容建立。
