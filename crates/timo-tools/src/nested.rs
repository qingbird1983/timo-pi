//! 嵌套工具执行接缝（设计 phase-3.5/11 §4.1；接缝 #8）。
//!
//! 工具（如 codemode）经 [`crate::ToolCallCtx::nested`] 调用其它工具。宿主由循环
//! 实现，保证参数校验、钩子、审阅、取消与记录只有一条路径；嵌套结果不进对话，
//! 用量经 [`NestedCallSummary::usage`] 并回父结果。

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use timo_ai::{Content, Usage};
use tokio_util::sync::CancellationToken;

use crate::definition::{ExecutionMode, Exposure, Namespace, ToolOutput};

/// 记录上限（pi `NESTED_CALL_LIMITS` 同值）。
pub mod limits {
    /// 一次顶层调用最多记录多少条嵌套调用。
    pub const MAX_CALLS: usize = 256;
    /// 单次调用参数的 JSON 字节上限；超过则不记录参数。
    pub const MAX_ARGUMENT_BYTES_PER_CALL: usize = 8 * 1024;
    /// 一次顶层调用记录参数的累计字节上限。
    pub const MAX_ARGUMENT_BYTES_TOTAL: usize = 32 * 1024;
    /// 记录里错误文本的字符上限。
    pub const MAX_ERROR_CHARS: usize = 500;
}

/// 嵌套调用方可选的工具元数据（描述渲染、值映射与名字消毒用）。
#[derive(Debug, Clone)]
pub struct CallableTool {
    /// 工具名（注册表原名）。
    pub name: String,
    /// 模型描述。
    pub description: String,
    /// 参数 JSON Schema。
    pub parameters: Value,
    /// 结构化输出 Schema（有则嵌套结果解析为 `structured_content`）。
    pub output_schema: Option<Value>,
    /// 命名空间（描述分组用）。
    pub namespace: Option<Namespace>,
    /// 系统提示准则（声明里一并给脚本看）。
    pub prompt_guidelines: Vec<String>,
    /// 执行模式（决定嵌套调用是否串行）。
    pub execution_mode: ExecutionMode,
    /// 暴露面（`Deferred` 默认不进描述，但仍可调）。
    pub exposure: Exposure,
}

/// 一次嵌套调用请求；调用 id 由宿主分配（`<父>/<n>`）。
#[derive(Debug, Clone)]
pub struct NestedCall {
    /// 父调用 id（模型发起的那次工具调用）。
    pub parent_id: String,
    /// 目标工具名。
    pub name: String,
    /// 调用参数（已序列化的 JSON 值）。
    pub arguments: Value,
    /// 取消令牌（父取消时一并取消）。
    pub cancel: CancellationToken,
    /// 进度通道（与父工具共用；settle 后发送被忽略）。
    pub updates: tokio::sync::mpsc::Sender<ToolOutput>,
}

/// 一次嵌套调用的结果。
#[derive(Debug, Clone)]
pub struct NestedOutcome {
    /// 工具输出（失败以 `is_error` 表达，不抛错）。
    pub output: ToolOutput,
    /// 实际耗时（毫秒）。
    pub duration_ms: u64,
}

/// 单条嵌套调用的状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedCallStatus {
    /// 仍在执行（快照里出现即表示记录不完整）。
    Running,
    /// 成功。
    Ok,
    /// 工具返回了错误结果。
    Error,
    /// 父调用取消。
    Cancelled,
}

/// 一次嵌套调用的记录行。
#[derive(Debug, Clone)]
pub struct NestedCallRecord {
    /// 调用 id。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// 参数（超过字节上限时为 `None`）。
    pub arguments: Option<Value>,
    /// 参数 JSON 字节数（参数被省略时仍记录）。
    pub arguments_bytes: Option<usize>,
    /// 状态。
    pub status: NestedCallStatus,
    /// 耗时（毫秒；未结束为 `None`）。
    pub duration_ms: Option<u64>,
    /// 错误文本（截到 [`limits::MAX_ERROR_CHARS`]）。
    pub error: Option<String>,
}

/// 一次顶层调用的嵌套记录汇总。
#[derive(Debug, Clone, Default)]
pub struct NestedCallSummary {
    /// 调用记录（最多 [`limits::MAX_CALLS`] 条）。
    pub calls: Vec<NestedCallRecord>,
    /// 记录是否完整（未超限、且没有仍在执行的调用）。
    pub complete: bool,
    /// 全部嵌套结果的用量之和（含未记录进 `calls` 的调用）。
    pub usage: Option<Usage>,
}

/// 嵌套执行宿主（循环实现；工具只认本 trait）。
pub trait NestedToolHost: Send + Sync {
    /// 可调工具的元数据快照（调用方自行过滤，例如去掉自己）。
    fn catalog(&self) -> Vec<CallableTool>;
    /// 本批工具调用是否整体串行（`tool_execution = sequential`）。
    fn is_sequential(&self) -> bool;
    /// 执行一次嵌套调用；失败以 `is_error` 结果返回，不抛错。
    fn execute(&self, call: NestedCall)
    -> Pin<Box<dyn Future<Output = NestedOutcome> + Send + '_>>;
    /// 当前记录快照（不改状态；失败文案用）。
    fn snapshot(&self, parent_id: &str) -> Option<NestedCallSummary>;
    /// 取走记录（agent 在顶层调用收尾时合并用量）。
    fn take(&self, parent_id: &str) -> Option<NestedCallSummary>;
}

/// 把一个用量加进累计值（逐项相加；`reasoning`/`cache_write_1h` 有值才加）。
pub fn merge_usage(into: &mut Usage, add: &Usage) {
    into.input += add.input;
    into.output += add.output;
    into.cache_read += add.cache_read;
    into.cache_write += add.cache_write;
    if add.reasoning.is_some() {
        into.reasoning = Some(into.reasoning.unwrap_or(0) + add.reasoning.unwrap_or(0));
    }
    if add.cache_write_1h.is_some() {
        into.cache_write_1h =
            Some(into.cache_write_1h.unwrap_or(0) + add.cache_write_1h.unwrap_or(0));
    }
    into.total_tokens += add.total_tokens;
    into.cost.input += add.cost.input;
    into.cost.output += add.cost.output;
    into.cost.cache_read += add.cost.cache_read;
    into.cost.cache_write += add.cost.cache_write;
    into.cost.total += add.cost.total;
}

/// 输出里的文本块拼接（与模型可见文本一致；图像不计）。
#[must_use]
pub fn output_text(output: &ToolOutput) -> String {
    output
        .content
        .iter()
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use timo_ai::Cost;

    #[test]
    fn merge_usage_sums_optional_fields() {
        let mut total = Usage {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
            reasoning: Some(5),
            total_tokens: 10,
            cost: Cost {
                input: 0.1,
                output: 0.2,
                cache_read: 0.3,
                cache_write: 0.4,
                total: 1.0,
            },
            ..Usage::default()
        };
        let add = Usage {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
            reasoning: None,
            cache_write_1h: Some(7),
            total_tokens: 100,
            cost: Cost {
                input: 1.0,
                output: 2.0,
                cache_read: 3.0,
                cache_write: 4.0,
                total: 10.0,
            },
        };
        merge_usage(&mut total, &add);
        assert_eq!(total.input, 11);
        assert_eq!(total.output, 22);
        assert_eq!(total.cache_read, 33);
        assert_eq!(total.cache_write, 44);
        assert_eq!(total.reasoning, Some(5), "缺省不覆盖已值");
        assert_eq!(total.cache_write_1h, Some(7));
        assert_eq!(total.total_tokens, 110);
        assert!((total.cost.total - 11.0).abs() < f64::EPSILON);
    }

    #[test]
    fn output_text_joins_text_blocks_only() {
        let output = ToolOutput {
            content: vec![
                Content::Text { text: "a".into() },
                Content::Image {
                    data: "x".into(),
                    mime_type: "image/png".into(),
                },
                Content::Text { text: "b".into() },
            ],
            ..ToolOutput::default()
        };
        assert_eq!(output_text(&output), "a\nb");
    }
}
