# P-022 NSH 焦点代理在无障碍激活时产生重复 child，宿主重绘即 panic

## 现象

Phase 4 真机验收中，只要插件面板在场（输入框 `autoFocus`）且系统无障碍接口被激活——
打开原生选文件夹对话框，或 UIA 自动化客户端连接/断开——宿主在下一次重绘时直接崩溃：

```text
thread 'main' panicked at accesskit_consumer-0.38.0/src/tree.rs:225:21:
TreeUpdate includes duplicate child #10705834630348379779
  accesskit_consumer::tree::State::update
  accesskit_windows::adapter::Adapter::update_if_active
  gpui_fast_windows::window::a11y_tree_update
  gpui::window::Window::draw_roots
```

本轮 5 次复现（导航、选目录、焦点后连 UIA、安装后点详情、再次点详情），重复 id 每次不同；
崩溃后窗口卡死（约 260MB），需 `taskkill`，偶尔残留约 40K 桩进程。没有 NSH 面板时不复现。

## 根因

三方栈组合缺陷，不是 Timo 代码路径：

1. NSH 0.3.0 `access.rs` 在无障碍激活且 NSH 元素有焦点时，会预置焦点代理
   `div().id("nsh-a11y-focus")`，用 `take_subtree` 移走焦点元素子树，再把代理节点
   顶到父节点首位、替换原焦点节点 id，并让代理承载焦点元素的原 children。
2. gpui-fast 0.1.10 的 `A11yNodeBuilder::push_leaf` 维护全局 `seen_ids`；合成节点构建
   时先把所有 entry 推入，再 `set_children(top)`。
3. `accesskit_consumer` 0.38.0 对整份更新里**任何**重复 child id 都 panic（`seen_child_ids`
   只遍历当次 `update.nodes`，即重复就在这一帧的 `TreeUpdate` 内部）。

插桩结论（临时副本，已删除）：

- 重复 id 全部命中 NSH 合成节点（`hash(("nsh", surface_key, view_id))`）：
  `#10705834630348379779`=key1/view18、`#7223334741684448574`=key1/view66、
  `#5220590099689921369`=key1/view72、`#8742214482047128939`=key2/view42，
  都是随数据/焦点移动的列表行，纠正了「重复 id 由 GPUI 生成」的初判。
- `synthetic` 打印只见 NSH 面板子树，曾据此误判「builder 无重复、来自持久树残留」；
  复验探针在整份更新层面记录到同一帧 3 个重复引用，确认重复在 `TreeUpdate` 内部，
  与焦点代理 + 列表行重排同帧发生有关。

**上游版本核查（2026-10-30）**：`nsh-platform-gpui` 最新 0.3.0（当前锁定）；
`gpui-fast` 最新 0.1.12 的 a11y 代码与 0.1.10 相同，仍依赖 `accesskit ^0.24` /
`accesskit_windows 0.34`（consumer 0.38），无升级即修复的路径。

## 修复

**已修复（2026-10-30，ADR-0018）**：判重发生在**单次 TreeUpdate 内部**（`seen_child_ids`
只来自 `update.nodes`），因此在 consumer 边界做消毒即可确定性止崩：

- `vendor/accesskit_consumer/`（0.38.0 原样副本 + 补丁），根 `Cargo.toml` 用
  `[patch.crates-io]` 指向；`workspace.exclude` 排除 fmt/clippy/test。
- 补丁：`State::update` 先 `strip_duplicate_children`（保留首个父节点）并 `log::warn!`，
  替代原 panic；单测 `strip_duplicate_children_keeps_first_parent`。
- 真机复验：面板输入框聚焦 + UIA 反复连接/断开 + 查询串逐字变化 + 最小化/还原；
  探针版记录到 `stripped 3 duplicate child reference(s)`（同一更新旧版必 panic），
  补丁版无 panic、进程存活。
- 补丁只防崩、不修正树；上游修复后按 `PATCH.md` 删除。仍建议向 NSH/gpui-fast 报 issue。

## 预防

1. 含焦点控件的面板验收必须包含「无障碍激活」用例（开 Narrator 或接 UIA 后重绘），
   不能只在普通鼠标操作下验收。
2. 升级 NSH/gpui-fast/gpui 任一版本后，先跑一次「面板 + 原生对话框 + 重绘」冒烟。
3. 第三方 UI 栈的已知缺陷要写进阶段报告与 `CONTEXT.md`，并在依赖锁文件里固定版本，
   避免无意升级放大影响面。
