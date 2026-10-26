//! 模型解析（设计 05 §5 优先级：显式 > 设置默认 > 首个有凭证的内置供应商）。

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
    for p in custom {
        let api = match p.api.as_deref().map(ApiKind::parse) {
            Some(None) => {
                return Err(KernelError::ModelNotFound(format!(
                    "provider {}: unknown api `{}` (phase 1: anthropic-messages | openai-completions)",
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
                headers: p.headers.clone(),
                compat: Compat::default(),
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
    // 1. 显式指定
    if let Some((provider, id)) = override_spec {
        let model = catalog
            .get(provider, id)
            .ok_or_else(|| KernelError::ModelNotFound(format!("{provider}/{id}")))?
            .clone();
        return Ok(finish(catalog, model, settings, explicit_key, custom));
    }
    // 2. settings 默认（凭证可用才采纳）
    if let (Some(dp), Some(dm)) = (&settings.default_provider, &settings.default_model)
        && let Some(m) = catalog.get(dp, dm)
    {
        let model = m.clone();
        let auth = build_auth(catalog, &model, explicit_key, custom);
        if auth.resolve().await.is_ok() {
            return Ok(finish_with(model, settings, auth));
        }
    }
    // 3. 首个有凭证的供应商（anthropic 先于 openai，目录序；自定义供应商殿后）
    for provider in builtin_provider_order(custom) {
        if let Some(m) = catalog.by_provider(&provider).into_iter().next().cloned() {
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
fn build_auth(
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
