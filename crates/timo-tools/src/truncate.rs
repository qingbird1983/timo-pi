//! 截断常量与算法（设计 03 §2；pi truncate.ts 同构）。

/// 行数上限。
pub const MAX_LINES: usize = 2000;

/// 字节上限（50KB；先到先赢）。
pub const MAX_BYTES: usize = 50 * 1024;

/// grep 匹配行截断长度。
pub const GREP_MAX_LINE: usize = 500;

/// 尾注：头截断后的续读提示。
fn head_note(start: usize, end: usize, total: usize, byte_capped: bool) -> String {
    let mut note = format!(
        "\n\n[Showing lines {start}-{end} of {total}. Use offset={}.",
        end + 1
    );
    if byte_capped {
        note.push_str(" (50KB limit)");
    }
    note.push(']');
    note
}

/// 头截断（keep start；不切半行；read/grep/find 用）。
///
/// 返回（正文, 是否截断）。
#[must_use]
pub fn truncate_head(text: &str) -> (String, bool) {
    let total = text.lines().count();
    if total <= MAX_LINES && text.len() <= MAX_BYTES {
        return (text.to_string(), false);
    }
    let mut kept = 0usize;
    let mut bytes = 0usize;
    for line in text.lines().take(MAX_LINES) {
        let lb = line.len() + 1;
        if bytes + lb > MAX_BYTES && kept > 0 {
            break;
        }
        bytes += lb;
        kept += 1;
    }
    let body: String = text.lines().take(kept).collect::<Vec<_>>().join("\n");
    let byte_capped = bytes >= MAX_BYTES && kept < total;
    (body + &head_note(1, kept, total, byte_capped), true)
}

/// 尾截断（keep end；允许半行；bash 用）。
///
/// 返回（正文, 是否截断）。
#[must_use]
pub fn truncate_tail(text: &str) -> (String, bool) {
    if text.lines().count() <= MAX_LINES && text.len() <= MAX_BYTES {
        return (text.to_string(), false);
    }
    let mut body: String = text
        .lines()
        .rev()
        .take(MAX_LINES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    if body.len() > MAX_BYTES {
        let cut = body.len() - MAX_BYTES;
        let at = (cut..=body.len())
            .find(|i| body.is_char_boundary(*i))
            .unwrap_or(body.len());
        body = body[at..].to_string();
    }
    (body, true)
}

/// 中截断（head+tail + 提示；MCP 结果用，pi truncateMiddle）。
///
/// 返回（正文, 是否截断）。
#[must_use]
pub fn truncate_middle(text: &str, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text.to_string(), false);
    }
    let half = cap / 2;
    let head: String = String::from_utf8_lossy(&text.as_bytes()[..char_floor(text, half)]).into();
    let tail_start = char_floor(text, text.len() - half);
    let tail: String = String::from_utf8_lossy(&text.as_bytes()[tail_start..]).into();
    let omitted = text.len() - head.len() - tail.len();
    (format!("{head}\n…{omitted} chars truncated…\n{tail}"), true)
}

/// 向后找合法字符边界。
fn char_floor(text: &str, mut idx: usize) -> usize {
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_truncates_by_lines_with_note() {
        let text: String = (1..=3000)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (body, truncated) = truncate_head(&text);
        assert!(truncated);
        assert!(body.contains("[Showing lines 1-2000 of 3000. Use offset=2001.]"));
        assert!(body.starts_with("L1\n"));
    }

    #[test]
    fn head_no_truncation_passthrough() {
        let (body, truncated) = truncate_head("a\nb\nc");
        assert!(!truncated);
        assert_eq!(body, "a\nb\nc");
    }

    #[test]
    fn tail_keeps_end_and_allows_partial_line() {
        let text: String = (1..=3000)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (body, truncated) = truncate_tail(&text);
        assert!(truncated);
        assert!(body.contains("L3000"));
        assert!(!body.contains("L1\nL2"));
    }

    #[test]
    fn middle_reports_omitted_chars() {
        let text = "x".repeat(10_000);
        let (body, truncated) = truncate_middle(&text, 1000);
        assert!(truncated);
        assert!(body.contains("chars truncated"));
        assert!(body.len() < 2000);
    }
}
