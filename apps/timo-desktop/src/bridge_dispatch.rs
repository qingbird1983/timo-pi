//! 非 run 期命令的内核侧分发（从 `bridge.rs` 拆出，守住单文件行数预算）。

use std::path::Path;
use std::sync::{Arc, Mutex};

use timo_kernel::Kernel;
use timo_kernel::config::KEYCHAIN_SERVICE;
use tokio::sync::mpsc::UnboundedSender;

use crate::settings::ReviewPrefs;

use super::BridgeCommand;
use super::bridge_events::BridgeEvent;
use super::bridge_plugins;
use super::providers::{models_path, provider_infos, save_provider_key};

/// 处理一条命令；`Run` 由主循环单独持有（需要同时等取消与插件请求）。
pub(super) async fn dispatch(
    command: BridgeCommand,
    kernel: &mut Kernel,
    data_dir: Option<&Path>,
    prefs: &Arc<Mutex<ReviewPrefs>>,
    event_tx: &UnboundedSender<BridgeEvent>,
) {
    match command {
        BridgeCommand::Run(_) | BridgeCommand::Cancel => {}
        BridgeCommand::RefreshSessions => send_sessions(kernel, event_tx),
        BridgeCommand::NewSession => {
            kernel.new_session();
            let _ = event_tx.send(BridgeEvent::Opened {
                id: None,
                history: vec![],
            });
            let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
        }
        BridgeCommand::OpenSession(id) => match kernel.resume(&id).await {
            Ok(()) => {
                let history = kernel.history().unwrap_or_default().to_vec();
                let _ = event_tx.send(BridgeEvent::Opened {
                    id: Some(id),
                    history,
                });
                let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
            }
            Err(error) => {
                let _ = event_tx.send(BridgeEvent::OpenFailed(error.to_string()));
            }
        },
        BridgeCommand::DeleteSession(id) => match kernel.delete_session(&id) {
            Ok(()) => {
                let _ = event_tx.send(BridgeEvent::Deleted(id));
                send_sessions(kernel, event_tx);
                let _ = event_tx.send(BridgeEvent::Usage(kernel.usage_summary()));
            }
            Err(error) => {
                let _ = event_tx.send(BridgeEvent::DeleteFailed(error.to_string()));
            }
        },
        BridgeCommand::SetReviewPrefs { files, commands } => {
            if let Ok(mut prefs) = prefs.lock() {
                prefs.files = files;
                prefs.commands = commands;
            }
        }
        BridgeCommand::ListProviders => {
            let model = kernel.model_label();
            let list = provider_infos(data_dir, &model);
            let _ = event_tx.send(BridgeEvent::Providers(list));
        }
        BridgeCommand::SaveProviderKey { provider, secret } => {
            let path = models_path(data_dir);
            match save_provider_key(path, &provider, &secret).await {
                Ok(()) => {
                    let _ = event_tx.send(BridgeEvent::ProviderSaved(provider));
                    let model = kernel.model_label();
                    let list = provider_infos(data_dir, &model);
                    let _ = event_tx.send(BridgeEvent::Providers(list));
                }
                Err(error) => {
                    let _ = event_tx.send(BridgeEvent::ProviderError(error));
                }
            }
        }
        BridgeCommand::RemoveProviderKey { provider } => {
            let provider_name = provider.clone();
            let removed = tokio::task::spawn_blocking(move || {
                timo_kernel::config::keychain_delete(KEYCHAIN_SERVICE, &provider_name)
            })
            .await;
            match removed {
                Ok(Ok(())) => {
                    let _ = event_tx.send(BridgeEvent::ProviderRemoved(provider));
                    let model = kernel.model_label();
                    let list = provider_infos(data_dir, &model);
                    let _ = event_tx.send(BridgeEvent::Providers(list));
                }
                Ok(Err(error)) => {
                    let _ = event_tx.send(BridgeEvent::ProviderError(error.to_string()));
                }
                Err(error) => {
                    let _ = event_tx.send(BridgeEvent::ProviderError(error.to_string()));
                }
            }
        }
        BridgeCommand::ListPlugins => bridge_plugins::list(kernel, event_tx),
        BridgeCommand::SetPluginEnabled { id, enabled } => {
            bridge_plugins::set_enabled(kernel, event_tx, &id, enabled);
        }
        BridgeCommand::SetPluginTier { id, tier } => {
            bridge_plugins::set_tier(kernel, event_tx, &id, tier);
        }
        BridgeCommand::UninstallPlugin { id } => {
            bridge_plugins::uninstall(kernel, event_tx, &id);
        }
        BridgeCommand::RevokePlugin { id } => {
            bridge_plugins::revoke(kernel, event_tx, &id);
        }
        BridgeCommand::LoadMarket => bridge_plugins::load_market(kernel, event_tx, None),
        BridgeCommand::RefreshMarket => bridge_plugins::refresh(kernel, event_tx).await,
        BridgeCommand::SaveMarketIndex { url } => {
            bridge_plugins::save_index(kernel, event_tx, url).await;
        }
        BridgeCommand::ToggleMarketFavorite { id } => {
            bridge_plugins::toggle_favorite(kernel, event_tx, &id);
        }
        BridgeCommand::ToggleMarketBlocked { id } => {
            bridge_plugins::toggle_blocked(kernel, event_tx, &id);
        }
        BridgeCommand::InstallMarketPlugin { id } => {
            bridge_plugins::install_market(kernel, event_tx, &id).await;
        }
    }
}

fn send_sessions(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>) {
    let _ = event_tx.send(BridgeEvent::Sessions(kernel.list_sessions()));
}
