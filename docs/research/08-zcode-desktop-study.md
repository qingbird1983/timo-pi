# 调研 08：ZCode 桌面端交互逻辑与数据流源码调查（计划 + 源码地图）

- 状态：**草稿**（2026-10-27 立项；执行分批，产出转定稿后喂给视觉精修批次与桌面交互批次）
- 关联：ADR-0013；research/07 §8（所有者视觉方向 v1，已由 ZCode 实拍印证）；research/06（pi 交互逻辑调查——分工见 §5）
- 调查对象：`F:\Warehouse\Github\ZCode`（本地完整源码）

## 1. 定位与合规

- **许可证：Apache-2.0**（已核验 LICENSE）。宽松许可，与 pi（MIT）同等待遇：**可阅读、理解行为、借鉴交互逻辑与数据流；不复制代码**（本项目 Rust + gpui-kit，与对方 Electron + React 技术栈完全不同，天然洁净室）。
- 定位：所有者指定的**最高相关对标**（research/07 §3/§8.3g）——同为桌面 AI 编码代理，其右栏卡片式布局被所有者确认"与 v1 构想几乎一模一样"。截图只够看"长什么样"，源码能回答"怎么动的"。

## 2. 源码地图（2026-10-27 初勘）

| 位置 | 内容 | 价值 |
|---|---|---|
| `docs/`（330 篇） | **成文的交互设计文档库**，标题精准命中：agent-tool-call-renderer（工具调用渲染）、code-viewer-diffs（diff 查看）、composer-model-thinking（composer 模型/思考档）、conversation-session-case-catalog（会话用例目录）、desktop-html-preview-open（HTML 预览打开）、git-review-file-context-menu（git 审查菜单）、agent-background-subagent-join（子代理后台汇合）、automations-main-window-drag-region（拖拽区）、assistant-message-markdown-type-scale（消息字号体系）等 | **第一优先**——文档读起来比代码快一个量级，且是对方自己写的行为规格 |
| `docs/adr/`（4 篇）+ `ZCode 项目架构分析与改进建议.md` | 架构决策 | 第二优先 |
| `DESIGN.md` + `packages/ui/src/styles.css` | **成文设计系统**：`text-ui-*` 字号强制档位、主题 token（Zai Light/Dark） | 视觉精修批次的 token 体系对照（gpui-kit 语义 token ↔ 对方 CSS 变量） |
| `packages/desktop/src/` | Electron 五层：main / preload / renderer / host / scheduler | renderer 层的组件实现 = 交互逻辑最终事实；按需抽查（第三优先） |
| `apps/zcode-cli/packages/core/src/permission/service.ts` | 权限判定服务（NOTICE.md 引用） | 审批档位（§8.3b 四档菜单）的成熟参照 |
| `NOTICE.md` | 权限模式（build/yolo）、风险边界的产品级说明 | 权限文案与语义参照 |

## 3. 调查维度与产出物

以 research/07 §8 已建档的 UI 元素为索引，逐个去源码回答"数据从哪来、状态怎么流转、边界怎么处理"：

| 维度 | 对照的已建档元素 | 产出 |
|---|---|---|
| 轮次与过程渲染 | §8.3c/f 三层密度（汇总行/过程行/单步详情）+ Trace 词汇 | 《过程渲染数据流》：事件→行状态的映射、折叠状态归属、长内容截断策略 |
| 交付物与撤销 | §8.3g 文件更改汇总行（审查/打开/撤销） | 《变更追踪机制》：diff 统计来源、按轮 checkpoint/撤销实现、git 依赖边界 |
| 右栏宿主体系 | §8.3e/§8.3g 四宿主（审阅/浏览器/子代理/文件浏览） | 《面板宿主模型》：槽位注册、标签生命周期、与对话的双向事件（如"报错回对话"） |
| 权限模式 | §8.3b 四档菜单 + composer 芯片 | 《模式→行为映射表》：每档放行/拦截矩阵、allowlist 持久化、与对话内确认的联动 |
| 会话/项目树 | 左栏任务列表+项目两级树 | 《会话元数据流》：标题自动生成、时间戳、状态点、排序 |
| 设置体系 | §8.3d 设置页条目 | 《设置项→行为对照》：逐项落点（哪层存储、即时生效路径） |

产出物直接回填 `design/`（视觉精修批次详设 + 桌面交互批次），**禁止复制代码与文案**。

## 4. 执行计划（分三批，可与视觉精修批次并行）

1. **第一批（文档层，~1 天量级）**：通读 §2 列出的 10~15 篇高相关 docs，产出 §3 各维度的初稿对照表；
2. **第二批（代码层抽查）**：对文档没讲清的行为，进 `packages/desktop/src/renderer` 按组件名定位实现核对；
3. **第三批（回填与适配微调）**：结合 Timo 现状（gpui-kit 能力、内核接缝）写"采纳/改造/不采纳"结论，进设计稿。

## 5. 与 research/06（pi 交互逻辑调查）的分工

- **research/06 = pi**：行为语义与提示词工程的**复刻蓝本**（ADR-0013，方向纪律约束"照 pi 做"）；
- **research/08 = ZCode**：桌面产品的**交互形态参照**（Apache-2.0 同类实现，所有者指定"感觉就按这个"）；
- 冲突时裁决规则：**行为语义问 pi，交互形态问 ZCode，所有者拍板收口**——两份调查的结论都进 design/，由设计稿统一。
- **主次确认（所有者 2026-10-27）**：视觉精修以 ZCode 为**首要基准**（成熟完善的产品），旧 Timo 降为**辅助参考**（不全面、多 bug，仅补 ZCode 没有的自有构想：吉祥物/磨砂主题/几何数值，见 research/07 §8.3h）——同一元素先问 ZCode，防止多源混用成"大杂烩"。
