# H-012：审查收债完成

- 日期：2026-10-28
- 主题：清偿 2026-10-27 三路审查的 A/B/C/D 清单（A1、B1~B9、C1~C10、D1~D9）

## 本轮做了什么

1. **A 组（数据安全）**：`SessionWriter::open` 续写前调用 `repair_tail`——末尾无换行时，整行可解析则补换行，否则截断到上一换行；`IdGen` 增加进程级 `shared()` 单例。
2. **B 组（代码缺口）**：桌面壳改走 `timo_kernel::config`/`data_dir`；MCP 等待受 `ctx.cancel` 约束；扩展工具 per-call `AtomicBool` 交给 QuickJS 中断处理器；Unix 子进程 `process_group(0)` + 整组 `kill`；钩子单扩展失败记诊断后继续；B2/B3/B9 语义按实现定稿进 architecture/02/04。
3. **C 组（文档回填）**：C9 历史阶段文档加依赖修订注记；C10 依赖准入表补 `time`，同步补登 `globset`、`unicode-normalization`。
4. **D 组（复杂度）**：删除未接线执行模式/队列模式、零调用 API、未消费配置等约 330 行与 1 个依赖；日期换算与路径通配分别复用 `time` / `globset`（先证语义等价）。

## 验证

- workspace **419 tests** 全绿；fmt / clippy `-D warnings` / check-size 通过。
- 变异验证：B5 跨扩展隔离、`globset` 的「尾 `/**` 吞 0 段」与「元字符转义」两层、A1 修复均确认恢复前红；内核取消分支的破坏性变异被工具分类器拦截，改由宿主层变异 + 内核测试双覆盖。
- `clean-build`：22.1GiB → 14.0GiB（增量 1.1 / deps 11.7）。

## 遗留与下一步

- **Phase 4（GUI 插件系统）**：先定稿 `docs/roadmap/phase-4-gui-plugins.md` 再启动。
- 本地收债提交未 push；推送后核远端 CI。
- Unix 子进程组清理无 Windows CI 覆盖，随平台打包实测。
- 未决：NSH 人工验收、生产签名密钥分发、无头 stderr 噪音、桌面壳遗留功能。

## 已提取的沉淀

- P-016：错误隔离会改变派发返回值（B5 让两个既有预算测试从 Err 变 Ok(Null)）。
