//! 工具审阅 sheet：diff 生成、串行队列与决定回传（设计 01 §7、04 §4~§5）。

use std::collections::{HashSet, VecDeque};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::sheet::Sheet;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
use timo_kernel::{ReviewDecision, ReviewKind, ToolReviewRequest};
use tokio::sync::oneshot;

use crate::app::DesktopApp;
use crate::features::palette::{AcceptReviewAction, RejectReviewAction};

/// 单个文件 diff 上限（设计 04 §5）。
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
/// 单个文件 diff 行数上限。
const MAX_DIFF_LINES: usize = 5_000;

/// 一条等待决定的审阅请求。
pub struct PendingReview {
    /// 工具调用与副作用描述。
    pub request: ToolReviewRequest,
    /// 回传通道。
    pub respond: oneshot::Sender<ReviewDecision>,
    /// 已解析的 diff（文件改动且可展示时）。
    pub diff: Option<Entity<DiffState>>,
    /// 增删行数（可计算时）。
    pub stats: Option<(usize, usize)>,
    /// 是否过大/二进制，只展示摘要。
    pub summary_only: bool,
}

/// 审阅队列与“本会话允许”缓存（设计 04 §7）。
pub struct ReviewState {
    /// 串行队列，front 为当前展示项。
    pub queue: VecDeque<PendingReview>,
    /// sheet 是否已打开。
    pub open: bool,
    /// “本会话始终允许”勾选。
    pub always: bool,
    /// 本会话允许键（工具名 + 路径/命令）。
    pub allow: HashSet<String>,
    /// 拒绝原因（可选）。
    pub reason: Entity<TextareaState>,
    /// sheet 开合时用于承接快捷键的焦点。
    pub focus: FocusHandle,
}

impl ReviewState {
    /// 创建队列与原因输入框。
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            queue: VecDeque::new(),
            open: false,
            always: false,
            allow: HashSet::new(),
            reason: cx.new(|cx| {
                TextareaState::new(window, cx).placeholder(t!("review.reason").to_string())
            }),
            focus: cx.focus_handle(),
        }
    }

    /// 清空本会话允许项（设置页按钮）。
    pub fn clear_allow(&mut self) {
        self.allow.clear();
    }
}

/// 缓存键：文件用 `工具:路径`，命令用工具名。
#[must_use]
pub fn cache_key(request: &ToolReviewRequest) -> String {
    match &request.kind {
        ReviewKind::FileChange { path, .. } => {
            format!("{}:{}", request.tool_name, path.display())
        }
        ReviewKind::Command { .. } => request.tool_name.clone(),
    }
}

impl DesktopApp {
    /// 审阅请求到达：命中缓存直接放行，否则入队并标记 trace。
    pub(crate) fn handle_review_request(
        &mut self,
        request: ToolReviewRequest,
        respond: oneshot::Sender<ReviewDecision>,
        cx: &mut Context<Self>,
    ) {
        let key = cache_key(&request);
        if self.review.allow.contains(&key) {
            let _ = respond.send(ReviewDecision::Allow);
            return;
        }
        self.chat
            .update(cx, |chat, _| chat.mark_review(&request.tool_call_id));
        let (diff, stats, summary_only) = build_diff(&request, cx);
        self.review.queue.push_back(PendingReview {
            request,
            respond,
            diff,
            stats,
            summary_only,
        });
        cx.notify();
    }

    /// 打开下一条待审（若尚未打开）。
    pub(crate) fn open_next_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review.queue.is_empty() || self.review.open {
            return;
        }
        self.review.open = true;
        let weak = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, window, cx| match weak.upgrade() {
            Some(app) => app.update(cx, |this, cx| this.review_sheet(sheet, window, cx)),
            None => sheet,
        });
        // 焦点移入审阅 sheet，快捷键立即可用。
        // sheet 挂载时会先把焦点放在自己的宿主上，因此延后一帧再抢回来。
        let focus = self.review.focus.clone();
        cx.defer_in(window, move |_, window, cx| {
            window.focus(&focus, cx);
        });
    }

    /// 接受：回传允许、更新 trace、关闭 sheet 并串行打开下一条。
    pub(crate) fn accept_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_review(ReviewDecision::Allow, window, cx);
    }

    /// 拒绝：原因来自输入框（空则默认“用户拒绝”）。
    pub(crate) fn reject_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reason = self.review.reason.read(cx).value().trim().to_string();
        let reason = if reason.is_empty() {
            t!("review.default_reason").to_string()
        } else {
            reason
        };
        self.review
            .reason
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.finish_review(ReviewDecision::Reject { reason }, window, cx);
    }

    /// Esc/点击遮罩关闭 sheet = 拒绝（“未审阅”）。
    pub(crate) fn dismiss_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_review(
            ReviewDecision::Reject {
                reason: t!("review.dismissed").to_string(),
            },
            window,
            cx,
        );
    }

    fn finish_review(
        &mut self,
        decision: ReviewDecision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.review.queue.pop_front() else {
            self.review.open = false;
            return;
        };
        let allowed = matches!(decision, ReviewDecision::Allow);
        let call_id = pending.request.tool_call_id.clone();
        if allowed && self.review.always {
            let key = cache_key(&pending.request);
            self.review.allow.insert(key);
        }
        let _ = pending.respond.send(decision);
        self.chat
            .update(cx, |chat, _| chat.resolve_review(&call_id, allowed));
        self.review.open = false;
        window.close_sheet(cx);
        cx.defer_in(window, |this, window, cx| {
            this.open_next_review(window, cx);
        });
        cx.notify();
    }

    /// 构建审阅 sheet（标题、摘要/diff、原因输入与底部按钮）。
    fn review_sheet(
        &mut self,
        sheet: Sheet,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Sheet {
        let Some(front) = self.review.queue.front() else {
            return sheet;
        };
        let request = front.request.clone();
        let stats = front.stats;
        let summary_only = front.summary_only;
        let diff = front.diff.clone();
        let pending = self.review.queue.len();
        let always = self.review.always;
        let weak_check = cx.entity().downgrade();
        let weak_accept = cx.entity().downgrade();
        let weak_reject = cx.entity().downgrade();
        let weak_close = cx.entity().downgrade();
        let reason = self.review.reason.clone();

        let title = match &request.kind {
            ReviewKind::FileChange { .. } => t!("review.title_file").to_string(),
            ReviewKind::Command { .. } => t!("review.title_command").to_string(),
        };
        let click = t!("review.click_to_diff").to_string();
        let mut body = v_flex().gap_3().w_full();
        match &request.kind {
            ReviewKind::FileChange {
                path,
                display_path,
                before,
                after,
                binary,
            } => {
                let (added, removed) = stats.unwrap_or((0, 0));
                body = body.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().font_semibold().child(display_path.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "+{added} -{removed} · {}",
                                    if before.is_none() {
                                        t!("review.new_file")
                                    } else {
                                        t!("review.modified")
                                    }
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(path.display().to_string()),
                        ),
                );
                if summary_only || *binary || diff.is_none() {
                    body = body.child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_3()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {}",
                                if *binary {
                                    t!("review.binary").to_string()
                                } else {
                                    t!("review.too_large").to_string()
                                },
                                t!(
                                    "review.sizes",
                                    before = before.as_ref().map_or(0, String::len),
                                    after = after.len()
                                )
                            )),
                    );
                } else if let Some(diff) = diff {
                    body = body.child(div().h(px(420.)).w_full().child(Diff::new(&diff)));
                } else {
                    body = body.child(div().text_xs().child(click));
                }
            }
            ReviewKind::Command {
                command,
                cwd,
                shell,
            } => {
                body = body
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{shell} · {}", cwd.display())),
                    )
                    .child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .p_3()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_sm()
                            .child(command.clone()),
                    );
            }
        }
        body = body.child(Textarea::new(&reason).h(px(64.)));

        let footer = h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                Checkbox::new("review-always")
                    .checked(always)
                    .label(t!("review.always").to_string())
                    .on_click(move |checked, _, cx| {
                        if let Some(app) = weak_check.upgrade() {
                            app.update(cx, |this, _| this.review.always = *checked);
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("review-reject")
                    .outline()
                    .label(t!("review.reject").to_string())
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak_reject.upgrade() {
                            app.update(cx, |this, cx| this.reject_review(window, cx));
                        }
                    }),
            )
            .child(
                Button::new("review-accept")
                    .primary()
                    .label(t!("review.accept").to_string())
                    .on_click(move |_, window, cx| {
                        if let Some(app) = weak_accept.upgrade() {
                            app.update(cx, |this, cx| this.accept_review(window, cx));
                        }
                    }),
            );

        let heading = h_flex()
            .gap_2()
            .items_center()
            .child(div().text_sm().font_semibold().child(title))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("review.pending", n = pending).to_string()),
            );

        let body = body
            .track_focus(&self.review.focus)
            .key_context("ReviewSheet")
            .on_action(cx.listener(|this, _: &AcceptReviewAction, window, cx| {
                this.accept_review(window, cx);
            }))
            .on_action(cx.listener(|this, _: &RejectReviewAction, window, cx| {
                this.reject_review(window, cx);
            }));

        sheet
            .size(px(480.))
            .title(heading)
            .footer(footer)
            .on_close(move |_, window, cx| {
                if let Some(app) = weak_close.upgrade() {
                    app.update(cx, |this, cx| this.dismiss_review(window, cx));
                }
            })
            .child(body)
    }
}

fn needs_summary(before: Option<&str>, after: &str, binary: bool) -> bool {
    binary
        || before.map_or(0, str::len) + after.len() > MAX_FILE_BYTES
        || after.lines().count() > MAX_DIFF_LINES
        || before.map_or(0, |text| text.lines().count()) > MAX_DIFF_LINES
}

fn build_diff(
    request: &ToolReviewRequest,
    cx: &mut Context<DesktopApp>,
) -> (Option<Entity<DiffState>>, Option<(usize, usize)>, bool) {
    let ReviewKind::FileChange {
        display_path,
        before,
        after,
        binary,
        ..
    } = &request.kind
    else {
        return (None, None, false);
    };
    if needs_summary(before.as_deref(), after, *binary) {
        return (None, None, true);
    }
    let previous = before.as_deref().unwrap_or("");
    let diff = similar::TextDiff::from_lines(previous, after);
    let (mut added, mut removed) = (0, 0);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added += 1,
            similar::ChangeTag::Delete => removed += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    let header_old = if before.is_none() {
        "/dev/null".to_string()
    } else {
        format!("a/{display_path}")
    };
    let patch = diff
        .unified_diff()
        .context_radius(3)
        .header(&header_old, &format!("b/{display_path}"))
        .to_string();
    let entity = DiffFile::parse(&patch)
        .ok()
        .filter(|files| !files.is_empty())
        .map(|files| cx.new(|cx| DiffState::new(files, cx)));
    (entity, Some((added, removed)), false)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{MAX_DIFF_LINES, MAX_FILE_BYTES, cache_key, needs_summary};
    use gpui_kit::component::diff::DiffFile;
    use std::path::PathBuf;
    use timo_kernel::{ReviewKind, ToolReviewRequest};

    fn file(before: Option<&str>, after: &str, binary: bool) -> ToolReviewRequest {
        ToolReviewRequest {
            tool_call_id: "c1".into(),
            tool_name: "write".into(),
            kind: ReviewKind::FileChange {
                path: PathBuf::from("/work/a.txt"),
                display_path: "a.txt".into(),
                before: before.map(str::to_string),
                after: after.into(),
                binary,
            },
        }
    }

    #[test]
    fn patch_parses_with_expected_content() {
        let diff = similar::TextDiff::from_lines(
            "a
b
c
", "a
B
c
",
        );
        let patch = diff
            .unified_diff()
            .context_radius(3)
            .header("a/a.txt", "b/a.txt")
            .to_string();
        let files = DiffFile::parse(&patch).expect("parse");
        assert_eq!(files.len(), 1);
        assert!(patch.contains("+B"));
        assert!(patch.contains("-b"));
    }

    #[test]
    fn cache_key_uses_path_or_tool() {
        assert_eq!(cache_key(&file(None, "x", false)), "write:/work/a.txt");
        let command = ToolReviewRequest {
            tool_call_id: "c2".into(),
            tool_name: "bash".into(),
            kind: ReviewKind::Command {
                command: "ls".into(),
                cwd: PathBuf::from("/work"),
                shell: "bash".into(),
            },
        };
        assert_eq!(cache_key(&command), "bash");
    }

    #[test]
    fn summary_boundaries_cover_binary_and_limits() {
        assert!(needs_summary(Some("a"), "b", true));
        assert!(!needs_summary(Some("a"), "b", false));
        let huge = "x
"
        .repeat(MAX_DIFF_LINES + 1);
        assert!(needs_summary(None, &huge, false));
        let big = "x".repeat(MAX_FILE_BYTES + 1);
        assert!(needs_summary(None, &big, false));
    }
}
