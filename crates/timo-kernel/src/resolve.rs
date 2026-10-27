//! 模型解析（设计 05 §5 优先级：显式 > 设置默认 > 首个有凭证的内置供应商）。

use std::sync::{Arc, Mutex};

use timo_agent::ModelDispatch;
use timo_ai::{ApiKind, AuthResolver, Catalog, Compat, Model, RateCard};
use timo_config::{CustomProvider, Settings, ThinkingLevel};

use crate::Result;
use crate::error::KernelError;

/// 解析结果：模型 + 生效思考档位 + 认证解析器。
pub struct ResolvedModel {
    /// 模型。
    pub model: Model,
    /// 思考档位（Off = 不发 thinking）。
    pub thinking: ThinkingLevel,
    /// 凭证解析器。
    pub auth: std::sync::Arc<AuthResolver>,
}

/// 装载目录：内置资产 + models.json 自定义供应商（设计 05 §3）。
///
/// # Errors
/// models.json 非法 → [`KernelError::Config`]；api 协议名未知 → 同上。
pub fn load_catalog(custom: &[CustomProvider]) -> Result<Catalog> {
    let mut catalog = Catalog::builtin();
    let default_compat = Compat::default();
    for p in custom {
        let api = match p.api.as_deref().map(ApiKind::parse) {
            Some(None) => {
                return Err(KernelError::ModelNotFound(format!(
                    "provider {}: unknown api `{}` (known: anthropic-messages | openai-completions | pi-virtual)",
                    p.name,
                    p.api.as_deref().unwrap_or_default()
                )));
            }
            Some(Some(api)) => api,
            None => ApiKind::OpenaiCompletions,
        };
        for m in &p.models {
            catalog.upsert(Model {
                id: m.id.clone(),
                name: m.name.clone().unwrap_or_else(|| m.id.clone()),
                provider: p.name.clone(),
                api,
                base_url: m
                    .base_url
                    .clone()
                    .or_else(|| p.base_url.clone())
                    .unwrap_or_default(),
                reasoning: m.reasoning,
                input: m.input.clone(),
                cost: RateCard {
                    input: m.cost.input,
                    output: m.cost.output,
                    cache_read: m.cost.cache_read,
                    cache_write: m.cost.cache_write,
                    tiers: vec![],
                },
                context_window: m.context_window,
                max_tokens: m.max_tokens,
                // 供应商级在前、模型级覆盖同名（设计 10 §4.1）。
                headers: timo_ai::merge_headers(p.headers.clone(), &m.headers),
                cache_ttl_seconds: m.cache_ttl_seconds,
                // 未声明的兼容项沿用默认（Phase 1 行为不变）。
                compat: Compat {
                    max_tokens_field: m
                        .compat
                        .max_tokens_field
                        .unwrap_or(default_compat.max_tokens_field),
                    supports_usage_in_streaming: m
                        .compat
                        .supports_usage_in_streaming
                        .unwrap_or(default_compat.supports_usage_in_streaming),
                    supports_finish_reason: m
                        .compat
                        .supports_finish_reason
                        .unwrap_or(default_compat.supports_finish_reason),
                    send_auth_header: p.auth_header.unwrap_or(default_compat.send_auth_header),
                },
            });
        }
    }
    Ok(catalog)
}

/// 模型解析主入口（设计 05 §5）。
///
/// # Errors
/// 显式指定未命中 → [`KernelError::ModelNotFound`]；全无凭证 → 同（经 NoCredential）。
pub async fn resolve_model(
    catalog: &Catalog,
    settings: &Settings,
    override_spec: Option<&(String, String)>,
    explicit_key: Option<&str>,
    custom: &[CustomProvider],
) -> Result<ResolvedModel> {
    // 1. 显式指定（provider 为空 = 裸 id：目录内唯一才可用，设计 08 §5）
    if let Some((provider, id)) = override_spec {
        let model = if provider.is_empty() {
            match catalog.by_id(id).as_slice() {
                [] => return Err(KernelError::ModelNotFound(id.clone())),
                [only] => (*only).clone(),
                many => {
                    let candidates: Vec<String> = many
                        .iter()
                        .map(|m| format!("{}/{}", m.provider, m.id))
                        .collect();
                    return Err(KernelError::ModelNotFound(format!(
                        "`{id}` is ambiguous, use one of: {}",
                        candidates.join(", ")
                    )));
                }
            }
        } else {
            catalog
                .get(provider, id)
                .ok_or_else(|| KernelError::ModelNotFound(format!("{provider}/{id}")))?
                .clone()
        };
        return Ok(finish(catalog, model, settings, explicit_key, custom));
    }
    // 2. settings 默认（凭证可用才采纳）
    if let (Some(dp), Some(dm)) = (&settings.default_provider, &settings.default_model)
        && let Some(m) = catalog.get(dp, dm)
    {
        let model = m.clone();
        let auth = build_auth(catalog, &model, explicit_key, custom);
        // 虚拟条目不需要自身凭证：路由会挑一个有凭证的物理模型（设计 10 §4.2）。
        if model.is_virtual() || auth.resolve().await.is_ok() {
            return Ok(finish_with(model, settings, auth));
        }
    }
    // 3. 首个有凭证的供应商（anthropic 先于 openai，目录序；自定义供应商殿后）
    for provider in builtin_provider_order(custom) {
        if let Some(m) = first_physical(catalog, &provider) {
            let auth = build_auth(catalog, &m, explicit_key, custom);
            if auth.resolve().await.is_ok() {
                return Ok(finish_with(m, settings, auth));
            }
        }
    }
    Err(KernelError::ModelNotFound(
        "no provider with usable credential (set ANTHROPIC_API_KEY / OPENAI_API_KEY or --api-key)"
            .into(),
    ))
}

fn finish(
    catalog: &Catalog,
    model: Model,
    settings: &Settings,
    explicit_key: Option<&str>,
    custom: &[CustomProvider],
) -> ResolvedModel {
    let auth = build_auth(catalog, &model, explicit_key, custom);
    finish_with(model, settings, auth)
}

fn finish_with(
    model: Model,
    settings: &Settings,
    auth: std::sync::Arc<AuthResolver>,
) -> ResolvedModel {
    ResolvedModel {
        model,
        thinking: settings
            .default_thinking_level
            .unwrap_or(ThinkingLevel::Medium),
        auth,
    }
}

/// 自动兜底只从物理模型里挑：虚拟模型必须先路由，不能被自动选中。
fn first_physical(catalog: &Catalog, provider: &str) -> Option<Model> {
    catalog
        .by_provider(provider)
        .into_iter()
        .find(|m| !m.is_virtual())
        .cloned()
}

/// 内置供应商顺序 + 自定义供应商（去重）。
fn builtin_provider_order(custom: &[CustomProvider]) -> Vec<String> {
    let mut order = vec!["anthropic".to_string(), "openai".to_string()];
    for p in custom {
        if !order.contains(&p.name) {
            order.push(p.name.clone());
        }
    }
    order
}

/// 供应商认证解析器：显式 > provider api_key（自定义）> 内置 env 表（设计 05 §4）。
pub(crate) fn build_auth(
    catalog: &Catalog,
    model: &Model,
    explicit_key: Option<&str>,
    custom: &[CustomProvider],
) -> std::sync::Arc<AuthResolver> {
    let provider_ref = custom
        .iter()
        .find(|p| p.name == model.provider)
        .and_then(|p| p.api_key.clone());
    let env_names: Vec<String> = if provider_ref.is_none() {
        catalog.provider_env(&model.provider)
    } else {
        vec![]
    };
    std::sync::Arc::new(AuthResolver {
        explicit: explicit_key.map(String::from),
        provider_ref,
        env_names,
    })
}

/// 请求期模型装配：目录 + 凭证 + 适配器缓存（设计 10 §4.2）。
pub(crate) struct CatalogDispatch {
    catalog: Catalog,
    custom: Vec<CustomProvider>,
    explicit: Option<String>,
    apis: Mutex<Vec<(ApiKind, Arc<dyn timo_ai::ProviderApi>)>>,
}

impl CatalogDispatch {
    /// 由装配期目录与凭证来源构造。
    pub(crate) fn new(
        catalog: Catalog,
        custom: Vec<CustomProvider>,
        explicit: Option<String>,
    ) -> Self {
        Self {
            catalog,
            custom,
            explicit,
            apis: Mutex::new(Vec::new()),
        }
    }
}

impl ModelDispatch for CatalogDispatch {
    fn resolve(&self, provider: &str, id: &str) -> Option<Model> {
        self.catalog
            .get(provider, id)
            .filter(|model| !model.is_virtual())
            .cloned()
    }

    fn api(&self, model: &Model) -> Arc<dyn timo_ai::ProviderApi> {
        let mut cache = self
            .apis
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, api)) = cache.iter().find(|(kind, _)| *kind == model.api) {
            return Arc::clone(api);
        }
        let api = timo_ai::api_for(model.api);
        cache.push((model.api, Arc::clone(&api)));
        api
    }

    fn auth(&self, model: &Model) -> Arc<AuthResolver> {
        build_auth(&self.catalog, model, self.explicit.as_deref(), &self.custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn custom(json: serde_json::Value) -> timo_config::CustomProvider {
        serde_json::from_value(json).expect("provider fixture")
    }

    #[test]
    fn model_headers_override_provider_headers_and_compat_is_applied() {
        let catalog = load_catalog(&[custom(serde_json::json!({
            "name": "gw",
            "base_url": "https://gw.example/v1",
            "api": "openai-completions",
            "auth_header": false,
            "headers": [["x-org", "acme"], ["x-shared", "provider"]],
            "models": [{
                "id": "m1",
                "headers": [["x-shared", "model"]],
                "compat": { "max_tokens_field": true, "supports_usage_in_streaming": false }
            }]
        }))])
        .expect("catalog");

        let model = catalog.get("gw", "m1").expect("model");
        assert_eq!(
            model.headers,
            vec![
                ("x-org".to_string(), "acme".to_string()),
                ("x-shared".to_string(), "model".to_string()),
            ]
        );
        assert!(model.compat.max_tokens_field);
        assert!(!model.compat.supports_usage_in_streaming);
        assert!(model.compat.supports_finish_reason, "未声明的项沿用默认");
        assert!(!model.compat.send_auth_header);
    }

    fn virtual_providers() -> Vec<CustomProvider> {
        vec![custom(serde_json::json!({
            "name": "router",
            "api": "pi-virtual",
            "models": [{ "id": "auto", "name": "Auto" }]
        }))]
    }

    #[tokio::test]
    async fn a_virtual_entry_is_listed_and_selectable_but_never_a_route_target() {
        let providers = virtual_providers();
        let catalog = load_catalog(&providers).expect("catalog");
        let entry = catalog.get("router", "auto").expect("virtual entry");
        assert!(entry.is_virtual());
        assert_eq!(entry.name, "Auto");
        assert_eq!(
            (entry.cost.input, entry.cost.output),
            (0.0, 0.0),
            "虚拟条目不携带费率"
        );
        assert!(entry.base_url.is_empty());

        let resolved = resolve_model(
            &catalog,
            &Settings::default(),
            Some(&("router".to_string(), "auto".to_string())),
            Some("k"),
            &providers,
        )
        .await
        .expect("virtual selection is allowed; the adapter rejects unrouted requests");
        assert!(resolved.model.is_virtual());

        let dispatch = CatalogDispatch::new(catalog, providers, Some("k".to_string()));
        assert!(
            dispatch.resolve("router", "auto").is_none(),
            "虚拟条目不能成为路由目标"
        );
    }

    #[tokio::test]
    async fn dispatch_resolves_physical_models_and_caches_one_adapter_per_kind() {
        let providers = virtual_providers();
        let mut catalog = load_catalog(&providers).expect("catalog");
        catalog.upsert(Model::descriptor(
            "mock",
            "small",
            ApiKind::AnthropicMessages,
        ));
        let dispatch = CatalogDispatch::new(catalog, providers, Some("k".to_string()));
        let small = dispatch.resolve("mock", "small").expect("physical model");
        assert!(!small.is_virtual());
        assert!(dispatch.resolve("mock", "missing").is_none());
        let first = dispatch.api(&small);
        let second = dispatch.api(&small);
        assert!(Arc::ptr_eq(&first, &second), "同协议适配器应复用");
        assert_eq!(
            dispatch.auth(&small).resolve().await.expect("explicit key"),
            "k"
        );
    }

    #[test]
    fn auto_selection_never_picks_a_virtual_entry() {
        let providers = vec![custom(serde_json::json!({
            "name": "gw",
            "models": [{ "id": "m" }]
        }))];
        let mut catalog = load_catalog(&providers).expect("catalog");
        // 目录序里 `a` 排在 `m` 前面：兜底必须跳过虚拟条目。
        catalog.upsert(Model::virtual_entry("gw", "a", "A"));
        assert_eq!(first_physical(&catalog, "gw").expect("physical").id, "m");

        catalog.upsert(Model::virtual_entry("router", "auto", "Auto"));
        assert!(
            first_physical(&catalog, "router").is_none(),
            "只有虚拟条目的供应商不能被自动选中"
        );
    }

    #[test]
    fn undeclared_switches_keep_phase_one_defaults() {
        let catalog = load_catalog(&[custom(serde_json::json!({
            "name": "plain",
            "models": [{ "id": "m" }]
        }))])
        .expect("catalog");

        let model = catalog.get("plain", "m").expect("model");
        assert!(model.headers.is_empty(), "{:?}", model.headers);
        let defaults = Compat::default();
        assert_eq!(model.compat.max_tokens_field, defaults.max_tokens_field);
        assert_eq!(
            model.compat.supports_usage_in_streaming,
            defaults.supports_usage_in_streaming
        );
        assert_eq!(
            model.compat.supports_finish_reason,
            defaults.supports_finish_reason
        );
        assert!(model.compat.send_auth_header, "缺省必须仍然发认证头");
    }
}
