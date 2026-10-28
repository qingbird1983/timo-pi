//! 供应商与钥匙串桥接辅助（从 `bridge.rs` 拆出；设计 02 §5）。

use std::path::{Path, PathBuf};

use timo_kernel::config::{CredentialSource, KEYCHAIN_SERVICE, ValueRef};

use super::{CredentialInfo, ProviderInfo};

pub(super) fn models_path(data_dir: Option<&Path>) -> Option<PathBuf> {
    match data_dir {
        Some(dir) => Some(dir.join("models.json")),
        None => timo_kernel::config::models_path().ok(),
    }
}

pub(super) fn provider_infos(data_dir: Option<&Path>, current_model: &str) -> Vec<ProviderInfo> {
    let mut list = Vec::new();
    let Some(path) = models_path(data_dir) else {
        return list;
    };
    if let Ok(custom) = timo_kernel::config::load_custom_providers(&path) {
        for provider in custom {
            let credential = match &provider.api_key {
                Some(value) => match value {
                    // 引用保留但条目已被移除时显示“未配置”，避免误导。
                    ValueRef::Keychain { service, account }
                        if timo_kernel::config::keychain_get(service, account).is_err() =>
                    {
                        CredentialInfo::None
                    }
                    _ => credential_info(value),
                },
                None => CredentialInfo::None,
            };
            list.push(ProviderInfo {
                id: provider.name.clone(),
                models: provider
                    .models
                    .iter()
                    .map(|model| model.id.clone())
                    .collect(),
                credential,
                editable: true,
            });
        }
    }
    let (provider, model) = current_model.split_once('/').unwrap_or((current_model, ""));
    if !list.iter().any(|info| info.id == provider) {
        list.push(ProviderInfo {
            id: provider.to_string(),
            models: if model.is_empty() {
                vec![]
            } else {
                vec![model.to_string()]
            },
            credential: CredentialInfo::Builtin,
            editable: false,
        });
    }
    list
}

pub(super) fn credential_info(value: &ValueRef) -> CredentialInfo {
    match value.source() {
        CredentialSource::Keychain { service, account } => {
            CredentialInfo::Keychain { service, account }
        }
        CredentialSource::Env(name) => CredentialInfo::Env(name),
        CredentialSource::Command => CredentialInfo::Command,
        CredentialSource::Literal => CredentialInfo::Literal,
    }
}

pub(super) async fn save_provider_key(
    path: Option<PathBuf>,
    provider: &str,
    secret: &str,
) -> Result<(), String> {
    let Some(path) = path else {
        return Err("data directory is unavailable".into());
    };
    let provider_owned = provider.to_string();
    let secret_owned = secret.to_string();
    tokio::task::spawn_blocking(move || {
        let old = timo_kernel::config::keychain_get(KEYCHAIN_SERVICE, &provider_owned).ok();
        timo_kernel::config::keychain_set(KEYCHAIN_SERVICE, &provider_owned, &secret_owned)
            .map_err(|error| error.to_string())?;
        let value = ValueRef::Keychain {
            service: KEYCHAIN_SERVICE.to_string(),
            account: provider_owned.clone(),
        };
        if let Err(error) =
            timo_kernel::config::set_provider_api_key(&path, &provider_owned, &value)
        {
            // 回滚钥匙串，避免留下与配置不一致的新密钥。
            match old {
                Some(previous) => {
                    let _ = timo_kernel::config::keychain_set(
                        KEYCHAIN_SERVICE,
                        &provider_owned,
                        &previous,
                    );
                }
                None => {
                    let _ = timo_kernel::config::keychain_delete(KEYCHAIN_SERVICE, &provider_owned);
                }
            }
            return Err(error.to_string());
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}
