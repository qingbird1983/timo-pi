# H-013 Phase 4 P1-c：slot 运行时与宿主 API v1

- 日期：2026-10-29
- 范围：Phase 4 P1-c（设计 03 slot / 04 宿主 API v1 / 05 运行时隔离）

## 本轮做了什么

1. **宿主 API v1 接线**
   - `HostApi::start` 每实例起一个宿主线程（current-thread tokio），`HostChannel` handler
     只把调用入队；查询、存储、`runTool` 在宿主线程执行，结果用 `HostReply` 回包。
   - 查询/动作经内核契约层 `timo_kernel::plugins::host` 判定：未知/未声明 E740，参数 E741，
     挡位 E722（`runTool` 由内核注册表的只读注解/工具类别裁决），查询 3s、`runTool` 30s（E742）。
   - `models.list` 与 `runTool` 经 `PluginRequest` 回内核，复用审阅/取消/审计管线。
   - `openPath` 先过 `check_open_path`（工作目录 + 生效 `fs.read` 作用域，E722）。
2. **存储与主题**
   - `PluginStorage` 绑定内核数据根：`<data>/plugins/<id>/storage.json`；单值 64KB、总量 1MB，
     E744，临时文件 + rename。
   - `theme.update` 走 `sanitize_theme`；越界键产生 E743 诊断，合法 token 形成补丁覆盖到
     `theme.changed` 与 `set_theme_json`。
3. **运行时**
   - `panel` 槽位：B 轨、已启用、无 Error、声明 panel；按 order/标题/id 排序取前 5（E751）。
   - 装载：清单复核（id/版本/nsh_id）→ `verify_package` → `AppView::new` → `run_package`；
     1s 健康门后 `accept_package`，失败移除新实例并保留旧 Running 实例。
   - 看门狗 2s 一轮、等 1s、连续 2 次无 pong → stop + E742；运行期崩溃 E742；60s 内 2 次自动禁用。
   - 失败即写 `failed`，同版本不再自动重载；用户关/开插件才清（对应设计 05「不自动重启」）。
   - `sessions.changed` 500ms 节流；退出 `shutdown` 统一 `stop()`。
4. **市场安装接线**：市场卡片「安装」发送 `InstallMarketPlugin` → 下载 → `verify_dir` →
   `pending_install` 挡位确认卡 → 内核安装账本；UI 测试断言命令确实发出。
5. **体量**：`host_api.rs` 拆出 `host_worker.rs`；`runtime.rs` 拆为 `runtime/{mod,load,events}.rs`；
   内核 `host/mod.rs` 拆出 `path.rs`。全部 ≤500 行。

## 验证

- `cargo fmt --all`、`cargo clippy --workspace --all-targets -- -D warnings` 干净。
- `cargo test --workspace`：**495 passed / 0 failed**。
- `scripts/check-size` OK；`target/` 清理见 CHANGELOG。
- 手工验收未做：NSH 真实插件装载与看门狗需要真机包，CI 仅覆盖拒绝路径。

## 遗留与下一步

- **P1-d**：slot 容器渲染与诊断面（侧栏 panel 起步）；容器到位前不扩 slot 种类。
- 设计 05 §7 的「stop 后 3s 未退出标 Leaked」未实现，随 P1-d 补。
- 健康门无首帧信号（NSH 0.3.0 无公开回调），已在设计 05 §3 记实现注记。
- 本地提交尚未 push；推送后核远端 CI。

## 已提取的沉淀

- 新增 [P-018](../pitfalls/P-018-plugin-storage-root-vs-app-dir.md)：插件存储根与 NSH
  私有目录不能复用一个 `data_dir` 字段；拆 impl 时跨模块方法要 `pub(super)`。
- 累计教训不重复：P-004/P-006（上游 API 以源码为准）、P-016（错误隔离改变返回值）继续适用。
