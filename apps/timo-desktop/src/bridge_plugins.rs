//! 插件与市场命令的内核侧处理（从 `bridge.rs` 拆出）。

use timo_kernel::Kernel;
use timo_kernel::plugins::market::{self, MarketIndex, MarketWarning};
use timo_kernel::plugins::{self, PermissionTier};
use tokio::sync::mpsc::UnboundedSender;

use super::bridge_events::{BridgeEvent, MarketSnapshot};

pub(super) fn list(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>) {
    let entries = plugins::list_plugins(kernel.data_dir(), kernel.work_dir());
    let _ = event_tx.send(BridgeEvent::Plugins(entries));
}

pub(super) fn set_enabled(
    kernel: &Kernel,
    event_tx: &UnboundedSender<BridgeEvent>,
    id: &str,
    enabled: bool,
) {
    match plugins::set_enabled(kernel.data_dir(), id, enabled) {
        Ok(()) => list(kernel, event_tx),
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::PluginError(error.to_string()));
        }
    }
}

pub(super) fn set_tier(
    kernel: &Kernel,
    event_tx: &UnboundedSender<BridgeEvent>,
    id: &str,
    tier: PermissionTier,
) {
    match plugins::set_tier(kernel.data_dir(), id, tier) {
        Ok(()) => list(kernel, event_tx),
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::PluginError(error.to_string()));
        }
    }
}

pub(super) fn uninstall(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>, id: &str) {
    match plugins::uninstall_plugin(kernel.data_dir(), kernel.work_dir(), id) {
        Ok(()) => {
            list(kernel, event_tx);
            load_market(kernel, event_tx, None);
        }
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::PluginError(error.to_string()));
        }
    }
}

pub(super) fn revoke(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>, id: &str) {
    match plugins::revoke_plugin(kernel.data_dir(), kernel.work_dir(), id) {
        Ok(()) => list(kernel, event_tx),
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::PluginError(error.to_string()));
        }
    }
}

pub(super) fn load_market(
    kernel: &Kernel,
    event_tx: &UnboundedSender<BridgeEvent>,
    offline: Option<String>,
) {
    let data_dir = kernel.data_dir();
    let (index, warnings) = match market::load_cached(data_dir) {
        Ok(Some(load)) => (Some(load.index), warning_lines(&load.warnings)),
        Ok(None) => (None, vec![]),
        Err(error) => (None, vec![error.to_string()]),
    };
    let _ = event_tx.send(BridgeEvent::Market(market_snapshot(
        kernel, index, warnings, offline,
    )));
}

pub(super) async fn refresh(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>) {
    let Some(url) = market::index_url(kernel.data_dir()) else {
        load_market(
            kernel,
            event_tx,
            Some("market index url is not configured".into()),
        );
        return;
    };
    match market::refresh(kernel.data_dir(), &url).await {
        Ok(market::RefreshOutcome::Updated(load)) => {
            let snapshot = market_snapshot(
                kernel,
                Some(load.index),
                warning_lines(&load.warnings),
                None,
            );
            let _ = event_tx.send(BridgeEvent::Market(snapshot));
        }
        Ok(market::RefreshOutcome::NotModified) => load_market(kernel, event_tx, None),
        Ok(market::RefreshOutcome::Offline(message)) => {
            load_market(kernel, event_tx, Some(message));
        }
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::MarketError(error.to_string()));
        }
    }
}

pub(super) async fn save_index(
    kernel: &Kernel,
    event_tx: &UnboundedSender<BridgeEvent>,
    url: Option<String>,
) {
    match market::set_index_url(kernel.data_dir(), url.as_deref()) {
        Ok(()) if url.is_some() => refresh(kernel, event_tx).await,
        Ok(()) => load_market(kernel, event_tx, None),
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::MarketError(error.to_string()));
        }
    }
}

pub(super) fn toggle_favorite(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>, id: &str) {
    match market::toggle_favorite(kernel.data_dir(), id) {
        Ok(_) => load_market(kernel, event_tx, None),
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::MarketError(error.to_string()));
        }
    }
}

pub(super) fn toggle_blocked(kernel: &Kernel, event_tx: &UnboundedSender<BridgeEvent>, id: &str) {
    match market::toggle_blocked(kernel.data_dir(), id) {
        Ok(_) => load_market(kernel, event_tx, None),
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::MarketError(error.to_string()));
        }
    }
}

/// 下载市场包到暂存目录；验签、挡位确认与落盘仍由桌面层负责（设计 08 §4）。
pub(super) async fn install_market(
    kernel: &Kernel,
    event_tx: &UnboundedSender<BridgeEvent>,
    id: &str,
) {
    let data_dir = kernel.data_dir();
    let plugin = market::load_cached(data_dir)
        .ok()
        .flatten()
        .and_then(|load| {
            load.index
                .plugins
                .into_iter()
                .find(|plugin| plugin.id == id)
        });
    let Some(plugin) = plugin else {
        let _ = event_tx.send(BridgeEvent::MarketDownloadFailed {
            id: id.to_string(),
            message: format!("plugin `{id}` is not in the market index"),
        });
        return;
    };
    match market::download(data_dir, &plugin).await {
        Ok(dir) => {
            let _ = event_tx.send(BridgeEvent::MarketDownloaded {
                id: id.to_string(),
                dir,
            });
        }
        Err(error) => {
            let _ = event_tx.send(BridgeEvent::MarketDownloadFailed {
                id: id.to_string(),
                message: error.to_string(),
            });
        }
    }
}

fn market_snapshot(
    kernel: &Kernel,
    index: Option<MarketIndex>,
    warnings: Vec<String>,
    offline: Option<String>,
) -> MarketSnapshot {
    let data_dir = kernel.data_dir();
    MarketSnapshot {
        index,
        warnings,
        offline,
        state: market::load_state(data_dir),
        index_url: market::index_url(data_dir),
    }
}

fn warning_lines(warnings: &[MarketWarning]) -> Vec<String> {
    warnings
        .iter()
        .map(|warning| format!("{}: {}", warning.code, warning.message))
        .collect()
}
