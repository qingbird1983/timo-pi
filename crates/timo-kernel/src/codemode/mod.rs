//! codemode：脚本沙箱与嵌套工具调用（设计 phase-3.5/11）。
//!
//! 工具默认关闭（`ModelOnly` 暴露面，未激活时 `Hidden`）；脚本经
//! [`timo_tools::ToolCallCtx::nested`] 复用同一条工具管线，只把脚本输出送回模型。

mod describe;
mod format;
mod sandbox;
mod tool;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tool_tests;

use std::sync::Arc;
use std::time::Duration;

use timo_tools::{CallableTool, Exposure, ToolRegistry};

/// 沙箱资源限制（默认取蓝本值；测试可传更小值）。
#[derive(Debug, Clone)]
pub(crate) struct Limits {
    /// 进程内并行脚本数上限。
    pub workers: usize,
    /// 等待空闲 worker 的上界。
    pub worker_wait: Duration,
    /// 单个 QuickJS 堆上限（字节）。
    pub memory_bytes: usize,
    /// 输出文本的字符上限（`text`/`console` 合计）。
    pub max_output_chars: usize,
    /// 输出调用次数上限（`text`/`console`）。
    pub max_output_calls: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            workers: 2,
            worker_wait: Duration::from_secs(60),
            memory_bytes: 256 * 1024 * 1024,
            max_output_chars: 16_777_216,
            max_output_calls: 100_000,
        }
    }
}

/// 内核装配用的默认限制。
pub(crate) fn limits() -> Arc<Limits> {
    Arc::new(Limits::default())
}

/// 注册表里可被脚本调用的工具（去掉 `Hidden`；`codemode` 由工具定义自行去掉）。
pub(crate) fn catalog(registry: &ToolRegistry) -> Vec<CallableTool> {
    registry
        .all()
        .into_iter()
        .filter(|tool| tool.exposure != Exposure::Hidden)
        .map(|tool| CallableTool {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
            output_schema: tool.output_schema.clone(),
            namespace: tool.namespace.clone(),
            prompt_guidelines: tool.prompt_guidelines.clone(),
            execution_mode: tool.execution_mode,
            exposure: tool.exposure,
        })
        .collect()
}

/// 构造工具定义（内核按激活状态覆盖暴露面）。
pub(crate) fn definition(catalog: Vec<CallableTool>) -> timo_tools::ToolDefinition {
    tool::definition(catalog, limits())
}
