//! 路由辅件：reason 判定、previous 快照与按模型取 adapter/凭证（设计 10 §4.2）。
//!
//! 从 `run/mod.rs` 拆出以守住单文件行数预算（标准 08 §4）。

use timo_ai::{Message, StopReason};

use crate::hooks::{RouteChoice, RouteReason, RouteRequest};

use super::Ctx;

impl Ctx<'_> {
    /// 本请求的原因：最后一条 assistant 之后有用户消息 = `User`，否则 `Continuation`。
    pub(super) fn turn_reason(&self) -> RouteReason {
        let start = self
            .messages
            .iter()
            .rposition(|m| matches!(m, Message::Assistant(_)))
            .map_or(0, |index| index + 1);
        if self.messages[start..]
            .iter()
            .any(|m| matches!(m, Message::User(_)))
        {
            RouteReason::User
        } else {
            RouteReason::Continuation
        }
    }

    /// `messages` 里最近一次成功响应的物理模型与档位（跳过失败/中止）。
    pub(super) fn previous_response(&self) -> Option<(timo_ai::Model, timo_config::ThinkingLevel)> {
        self.messages.iter().rev().find_map(|m| match m {
            Message::Assistant(a)
                if !matches!(a.stop_reason, StopReason::Error | StopReason::Aborted) =>
            {
                Some((
                    self.response_model(&a.provider, &a.model, &a.api),
                    a.thinking_level.unwrap_or(timo_config::ThinkingLevel::Off),
                ))
            }
            _ => None,
        })
    }

    /// 响应消息声明的模型：有目录时取目录条目，否则只构造身份条目。
    pub(super) fn response_model(&self, provider: &str, id: &str, api: &str) -> timo_ai::Model {
        if let Some(dispatch) = &self.config.dispatch
            && let Some(model) = dispatch.resolve(provider, id)
        {
            return model;
        }
        let api = timo_ai::ApiKind::parse(api).unwrap_or(timo_ai::ApiKind::OpenaiCompletions);
        timo_ai::Model::descriptor(provider, id, api)
    }

    /// 问一次路由；结果要经目录核对（不信任扩展回传的其余字段）。
    pub(super) async fn route_choice(
        &mut self,
        reason: RouteReason,
        failed: Option<(&timo_ai::Model, timo_config::ThinkingLevel)>,
    ) -> std::result::Result<
        Option<(
            timo_ai::Model,
            timo_config::ThinkingLevel,
            Option<serde_json::Value>,
        )>,
        String,
    > {
        let Some(hooks) = self.config.hooks.clone() else {
            return Ok(None);
        };
        let previous = self.previous_response();
        let state = if reason == RouteReason::Direct {
            None
        } else {
            self.route_state.as_ref()
        };
        let request = RouteRequest {
            model: &self.selection,
            thinking: self.selected_thinking,
            reason,
            previous: previous.as_ref().map(|(model, level)| (model, *level)),
            failed,
            state,
            messages: &self.messages,
        };
        let choice: RouteChoice = match hooks.route_model(request).await {
            Ok(Some(choice)) => choice,
            Ok(None) => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "Virtual model {}/{}: route failed: {error}",
                    self.selection.provider, self.selection.id
                ));
            }
        };
        let target = match &self.config.dispatch {
            Some(dispatch) => dispatch
                .resolve(&choice.model.provider, &choice.model.id)
                .ok_or_else(|| {
                    format!(
                        "Virtual model {}/{} routed to {}/{}, which is not a physical model",
                        self.selection.provider,
                        self.selection.id,
                        choice.model.provider,
                        choice.model.id
                    )
                })?,
            None => choice.model,
        };
        Ok(Some((target, choice.thinking, choice.state)))
    }

    /// 本请求模型的协议适配器（无 dispatch 时沿用装配期那一个）。
    pub(super) fn api_for(
        &self,
        model: &timo_ai::Model,
    ) -> std::sync::Arc<dyn timo_ai::ProviderApi> {
        match &self.config.dispatch {
            Some(dispatch) => dispatch.api(model),
            None => std::sync::Arc::clone(&self.config.api),
        }
    }

    /// 本请求模型的凭证解析器（无 dispatch 时沿用装配期那一个）。
    pub(super) fn auth_for(&self, model: &timo_ai::Model) -> std::sync::Arc<timo_ai::AuthResolver> {
        match &self.config.dispatch {
            Some(dispatch) => dispatch.auth(model),
            None => std::sync::Arc::clone(&self.config.auth),
        }
    }
}
