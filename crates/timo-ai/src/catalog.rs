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
}

impl ApiKind {
    /// 目录 JSON 中的规范名。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenaiCompletions => "openai-completions",
        }
    }

    /// 解析目录 JSON 中的协议名。
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "anthropic-messages" => Some(Self::AnthropicMessages),
            "openai-completions" => Some(Self::OpenaiCompletions),
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
}

impl Default for Compat {
    fn default() -> Self {
        Self {
            max_tokens_field: false,
            supports_usage_in_streaming: true,
            supports_finish_reason: true,
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
    /// openai-completions 行为开关。
    #[serde(default)]
    pub compat: Compat,
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

    /// 合并一段目录资产 JSON（同名模型覆盖：后到者胜，供自定义目录使用）。
    pub fn merge_file(&mut self, raw: &str) {
        let file: CatalogFile = match serde_json::from_str(raw) {
            Ok(f) => f,
            Err(_) => return, // 目录资产损坏不 panic：跳过并在运行时以“未知模型”报错
        };
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
