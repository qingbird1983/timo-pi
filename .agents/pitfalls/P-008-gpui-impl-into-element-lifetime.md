# P-008：返回 `impl IntoElement` 的视图方法在循环里复用 `&mut Context` 会编译失败

- 发现：2026-10-27，`apps/timo-desktop/src/features/sidebar.rs` 渲染会话列表。
- 现象：`for session in &self.sessions { rows.push(self.session_row(session, cx)); }` 报 `E0499: cannot borrow *cx as mutable more than once at a time`；把迭代改成 `.children(...map(...))` 则报 `captured variable cannot escape FnMut closure body`。
- 根因：`fn session_row(..., cx: &mut Context<Self>) -> impl IntoElement` 的返回类型默认捕获**全部输入生命周期**，包括 `&mut Context`。等价于返回一个借用了上下文可变引用的值，自然无法在循环里多次借用。元素本身其实没有借用 `cx`，是 RPIT 的保守捕获造成的。
- 修复：把该方法返回类型改成 `AnyElement`，并在末尾 `.into_any_element()` 擦除生命周期；列表先 `Vec` 收集再 `.children(rows)`。
- 预防：需要在一个 `cx` 上反复构造行的视图方法，返回 `AnyElement`（或 `Div` 等具体元素），不要用 `impl IntoElement`；一次性的局部 `v_flex()` 链仍可用 `impl IntoElement`。
- 同批小坑（gpui-kit 0.8）：
  - `ElementId` 不接受 `(&str, String)` 元组，动态 id 用 `format!("prefix-{key}")`。
  - `Button::disabled` 来自 `gpui_kit::component::Disableable`，`window.open_alert_dialog` 来自 `WindowExt`，`when`/`when_some` 来自 `FluentBuilder`，都要显式导入。
  - 组件图标只包含 `default-icons.txt` 子集；`X`/`Trash` 不在其中，删除用 `IconName::Delete` 或 `CircleX`。
