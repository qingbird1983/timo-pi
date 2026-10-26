//! 工具批执行（设计 02 §5）：顺序/并发、钩子裁决与结果回填。

use std::sync::Arc;

use timo_ai::{Content, Message, ToolResultMessage};

use super::{Ctx, ToolExecutionMode};
use crate::events::KernelEvent;
use crate::hooks::ToolOutcome;
use crate::tool_calls::{error_result_message, tool_args, tool_id, tool_name};

impl<'a> Ctx<'a> {
    /// 工具批执行（设计 02 §5）；返回（结果消息, 是否全部 terminate）。
    pub(super) async fn execute_tools(&self, calls: &[Content]) -> (Vec<Message>, bool) {
        match self.config.tool_execution {
            ToolExecutionMode::Sequential => self.execute_tools_sequential(calls).await,
            ToolExecutionMode::Parallel => self.execute_tools_parallel(calls).await,
        }
    }

    async fn execute_tools_sequential(&self, calls: &[Content]) -> (Vec<Message>, bool) {
        let mut out = vec![];
        let mut all_term = true;
        for call in calls {
            if self.cancel.is_cancelled() {
                out.push(self.fail_tool_call(call, "Operation aborted").await);
                all_term = false;
                continue;
            }
            let (m, term) = self.run_one(call).await;
            all_term &= term;
            out.push(m);
        }
        let non_empty = !out.is_empty();
        (out, all_term && non_empty)
    }

    async fn execute_tools_parallel(&self, calls: &[Content]) -> (Vec<Message>, bool) {
        // 并发执行（join_all 轮询并发，IO 语义）；结果按原始顺序回填（pi 同构）
        let futures = calls.iter().map(|call| async {
            if self.cancel.is_cancelled() {
                return (self.fail_tool_call(call, "Operation aborted").await, false);
            }
            self.run_one(call).await
        });
        let results = futures::future::join_all(futures).await;
        let all_term = results.iter().all(|(_, t)| *t);
        let non_empty = !results.is_empty();
        (
            results.into_iter().map(|(m, _)| m).collect(),
            all_term && non_empty,
        )
    }

    /// 单工具全流程：Requested → （钩子）→ Started → 执行 → Finished。
    async fn run_one(&self, call: &Content) -> (Message, bool) {
        let started = std::time::Instant::now();
        let name = tool_name(call);
        let id = tool_id(call);
        self.bus.emit(KernelEvent::ToolCallRequested {
            call: Arc::new(call.clone()),
        });
        let mut effective = call.clone();
        let mut force_terminate = false;
        if let Some(hooks) = &self.config.hooks
            && let Some(decision) = hooks.before_tool_call(&effective).await
        {
            if let Some(reason) = decision.block {
                let reason = format!("blocked by extension: {reason}");
                self.bus.emit(KernelEvent::ToolCallStarted {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    args: tool_args(&effective),
                });
                let message = error_result_message(&id, &name, &reason, None, self.now_ms());
                self.bus.emit(KernelEvent::ToolCallFinished {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    result: Arc::new(message.clone()),
                    duration_ms: None,
                });
                return (message, false);
            }
            if let Some(args) = decision.args
                && let Content::ToolCall {
                    arguments,
                    arguments_error,
                    ..
                } = &mut effective
            {
                *arguments = args;
                *arguments_error = None;
            }
            force_terminate = decision.terminate;
        }
        self.bus.emit(KernelEvent::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            args: tool_args(&effective),
        });
        let result = self.execute_validated(&effective).await;
        let duration = started.elapsed().as_millis() as u64;
        match result {
            Ok(mut output) => {
                if let Some(hooks) = &self.config.hooks
                    && let Some(patch) = hooks
                        .after_tool_call(&ToolOutcome {
                            tool_call_id: &id,
                            tool_name: &name,
                            output: &output,
                        })
                        .await
                {
                    patch.apply(&mut output);
                }
                let message = Message::ToolResult(ToolResultMessage {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    content: output.content,
                    is_error: output.is_error,
                    usage: None,
                    duration_ms: Some(duration),
                    timestamp: self.now_ms(),
                });
                self.bus.emit(KernelEvent::ToolCallFinished {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    result: Arc::new(message.clone()),
                    duration_ms: Some(duration),
                });
                (message, output.terminate || force_terminate)
            }
            Err(e) => {
                let message =
                    error_result_message(&id, &name, &e.to_string(), Some(duration), self.now_ms());
                self.bus.emit(KernelEvent::ToolCallFinished {
                    tool_call_id: id.clone(),
                    tool_name: name.clone(),
                    result: Arc::new(message.clone()),
                    duration_ms: Some(duration),
                });
                (message, force_terminate)
            }
        }
    }

    async fn execute_validated(
        &self,
        call: &Content,
    ) -> timo_tools::Result<timo_tools::ToolOutput> {
        let Content::ToolCall {
            name,
            arguments,
            arguments_error,
            ..
        } = call
        else {
            return Err(timo_tools::ToolError::InvalidParams {
                reason: "not a tool call".into(),
            });
        };
        if let Some(err) = arguments_error {
            return Err(timo_tools::ToolError::InvalidParams {
                reason: format!("arguments are not valid JSON: {err}"),
            });
        }
        let (tx, mut rx) = tokio::sync::mpsc::channel::<timo_tools::ToolOutput>(16);
        let bus = self.bus.clone();
        let id = tool_id(call);
        let (fid, ftool) = (id.clone(), name.clone());
        let forwarder = tokio::spawn(async move {
            while let Some(partial) = rx.recv().await {
                bus.emit(KernelEvent::ToolCallUpdate {
                    tool_call_id: fid.clone(),
                    tool_name: ftool.clone(),
                    partial: Arc::new(partial),
                });
            }
        });
        let ctx = timo_tools::ToolCallCtx {
            tool_call_id: id.clone(),
            params: arguments.clone(),
            cancel: self.cancel.clone(),
            updates: tx,
            cwd: self.config.cwd.clone(),
            review: self.config.review.clone(),
        };
        let out = self.config.tools.execute(name, ctx).await;
        forwarder.abort();
        out
    }

    /// 截断批中失败的工具调用（pi failToolCallsFromTruncatedMessage）。
    pub(super) async fn fail_tool_call(&self, call: &Content, reason: &str) -> Message {
        let name = tool_name(call);
        let id = tool_id(call);
        self.bus.emit(KernelEvent::ToolCallRequested {
            call: Arc::new(call.clone()),
        });
        self.bus.emit(KernelEvent::ToolCallStarted {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            args: tool_args(call),
        });
        let message = error_result_message(&id, &name, reason, None, self.now_ms());
        self.bus.emit(KernelEvent::ToolCallFinished {
            tool_call_id: id.clone(),
            tool_name: name.clone(),
            result: Arc::new(message.clone()),
            duration_ms: None,
        });
        message
    }
}
