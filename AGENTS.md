# AGENTS.md —— AI 协作入口（薄索引）

> 本文件**只做导航与铁律**，禁止记录状态、进度、阶段指针或任何流水账。
> AI 的"当前状态"唯一来源：`.agent/CONTEXT.md`（第一站，先读它）。
> 进度看板唯一在 `docs/roadmap/00-roadmap.md`；进展流水唯一在 `CHANGELOG.md`。

## 文档索引（按需进入）

| 你要做什么 | 去哪里 |
|---|---|
| 了解项目 / 对外介绍 | [README.md](README.md)（英）/ [README.zh-CN.md](README.zh-CN.md)（中） |
| 接手工作 / 看当前状态 | [.agent/CONTEXT.md](.agent/CONTEXT.md) → 交接规则见 [.agent/README.md](.agent/README.md) |
| 看进度 / 阶段规划 | [docs/roadmap/00-roadmap.md](docs/roadmap/00-roadmap.md)（唯一看板）+ 当前 phase 文档 |
| 查"为什么这么设计" | [docs/adr/](docs/adr/)（编号递增，只增不改） |
| 查系统设计与约束 | [docs/architecture/](docs/architecture/)（先读 01；接缝必读 03；插件 04；会话格式 05） |
| 查做事规则 | [docs/standards/](docs/standards/)（8 篇） |
| 写码纪律（各阶段写代码时） | [.agents/skills/ponytail/](.agents/skills/ponytail/SKILL.md)——最小完整改动纪律（MIT，vendored） |
| 桌面开发（Phase 3） | [.agents/skills/](.agents/skills/README.md)——gpui-kit 官方技能（框架/设计规范，vendored） |
| 查术语 | [docs/glossary.md](docs/glossary.md) |
| 新增文档登记 | [docs/README.md](docs/README.md)（文档地图，禁止孤儿文档） |

## 铁律（细则见链接，违反即返工）

1. **文档先行**：当前阶段设计文档未定稿，禁止写实现代码 —— [standards/03](docs/standards/03-documentation.md)。
2. **决策留痕**：重大技术决策必须新增 ADR，不得只做不记 —— [standards/03](docs/standards/03-documentation.md)。
3. **收尾四件事**：重写 CONTEXT 交接卡、归档 handoff、沉淀 pitfalls/skills、体量核对；教训当轮提取 —— [.agent/README.md](.agent/README.md)、[standards/07 §7](docs/standards/07-ai-collaboration.md)。
4. **安全红线**：密钥、个人路径不入仓库；插件默认无能力、显式授权 —— [standards/06](docs/standards/06-security.md)。
5. **范围纪律**：只做当前阶段"做"清单内的事，超范围先 backlog 或 ADR —— [standards/07](docs/standards/07-ai-collaboration.md)。
6. **方向纪律**：内核阶段（Phase 1~2）**严格对齐 pi、忠实复刻**——行为语义以 pi 为唯一蓝本，创新构想一律记入 backlog/ADR 延后至项目稳定，当前阶段照 pi 做 —— [ADR-0011](docs/adr/ADR-0011-kernel-phase-follow-pi.md)。
7. **外部代码**：GPL/LGPL 项目（**PI-Desktop**、**pi-flash**）源码严禁引用，只读思路；pi 上游（MIT）仅可阅读理解行为，不得复制 —— [standards/07 §5](docs/standards/07-ai-collaboration.md)、[research/04](docs/research/04-reference-pi-desktop.md)、[research/05](docs/research/05-reference-pi-flash.md)。
8. **体量红线**：docs/.agent/代码/依赖的预算见 [standards/08](docs/standards/08-size-control.md)，超标当期处置或登记豁免。

> 语言约定：文档与对用户的沟通用中文；代码、标识符、commit message 用英文。
