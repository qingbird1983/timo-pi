# 架构 03：关键接缝清单（MVP 防返工核心）

- 状态：定稿
- 日期：2026-10-26
- 关联：ADR-0006/0007；architecture/02；roadmap/phase-1

> 本清单是"精简 MVP 将来能否逐步长成完整 pi 功能"的保证书。每条接缝给出：MVP 做到什么程度、为什么不可逆、验收检查点。**每个阶段收尾 review 必须逐条核对本清单。**

## 四大接缝

### #1 会话磁盘格式 —— 最贵、几乎不可逆
- MVP 做到：按 architecture/05 的**最终形态**实现（版本化 entry、树形 parentId、可扩展 payload、append-only、未知类型跳过）。
- 为什么不可逆：唯一"后补 = 写迁移工具"的项；其余一切都能加。
- 检查点：格式文档状态=定稿；向后兼容测试（旧版本文件必读）进 CI。

### #2 内部事件总线 —— 所有扩展点的唯一接缝
- MVP 做到：循环内零硬编码 hook；architecture/02 §3 事件集全集 emit；扩展宿主只是订阅者之一。
- 为什么不可逆：hook 点散落硬编码后，任何新消费方（扩展/GUI/遥测）都要回头开洞重写循环。
- 检查点：`timo-agent` 代码中不存在 `if extension` 分支；新增订阅者无需改动循环即可编译运行。

### #3 协议 trait + 数据驱动模型目录
- MVP 做到：两种协议收敛到同一 trait 后；模型目录是纯数据文件（可从 pi 目录同步子集）；认证为枚举（OAuth 占位）。
- 为什么不可逆：静态 key 假设焊死后，OAuth/自定义供应商极难补；协议散写则每加一家全盘改。
- 检查点：新增一种协议 = 新增一个实现文件 + 目录数据，不触碰循环与工具层（Phase 5 验证）。

### #4 工具元模型
- MVP 做到：ToolDefinition 一开始就带 exposure（direct/model-only/codemode/deferred/hidden）、命名空间前缀、annotations。
- 为什么不可逆：工具系统是 MCP、扩展、codemode、tool-search 的共同汇点，元数据缺失则四路全部返工。
- 检查点：MCP 工具、扩展工具、内置工具在同一注册表中不可区分来源（仅前缀不同）。

## 三小件（便宜但后补极疼）

| # | 接缝 | MVP 要求 |
|---|---|---|
| 5 | JSON Schema 通货 | 工具/MCP/扩展统一用 JSON Schema 校验；禁止引入第二套 schema 体系 |
| 6 | 富流事件枚举 | StreamEvent 按 architecture/02 §2 粒度；禁止裸字符串流 |
| 7 | 取消贯穿 | CancellationToken 从入口传到 HTTP/子进程/工具；kill -9 后会话文件可读 |

## 配套纪律

- 认证枚举占位（OAuth 不实现但留位）——已并入 #3。
- 接缝核对纳入《阶段进度报告》模板（standards/03 附录）。
