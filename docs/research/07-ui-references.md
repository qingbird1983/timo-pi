# 调研 07：桌面壳 UI 参考清单（视觉精修批次的前置素材）

- 状态：**草稿**（2026-10-27 图片搜索第一轮；所有者筛选增补后，视觉精修批次启动时转定稿）
- 关联：ADR-0013；roadmap §1"桌面壳视觉精修"条目；design/phase-3/01-ui-architecture.md（信息架构，已定稿）；gpui-kit-design-guides（规范设计语言，vendored）

## 1. 用途与使用方法

为桌面壳**视觉精修**（产品级 UI）提供参考方向。本清单是素材池，不是设计稿：

1. 浏览图片，**划掉**不喜欢的产品/风格，把自己找到的参考随时补进来（链接或本地文件路径均可注明）；
2. 视觉精修批次启动时，从筛选后的清单提炼 Timo 的视觉方向词（密度/明暗/圆角/留白），写成视觉设计稿（届时立 `design/` 文档，遵守 gpui-kit-design-guides）；
3. **只借鉴布局语言与质感方向，不复制任何产品的具体视觉**。

> 图片链接是搜索代理的缓存快照，可能失效；每个产品同时给出官方站链接（长期有效，可现场体验真实 UI）。

## 2. A 类：AI 助手桌面端（聊天流为中心，留白多）

| 产品 | 官方站 | 参考图 | 设计语言笔记 |
|---|---|---|---|
| Claude Desktop | [claude.ai/download](https://claude.ai/download) | [会话侧栏+设置入口](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/8f5fbb9074d1.png)（官方来源） | 左侧会话列表极简（纯文字条目，弱分组）；主区单列聊天流，行宽约束（约 48em 居中），留白大；暖色中性底，强调色克制 |
| ChatGPT Desktop | [chatgpt.com](https://chatgpt.com) | [设置页（权限开关/语言/菜单栏）](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/74dd8fd30af5.png)；[对话流备选](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/ed33f03d22d7.jpg) | 设置页是好的参照：左分组右内容的两栏设置、逐项开关型权限管理（对应我们"审批档位"的设置面）；消息气泡极轻（近无边框） |

## 3. B 类：开发者工具（信息密度高，面板化，暗色专业感）

| 产品 | 官方站 | 参考图 | 设计语言笔记 |
|---|---|---|---|
| Cursor | [cursor.com](https://cursor.com) | [欢迎屏+模型选择+Agent Review 弹窗](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/6be7e694c919.jpg)；[暗色主界面](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/5294086f05de.png) | 模型选择下拉、Agent Review 弹窗的形态与我们的审阅面板直接对应；暗色为主，面板边界用 1px 细线+微弱底色差区分，不重投影 |
| Zed | [zed.dev](https://zed.dev) | [Agent 面板（线程列表+消息流+底部输入）](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/ce6022361478.webp)（官方）；[LLM Providers 设置](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/5a79d0e7b19a.png) | **与本壳同构度最高的一张**：线程列表=我们会话侧栏、消息流=聊天区、底部输入=composer；Rust 原生渲染（同 GPUI 血统），密集但清爽，等宽字体参与界面身份 |
| Warp | [warp.dev](https://www.warp.dev) | （首轮搜索无理想图，待补） | AI 终端：块状命令输出（block）是它的招牌——对我们"工具过程 trace 折叠卡"的块状呈现有直接借鉴价值 |

## 4. C 类：现代效率工具（精致 SaaS 质感，层次细腻）

| 产品 | 官方站 | 参考图 | 设计语言笔记 |
|---|---|---|---|
| Linear | [linear.app](https://linear.app) | [暗色主界面 A](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/bbee2b9f3fb7.jpg)；[暗色主界面 B](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/53fd6d9e945d.png) | 公认的"暗色 SaaS 质感"标杆：侧栏分组+紧凑行高、状态徽章（对应我们运行中/完成状态）、快捷键提示内嵌在 UI 里（对应我们"主路径必须无鼠标可完成"） |
| Notion | [notion.com](https://www.notion.com) | [官方界面图（超清）](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/add0eed6a990.png)；[侧栏工作区备选](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/ce84baca5d2e.jpg) | 侧栏信息组织（分组/悬停操作/折叠）与文档流的呼吸感；亮色为主，图标线性细描 |
| Raycast | [raycast.com](https://www.raycast.com) | [命令面板+动作菜单](https://image-search-mcp-cn-beijing.oss-cn-beijing.aliyuncs.com/image-search-mcp/images-ppt/aa7b1eefb13e.jpg) | **命令面板的直接参照**（对应 Phase 3 待做的命令面板）：输入即过滤、行内图标+快捷键右对齐、actions 二级菜单、毛玻璃浮层 |

## 5. 共性提炼（对照 01-ui-architecture 的骨架）

1. **左会话列表 + 中内容流 (+ 右详情/审阅) 是绝对主流**——我们的 248 侧栏 + 聊天流 + 400 审阅面板与全部三类产品一致，**骨架不需要推翻**；
2. **会话即一级对象**：Zed 的"New Thread"置顶按钮、Claude/ChatGPT 的会话列表，与我们的"新建任务 + 项目分组"同构；
3. **暗色优先、明暗双主题标配**：四家默认暗色；我们已走 gpui-kit 语义 token，方向一致；
4. **命令面板是效率标配**：Raycast/Linear 的 cmdk 模式是 Phase 3 命令面板批次的现成蓝本；
5. **设置页两栏模式**：左分组右内容、权限逐项开关（ChatGPT 设置页、Zed providers）——对应我们设置/钥匙串批次与"审批档位"设置面；
6. **状态反馈进头部与状态栏**：运行中徽章、进度、用量（Linear 状态徽章、我们 Chat head 的进度条设计一致）；
7. **块状过程呈现**：Warp 的 block、各家工具调用的折叠卡——对应我们工具过程 trace 的折叠卡，视觉精修时的重点打磨对象。

## 6. 待补清单

- Warp 主界面理想截图（官方站有，搜索图库暂无）
- Claude/ChatGPT 暗色主界面各一张（首轮拿到的是设置页/亮色）
- 所有者自己收集的参考（随时补充到对应类别）

## 7. 流转约定

- 本篇保持草稿态直至视觉精修批次启动；届时：所有者定稿筛选 → 提炼视觉方向词 → 立 `design/` 视觉设计稿（≤500 行/篇，gpui-kit-design-guides 合规）→ 实现批次。
- 本篇只装参考素材与笔记，不装设计决策；设计决策进 design/ 文档，重大取舍进 ADR。
