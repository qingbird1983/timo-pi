//! edit 的匹配、替换计算与审阅（父模块负责参数解析与工具定义）。

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::{Edit, normalize};
use crate::Result;
use crate::definition::ToolOutput;
use crate::error::ToolError;
use crate::review::{ReviewKind, ToolReview, ToolReviewRequest, ask, display_path};

pub(super) async fn apply_to_file(path: &Path, edits: &[Edit]) -> Result<ToolOutput> {
    let prepared = prepare(path, edits).await?;
    commit(path, prepared).await
}

/// 读旧内容、完成匹配与替换计算；任何匹配错误都在审阅之前返回。
async fn prepare(path: &Path, edits: &[Edit]) -> Result<Prepared> {
    let raw = tokio::fs::read(path)
        .await
        .map_err(|e| crate::tools::write::io_err(path.to_path_buf(), e))?;
    let bom = raw.starts_with(&[0xEF, 0xBB, 0xBF]);
    let body = if bom { &raw[3..] } else { &raw };
    let binary = std::str::from_utf8(body).is_err();
    let text = String::from_utf8_lossy(body).into_owned();
    let crlf = text.contains("\r\n");
    let normalized_text = text.replace("\r\n", "\n");
    let lines: Vec<String> = normalized_text.split('\n').map(String::from).collect();

    let mut replacements: Vec<Replacement> = vec![];
    let mut first_changed: Option<usize> = None;
    for edit in edits {
        let (start, len, fuzzy) = locate(&lines, &edit.old)?;
        // 精确匹配只替换命中片段（保留同行前后缀）；模糊回退按整行块替换。
        let replacement = if fuzzy {
            let block_start = byte_offset_of_line(&normalized_text, start);
            let block_end = byte_offset_of_line(&normalized_text, start + len - 1)
                + lines[start + len - 1].len();
            Replacement {
                start: block_start,
                end: block_end,
                text: edit.new.replace("\r\n", "\n"),
            }
        } else {
            let idx = normalized_text
                .find(&edit.old)
                .ok_or_else(|| ToolError::InvalidParams {
                    reason: "oldText not found".into(),
                })?;
            Replacement {
                start: idx,
                end: idx + edit.old.len(),
                text: edit.new.replace("\r\n", "\n"),
            }
        };
        if replacements
            .iter()
            .any(|r| replacement.start < r.end && r.start < replacement.end)
        {
            return Err(ToolError::InvalidParams {
                reason: format!(
                    "edits overlap in {}. Merge them into one edit.",
                    path.display()
                ),
            });
        }
        if first_changed.is_none() {
            first_changed = Some(start + 1);
        }
        replacements.push(replacement);
    }
    if replacements.is_empty() {
        return Err(ToolError::InvalidParams {
            reason: "no edits applied".into(),
        });
    }
    // 逆序应用，避免前一次替换移动后一次区间。
    replacements.sort_by_key(|r| r.start);
    let mut out = normalized_text.clone();
    for r in replacements.iter().rev() {
        out.replace_range(r.start..r.end, &r.text);
    }
    let new_lines: Vec<String> = out.split('\n').map(String::from).collect();
    if crlf {
        out = out.replace('\n', "\r\n");
    }
    if bom {
        out.insert(0, '\u{FEFF}');
    }
    Ok(Prepared {
        out,
        diff: build_diff(&lines, &new_lines),
        first_changed: first_changed.unwrap_or(1),
        before: if binary { None } else { Some(text) },
        binary,
        blocks: replacements.len(),
    })
}

pub(super) async fn apply_with_review(
    path: &Path,
    edits: &[Edit],
    review: Arc<dyn ToolReview>,
    tool_call_id: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) -> Result<ToolOutput> {
    let prepared = prepare(path, edits).await?;
    ask(
        Some(&review),
        cancel,
        ToolReviewRequest {
            tool_call_id: tool_call_id.to_string(),
            tool_name: "edit".into(),
            kind: ReviewKind::FileChange {
                path: path.to_path_buf(),
                display_path: display_path(cwd, path),
                before: prepared.before.clone(),
                after: prepared.out.clone(),
                binary: prepared.binary,
            },
        },
    )
    .await?;
    commit(path, prepared).await
}

async fn commit(path: &Path, prepared: Prepared) -> Result<ToolOutput> {
    tokio::fs::write(path, &prepared.out)
        .await
        .map_err(|e| crate::tools::write::io_err(path.to_path_buf(), e))?;
    Ok(ToolOutput {
        content: vec![timo_ai::Content::Text {
            text: format!(
                "Successfully replaced {} block(s) in {}.",
                prepared.blocks,
                path.display()
            ),
        }],
        details: json!({
            "diff": prepared.diff,
            "firstChangedLine": prepared.first_changed,
        }),
        ..ToolOutput::default()
    })
}

/// 审阅所需的计算结果（尚未写盘）。
struct Prepared {
    out: String,
    diff: String,
    first_changed: usize,
    before: Option<String>,
    binary: bool,
    blocks: usize,
}

/// 一次替换的字节区间与替换文本（相对规范化后的原文）。
struct Replacement {
    start: usize,
    end: usize,
    text: String,
}

/// 在行数组中定位 old 文本：精确（整文 indexOf）→ 模糊（规范化行滑窗）。
/// 返回（起始行, 覆盖行数, 是否模糊）。
fn locate(lines: &[String], old: &str) -> Result<(usize, usize, bool)> {
    let joined = lines.join("\n");
    let occurrences = joined.matches(old).count();
    if occurrences == 1 {
        let idx = joined.find(old).ok_or_else(|| ToolError::InvalidParams {
            reason: "oldText not found".into(),
        })?;
        let start = joined[..idx].matches('\n').count();
        let len = old.matches('\n').count() + 1;
        return Ok((start, len, false));
    }
    if occurrences > 1 {
        return Err(ToolError::InvalidParams {
            reason: format!(
                "Found {occurrences} occurrences of oldText — it must be unique (include more context)"
            ),
        });
    }
    // 模糊：规范化后按行滑窗（对齐 pi 语义：仅触及行回写）
    let norm_lines: Vec<String> = lines.iter().map(|l| normalize(l)).collect();
    let old_lines: Vec<String> = normalize(old).split('\n').map(String::from).collect();
    let mut hits: Vec<usize> = vec![];
    if old_lines.len() <= norm_lines.len() {
        for start in 0..=(norm_lines.len() - old_lines.len()) {
            if norm_lines[start..start + old_lines.len()] == old_lines[..] {
                hits.push(start);
            }
        }
    }
    match hits.len() {
        1 => Ok((hits[0], old_lines.len(), true)),
        0 => Err(ToolError::InvalidParams {
            reason: "oldText not found — it must match exactly including all whitespace \
                     (fuzzy pass also failed)"
                .into(),
        }),
        n => Err(ToolError::InvalidParams {
            reason: format!("Found {n} fuzzy occurrences of oldText — it must be unique"),
        }),
    }
}

fn byte_offset_of_line(joined: &str, line: usize) -> usize {
    joined
        .split('\n')
        .take(line)
        .map(|l| l.len() + 1)
        .sum::<usize>()
        .saturating_sub(1)
}

/// 简易 +/- diff（前 200 行内），pi details.diff 同构精神。
fn build_diff(old: &[String], new: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < old.len().max(new.len()) && out.lines().count() < 200 {
        let o = old.get(i);
        let n = new.get(i);
        match (o, n) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => {
                out.push_str(&format!("{}- {a}\n{}+ {b}\n", line_no(i), line_no(i)));
            }
            (Some(a), None) => out.push_str(&format!("{}- {a}\n", line_no(i))),
            (None, Some(b)) => out.push_str(&format!("{}+ {b}\n", line_no(i))),
            (None, None) => break,
        }
        i += 1;
    }
    out
}

fn line_no(i: usize) -> String {
    format!("{:>4} ", i + 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::definition::ToolCallCtx;
    use crate::tools::edit::definition;

    #[test]
    fn exact_unique_replacement() {
        let lines: Vec<String> = vec!["fn a() {}".into(), "let x = 1;".into(), "fn b() {}".into()];
        let (start, len, fuzzy) = locate(&lines, "let x = 1;").unwrap();
        assert_eq!((start, len, fuzzy), (1, 1, false));
    }

    #[test]
    fn duplicate_exact_is_error() {
        let lines: Vec<String> = vec!["dup".into(), "other".into(), "dup".into()];
        assert!(locate(&lines, "dup").is_err());
    }

    #[test]
    fn fuzzy_matches_trim_end_and_quotes() {
        let lines: Vec<String> = vec!["title = “Hello”  ".into(), "code( )".into()];
        let (start, len, fuzzy) = locate(&lines, "title = 'Hello'").unwrap();
        assert_eq!((start, len, fuzzy), (0, 1, true));
    }

    #[test]
    fn fuzzy_not_found_is_error() {
        let lines: Vec<String> = vec!["alpha".into()];
        assert!(locate(&lines, "omega").is_err());
    }

    async fn apply(name: &str, content: &str, edits: Vec<Edit>) -> String {
        let dir = std::env::temp_dir().join(format!("timo-edit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.txt");
        std::fs::write(&p, content).unwrap();
        apply_to_file(&p, &edits).await.unwrap();
        std::fs::read_to_string(&p).unwrap()
    }

    #[tokio::test]
    async fn single_line_edit_keeps_other_lines_exact() {
        // 真实事故回归：2 行文件被写成了 5 行（多空行 + 重复行）。
        let out = apply(
            "single",
            "status: draft\nowner: timo\n",
            vec![Edit {
                old: "status: draft".into(),
                new: "status: ready".into(),
            }],
        )
        .await;
        assert_eq!(out, "status: ready\nowner: timo\n");
    }

    #[tokio::test]
    async fn inline_edit_preserves_prefix_and_suffix() {
        let out = apply(
            "inline",
            "a = draft\nkeep\n",
            vec![Edit {
                old: "draft".into(),
                new: "ready".into(),
            }],
        )
        .await;
        assert_eq!(out, "a = ready\nkeep\n");
    }

    #[tokio::test]
    async fn multiline_exact_edit() {
        let out = apply(
            "multi",
            "one\ntwo\nthree\n",
            vec![Edit {
                old: "one\ntwo".into(),
                new: "uno\ndos".into(),
            }],
        )
        .await;
        assert_eq!(out, "uno\ndos\nthree\n");
    }

    #[tokio::test]
    async fn multiple_edits_apply_to_original() {
        let out = apply(
            "many",
            "a: 1\nb: 2\nc: 3\n",
            vec![
                Edit {
                    old: "a: 1".into(),
                    new: "a: 10".into(),
                },
                Edit {
                    old: "c: 3".into(),
                    new: "c: 30".into(),
                },
            ],
        )
        .await;
        assert_eq!(out, "a: 10\nb: 2\nc: 30\n");
    }

    #[tokio::test]
    async fn end_to_end_crlf_bom_preserved() {
        let dir = std::env::temp_dir().join(format!("timo-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.txt");
        std::fs::write(&p, "\u{FEFF}name = “A”\r\nkeep\r\n").unwrap();
        let edits = vec![Edit {
            old: "name = 'A'".into(),
            new: "name = 'B'".into(),
        }];
        apply_to_file(&p, &edits).await.unwrap();
        let out = std::fs::read(&p).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert_eq!(s, "\u{FEFF}name = 'B'\r\nkeep\r\n");
    }

    fn review_dir(name: &str) -> (std::path::PathBuf, Arc<Path>) {
        let dir = std::env::temp_dir().join(format!("timo-edit-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.txt"), "a\nb\n").unwrap();
        let cwd: Arc<Path> = Arc::from(dir.as_path());
        (dir, cwd)
    }

    fn review_ctx(cwd: Arc<Path>, review: Arc<dyn ToolReview>, old_text: &str) -> ToolCallCtx {
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        ToolCallCtx {
            tool_call_id: "call-1".into(),
            params: json!({
                "path": "f.txt",
                "edits": [{"oldText": old_text, "newText": "z"}]
            }),
            cancel: CancellationToken::new(),
            updates: tx,
            cwd,
            review: Some(review),
            nested: None,
        }
    }

    #[tokio::test]
    async fn reject_keeps_the_file_byte_identical() {
        use crate::review::test_support::RecordingReview;
        use crate::review::{ReviewDecision, ReviewKind};

        let (dir, cwd) = review_dir("reject");
        let review = RecordingReview::new(ReviewDecision::Reject {
            reason: "keep".into(),
        });
        let err = definition(cwd.clone())
            .run(review_ctx(cwd, review.clone(), "a"))
            .await
            .expect_err("rejected");
        assert!(err.to_string().contains("Rejected by user: keep"));
        assert_eq!(
            std::fs::read_to_string(dir.join("f.txt")).unwrap(),
            "a\nb\n"
        );
        match review.last().kind {
            ReviewKind::FileChange {
                display_path,
                before,
                after,
                ..
            } => {
                assert_eq!(display_path, "f.txt");
                assert_eq!(before.as_deref(), Some("a\nb\n"));
                assert_eq!(after, "z\nb\n");
            }
            other => panic!("file change expected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn accept_applies_the_precomputed_content() {
        use crate::review::ReviewDecision;
        use crate::review::test_support::RecordingReview;

        let (dir, cwd) = review_dir("accept");
        let review = RecordingReview::new(ReviewDecision::Allow);
        definition(cwd.clone())
            .run(review_ctx(cwd, review.clone(), "a"))
            .await
            .expect("allowed");
        assert_eq!(
            std::fs::read_to_string(dir.join("f.txt")).unwrap(),
            "z\nb\n"
        );
        assert_eq!(review.calls(), 1);
    }

    #[tokio::test]
    async fn match_failure_happens_before_review() {
        use crate::review::ReviewDecision;
        use crate::review::test_support::RecordingReview;

        let (_dir, cwd) = review_dir("nomatch");
        let review = RecordingReview::new(ReviewDecision::Allow);
        let err = definition(cwd.clone())
            .run(review_ctx(cwd, review.clone(), "missing"))
            .await
            .expect_err("no match");
        assert!(matches!(err, ToolError::InvalidParams { .. }));
        assert_eq!(review.calls(), 0);
    }
}
