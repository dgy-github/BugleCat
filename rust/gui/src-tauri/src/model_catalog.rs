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
    pub pricing_note: Option<String>,
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

const UPDATED_AT: &str = "2026-08-27";
const DEEPSEEK_PRICING: &str = "https://api-docs.deepseek.com/zh-cn/quick_start/pricing";
const BAILIAN_PRICING: &str = "https://help.aliyun.com/zh/model-studio/qwen3-7-max";
const ARK_PRICING: &str = "https://www.volcengine.com/product/ark";
const ZHIPU_PRICING: &str = "https://bigmodel.cn/pricing";
const MOONSHOT_PRICING: &str = "https://platform.kimi.ai/";
const MINIMAX_PRICING: &str = "https://platform.minimaxi.com/docs/guides/pricing-paygo";
const OPENAI_PRICING: &str = "https://developers.openai.com/api/docs/models/compare";
const GEMINI_PRICING: &str = "https://ai.google.dev/gemini-api/docs/latest-model";
const OPENROUTER_PRICING: &str = "https://openrouter.ai/models";
const YUNMO_HOME: &str = "https://api.yunmo-ai.com/";

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
        pricing_note: None,
        source_url: source_url.into(),
        updated_at: UPDATED_AT.into(),
        context_length,
        direct_available: true,
    }
}

fn with_pricing_note(mut preset: CatalogModel, note: &str) -> CatalogModel {
    preset.pricing_note = Some(note.into());
    preset
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
                with_pricing_note(model(
                    "deepseek",
                    "deepseek-v4-flash",
                    "DeepSeek V4 Flash",
                    "https://api.deepseek.com",
                    3.0,
                    9.0,
                    "CNY",
                    DEEPSEEK_PRICING,
                    Some(1_000_000),
                ), "显示高峰期缓存未命中价；工作日 9:00–12:00、14:00–18:00（北京时间）以外为空闲时段半价，缓存命中另计"),
                with_pricing_note(model(
                    "deepseek",
                    "deepseek-v4-pro",
                    "DeepSeek V4 Pro",
                    "https://api.deepseek.com",
                    9.0,
                    27.0,
                    "CNY",
                    DEEPSEEK_PRICING,
                    Some(1_000_000),
                ), "显示高峰期缓存未命中价；工作日 9:00–12:00、14:00–18:00（北京时间）以外为空闲时段半价，缓存命中另计"),
                with_pricing_note(model(
                    "deepseek",
                    "deepseek-v4-flash-vision-exp",
                    "DeepSeek V4 Flash Vision Exp",
                    "https://api.deepseek.com",
                    3.0,
                    9.0,
                    "CNY",
                    DEEPSEEK_PRICING,
                    Some(1_000_000),
                ), "实验性视觉模型；显示高峰期缓存未命中价，空闲时段半价，缓存命中另计"),
            ],
        },
        CatalogProvider {
            id: "bailian".into(),
            name: "阿里百炼".into(),
            models: vec![model(
                "bailian",
                "qwen3.7-max",
                "Qwen3.7 Max",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                12.0,
                36.0,
                "CNY",
                BAILIAN_PRICING,
                Some(1_000_000),
            )],
        },
        CatalogProvider {
            id: "ark".into(),
            name: "火山方舟".into(),
            models: vec![
                model(
                    "ark",
                    "doubao-seed-evolving",
                    "豆包 Seed Evolving",
                    "https://ark.cn-beijing.volces.com/api/v3",
                    6.0,
                    30.0,
                    "CNY",
                    ARK_PRICING,
                    Some(1_000_000),
                ),
                model(
                    "ark",
                    "doubao-seed-2.0-code",
                    "豆包 Seed 2.0 Code",
                    "https://ark.cn-beijing.volces.com/api/v3",
                    3.2,
                    16.0,
                    "CNY",
                    ARK_PRICING,
                    None,
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
                    8.0,
                    28.0,
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
                    "kimi-k3",
                    "Kimi K3",
                    "https://api.moonshot.ai/v1",
                    3.0,
                    15.0,
                    "USD",
                    MOONSHOT_PRICING,
                    Some(1_000_000),
                ),
                model(
                    "moonshot",
                    "kimi-k2.7-code",
                    "Kimi K2.7 Code",
                    "https://api.moonshot.ai/v1",
                    0.95,
                    4.0,
                    "USD",
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
                    "MiniMax-M3",
                    "MiniMax M3",
                    "https://api.minimaxi.com/v1",
                    2.1,
                    8.4,
                    "CNY",
                    MINIMAX_PRICING,
                    None,
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
                    "gpt-5.6-sol",
                    "GPT-5.6 Sol",
                    "https://api.openai.com/v1",
                    5.0,
                    30.0,
                    "USD",
                    OPENAI_PRICING,
                    Some(1_050_000),
                ),
                model(
                    "openai",
                    "gpt-5.6-terra",
                    "GPT-5.6 Terra",
                    "https://api.openai.com/v1",
                    2.0,
                    12.0,
                    "USD",
                    OPENAI_PRICING,
                    Some(1_050_000),
                ),
                model(
                    "openai",
                    "gpt-5.6-luna",
                    "GPT-5.6 Luna",
                    "https://api.openai.com/v1",
                    0.2,
                    1.2,
                    "USD",
                    OPENAI_PRICING,
                    Some(1_050_000),
                ),
            ],
        },
        CatalogProvider {
            id: "yunmo".into(),
            name: "云末 AI（中转）".into(),
            models: vec![{
                let mut preset = model(
                    "yunmo",
                    "gpt-5.6-sol",
                    "GPT-5.6 Sol（云末中转）",
                    "https://api.yunmo-ai.com/v1",
                    0.0,
                    0.0,
                    "USD",
                    YUNMO_HOME,
                    None,
                );
                preset.price_source = PriceSource::Aggregator;
                preset.pricing_note =
                    Some("中转站实际计费以云末控制台为准；模型 ID 已通过 /v1/models 实测。".into());
                preset
            }],
        },
        CatalogProvider {
            id: "gemini".into(),
            name: "Google Gemini".into(),
            models: vec![with_pricing_note(
                model(
                    "gemini",
                    "gemini-3.7-flash",
                    "Gemini 3.7 Flash",
                    "https://generativelanguage.googleapis.com/v1beta/openai",
                    0.75,
                    3.75,
                    "USD",
                    GEMINI_PRICING,
                    Some(1_000_000),
                ),
                "当前限时价格，至 2026-12-31；之后按官网标准价调整。",
            )],
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

pub fn yunmo_model(model_id: &str) -> CatalogModel {
    let mut preset = model(
        "yunmo",
        model_id,
        model_id,
        "https://api.yunmo-ai.com/v1",
        0.0,
        0.0,
        "USD",
        YUNMO_HOME,
        None,
    );
    preset.price_source = PriceSource::Aggregator;
    preset.pricing_note =
        Some("由云末 /v1/models 实时返回；实际模型能力与费用以中转站为准。".into());
    preset
}

pub fn openrouter_model(model: ncx_core::DiscoveredProviderModel) -> CatalogModel {
    CatalogModel {
        provider_id: "openrouter".into(),
        model_id: model.id.clone(),
        display_name: model.name,
        base_url: "https://openrouter.ai/api/v1".into(),
        price_in: model.input_price_per_million.unwrap_or(0.0),
        price_out: model.output_price_per_million.unwrap_or(0.0),
        price_currency: "USD".into(),
        price_source: PriceSource::Aggregator,
        pricing_note: None,
        source_url: OPENROUTER_PRICING.into(),
        updated_at: UPDATED_AT.into(),
        context_length: model.context_length,
        direct_available: true,
    }
}

/// Convert OpenRouter's public per-token prices into the per-million-token
/// display unit used by nanocodex. An absent price is kept as zero (unknown).
pub fn parse_openrouter_models(json: &str) -> Result<Vec<CatalogModel>, String> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| format!("OpenRouter 目录格式错误：{e}"))?;
    let models = ncx_core::parse_catalog_models(&root)
        .into_iter()
        .map(openrouter_model)
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err("OpenRouter 目录没有返回可用模型".to_string());
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
            "yunmo",
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
            .filter(|provider| !matches!(provider.id.as_str(), "openrouter" | "yunmo"))
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

        assert_eq!(
            ids,
            vec![
                "deepseek-v4-flash",
                "deepseek-v4-pro",
                "deepseek-v4-flash-vision-exp",
            ]
        );
        assert!(deepseek
            .models
            .iter()
            .all(|model| model.price_currency == "CNY"));
        assert_eq!(deepseek.models[0].price_in, 3.0);
        assert_eq!(deepseek.models[0].price_out, 9.0);
        assert_eq!(deepseek.models[1].price_in, 9.0);
        assert_eq!(deepseek.models[1].price_out, 27.0);
        assert_eq!(deepseek.models[2].price_in, 3.0);
        assert_eq!(deepseek.models[2].price_out, 9.0);
        assert!(deepseek
            .models
            .iter()
            .all(|model| model.pricing_note.is_some()));
    }

    #[test]
    fn official_catalog_uses_the_audited_current_models() {
        let expected = [
            ("bailian", vec!["qwen3.7-max"]),
            ("ark", vec!["doubao-seed-evolving", "doubao-seed-2.0-code"]),
            ("zhipu", vec!["glm-5.2", "glm-4.7-flash"]),
            ("moonshot", vec!["kimi-k3", "kimi-k2.7-code"]),
            ("minimax", vec!["MiniMax-M3", "MiniMax-M2.7-highspeed"]),
            (
                "openai",
                vec!["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"],
            ),
            ("gemini", vec!["gemini-3.7-flash"]),
            ("yunmo", vec!["gpt-5.6-sol"]),
        ];

        let providers = catalog();
        for (provider_id, expected_ids) in expected {
            let provider = providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .expect("missing provider");
            let ids = provider
                .models
                .iter()
                .map(|model| model.model_id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(ids, expected_ids, "unexpected models for {provider_id}");
        }

        let expected_prices = [
            ("bailian", "qwen3.7-max", 12.0, 36.0, "CNY"),
            ("ark", "doubao-seed-evolving", 6.0, 30.0, "CNY"),
            ("ark", "doubao-seed-2.0-code", 3.2, 16.0, "CNY"),
            ("zhipu", "glm-5.2", 8.0, 28.0, "CNY"),
            ("zhipu", "glm-4.7-flash", 0.0, 0.0, "CNY"),
            ("moonshot", "kimi-k3", 3.0, 15.0, "USD"),
            ("moonshot", "kimi-k2.7-code", 0.95, 4.0, "USD"),
            ("minimax", "MiniMax-M3", 2.1, 8.4, "CNY"),
            ("minimax", "MiniMax-M2.7-highspeed", 4.2, 16.8, "CNY"),
            ("openai", "gpt-5.6-sol", 5.0, 30.0, "USD"),
            ("openai", "gpt-5.6-terra", 2.0, 12.0, "USD"),
            ("openai", "gpt-5.6-luna", 0.2, 1.2, "USD"),
            ("gemini", "gemini-3.7-flash", 0.75, 3.75, "USD"),
        ];
        for (provider_id, model_id, price_in, price_out, currency) in expected_prices {
            let model = providers
                .iter()
                .find(|provider| provider.id == provider_id)
                .and_then(|provider| {
                    provider
                        .models
                        .iter()
                        .find(|model| model.model_id == model_id)
                })
                .expect("missing audited model");
            assert_eq!(
                model.price_in, price_in,
                "unexpected input price for {model_id}"
            );
            assert_eq!(
                model.price_out, price_out,
                "unexpected output price for {model_id}"
            );
            assert_eq!(
                model.price_currency, currency,
                "unexpected currency for {model_id}"
            );
            assert_eq!(model.price_source, PriceSource::OfficialDirect);
            assert_eq!(model.updated_at, UPDATED_AT);
        }
    }

    #[test]
    fn limited_time_official_price_declares_its_expiry() {
        let gemini = find_preset("gemini", "gemini-3.7-flash").expect("missing Gemini preset");

        assert_eq!(
            gemini.pricing_note.as_deref(),
            Some("当前限时价格，至 2026-12-31；之后按官网标准价调整。")
        );
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
