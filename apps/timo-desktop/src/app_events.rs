//! 桥接事件分发（从 `app.rs` 拆出，设计 01 §5）。

use gpui_kit::*;

use crate::app::DesktopApp;
use crate::bridge::BridgeEvent;
use timo_kernel::KernelEvent;

impl DesktopApp {
    pub(crate) fn handle_bridge(&mut self, event: BridgeEvent, cx: &mut Context<Self>) {
        match event {
            BridgeEvent::Ready { model, sessions } => {
                self.ready = true;
                self.fatal = None;
                self.model = Some(model);
                self.sync_sessions(sessions);
            }
            BridgeEvent::Sessions(sessions) => {
                self.sync_sessions(sessions);
            }
            BridgeEvent::Commands(commands) => {
                self.slash_commands = commands;
            }
            BridgeEvent::Usage(usage) => {
                self.usage = usage;
            }
            BridgeEvent::Deleted(id) => {
                if self.session_id.as_deref() == Some(id.as_str()) {
                    self.session_id = None;
                    self.session_title = t!("chat.empty_session").to_string();
                    self.chat.update(cx, |chat, _| chat.reset());
                    self.sync_scroller(cx);
                }
            }
            BridgeEvent::Opened { id, history } => {
                self.session_id = id;
                self.chat.update(cx, |chat, _| chat.load_history(&history));
                self.sync_scroller(cx);
                self.refresh_title(cx);
            }
            BridgeEvent::Running => {
                self.chat.update(cx, |chat, _| chat.running = true);
            }
            BridgeEvent::Finished => {
                self.chat.update(cx, |chat, _| chat.running = false);
                self.cancel_pending_reviews(cx);
                self.refresh_title(cx);
            }
            BridgeEvent::Kernel(event) => {
                let before = self.chat.read(cx).rows.len();
                self.chat.update(cx, |chat, _| chat.apply(&event));
                let after = self.chat.read(cx).rows.len();
                if after != before {
                    self.sync_scroller(cx);
                }
                if matches!(event, KernelEvent::AgentFinished { .. }) {
                    self.refresh_title(cx);
                }
            }
            BridgeEvent::Review { request, respond } => {
                self.handle_review_request(request, respond, cx);
            }
            BridgeEvent::Providers(providers) => {
                self.providers = providers;
                self.provider_error = None;
            }
            BridgeEvent::ProviderSaved(provider) => {
                self.provider_error = None;
                self.provider_feedback =
                    Some(t!("settings.key_saved", provider = provider).to_string());
                self.notice = Some(t!("settings.key_saved", provider = provider).to_string());
            }
            BridgeEvent::ProviderRemoved(provider) => {
                self.provider_feedback =
                    Some(t!("settings.key_removed", provider = provider).to_string());
                self.notice = Some(t!("settings.key_removed", provider = provider).to_string());
            }
            BridgeEvent::ProviderError(message) => {
                self.provider_error = Some(message);
                self.provider_feedback = None;
            }
            BridgeEvent::Plugins(entries) => {
                self.plugins_error = None;
                self.plugins = match self.plugin_runtime.as_mut() {
                    Some(runtime) => runtime.set_entries(entries),
                    None => entries,
                };
            }
            BridgeEvent::PluginError(message) => {
                self.plugins_error = Some(message);
            }
            BridgeEvent::Market(snapshot) => {
                self.market_index = snapshot.index;
                self.market_warnings = snapshot.warnings;
                self.market_offline = snapshot.offline;
                self.market_state = snapshot.state;
                self.market_url = snapshot.index_url;
                self.market_error = None;
            }
            BridgeEvent::MarketError(message) => {
                self.market_error = Some(message);
            }
            BridgeEvent::MarketDownloaded { id, dir } => {
                if self.market_installing.as_deref() == Some(id.as_str()) {
                    self.market_installing = None;
                }
                cx.spawn(async move |this, cx| {
                    let outcome = timo_kernel::data_dir()
                        .map_err(|error| error.to_string())
                        .and_then(|data_dir| {
                            crate::features::plugins::nsh::install::verify_dir(&data_dir, &dir).map(
                                |mut candidate| {
                                    candidate.cleanup = true;
                                    candidate
                                },
                            )
                        });
                    let _ = this.update(cx, |this, cx| {
                        match outcome {
                            Ok(candidate) => {
                                this.pending_install = Some(candidate);
                                this.market_error = None;
                            }
                            Err(message) => {
                                let _ = std::fs::remove_dir_all(&dir);
                                this.pending_install = None;
                                this.market_error = Some(message);
                            }
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            BridgeEvent::MarketDownloadFailed { id, message } => {
                if self.market_installing.as_deref() == Some(id.as_str()) {
                    self.market_installing = None;
                }
                self.market_error = Some(message);
            }
            BridgeEvent::Fatal(message) => {
                self.ready = false;
                self.fatal = Some(message);
            }
            BridgeEvent::OpenFailed(message) => {
                self.notice = Some(t!("chat.open_failed", message = message).to_string());
            }
            BridgeEvent::DeleteFailed(message) => {
                self.notice = Some(t!("chat.delete_failed", message = message).to_string());
            }
            BridgeEvent::RunFailed(message) => {
                self.notice = Some(t!("chat.run_failed", message = message).to_string());
            }
            BridgeEvent::Busy => {
                self.notice = Some(t!("chat.busy").to_string());
            }
            BridgeEvent::Lagged(skipped) => {
                self.notice = Some(t!("chat.lagged", n = skipped).to_string());
            }
        }
    }
}
