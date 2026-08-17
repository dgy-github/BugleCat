//! Curated OpenAI-compatible model presets and OpenRouter's public catalog parser.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceSource {
    /// 厂商官网公布的直连接口价。
    OfficialDirect,
    /// 聚合平台按其自身渠道计费的价格。
    Aggregator,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CatalogModel {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub base_url: String,
    pub price_in: f64,
    pub price_out: f64,
    pub price_currency: String,
    pub price_source: PriceSource,
    pub source_url: String,
    pub updated_at: String,
    pub context_length: Option<u64>,
    pub direct_available: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub models: Vec<CatalogModel>,
}

const UPDATED_AT: &str = "2026-08-17";
const DEEPSEEK_PRICING: &str = "https://api-docs.deepseek.com/zh-cn/quick_start/pricing";
const BAILIAN_PRICING: &str = "https://help.aliyun.com/zh/model-studio/model-pricing";
const ARK_PRICING: &str = "https://www.volcengine.com/product/ark";
const ZHIPU_PRICING: &str = "https://docs.bigmodel.cn/cn/guide/start/model-overview";
const MOONSHOT_PRICING: &str = "https://platform.moonshot.cn/docs/pricing/chat";
const MINIMAX_PRICING: &str = "https://platform.minimaxi.com/docs/guides/pricing-paygo";
const OPENAI_PRICING: &str = "https://openai.com/api/pricing/";
const GEMINI_PRICING: &str = "https://ai.google.dev/gemini-api/docs/pricing";
const OPENROUTER_PRICING: &str = "https://openrouter.ai/models";

fn model(
    provider_id: &str,
    model_id: &str,
    display_name: &str,
    base_url: &str,
    price_in: f64,
    price_out: f64,
    price_currency: &str,
    source_url: &str,
    context_length: Option<u64>,
) -> CatalogModel {
    CatalogModel {
        provider_id: provider_id.into(),
        model_id: model_id.into(),
        display_name: display_name.into(),
        base_url: base_url.into(),
        price_in,
        price_out,
        price_currency: price_currency.into(),
        price_source: PriceSource::OfficialDirect,
        source_url: source_url.into(),
        updated_at: UPDATED_AT.into(),
        context_length,
        direct_available: true,
    }
}

fn aggregator_model(
    model_id: &str,
    display_name: &str,
    price_in: f64,
    price_out: f64,
    context_length: Option<u64>,
) -> CatalogModel {
    let mut preset = model(
        "openrouter",
        model_id,
        display_name,
        "https://openrouter.ai/api/v1",
        price_in,
        price_out,
        "USD",
        OPENROUTER_PRICING,
        context_length,
    );
    preset.price_source = PriceSource::Aggregator;
    preset
}

/// A small, maintained set of coding-capable models. Prices are base input and
/// output rates only; provider billing may apply cache or long-context tiers.
pub fn catalog() -> Vec<CatalogProvider> {
    vec![
        CatalogProvider {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            models: vec![
                model(
                    "deepseek",
                    "deepseek-v4-flash",
                    "DeepSeek V4 Flash",
                    "https://api.deepseek.com",
                    1.0,
                    2.0,
                    "CNY",
                    DEEPSEEK_PRICING,
                    Some(1_000_000),
                ),
                model(
                    "deepseek",
                    "deepseek-v4-pro",
                    "DeepSeek V4 Pro",
                    "https://api.deepseek.com",
                    3.0,
                    6.0,
                    "CNY",
                    DEEPSEEK_PRICING,
                    Some(1_000_000),
                ),
            ],
        },
        CatalogProvider {
            id: "bailian".into(),
            name: "阿里百炼".into(),
            models: vec![
                model(
                    "bailian",
                    "qwen3-coder-next",
                    "Qwen3 Coder Next",
                    "https://dashscope.aliyuncs.com/compatible-mode/v1",
                    1.0,
                    4.0,
                    "CNY",
                    BAILIAN_PRICING,
                    Some(256_000),
                ),
                model(
                    "bailian",
                    "qwen3.8-2.4t-a95b",
                    "Qwen3.8",
                    "https://dashscope.aliyuncs.com/compatible-mode/v1",
                    12.0,
                    36.0,
                    "CNY",
                    BAILIAN_PRICING,
                    Some(1_000_000),
                ),
            ],
        },
        CatalogProvider {
            id: "ark".into(),
            name: "火山方舟".into(),
            models: vec![
                model(
                    "ark",
                    "doubao-seed-2-1-pro-260215",
                    "豆包 Seed 2.1 Pro",
                    "https://ark.cn-beijing.volces.com/api/v3",
                    6.0,
                    30.0,
                    "CNY",
                    ARK_PRICING,
                    Some(256_000),
                ),
                model(
                    "ark",
                    "doubao-seed-evolving-250715",
                    "豆包 Seed Evolving",
                    "https://ark.cn-beijing.volces.com/api/v3",
                    6.0,
                    30.0,
                    "CNY",
                    ARK_PRICING,
                    Some(256_000),
                ),
            ],
        },
        CatalogProvider {
            id: "zhipu".into(),
            name: "智谱 AI".into(),
            models: vec![
                model(
                    "zhipu",
                    "glm-5.2",
                    "GLM-5.2",
                    "https://open.bigmodel.cn/api/paas/v4",
                    10.0,
                    33.0,
                    "CNY",
                    ZHIPU_PRICING,
                    Some(1_000_000),
                ),
                model(
                    "zhipu",
                    "glm-4.7-flash",
                    "GLM-4.7 Flash（免费）",
                    "https://open.bigmodel.cn/api/paas/v4",
                    0.0,
                    0.0,
                    "CNY",
                    ZHIPU_PRICING,
                    None,
                ),
            ],
        },
        CatalogProvider {
            id: "moonshot".into(),
            name: "月之暗面 Kimi".into(),
            models: vec![
                model(
                    "moonshot",
                    "kimi-k2.5",
                    "Kimi K2.5",
                    "https://api.moonshot.cn/v1",
                    4.0,
                    16.0,
                    "CNY",
                    MOONSHOT_PRICING,
                    Some(256_000),
                ),
                model(
                    "moonshot",
                    "kimi-k2.5-turbo-preview",
                    "Kimi K2.5 Turbo",
                    "https://api.moonshot.cn/v1",
                    8.0,
                    24.0,
                    "CNY",
                    MOONSHOT_PRICING,
                    Some(256_000),
                ),
            ],
        },
        CatalogProvider {
            id: "minimax".into(),
            name: "MiniMax".into(),
            models: vec![
                model(
                    "minimax",
                    "MiniMax-M2.7",
                    "MiniMax M2.7",
                    "https://api.minimaxi.com/v1",
                    2.1,
                    8.4,
                    "CNY",
                    MINIMAX_PRICING,
                    Some(204_800),
                ),
                model(
                    "minimax",
                    "MiniMax-M2.7-highspeed",
                    "MiniMax M2.7 极速版",
                    "https://api.minimaxi.com/v1",
                    4.2,
                    16.8,
                    "CNY",
                    MINIMAX_PRICING,
                    Some(204_800),
                ),
            ],
        },
        CatalogProvider {
            id: "openai".into(),
            name: "OpenAI".into(),
            models: vec![
                model(
                    "openai",
                    "gpt-5",
                    "GPT-5",
                    "https://api.openai.com/v1",
                    1.25,
                    10.0,
                    "USD",
                    OPENAI_PRICING,
                    None,
                ),
                model(
                    "openai",
                    "gpt-5-mini",
                    "GPT-5 mini",
                    "https://api.openai.com/v1",
                    0.25,
                    2.0,
                    "USD",
                    OPENAI_PRICING,
                    None,
                ),
            ],
        },
        CatalogProvider {
            id: "gemini".into(),
            name: "Google Gemini".into(),
            models: vec![
                model(
                    "gemini",
                    "gemini-2.5-pro",
                    "Gemini 2.5 Pro",
                    "https://generativelanguage.googleapis.com/v1beta/openai",
                    1.25,
                    10.0,
                    "USD",
                    GEMINI_PRICING,
                    Some(1_000_000),
                ),
                model(
                    "gemini",
                    "gemini-2.5-flash",
                    "Gemini 2.5 Flash",
                    "https://generativelanguage.googleapis.com/v1beta/openai",
                    0.30,
                    2.50,
                    "USD",
                    GEMINI_PRICING,
                    Some(1_000_000),
                ),
            ],
        },
        CatalogProvider {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            models: vec![aggregator_model(
                "openrouter/auto",
                "OpenRouter 自动路由",
                0.0,
                0.0,
                None,
            )],
        },
    ]
}

pub fn find_preset(provider_id: &str, model_id: &str) -> Option<CatalogModel> {
    catalog()
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .and_then(|provider| {
            provider
                .models
                .into_iter()
                .find(|model| model.model_id == model_id)
        })
}

/// Convert OpenRouter's public per-token prices into the per-million-token
/// display unit used by nanocodex. An absent price is kept as zero (unknown).
pub fn parse_openrouter_models(json: &str) -> Result<Vec<CatalogModel>, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| format!("OpenRouter 目录格式错误：{e}"))?;
    let rows = root
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "OpenRouter 目录缺少 data 数组".to_string())?;
    let mut models = Vec::new();
    for row in rows {
        let Some(model_id) = row
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            return Err("OpenRouter 目录存在缺少模型 ID 的条目".into());
        };
        let price = |field: &str| {
            row.get("pricing")
                .and_then(|value| value.get(field))
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| value * 1_000_000.0)
                .unwrap_or(0.0)
        };
        models.push(CatalogModel {
            provider_id: "openrouter".into(),
            model_id: model_id.into(),
            display_name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(model_id)
                .into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            price_in: price("prompt"),
            price_out: price("completion"),
            price_currency: "USD".into(),
            price_source: PriceSource::Aggregator,
            source_url: OPENROUTER_PRICING.into(),
            updated_at: UPDATED_AT.into(),
            context_length: row.get("context_length").and_then(Value::as_u64),
            direct_available: true,
        });
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_presets_have_an_endpoint_price_and_source() {
        let providers = catalog();
        for id in [
            "deepseek",
            "bailian",
            "ark",
            "zhipu",
            "moonshot",
            "minimax",
            "openai",
            "gemini",
            "openrouter",
        ] {
            let provider = providers
                .iter()
                .find(|provider| provider.id == id)
                .expect("missing provider");
            assert!(!provider.models.is_empty());
            for model in &provider.models {
                assert!(!model.model_id.is_empty());
                assert!(model.base_url.starts_with("https://"));
                assert!(matches!(model.price_currency.as_str(), "CNY" | "USD"));
                assert!(model.source_url.starts_with("https://"));
            }
        }
    }

    #[test]
    fn official_catalog_and_openrouter_catalog_have_distinct_price_sources() {
        for provider in catalog()
            .into_iter()
            .filter(|provider| provider.id != "openrouter")
        {
            assert!(provider
                .models
                .iter()
                .all(|model| model.price_source == PriceSource::OfficialDirect));
        }

        let models = parse_openrouter_models(
            r#"{"data":[{"id":"vendor/example","pricing":{"prompt":"0.000001","completion":"0.000002"}}]}"#,
        )
        .unwrap();
        assert_eq!(models[0].price_source, PriceSource::Aggregator);
    }

    #[test]
    fn deepseek_official_catalog_uses_v4_models_and_not_retired_aliases() {
        let deepseek = catalog()
            .into_iter()
            .find(|provider| provider.id == "deepseek")
            .expect("missing DeepSeek provider");
        let ids = deepseek
            .models
            .iter()
            .map(|model| model.model_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["deepseek-v4-flash", "deepseek-v4-pro"]);
        assert!(deepseek
            .models
            .iter()
            .all(|model| model.price_currency == "CNY"));
        assert_eq!(deepseek.models[0].price_in, 1.0);
        assert_eq!(deepseek.models[0].price_out, 2.0);
        assert_eq!(deepseek.models[1].price_in, 3.0);
        assert_eq!(deepseek.models[1].price_out, 6.0);
    }

    #[test]
    fn openrouter_prices_are_converted_from_per_token_to_per_million_usd() {
        let models = parse_openrouter_models(
            r#"{
              "data": [{"id":"openai/gpt-test","name":"GPT Test","context_length":128000,
              "pricing":{"prompt":"0.00000125","completion":"0.00001"}}]
            }"#,
        )
        .unwrap();
        assert_eq!(models[0].model_id, "openai/gpt-test");
        assert_eq!(models[0].price_in, 1.25);
        assert_eq!(models[0].price_out, 10.0);
        assert_eq!(models[0].price_currency, "USD");
    }

    #[test]
    fn openrouter_parser_rejects_missing_model_id_without_panicking() {
        assert!(parse_openrouter_models(r#"{"data":[{"name":"bad","pricing":{}}]}"#).is_err());
    }
}
