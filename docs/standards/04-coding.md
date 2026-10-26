# 规范 04：代码规范

- 状态：定稿（Phase 1 开工前允许修订，修订走 ADR）
- 日期：2026-10-26

## 1. 工具链与硬门槛

- Rust stable 最新版；`cargo fmt`（CI 检查）+ `cargo clippy -- -D warnings`（CI 硬门槛）。
- workspace 统一 `Cargo.lock` 提交入库；依赖一律锁版本。

## 2. 错误处理

- 库 crate：`thiserror` 定义类型化错误；错误链保留来源。
- 壳（apps/）：给错误补充用户可读上下文后展示；一切错误走事件总线 error 事件，**禁止 panic 外泄**。
- `unwrap()`/`expect()`：库代码禁用（clippy `unwrap_used` = deny）；确属不变量处须注释论证原因；测试中自由使用。

## 3. unsafe 与并发

- **禁止 unsafe**；确需（预期几乎没有）必须先写 ADR 说明必要性与审计方案。
- 异步一律 tokio；取消以 `CancellationToken` 显式传递（architecture/03 接缝 #7）；禁止"吞取消"（select! 分支必须覆盖取消臂）。
- 共享状态优先消息/事件而非锁；必须加锁时文档注明锁序。

## 4. 公共 API 纪律

- 公开项 100% doc comment（首行中文一句话 + 必要示例）；`missing_docs` = warn，Phase 1 末转 deny。
- 公共 API 变更 = 语义级变更：先文档后代码；对外承诺一经发布按 semver + 弃用流程（`#[deprecated]` + 迁移说明）。
- 内部细节一律 `pub(crate)` 起步，公开须有理由。

## 5. 依赖准入

- 原则：少而稳。优先官方/主流维护 crate；同等功能不引入第二个。
- **新依赖流程**：在本篇**附录 A（依赖准入表）**登记——名称、版本、用途、为何必须、维护活跃度、版本策略；replacements/升级同样登记。
- 禁止引入：未审计的 git 依赖、yanked 版本、与既有依赖功能重叠的包。

## 6. JS/TS 侧（A 轨 shim 与 B 轨宿主环境，Phase 2/4 适用）

- 只允许"打包产物"执行（rolldown 输出），禁止运行期动态 eval/远程加载代码。
- 宿主注入 API 面白名单化：环境默认无能力，能力显式注入（standards/06 §4）。
- TS 源码质量以 oxclint 类工具为准绳（Phase 2 选型），不另立体系。

## 7. 注释与可读性

- 注释写"为什么/约束"，不复述代码；临时方案必须带 `TODO(ADR-xxxx)` 或 issue 引用，禁止无主 TODO。
- 单测与实现同文件（Rust 惯例）；集成测试入 `crates/*/tests/`。

## 8. 写码纪律参考（ponytail）

- 动笔前参照 `.agents/skills/ponytail/SKILL.md`（MIT，vendored）：**最小完整改动**——只写任务必需的代码，优先复用仓库既有结构与依赖，不造没人要的抽象/包装/"为以后准备"的代码；但**绝不削减**边界校验、防数据丢失的错误处理、安全与必要测试。
- 与本规范的关系：ponytail 是工作纪律，本规范是硬门槛；冲突时以本规范为准。其 `shortcut:` 快捷标记在本项目按 §7 落地为 `TODO(...)` 形式。

## 附录 A：依赖准入表（Phase 1 初版，2026-10-26）

> 登记即准入，新依赖**先登记后引入**。版本为开工时 crates.io 最新稳定线；实际锁定以根 `Cargo.lock` 为准（standards/04 §1）。

| 依赖 | 版本线 | 用途 | 为何必须 | 维护方 | 备选与否决理由 |
|---|---|---|---|---|---|
| tokio | 1.x | 异步运行时 | 全项目异步基座（ADR-0001 既定） | tokio-rs 官方 | 无（既定） |
| tokio-util | 0.7 | CancellationToken | 接缝 #7 取消贯穿的唯一载体 | tokio-rs 官方 | 无 |
| reqwest | 0.13 | HTTP 客户端（流式响应） | 协议层唯一 HTTP 通道 | seanmonstar 等 | 裸 hyper：过底层，弃 |
| serde / serde_json | 1.x | 序列化 | 全仓 JSON 通货 | serde 官方 | 无 |
| thiserror | 2.x | 库错误派生 | standards/04 §2 硬门槛 | dtolnay | 无 |
| async-trait | 0.1 | async trait 对象化 | ProviderApi 需 dyn 分发（原生 async fn in trait 不支持 dyn） | dtolnay | 手写 Box<Future>：样板噪声，弃 |
| jsonschema | 0.6x | JSON Schema 校验 | 接缝 #5 唯一校验实现 | jsonschema-rs | 手写校验器：易错且即第二套体系，弃 |
| regex | 1.x | 正则引擎 | grep 工具方言（ripgrep 同引擎，D1） | rust-lang 官方 | 无 |
| ignore | 0.4 | .gitignore 目录遍历 | find/grep 遍历（D1，ripgrep 作者出品） | BurntSushi | 手写遍历：重造 ripgrep，弃 |
| image | 0.25 | 图像嗅探/缩放 | read 工具图像支持（D8） | image-rs | 无（pi 用 sharp 的对应物） |
| ulid | 3.x | 会话 entry id | 设计 04 §3：字典序=时间序+防回拨 | 社区（活跃） | uuid v7：无字典序保证，弃 |
| dirs | 7.x | 平台数据目录 | 设计 05 §1 | dirs-dev | 手写平台分支：无谓分叉，弃 |
| clap | 4.x | CLI 参数解析 | timo-cli | clap-rs 官方 | 无 |
| rmcp | 3.x | MCP 官方 SDK | ADR-0005 既定 | ModelContextProtocol 官方 | 无（既定） |
| futures | 0.3 | Stream 工具件 | 流适配组合子 | rust-lang 官方 | 无 |
| tempfile | 0.3 | 临时文件 | bash 全量输出/MCP 截断全文落盘（pi 同构行为） | Steven Allen（Rust 库团队） | 手写临时名：竞态风险，弃 |
| base64 | 0.22 | base64 编解码 | 图像内容/HTTP 基础件 | marshalx 等社区 | 手写：无谓，弃 |
| sha2 | 0.10 | sha256 | MCP 工具名防撞哈希（设计 03 §1，pi 同构） | RustCrypto | 无 |
| rolldown | 1.2.x | 安装期 JS/TS 打包 | ADR-0006 既定；内置 oxc 0.155 完成 TS 转换与依赖内联 | rolldown 团队 | 自写打包器：模块图/重导出/依赖内联正确性成本高，弃 |
| rquickjs | 0.14.x | QuickJS 宿主绑定 | ADR-0006 既定；支持 ES2020、异步与中断处理 | DelSkayn（活跃） | deno_core：ADR-0006 备选，体积/复杂度高，暂不采用 |

- 每 crate 直接依赖数 ≤ 15（standards/08 §4 软上限达标；workspace 共享依赖去重后不重复计入）。
- 新增行格式照抄本表；升级在同一行更新版本线并在 CHANGELOG 记一笔。
