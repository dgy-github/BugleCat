//! Merge a provider `/models` snapshot onto maintained official presets.

use crate::preset_catalog::{CatalogModel, CatalogProvider, PriceSource};

const UPDATED_AT: &str = "2026-09-16";
const LIVE_NOTE: &str = "由厂商 /models 实时发现；公开价尚未核验，费用以官网为准。";

pub fn live_official_model(
    provider: &CatalogProvider,
    discovered: &crate::DiscoveredProviderModel,
) -> CatalogModel {
    if let Some(existing) = provider
        .models
        .iter()
        .find(|model| model.model_id == discovered.id)
    {
        let mut model = existing.clone();
        if let Some(length) = discovered.context_length {
            model.context_length = Some(length);
        }
        return model;
    }
    let fallback = provider.models.first();
    CatalogModel {
        provider_id: provider.id.clone(),
        model_id: discovered.id.clone(),
        display_name: if discovered.name.trim().is_empty() || discovered.name == discovered.id {
            discovered.id.clone()
        } else {
            discovered.name.clone()
        },
        base_url: fallback
            .map(|model| model.base_url.clone())
            .unwrap_or_default(),
        price_in: discovered.input_price_per_million.unwrap_or(0.0),
        price_out: discovered.output_price_per_million.unwrap_or(0.0),
        price_currency: fallback
            .map(|model| model.price_currency.clone())
            .unwrap_or_else(|| "CNY".into()),
        price_source: PriceSource::OfficialDirect,
        pricing_note: Some(LIVE_NOTE.into()),
        source_url: fallback
            .map(|model| model.source_url.clone())
            .unwrap_or_default(),
        updated_at: UPDATED_AT.into(),
        context_length: discovered.context_length,
        direct_available: true,
    }
}

/// The provider's models as its `/models` snapshot lists them: maintained
/// presets that are still listed come first (keeping their audited prices),
/// then newly listed ids. Presets the provider no longer lists are retired
/// and dropped. An empty snapshot says nothing, so the presets stand.
pub fn merge_official_live_models(
    provider: &CatalogProvider,
    discovered: &[crate::DiscoveredProviderModel],
) -> Vec<CatalogModel> {
    if discovered.is_empty() {
        return provider.models.clone();
    }
    let listed = |id: &str| discovered.iter().any(|item| item.id == id);
    let mut models: Vec<CatalogModel> = provider
        .models
        .iter()
        .filter(|model| listed(&model.model_id))
        .map(|model| {
            let item = discovered
                .iter()
                .find(|item| item.id == model.model_id)
                .expect("filtered to listed presets");
            live_official_model(provider, item)
        })
        .collect();
    for item in discovered {
        if !models.iter().any(|model| model.model_id == item.id) {
            models.push(live_official_model(provider, item));
        }
    }
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset_catalog::catalog;

    #[test]
    fn live_official_catalog_follows_the_snapshot() {
        let deepseek = catalog()
            .into_iter()
            .find(|provider| provider.id == "deepseek")
            .expect("missing DeepSeek provider");
        let discovered = vec![
            crate::DiscoveredProviderModel {
                id: "deepseek-flash".into(),
                name: "DeepSeek Flash".into(),
                context_length: Some(1_000_000),
                input_price_per_million: None,
                output_price_per_million: None,
            },
            crate::DiscoveredProviderModel {
                id: "deepseek-new-exp".into(),
                name: "DeepSeek New Exp".into(),
                context_length: Some(256_000),
                input_price_per_million: None,
                output_price_per_million: None,
            },
        ];
        let merged = merge_official_live_models(&deepseek, &discovered);
        assert_eq!(
            merged
                .iter()
                .map(|model| model.model_id.as_str())
                .collect::<Vec<_>>(),
            // deepseek-v4-pro is not listed any more: retired, so dropped.
            ["deepseek-flash", "deepseek-new-exp"]
        );
        assert_eq!(
            merged[0].price_in, 2.0,
            "listed presets keep audited prices"
        );
        assert_eq!(merged[0].display_name, "DeepSeek V4.1 Flash");
        assert_eq!(merged[1].display_name, "DeepSeek New Exp");
        assert_eq!(merged[1].pricing_note.as_deref(), Some(LIVE_NOTE));
    }

    #[test]
    fn empty_snapshot_keeps_the_presets() {
        let deepseek = catalog()
            .into_iter()
            .find(|provider| provider.id == "deepseek")
            .expect("missing DeepSeek provider");
        let merged = merge_official_live_models(&deepseek, &[]);
        assert_eq!(merged.len(), deepseek.models.len());
    }
}
