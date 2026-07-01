use ncx_config::{load_config, Overrides};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSetting {
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct P1ExternalConfig {
    pub ark_api_key: Option<ResolvedSetting>,
    pub vl_api_key: Option<ResolvedSetting>,
    pub vl_base_url: Option<ResolvedSetting>,
    pub vl_model: Option<ResolvedSetting>,
    pub config_error: Option<String>,
}

impl P1ExternalConfig {
    pub fn load() -> Self {
        let loaded = load_config(Overrides::default());
        let (config, config_error) = match loaded {
            Ok(config) => (Some(config), None),
            Err(err) => (None, Some(err.to_string())),
        };

        let ark_config = config.as_ref().map(|cfg| cfg.ark_api_key.as_str());
        let vl_key_config = config.as_ref().map(|cfg| cfg.vl_api_key.as_str());
        let vl_base_config = config.as_ref().map(|cfg| cfg.vl_base_url.as_str());
        let vl_model_config = config.as_ref().map(|cfg| cfg.vl_model.as_str());

        Self {
            ark_api_key: resolve_setting(
                ark_config,
                "ncx-config ark_api_key",
                &["ARK_API_KEY", "NANOCODEX_ARK_API_KEY"],
                env_lookup,
            ),
            vl_api_key: resolve_setting(
                vl_key_config,
                "ncx-config vl_api_key",
                &["VL_API_KEY", "NANOCODEX_VL_API_KEY"],
                env_lookup,
            ),
            vl_base_url: resolve_setting(
                vl_base_config,
                "ncx-config vl_base_url",
                &["VL_BASE_URL", "NANOCODEX_VL_BASE_URL"],
                env_lookup,
            ),
            vl_model: resolve_setting(
                vl_model_config,
                "ncx-config vl_model",
                &["VL_MODEL", "NANOCODEX_VL_MODEL"],
                env_lookup,
            ),
            config_error,
        }
    }
}

fn resolve_setting<F>(
    config_value: Option<&str>,
    config_source: &str,
    env_keys: &[&str],
    env_lookup: F,
) -> Option<ResolvedSetting>
where
    F: Fn(&str) -> Option<String>,
{
    if let Some(value) = config_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(ResolvedSetting {
            value: value.to_string(),
            source: config_source.to_string(),
        });
    }

    env_keys.iter().find_map(|key| {
        env_lookup(key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| ResolvedSetting {
                value,
                source: format!("env {key}"),
            })
    })
}

fn env_lookup(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_value_wins_before_env_fallback() {
        let found = resolve_setting(
            Some(" from-config "),
            "ncx-config vl_model",
            &["VL_MODEL"],
            |_| Some("from-env".to_string()),
        )
        .expect("setting");

        assert_eq!(found.value, "from-config");
        assert_eq!(found.source, "ncx-config vl_model");
    }

    #[test]
    fn env_fallback_supports_runbook_key_aliases() {
        let found = resolve_setting(None, "ncx-config vl_api_key", &["VL_API_KEY"], |key| {
            (key == "VL_API_KEY").then(|| " direct-vl-key ".to_string())
        })
        .expect("setting");

        assert_eq!(found.value, "direct-vl-key");
        assert_eq!(found.source, "env VL_API_KEY");
    }

    #[test]
    fn blank_values_are_missing() {
        let found = resolve_setting(
            Some(" "),
            "ncx-config ark_api_key",
            &["ARK_API_KEY"],
            |_| Some(" ".to_string()),
        );

        assert!(found.is_none());
    }
}
