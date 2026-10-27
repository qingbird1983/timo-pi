//! 模型目录与供应商（设计 01 §6；接缝 #3：目录即纯数据）。
//!
//! 内置目录来自 `assets/models/*.json`（pi 目录子集，数据源 models.dev，
//! 与 pi 的生成管线同源；生成管线接入记 backlog——见 roadmap §3）。

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::errors::AiError;
use crate::types::{Cost, Usage};

/// 协议实现类型（接缝 #3：新增协议 = 新实现文件 + 目录数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiKind {
    /// Anthropic Messages API。
    AnthropicMessages,
    /// OpenAI Chat Completions API。
    OpenaiCompletions,
    /// 虚拟模型标记（设计 10 §4.2）：请求前必须先路由到物理模型。
    #[serde(rename = "pi-virtual")]
    Virtual,
}

impl ApiKind {
    /// 目录 JSON 中的规范名。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenaiCompletions => "openai-completions",
            Self::Virtual => "pi-virtual",
        }
    }

    /// 解析目录 JSON 中的协议名。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "anthropic-messages" => Some(Self::AnthropicMessages),
            "openai-completions" => Some(Self::OpenaiCompletions),
            "pi-virtual" => Some(Self::Virtual),
            _ => None,
        }
    }
}

/// openai-completions 的行为开关（pi compat 子集，Phase 1 显式配置不自动探测）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Compat {
    /// 用 `max_tokens` 而非 `max_completion_tokens`（deepseek/moonshot 等兼容网关）。
    pub max_tokens_field: bool,
    /// 流式响应携带 usage（`stream_options.include_usage`）。
    pub supports_usage_in_streaming: bool,
    /// 是否校验 finish_reason 必达。
    pub supports_finish_reason: bool,
    /// 是否发送协议自带的认证头（`authorization` / `x-api-key`）。
    ///
    /// 有些网关要求完全自定义的鉴权头，此时置 `false` 并用 `headers` 自己带；
    /// 缺省 `true` 保持 Phase 1 以来的行为（设计 10 §4.1）。
    pub send_auth_header: bool,
}

impl Default for Compat {
    fn default() -> Self {
        Self {
            max_tokens_field: false,
            supports_usage_in_streaming: true,
            supports_finish_reason: true,
            send_auth_header: true,
        }
    }
}

/// 模型条目（pi Model 的 Phase 1 子集）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// 模型 id（协议 `model` 字段）。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 所属供应商 id。
    pub provider: String,
    /// 协议类型。
    pub api: ApiKind,
    /// API 基底 URL。
    pub base_url: String,
    /// 是否推理模型（支持 thinking 档位）。
    pub reasoning: bool,
    /// 输入模态（text/image）。
    pub input: Vec<String>,
    /// 费率（USD/Mtok）。
    pub cost: RateCard,
    /// 上下文窗口。
    pub context_window: u64,
    /// 最大输出 token。
    pub max_tokens: u64,
    /// 额外请求头。
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// 前缀缓存 TTL（秒；None = 未知/不支持预热；phase-3.5/05）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ttl_seconds: Option<u64>,
    /// openai-completions 行为开关。
    #[serde(default)]
    pub compat: Compat,
}

impl Model {
    /// 是否虚拟条目（判据唯一：api 标记，对齐蓝本 `isVirtualModel`）。
    #[must_use]
    pub fn is_virtual(&self) -> bool {
        self.api == ApiKind::Virtual
    }

    /// 构造虚拟条目（设计 10 §4.2）：零费率、无端点，真实模型由路由决定。
    #[must_use]
    pub fn virtual_entry(provider: &str, id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            provider: provider.to_string(),
            api: ApiKind::Virtual,
            base_url: String::new(),
            reasoning: false,
            input: vec!["text".to_string()],
            cost: RateCard::default(),
            context_window: 0,
            max_tokens: 0,
            headers: vec![],
            cache_ttl_seconds: None,
            compat: Compat::default(),
        }
    }

    /// 只带身份的最小条目（路由钩子里 `previous`/`failed` 等跨参场景用）。
    ///
    /// 不是目录条目：宿主应按 `provider/id` 从目录重新取数据（设计 10 §4.2）。
    #[must_use]
    pub fn descriptor(provider: &str, id: &str, api: ApiKind) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            provider: provider.to_string(),
            api,
            base_url: String::new(),
            reasoning: false,
            input: vec!["text".to_string()],
            cost: RateCard::default(),
            context_window: 0,
            max_tokens: 0,
            headers: vec![],
            cache_ttl_seconds: None,
            compat: Compat::default(),
        }
    }
}

/// 费率卡（含阶梯；对齐 pi cost.tiers）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateCard {
    /// 输入费率。
    #[serde(default)]
    pub input: f64,
    /// 输出费率。
    #[serde(default)]
    pub output: f64,
    /// 缓存读费率。
    #[serde(default)]
    pub cache_read: f64,
    /// 缓存写费率。
    #[serde(default)]
    pub cache_write: f64,
    /// 阶梯（`input_tokens_above` 最大命中档整体适用）。
    #[serde(default)]
    pub tiers: Vec<Tier>,
}

/// 阶梯档。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tier {
    /// 输入 token 阈值。
    #[serde(rename = "inputTokensAbove")]
    pub input_tokens_above: u64,
    /// 输入费率。
    #[serde(default)]
    pub input: f64,
    /// 输出费率。
    #[serde(default)]
    pub output: f64,
    /// 缓存读费率。
    #[serde(default)]
    pub cache_read: f64,
    /// 缓存写费率。
    #[serde(default)]
    pub cache_write: f64,
}

/// 目录资产文件结构（`assets/models/<provider>.json`）。
#[derive(Debug, Deserialize)]
struct CatalogFile {
    provider: String,
    api: ApiKind,
    base_url: String,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    headers: Vec<(String, String)>,
    models: BTreeMap<String, ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    name: String,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    cost: RateCard,
    #[serde(default = "d_ctx")]
    context_window: u64,
    #[serde(default = "d_max")]
    max_tokens: u64,
    #[serde(default)]
    cache_ttl_seconds: Option<u64>,
}

fn d_ctx() -> u64 {
    128_000
}

fn d_max() -> u64 {
    16_384
}

/// 模型目录：内置资产 + 用户自定义（models.json）合并后的只读视图。
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    models: BTreeMap<String, Model>,
    /// 供应商 → 内置环境变量名表（凭证解析用；models.json 自定义供应商为空）。
    provider_env: BTreeMap<String, Vec<String>>,
}

impl Catalog {
    /// 装载内置目录（编译期内嵌 `assets/models/*.json`）。
    #[must_use]
    pub fn builtin() -> Self {
        let mut cat = Self::default();
        cat.merge_file(include_str!("../assets/models/anthropic.json"));
        cat.merge_file(include_str!("../assets/models/openai.json"));
        cat
    }

    /// 供应商的内置环境变量名表（设计 05 §4 优先级第 3 层）。
    #[must_use]
    pub fn provider_env(&self, provider: &str) -> Vec<String> {
        self.provider_env.get(provider).cloned().unwrap_or_default()
    }

    /// 合并一段目录资产 JSON（同名模型覆盖：后到者胜，供自定义目录使用）。
    pub fn merge_file(&mut self, raw: &str) {
        let file: CatalogFile = match serde_json::from_str(raw) {
            Ok(f) => f,
            Err(_) => return, // 目录资产损坏不 panic：跳过并在运行时以“未知模型”报错
        };
        self.provider_env
            .insert(file.provider.clone(), file.env.clone());
        for (id, e) in file.models {
            self.models.insert(
                format!("{}/{}", file.provider, id),
                Model {
                    id,
                    name: e.name,
                    provider: file.provider.clone(),
                    api: file.api,
                    base_url: file.base_url.clone(),
                    reasoning: e.reasoning,
                    input: e.input,
                    cost: e.cost,
                    context_window: e.context_window,
                    max_tokens: e.max_tokens,
                    headers: file.headers.clone(),
                    cache_ttl_seconds: e.cache_ttl_seconds,
                    compat: Compat::default(),
                },
            );
        }
    }

    /// 按 `provider/id` 精确查找。
    #[must_use]
    pub fn get(&self, provider: &str, id: &str) -> Option<&Model> {
        self.models.get(&format!("{provider}/{id}"))
    }

    /// 某供应商的全部模型（目录序）。
    #[must_use]
    pub fn by_provider(&self, provider: &str) -> Vec<&Model> {
        self.models
            .values()
            .filter(|m| m.provider == provider)
            .collect()
    }

    /// 按裸模型 id 跨供应商查找（`--model <id>` 不给 provider 时用；设计 08 §5）。
    /// 命中多个即歧义，调用方决定报错并列出候选。
    #[must_use]
    pub fn by_id(&self, id: &str) -> Vec<&Model> {
        let mut hits: Vec<&Model> = self.models.values().filter(|m| m.id == id).collect();
        hits.sort_by(|a, b| a.provider.cmp(&b.provider));
        hits
    }

    /// 插入或覆盖一个模型（供自定义供应商接入）。
    pub fn upsert(&mut self, model: Model) {
        self.models
            .insert(format!("{}/{}", model.provider, model.id), model);
    }

    /// 全部模型（目录序）。
    #[must_use]
    pub fn all(&self) -> Vec<&Model> {
        self.models.values().collect()
    }
}

/// 按费率卡计算费用（USD）；阶梯档按 `input_tokens_above` 最大命中整体适用（pi 同构）。
#[must_use]
pub fn calculate_cost(card: &RateCard, usage: &Usage) -> Cost {
    let mut rates = (card.input, card.output, card.cache_read, card.cache_write);
    let mut best: Option<u64> = None;
    for t in &card.tiers {
        if usage.input + usage.cache_read + usage.cache_write >= t.input_tokens_above
            && best.is_none_or(|b| t.input_tokens_above > b)
        {
            best = Some(t.input_tokens_above);
            rates = (t.input, t.output, t.cache_read, t.cache_write);
        }
    }
    let m = |tokens: u64, rate: f64| (tokens as f64 / 1_000_000.0) * rate;
    let c = Cost {
        input: m(usage.input, rates.0),
        output: m(usage.output, rates.1),
        cache_read: m(usage.cache_read, rates.2),
        cache_write: m(usage.cache_write, rates.3),
        total: 0.0,
    };
    Cost {
        total: c.input + c.output + c.cache_read + c.cache_write,
        ..c
    }
}

/// 目录装载错误（供调用方诊断；损坏条目跳过不致 panic）。
#[must_use]
pub fn catalog_load_error(raw: &str) -> Option<Arc<AiError>> {
    serde_json::from_str::<CatalogFile>(raw).err().map(|e| {
        Arc::new(AiError::Json {
            context: "builtin catalog".to_string(),
            detail: e.to_string(),
        })
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_models_declare_a_cache_ttl() {
        let catalog = Catalog::builtin();
        let anthropic = catalog.by_provider("anthropic");
        assert!(!anthropic.is_empty());
        for model in anthropic {
            assert_eq!(model.cache_ttl_seconds, Some(300), "{}", model.id);
        }
        for model in catalog.by_provider("openai") {
            assert_eq!(model.cache_ttl_seconds, None, "{}", model.id);
        }
    }
}
