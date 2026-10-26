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
- **新依赖流程**：在 `docs/standards/04-dependencies.md`（Phase 1 建立为准入登记表）登记——名称、用途、为何必须、维护活跃度、版本策略； replacements/升级同样登记。
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
