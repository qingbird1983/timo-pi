//! `read` 工具（设计 03 §3；pi read.ts 同构：文本行窗 + 图像嗅探缩放）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::Result;
use crate::definition::{ToolCallCtx, ToolDefinition, ToolOutput};
use crate::error::ToolError;
use crate::tools::write::str_param;
use crate::truncate::truncate_head;

/// 图像自动缩放默认档（pi 全局 fallback 2000×2000；jpeg 质量 85）。
const MAX_DIM: u32 = 2000;

/// 参数 schema。
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path (relative to cwd or absolute)"},
            "offset": {"type": "number", "description": "1-indexed line to start from"},
            "limit": {"type": "number", "description": "Max number of lines to read"}
        },
        "required": ["path"]
    })
}

/// 构造 read 工具定义。
#[must_use]
pub fn definition(cwd: Arc<Path>) -> ToolDefinition {
    ToolDefinition {
        name: "read".into(),
        label: "Read".into(),
        description: "Read a file from the local filesystem as text (with line-window controls) \
                      or a recognized image file."
            .into(),
        prompt_snippet: Some("read(path, offset?, limit?): read a text or image file".into()),
        prompt_guidelines: vec![
            "Use `offset`/`limit` to page through large files instead of re-reading them.".into(),
        ],
        parameters: schema(),
        output_schema: None,
        exposure: crate::definition::Exposure::Direct,
        namespace: None,
        annotations: crate::definition::Annotations {
            read_only_hint: Some(true),
            ..Default::default()
        },
        execution_mode: crate::definition::ExecutionMode::Parallel,
        execute: Arc::new(move |ctx| Box::pin(execute(ctx, cwd.clone()))),
    }
}

async fn execute(ctx: ToolCallCtx, cwd: Arc<Path>) -> Result<ToolOutput> {
    let _ = ctx.updates; // read 无进度更新
    let path = resolve(&ctx.cwd, str_param(&ctx.params, "path")?)?;
    sniff_and_read(&path, &ctx, cwd).await
}

fn num_param(params: &Value, key: &str) -> Result<Option<u64>> {
    Ok(params.get(key).and_then(Value::as_u64))
}

/// 相对路径 → 绝对（cwd 拼接）。
pub fn resolve(cwd: &Path, p: &str) -> Result<PathBuf> {
    let path = Path::new(p);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(cwd.join(path))
    }
}

async fn sniff_and_read(path: &Path, ctx: &ToolCallCtx, _cwd: Arc<Path>) -> Result<ToolOutput> {
    let bytes = tokio::fs::read(path).await.map_err(|e| ToolError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    if let Some(mime) = sniff_image(&bytes) {
        return read_image(path, bytes, mime, ctx).await;
    }
    read_text(&bytes, &ctx.params, &ctx.cancel).await
}

/// 魔数嗅探（jpg/png/gif/webp/bmp）。
#[must_use]
pub fn sniff_image(b: &[u8]) -> Option<&'static str> {
    match b.first()? {
        0xFF if b.len() > 2 && b[1] == 0xD8 => Some("image/jpeg"),
        0x89 if b.starts_with(&[0x89, b'P', b'N', b'G']) => Some("image/png"),
        0x47 if b.starts_with(b"GIF8") => Some("image/gif"),
        0x42 if b.starts_with(b"BM") => Some("image/bmp"),
        _ if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" => Some("image/webp"),
        _ => None,
    }
}

async fn read_text(bytes: &[u8], params: &Value, cancel: &CancellationToken) -> Result<ToolOutput> {
    let _ = cancel; // 读取为一次性同步操作；取消在循环层生效
    let text = String::from_utf8_lossy(bytes);
    let total = text.lines().count();
    let offset = num_param(params, "offset")?.unwrap_or(1).max(1);
    if offset as usize > total && total > 0 {
        return Err(ToolError::InvalidParams {
            reason: format!("Offset {offset} is beyond end of file ({total} lines total)"),
        });
    }
    let limit = num_param(params, "limit")?.map(|l| l as usize);
    let selected: Vec<&str> = text
        .lines()
        .skip(offset as usize - 1)
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    let joined = selected.join("\n");
    let (mut body, truncated) = truncate_head(&joined);
    // 显式 limit 提前结束的提示（pi 同构）
    let early = limit.is_some_and(|l| (offset as usize - 1) + l < total);
    if !truncated && early {
        let shown = (offset as usize - 1) + selected.len();
        body.push_str(&format!(
            "\n\n[…{} more lines in file. Use offset={} to continue.]",
            total - shown,
            shown + 1
        ));
    }
    Ok(ToolOutput::text(body))
}

async fn read_image(
    path: &Path,
    bytes: Vec<u8>,
    mime: &'static str,
    ctx: &ToolCallCtx,
) -> Result<ToolOutput> {
    let (data, note) = match resize(bytes.clone()) {
        Some(resized) => {
            let note = if resized.len() < bytes.len() {
                format!(
                    "[Image resized from {}KB to {}KB]",
                    bytes.len() / 1024,
                    resized.len() / 1024
                )
            } else {
                format!("[Image file: {mime}]")
            };
            (resized, note)
        }
        None => (bytes, format!("[Image file: {mime}]")),
    };
    let _ = ctx.updates;
    Ok(ToolOutput {
        content: vec![
            timo_ai::Content::Text {
                text: format!("Read image file {mime}. {note}"),
            },
            timo_ai::Content::Image {
                data: base64::engine::general_purpose::STANDARD.encode(data),
                mime_type: mime.to_string(),
            },
        ],
        details: json!({"path": path.display().to_string(), "kind": "image"}),
        ..ToolOutput::default()
    })
}

/// 等比缩放至 ≤2000×2000；解码失败返回 None（原样透传）。
fn resize(bytes: Vec<u8>) -> Option<Vec<u8>> {
    use image::GenericImageView as _;
    let img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = img.dimensions();
    if w <= MAX_DIM && h <= MAX_DIM {
        return None;
    }
    let scale = f64::from(MAX_DIM) / f64::from(w.max(h));
    let nw = ((f64::from(w) * scale) as u32).max(1);
    let nh = ((f64::from(h) * scale) as u32).max(1);
    let resized = img.resize(nw, nh, image::imageops::FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(vec![]);
    resized.write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn sniff_by_magic_bytes() {
        assert_eq!(
            sniff_image(&[0x89, b'P', b'N', b'G', 0x0D]),
            Some("image/png")
        );
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a"), Some("image/gif"));
        assert_eq!(sniff_image(b"hello"), None);
    }

    #[tokio::test]
    async fn text_read_offset_beyond_eof() {
        let dir = std::env::temp_dir().join(format!("timo-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.txt");
        std::fs::write(&p, "l1\nl2\nl3\n").unwrap();
        let params = json!({"path": p.display().to_string(), "offset": 9});
        let err = read_text(b"l1\nl2\nl3\n", &params, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("beyond end of file"));
    }

    #[tokio::test]
    async fn text_read_window_and_continue_note() {
        let text: String = (1..=10)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let params = json!({"limit": 3});
        let out = read_text(text.as_bytes(), &params, &CancellationToken::new())
            .await
            .unwrap();
        let timo_ai::Content::Text { text: body } = &out.content[0] else {
            panic!("text")
        };
        assert!(body.contains("L1") && body.contains("L3") && !body.contains("L4"));
        assert!(body.contains("[…7 more lines in file. Use offset=4 to continue.]"));
    }
}
