//! 嵌套工具执行宿主（设计 phase-3.5/11 §4.1）：复用同一条管线，不发总线事件。
//!
//! 记录按父调用 id 分桶；`take` 由循环在顶层调用收尾时取走并把用量并回结果。
//! 顺序规则：目标工具 `Sequential` 时，嵌套调用经宿主内一把队列串行化。

use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use timo_ai::Content;
use timo_tools::nested::{self, limits};
use timo_tools::{
    CallableTool, ExecutionMode, Exposure, NestedCall, NestedCallRecord, NestedCallStatus,
    NestedCallSummary, NestedOutcome, NestedToolHost, ToolCallCtx, ToolOutput, ToolRegistry,
    ToolReview,
};
use tokio_util::sync::CancellationToken;

use super::LoopConfig;
use crate::hooks::LoopHooks;

/// 一次顶层调用的记录桶。
struct Parent {
    next_id: usize,
    calls: Vec<NestedCallRecord>,
    complete: bool,
    argument_bytes: usize,
    usage: Option<timo_ai::Usage>,
}

impl Default for Parent {
    fn default() -> Self {
        Self {
            next_id: 1,
            calls: Vec::new(),
            complete: true,
            argument_bytes: 0,
            usage: None,
        }
    }
}

/// 循环实现的嵌套宿主（每次顶层工具调用一份）。
pub(super) struct AgentNestedHost {
    tools: Arc<ToolRegistry>,
    hooks: Option<Arc<dyn LoopHooks>>,
    run_cancel: CancellationToken,
    cwd: Arc<Path>,
    review: Option<Arc<dyn ToolReview>>,
    queue: tokio::sync::Mutex<()>,
    state: Mutex<HashMap<String, Parent>>,
}

impl AgentNestedHost {
    /// 从循环配置克隆一份宿主（`'static`，可交给工具长期持有）。
    pub(super) fn new(config: &LoopConfig, cancel: &CancellationToken) -> Self {
        Self {
            tools: Arc::clone(&config.tools),
            hooks: config.hooks.clone(),
            run_cancel: cancel.clone(),
            cwd: Arc::clone(&config.cwd),
            review: config.review.clone(),
            queue: tokio::sync::Mutex::new(()),
            state: Mutex::new(HashMap::new()),
        }
    }

    async fn run(&self, call: NestedCall) -> NestedOutcome {
        let started = Instant::now();
        let (id, index) = self.start(&call);
        let content = Content::ToolCall {
            id: id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
            arguments_error: None,
        };
        let output = if self.run_cancel.is_cancelled() || call.cancel.is_cancelled() {
            ToolOutput::error_text("Operation aborted")
        } else {
            let (effective, blocked, _terminate) =
                super::tools::prepare_call(self.hooks.as_ref(), &content).await;
            if let Some(reason) = blocked {
                ToolOutput::error_text(format!("blocked by extension: {reason}"))
            } else {
                let params = match &effective {
                    Content::ToolCall { arguments, .. } => arguments.clone(),
                    _ => Value::Null,
                };
                let sequential = self
                    .tools
                    .get(&call.name)
                    .is_some_and(|tool| tool.execution_mode == ExecutionMode::Sequential);
                let guard = if sequential {
                    Some(self.queue.lock().await)
                } else {
                    None
                };
                let ctx = ToolCallCtx {
                    tool_call_id: id.clone(),
                    params,
                    cancel: call.cancel.clone(),
                    updates: call.updates.clone(),
                    cwd: Arc::clone(&self.cwd),
                    review: self.review.clone(),
                    nested: None,
                };
                let output = match self.tools.execute(&call.name, ctx).await {
                    Ok(output) => output,
                    Err(error) => ToolOutput::error_text(error.to_string()),
                };
                drop(guard);
                super::tools::apply_after(self.hooks.as_ref(), &id, &call.name, output).await
            }
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        let status = if output.is_error {
            if self.run_cancel.is_cancelled() || call.cancel.is_cancelled() {
                NestedCallStatus::Cancelled
            } else {
                NestedCallStatus::Error
            }
        } else {
            NestedCallStatus::Ok
        };
        self.finish(&call.parent_id, index, status, duration_ms, &output);
        NestedOutcome {
            output,
            duration_ms,
        }
    }

    /// 分配 id 并登记记录；返回（id, 记录下标）。
    fn start(&self, call: &NestedCall) -> (String, Option<usize>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let parent = state.entry(call.parent_id.clone()).or_default();
        let id = format!("{}/{}", call.parent_id, parent.next_id);
        parent.next_id += 1;
        if parent.calls.len() >= limits::MAX_CALLS {
            parent.complete = false;
            return (id, None);
        }
        let bytes = serde_json::to_string(&call.arguments)
            .map(|json| json.len())
            .unwrap_or(0);
        let mut record = NestedCallRecord {
            id: id.clone(),
            name: call.name.clone(),
            arguments: None,
            arguments_bytes: None,
            status: NestedCallStatus::Running,
            duration_ms: None,
            error: None,
        };
        if bytes > limits::MAX_ARGUMENT_BYTES_PER_CALL
            || parent.argument_bytes + bytes > limits::MAX_ARGUMENT_BYTES_TOTAL
        {
            record.arguments_bytes = Some(bytes);
            parent.complete = false;
        } else {
            record.arguments = Some(call.arguments.clone());
            parent.argument_bytes += bytes;
        }
        parent.calls.push(record);
        (id, Some(parent.calls.len() - 1))
    }

    /// 收尾一条记录并累计用量。
    fn finish(
        &self,
        parent_id: &str,
        index: Option<usize>,
        status: NestedCallStatus,
        duration_ms: u64,
        output: &ToolOutput,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(parent) = state.get_mut(parent_id) else {
            return;
        };
        if let Some(index) = index
            && let Some(record) = parent.calls.get_mut(index)
        {
            record.status = status;
            record.duration_ms = Some(duration_ms);
            if output.is_error {
                record.error = Some(truncate_chars(
                    &nested::output_text(output),
                    limits::MAX_ERROR_CHARS,
                ));
            }
        }
        if let Some(usage) = &output.usage {
            match &mut parent.usage {
                Some(total) => nested::merge_usage(total, usage),
                None => parent.usage = Some(usage.clone()),
            }
        }
    }
}

impl NestedToolHost for AgentNestedHost {
    fn catalog(&self) -> Vec<CallableTool> {
        self.tools
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

    fn execute(
        &self,
        call: NestedCall,
    ) -> Pin<Box<dyn std::future::Future<Output = NestedOutcome> + Send + '_>> {
        Box::pin(self.run(call))
    }

    fn snapshot(&self, parent_id: &str) -> Option<NestedCallSummary> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.get(parent_id).map(summary_of)
    }

    fn take(&self, parent_id: &str) -> Option<NestedCallSummary> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.remove(parent_id).map(|parent| summary_of(&parent))
    }
}

fn summary_of(parent: &Parent) -> NestedCallSummary {
    NestedCallSummary {
        calls: parent.calls.clone(),
        complete: parent.complete
            && parent
                .calls
                .iter()
                .all(|call| call.status != NestedCallStatus::Running),
        usage: parent.usage.clone(),
    }
}

/// 按字符截断（pi 的 `slice(0, maxChars)` 同义）。
fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((index, _)) => text[..index].to_string(),
        None => text.to_string(),
    }
}
