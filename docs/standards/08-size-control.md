# 规范 08：体量与防膨胀管理

- 状态：定稿
- 日期：2026-10-26
- 关联：.agents/README.md；standards/01/03/04/07

## 0. 目的

一切"只增不减"的集合——文档、AI 知识库、代码、依赖——都会无限膨胀。膨胀不是风格问题，而是维护性风险：淹没关键信息、拖慢检索、放大 AI 上下文成本。本规范给每一类集合定**预算 + 清理机制 + 豁免流程**。

## 1. 总原则

1. **单一事实**：任何知识只在一处权威定义，其余位置一律引用；发现重复以"合并 + 引用"处置。
2. **流水账唯一**：全仓库唯一允许的追加式日志是 `CHANGELOG.md`。`docs/` 其余全部是**活性文档**（覆盖式修订）；会话记录、每日进展、过程笔记一律进 `.agents/`，禁止进入 `docs/`。
3. **预算是硬数字**：下列上限不是建议；超标必须在**当轮/当阶段**处置（拆分、精炼、合并、删除），或在进度报告登记豁免及理由。
4. **收尾强制核对**：每阶段《阶段进度报告》模板含"体量核对"一节（standards/03 附录），逐项给出结论。

## 2. 文档预算（docs/ 与根入口文件）

**入口文件定位约束**（防流水账第一线）：

- 根 `README.md`（英）/ `README.zh-CN.md`（中）：只做**项目说明与导航**，两份内容一致；禁止放进度看板、状态、流水账（看板唯一在 `docs/roadmap/00-roadmap.md`，流水唯一在 CHANGELOG）。
- 根 `AGENTS.md`：**薄索引**（导航 + 铁律链接）；禁止记录状态/进度/阶段指针——"当前状态"唯一来源是 `.agents/CONTEXT.md`。
- 新增根入口文件须先修订 standards/01。

| 对象 | 上限 | 超限处置 |
|---|---|---|
| 正式文档单篇（research/architecture/roadmap/standards/design） | ≤ 300 行 | 拆分（登记 docs/README 新序号）或精炼 |
| 单篇 ADR | ≤ 100 行 | 一篇只装一个决策，拆篇 |
| 单篇 phase/design 设计稿 | ≤ 500 行 | 按模块拆分到 design/phase-N/ 下多文件 |
| docs/ 文档总数 | 软上限 60 篇 | 触发整合评审（合并碎片、归并过时篇） |
| 阶段文档增量 | 每阶段新增 ≤ 12 篇 | 超出说明阶段切分过细，重新归并 |

- **过时机制**：被新结论取代的文档，顶部加"已取代：见 XXX"横幅并冻结；连续两个阶段无引用的过时篇可删除（git 历史留存）。
- **孤儿清零**：每阶段核对 docs/README.md 清单与实际文件一致，孤儿文档当阶段删除或登记。
- research/ 结论冻结：新调查新开篇，不改旧篇结论。

## 3. AI 知识库预算（.agents/）

预算与清理机制以 `.agents/README.md` §3 为准（CONTEXT ≤ 150 行、handoff 滚动 5 份、单篇沉淀 80/120 行、全目录 ≤ 40 文件且不含 vendored 技能子目录、教训当轮提取）。standards/08 负责其"规范级"地位：修改 .agents 预算必须修订本规范。

## 4. 代码预算（Phase 1 起生效）

| 对象 | 上限 | 说明 |
|---|---|---|
| 单源文件 | ≤ 500 行 | 超过必须拆分；pi 的教训：7,105 行的 interactive-mode.ts 是其扩展难移植的根源之一 |
| 单函数 | ≤ 60 行 | 指导值；配合 clippy 复杂度 lint |
| 单 crate | > 15,000 行触发拆分评审 | 只在阶段边界做拆分，避免中途动荡 |
| 死代码 | 零容忍 | `#[allow(dead_code)]` 等豁免必须附注释 + issue/ADR 归属；无主豁免 CI 拒绝 |
| 重复代码 | 阶段 review 处置 | 明显复制粘贴 ≥ 3 处必须抽公共实现 |
| 直接依赖数（每 crate） | 软上限 15 | 超出在依赖准入表说明 |

- **豁免流程**：确需超标（协议大表、生成代码、数据文件），代码内注释声明原因 + 进度报告登记豁免清单；**生成代码不计入预算**。
- **CI 落地**：Phase 1 起提供 `scripts/check-size` 类脚本（文件行数/豁免清单核对），先 warn 后视情况升 hard；文件级超标直接 fail，函数级提示。

## 5. 依赖预算

按 standards/04 §5 准入流程执行；补充两条防膨胀纪律：

1. 升级窗口化：NSH / gpui-kit 等快速演进依赖，每阶段最多升级一次且独立成提交，便于回滚。
2. "为了一个小功能引入一个大依赖"视为超标行为，准入表必须记录备选方案对比。

## 6. 核对与豁免登记（进度报告附录格式）

```markdown
## 体量核对（standards/08）
- docs/：新增 N 篇 / 总数 X 篇（上限 60）；超限篇目：
- .agents/：文件数 X（上限 40）；CONTEXT 行数：
- 代码：新增 >500 行文件：无 / 有（附豁免）；crate 最大行数：
- 依赖：新增 X 个（准入表已更新：是/否）
- 构建产物：target/ 大小 X GiB（软 15 / 硬 25；clean-build 前 → 后：）；分类：incremental X / deps X：
- 豁免登记：
```

- 同一规则在一个阶段内被豁免 **≥ 2 次** → 触发 standards/09 触发复查（处置对象是规则本身，不是续期豁免）。

## 7. 构建产物治理（target/，Rust 专项）

**背景**：Rust 的体量风险不在 git（target/ 永不入库），而在工作目录的编译产物——cargo 从不自动清理 target/，debuginfo 是体积大头；多构建形态、编辑器与命令行重复构建、嵌套 workspace 会成倍放大（参照：pi-flash 曾膨胀至 26G，research/05）。

**红线与预算**

- 全仓库**只允许一个 cargo workspace、一个 target/**；禁止嵌套 workspace（pi-flash `crates/gpui` 独立 workspace 为反例）。
- **按容量，不按年龄**：AI 会话会持续刷新产物 mtime，`cargo sweep -t 30` 通常清理 0，不能作为主策略。预算单位统一为 GiB（1024 进制）。

| 项 | 上限 | 处置 |
|---|---:|---|
| target/ 总量（软） | 15 GiB | `check-size` WARN；阶段收尾前必须 `scripts/clean-build` 清到线下 |
| target/ 总量（硬） | 25 GiB | `check-size` FAIL，阻断收尾/tag |
| `*/incremental` | 4 GiB | `clean-build` 第一步直接删（再生成最便宜） |
| `debug/deps` + fingerprint | 12 GiB | 由 `clean-build` 的容量淘汰控制 |
| sccache | 20 GiB | 独立 LRU；sccache 到位后 target 硬限收紧到 15 GiB |

- **清理阶梯**（`scripts/clean-build`）：① 删超过 4 GiB 的 incremental；② `cargo sweep --maxsize <软限>` 按容量淘汰；③ 仍超硬限且 sccache 在位 → `cargo clean`；无 sccache 就停下报错，不自动全清。
- **触发点**：工作中超过 20 GiB 只提醒；每阶段收尾必须跑一次 `clean-build` 并在报告里记“前 → 后”；超过 25 GiB 由 `check-size` 直接 FAIL。
- 门禁/CI 的构建建议 `CARGO_INCREMENTAL=0`，避免每轮再加一份增量缓存；本地交互式迭代可保留。
- 禁止把 `CARGO_TARGET_DIR` 设为多项目共享单目录（锁竞争 + 单点巨大）；如需给项目文件夹减负，可在 `.cargo/config.toml` 用 `[build] target-dir` 指向本机专用目录，并纳入同一清扫策略（默认不启用）。
- git 内禁止任何构建产物；超过 1MB 的二进制资产须先写 ADR（fixture 纪律见 standards/05）。

**Phase 1 落地清单（实现代码前先固化 `.cargo/config.toml`）**

```toml
[profile.dev]
debug = "line-tables-only"        # 保留回溯行号，砍掉大部分调试信息体积（经验值 60~80%）
[profile.dev.package."*"]
opt-level = 2                     # 依赖以 O2 编译：运行更快，体积反小于全量 debuginfo
[profile.release]
strip = "debuginfo"
lto = "thin"
```

- 本机启用 `RUSTC_WRAPPER=sccache`（缓存目录设上限 ~20 GiB，LRU 自动淘汰）——清扫后重编近乎免费；**未启用 sccache 时不得 `cargo clean`**。
- cargo ≥ 1.78 自带 `~/.cargo` registry 自动 GC（默认 10G 上限），无需额外工具。
- `scripts/` 提供：`target-size`（分类读数，单一口径）、`clean-build`（按容量清扫，支持 `--dry-run` / `--full`）、`check-size`（15/25 GiB 软硬限）。按龄的 `cargo sweep -t 30` 仅用于清理长期未用的工具链产物，不再是主策略。

