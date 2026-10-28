# H-011：Phase 3.5 收尾

- 日期：2026-10-28
- 主题：项目信任门控撤回、codemode CI 竞态修复、clean-build 与 tag `phase-3.5.end`

## 本轮做了什么

1. **信任门控整体撤回**：按所有者决定，产品不做 VS Code 式工作区信任。删除内核 `trust` 模块与决策链、CLI `/trust` 与 `--approve/--no-approve`、扩展 `project_trust` 事件（`ctx.isProjectTrusted()` 恢复恒 `true`）、子代理透传、桌面五项弹框与 `trust.json`；项目资源恢复无条件加载。删除详设 12、D80~D87、P-014；新增 ADR-0015。
2. **codemode CI 竞态修复**：SHIM 安装期撞 deadline 时按中断原因重述；主循环主动中断后不被脚本收尾错误覆盖。同组 limits 用例重复 20 次全绿，CI 复跑绿。
3. **阶段收尾**：门禁 fmt / clippy `-D warnings` / workspace 413 tests / check-size 全绿；`clean-build` 24.3GiB → 14.0GiB（incremental 0.0 / deps 12.8）；报告补阶段收尾，看板、docs 索引、CHANGELOG、CONTEXT 改为完成态；打 tag `phase-3.5.end`。

## 遗留与下一步

- **审查收债**（前置已满足）：按 `docs/roadmap/audit-debt.md` 清偿，A1 会话半行拼接最先（P-012），B1 桌面壳穿透门面次之。
- **Phase 4（GUI 插件系统）**：按 `docs/roadmap/phase-4-gui-plugins.md` 启动。
- **桌面壳视觉精修**：待 Phase 3.5 与 Phase 4 均收口后启动（research/07 → 设计稿 → 批次实现）。
- 未决：NSH 人工验收、生产签名密钥分发、无头 stderr 噪音、桌面壳遗留功能（roadmap phase-3 §6）。

## 已提取的沉淀

- P-015：主动中断的原因必须覆盖收尾错误（codemode 两条慢 runner 竞态）。
- P-013：Git Bash 会把 `/命令` 改写成路径（真机验收加 `MSYS_NO_PATHCONV=1`）。
- ADR-0015：不做项目信任门控（产品定位优先于上游行为对齐）。
