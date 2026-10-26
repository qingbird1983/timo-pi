//! SSE 解码器（设计 00 §5 D2：自研，~100 行，fixture 全覆盖）。
//!
//! 按 WHATWG SSE 规则解码：`field: value` 行、空行分发、多行 data 以 `\n` 连接、
//! `:` 开头为注释、值前的单个空格剥离。

/// 解码出的一条 SSE 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// `event:` 字段（None = 默认 "message"）。
    pub event: Option<String>,
    /// `data:` 字段（多行合并）。
    pub data: String,
}

/// 增量 SSE 解码器：喂字节块，吐完整事件。
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    /// 新建解码器。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一段 UTF-8 文本，返回其中完整的事件（以空行分隔）。
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        let mut events = vec![];
        while let Some(pos) = self.find_terminator() {
            let block: String = self.buf.drain(..pos).collect();
            self.drain_terminator();
            if let Some(ev) = parse_block(&block) {
                events.push(ev);
            }
        }
        events
    }

    /// 流结束时的残留处理：尾部未以空行结束的 data 块按事件补发（宽容模式）。
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let rest = std::mem::take(&mut self.buf);
        parse_block(&rest).into_iter().collect()
    }

    fn find_terminator(&self) -> Option<usize> {
        let lf = self.buf.find("\n\n").map(|p| (p, 1));
        let crlf = self.buf.find("\r\n\r\n").map(|p| (p, 2));
        match (lf, crlf) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a.0 + a.1 } else { b.0 + b.1 }),
            (Some(a), None) => Some(a.0 + a.1),
            (None, Some(b)) => Some(b.0 + b.1),
            (None, None) => None,
        }
    }

    fn drain_terminator(&mut self) {
        while self.buf.starts_with('\n') || self.buf.starts_with('\r') {
            self.buf.remove(0);
        }
    }
}

/// 解析单个事件块（字段行序列）。
fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event: Option<String> = None;
    let mut data_lines: Vec<String> = vec![];
    for line in block.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => data_lines.push(value.to_string()),
            _ => {} // id/retry 及未知字段忽略
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_event_with_prefix_space() {
        let mut d = SseDecoder::new();
        let evs = d.push("event: message_delta\ndata: {\"a\": 1}\n\n");
        assert_eq!(
            evs,
            vec![SseEvent {
                event: Some("message_delta".into()),
                data: "{\"a\": 1}".into()
            }]
        );
    }

    #[test]
    fn chunked_and_multiline_data() {
        let mut d = SseDecoder::new();
        assert!(d.push("data: l1\ndata: l2\n").is_empty());
        // 前一事件的收尾空行与下一事件同块到达：应产出两个事件
        let evs = d.push("\ndata: x\n\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].data, "l1\nl2");
        assert_eq!(evs[0].event, None);
        assert_eq!(evs[1].data, "x");
    }

    #[test]
    fn crlf_and_comments() {
        let mut d = SseDecoder::new();
        let evs = d.push(": keep-alive\r\n\r\nevent: e\r\ndata: v\r\n\r\n");
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].event.as_deref(), Some("e"));
    }

    #[test]
    fn data_only_block_no_terminator_finish() {
        let mut d = SseDecoder::new();
        assert!(d.push("data: tail").is_empty());
        let evs = d.finish();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].data, "tail");
    }
}
