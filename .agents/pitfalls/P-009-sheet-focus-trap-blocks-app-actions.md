# P-009：sheet 的焦点陷阱让 App 根级快捷键失效

- 发现：2026-10-27，`apps/timo-desktop/src/features/review.rs` 接入 Ctrl+Enter / Ctrl+Backspace。
- 现象：审阅 sheet 打开后按 Ctrl+Enter 没有任何反应；`#[gpui_kit::test]` 里等待 `oneshot` 的测试直接挂住（`blocking_recv` 永不返回）。
- 根因：`gpui-component` 的 `Sheet` 用 `BaseSheet` 包了一层宿主元素，带 `track_focus` + `focus_trap("sheet", …)` + `key_context("sheet")`。sheet 挂在 `Root` 的 overlay 层，是 App 根元素的**兄弟**而不是子节点；焦点在 sheet 宿主上时，按键沿焦点路径分发，App 根上的 `.on_action(...)` 与 `None` 上下文的 `KeyBinding` 都不在路径里。
- 修复：
  1. `ReviewState` 持有自己的 `FocusHandle`；sheet body 加 `.track_focus(&focus)` + `.key_context("ReviewSheet")` + `.on_action(...)`。
  2. 快捷键绑定改成 `KeyBinding::new("secondary-enter", AcceptReviewAction, Some("ReviewSheet"))`。
  3. `open_sheet` 宿主挂载时会先抢焦点，因此在 `open_next_review` 里用 `cx.defer_in` **延后一帧**再 `window.focus(&review.focus)`。
- 预防：任何需要键盘快捷键覆盖层（Sheet/Dialog）都在**覆盖层子树内部**承接焦点与 action，不要依赖 App 根的 action；测试若用 one-shot 等待，先确认 action 能派发。
- 同批小坑：
  - 关闭面板/设置后焦点可能停在被移除的元素上，全局快捷键随之失灵；关闭时显式把焦点还给输入框（`focus_composer`）。
  - `h_flex` 默认交叉轴居中；设置页这种“标题在顶、内容在下的页”要给内层 `h_flex` 补 `items_start()`，否则整块内容垂直居中。
  - 命令面板的查询是持久状态，重新打开必须 `CommandState::set_query("", …)`，否则上次过滤词残留。
