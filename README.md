# Timo（仓库名：timo-pi）

> **一句话定位**：用 Rust 重写 [pi](https://github.com/earendil-works/pi) 编码代理的内核（不含 TUI），以 gpui-kit + NSH 构建原生桌面端；插件策略为「**兼容编排类扩展 + 自建 GUI 原生插件面**」双轨制。

本项目实行**文档先行**：先定规范与设计，再写代码。项目所有者不读代码，进度只看两处：

1. 本页 **进度看板**
2. [CHANGELOG.md](CHANGELOG.md)（每次进展的流水记录）

---

## 进度看板

| 阶段 | 内容 | 状态 | 参考估时* | 说明 |
|---|---|---|---|---|
| Phase 0 | 仓库初始化、规范与设计文档定稿 | ✅ **已完成**（2026-10-26） | — | 全部 `docs/` 文档落盘并初始提交 |
| Phase 1 | 内核 MVP（2 种协议、agent 循环、核心工具、会话 v1、MCP） | ⬜ 未开始 | 6~10 周 | 开工前需完成 [phase-1 文档](docs/roadmap/phase-1-kernel-mvp.md) 定稿评审 |
| Phase 2 | 编排类扩展宿主（pi 插件兼容层） | ⬜ 未开始 | 3~6 周 | 依赖 Phase 1 |
| Phase 3 | gpui-kit 桌面壳（含 NSH 嵌入验证） | ⬜ 未开始 | 4~8 周 | 依赖 Phase 1 |
| Phase 4 | GUI 原生插件系统（TSX 插件面） | ⬜ 未开始 | 4~8 周 | 依赖 Phase 2 + 3 |

\* 估时为单人全职开发的参考值，仅用于量级感知，不作为承诺。

---

## 给项目所有者的阅读路径

1. [docs/roadmap/00-roadmap.md](docs/roadmap/00-roadmap.md) —— 总路线图：每个阶段做什么、怎么验收
2. [docs/adr/](docs/adr/) —— 已定下的重大决策及理由（共 9 篇，每篇 1~2 页）
3. [docs/glossary.md](docs/glossary.md) —— 术语表（看不懂的词先查这里）
4. [CHANGELOG.md](CHANGELOG.md) —— 进展流水

完整文档目录见 [docs/README.md](docs/README.md)。

---

## 待所有者决策的事项

| 事项 | 说明 | 是否阻塞 |
|---|---|---|
| **开源许可证** | 是否开源、选哪种（MIT / Apache-2.0 / AGPL…） | 不阻塞 Phase 1，阻塞对外发布 |
| **正式产品名** | 当前代号 "Timo"，仅为工作名 | 不阻塞 |
| **目标平台优先级** | Windows / macOS / Linux 的先后顺序 | 影响 Phase 3 细节排期 |

---

## 仓库原则（摘要）

- **文档先行**：阶段设计文档未定稿，禁止写实现代码（详见 [docs/standards/03-documentation.md](docs/standards/03-documentation.md)）。
- **重大决策留痕**：一切技术选型与不可逆设计必须写 ADR。
- **参考不抄码**：参考项目 PI-Desktop 为 LGPL-3.0，只借鉴架构思路，严禁引用其源码；pi 上游仅用于理解行为与协议。
- **密钥与个人路径不入库**（详见 [docs/standards/06-security.md](docs/standards/06-security.md)）。
