//! Durable provider-route directory shared by every host.
//!
//! A route owns protocol, endpoint, credential, model catalog and selection as
//! one record. Hosts may discover models or rebuild runtimes, but must not
//! reimplement persistence or combine individual fields from different routes.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{write_nanocodex_config, ConfigPaths};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderRoute {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub selected_model: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRouteInput {
    pub id: Option<String>,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ProviderRouteView {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub api_key_masked: String,
    pub has_api_key: bool,
    pub models: Vec<String>,
    pub active: bool,
    pub selected_model: String,
}

#[derive(Clone, Debug)]
pub struct ProviderDirectory {
    path: PathBuf,
    config_path: PathBuf,
}

impl ProviderDirectory {
    pub fn from_paths(paths: &ConfigPaths) -> Self {
        Self {
            path: paths.nanocodex.with_file_name("providers.json"),
            config_path: paths.nanocodex.clone(),
        }
    }

    pub fn at(path: impl Into<PathBuf>, config_path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            config_path: config_path.into(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Vec<ProviderRoute>, String> {
        if !self.path.is_file() {
            return Ok(Vec::new());
        }
        serde_json::from_slice(&fs::read(&self.path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("模型商配置损坏：{error}"))
    }

    pub fn get(&self, id: &str) -> Result<ProviderRoute, String> {
        self.load()?
            .into_iter()
            .find(|route| route.id == id)
            .ok_or_else(|| "模型商不存在".into())
    }

    pub fn views(
        &self,
        active_provider_id: &str,
        current_model: &str,
    ) -> Result<Vec<ProviderRouteView>, String> {
        Ok(self
            .load()?
            .into_iter()
            .map(|mut route| {
                route.active = route.id == active_provider_id;
                if route.active && route.selected_model.is_empty() {
                    route.selected_model = current_model.to_string();
                }
                route.into_view()
            })
            .collect())
    }

    pub fn upsert(&self, input: ProviderRouteInput) -> Result<ProviderRouteView, String> {
        validate_input(&input)?;
        let mut routes = self.load()?;
        let id = input
            .id
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "provider-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                )
            });
        let existing = routes.iter().position(|route| route.id == id);
        let preserved_key = existing
            .and_then(|index| routes.get(index))
            .map(|route| route.api_key.clone())
            .unwrap_or_default();
        let route = ProviderRoute {
            id: id.clone(),
            name: input.name.trim().to_string(),
            protocol: input.protocol,
            base_url: input.base_url.trim().trim_end_matches('/').to_string(),
            api_key: input
                .api_key
                .filter(|key| !key.trim().is_empty())
                .unwrap_or(preserved_key),
            models: dedupe_models(input.models),
            active: existing
                .and_then(|index| routes.get(index))
                .is_some_and(|route| route.active),
            selected_model: existing
                .and_then(|index| routes.get(index))
                .map(|route| route.selected_model.clone())
                .unwrap_or_default(),
        };
        if let Some(index) = existing {
            routes[index] = route.clone();
        } else {
            routes.push(route.clone());
        }
        self.save(&routes)?;
        Ok(route.into_view())
    }

    pub fn delete(&self, id: &str, active_provider_id: &str) -> Result<(), String> {
        if id == active_provider_id {
            return Err("当前正在使用该模型商，请先切换后再删除".into());
        }
        let mut routes = self.load()?;
        routes.retain(|route| route.id != id);
        self.save(&routes)
    }

    /// Persist selection and the flat compatibility snapshot as one logical
    /// operation. If the config write fails, the route file is restored.
    pub fn activate(&self, id: &str, model: &str) -> Result<ProviderRoute, String> {
        self.activate_with_updates(id, model, &HashMap::new())
    }

    /// Create or update a complete route and activate it in one transaction.
    /// This is used by curated presets so a provider switch cannot combine a
    /// preset endpoint/model with the credential from the previously active
    /// route. Both files are restored if the compatibility snapshot fails.
    pub fn upsert_and_activate(
        &self,
        input: ProviderRouteInput,
        model: &str,
        extra_updates: &HashMap<String, String>,
    ) -> Result<ProviderRoute, String> {
        validate_input(&input)?;
        let id = input
            .id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .ok_or("原子激活要求稳定的模型商 ID")?
            .to_string();
        let previous_routes = self.load()?;
        let previous_key = previous_routes
            .iter()
            .find(|route| route.id == id)
            .map(|route| route.api_key.clone())
            .unwrap_or_default();
        let route = ProviderRoute {
            id: id.clone(),
            name: input.name.trim().to_string(),
            protocol: input.protocol,
            base_url: input.base_url.trim().trim_end_matches('/').to_string(),
            api_key: input
                .api_key
                .filter(|key| !key.trim().is_empty())
                .unwrap_or(previous_key),
            models: dedupe_models(input.models),
            active: false,
            selected_model: String::new(),
        };
        let mut candidate_routes = previous_routes.clone();
        if let Some(index) = candidate_routes.iter().position(|item| item.id == id) {
            candidate_routes[index] = route;
        } else {
            candidate_routes.push(route);
        }
        self.commit_activation(
            candidate_routes,
            &id,
            model,
            extra_updates,
            &previous_routes,
        )
    }

    fn activate_with_updates(
        &self,
        id: &str,
        model: &str,
        extra_updates: &HashMap<String, String>,
    ) -> Result<ProviderRoute, String> {
        let routes = self.load()?;
        let previous = routes.clone();
        self.commit_activation(routes, id, model, extra_updates, &previous)
    }

    fn commit_activation(
        &self,
        mut routes: Vec<ProviderRoute>,
        id: &str,
        model: &str,
        extra_updates: &HashMap<String, String>,
        previous: &[ProviderRoute],
    ) -> Result<ProviderRoute, String> {
        if !valid_model_id(model) {
            return Err("模型 ID 无效".into());
        }
        let index = routes
            .iter()
            .position(|route| route.id == id)
            .ok_or("模型商不存在")?;
        if routes[index].api_key.trim().is_empty() {
            return Err("请先配置该模型商 Token".into());
        }
        if !routes[index]
            .models
            .iter()
            .any(|candidate| candidate == model)
        {
            return Err(format!(
                "模型 {model} 不属于当前模型商 {}",
                routes[index].name
            ));
        }
        for route in &mut routes {
            route.active = route.id == id;
            if route.active {
                route.selected_model = model.to_string();
            }
        }
        let selected = routes[index].clone();
        self.save(&routes)?;
        let available_models = selected.models.join(",");
        let mut updates = HashMap::from([
            ("api_key".to_string(), selected.api_key.clone()),
            ("base_url".to_string(), selected.base_url.clone()),
            ("model".to_string(), selected.selected_model.clone()),
            ("provider_protocol".to_string(), selected.protocol.clone()),
            ("active_provider_id".to_string(), selected.id.clone()),
            ("available_models".to_string(), available_models),
            // Custom relays have no trustworthy catalog price. Carrying the
            // previous Provider's rate across a Route switch would fabricate
            // cost telemetry, so reset estimates until the user configures a
            // route-specific price explicitly.
            ("price_in".to_string(), "0".to_string()),
            ("price_out".to_string(), "0".to_string()),
        ]);
        for (key, value) in extra_updates {
            if matches!(key.as_str(), "price_in" | "price_out" | "price_currency") {
                updates.insert(key.clone(), value.clone());
            }
        }
        let borrowed = updates
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<HashMap<_, _>>();
        if let Err(error) = write_nanocodex_config(&borrowed, &self.config_path) {
            let _ = self.save(previous);
            return Err(error.to_string());
        }
        Ok(selected)
    }

    pub fn select_model(&self, id: &str, model: &str) -> Result<(), String> {
        let mut routes = self.load()?;
        let route = routes
            .iter_mut()
            .find(|route| route.id == id)
            .ok_or("模型商不存在")?;
        if !route.models.iter().any(|candidate| candidate == model) {
            return Err(format!("模型 {model} 不属于当前模型商 {}", route.name));
        }
        route.selected_model = model.to_string();
        self.save(&routes)
    }

    /// Refresh a maintained route's model catalog without replacing its
    /// credential or current selection. When the route is active, keep the
    /// flat compatibility snapshot in sync as part of the same transaction.
    pub fn reconcile_models(&self, id: &str, models: Vec<String>) -> Result<ProviderRoute, String> {
        let models = dedupe_models(models);
        if models.is_empty() {
            return Err("模型目录不能为空".into());
        }
        let mut routes = self.load()?;
        let previous = routes.clone();
        let route = routes
            .iter_mut()
            .find(|route| route.id == id)
            .ok_or("模型商不存在")?;
        if !models.iter().any(|model| model == &route.selected_model) {
            return Err(format!(
                "当前模型 {} 不在更新后的模型目录中",
                route.selected_model
            ));
        }
        if route.models == models {
            return Ok(route.clone());
        }
        route.models = models;
        let updated = route.clone();
        self.save(&routes)?;
        if updated.active {
            let available_models = updated.models.join(",");
            let updates = HashMap::from([("available_models", available_models.as_str())]);
            if let Err(error) = write_nanocodex_config(&updates, &self.config_path) {
                let _ = self.save(&previous);
                return Err(error.to_string());
            }
        }
        Ok(updated)
    }

    pub fn clear_active_flags(&self) -> Result<(), String> {
        let mut routes = self.load()?;
        if routes.iter().all(|route| !route.active) {
            return Ok(());
        }
        for route in &mut routes {
            route.active = false;
        }
        self.save(&routes)
    }

    fn save(&self, routes: &[ProviderRoute]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_vec_pretty(routes).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        fs::rename(tmp, &self.path).map_err(|error| error.to_string())
    }
}

impl ProviderRoute {
    pub fn into_view(self) -> ProviderRouteView {
        let tail = self
            .api_key
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        ProviderRouteView {
            id: self.id,
            name: self.name,
            protocol: self.protocol,
            base_url: self.base_url,
            api_key_masked: if tail.is_empty() {
                String::new()
            } else {
                format!("****{tail}")
            },
            has_api_key: !self.api_key.is_empty(),
            models: self.models,
            active: self.active,
            selected_model: self.selected_model,
        }
    }
}

fn validate_input(input: &ProviderRouteInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("模型商名称不能为空".into());
    }
    if !matches!(input.protocol.as_str(), "openai" | "anthropic") {
        return Err("不支持的模型协议".into());
    }
    let url = url::Url::parse(input.base_url.trim()).map_err(|_| "Base URL 无效")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Base URL 只支持 HTTP/HTTPS".into());
    }
    Ok(())
}

fn dedupe_models(models: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| valid_model_id(model) && seen.insert(model.clone()))
        .collect()
}

pub fn valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> ProviderDirectory {
        let root = std::env::temp_dir().join(format!(
            "ncx-provider-directory-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        ProviderDirectory::at(root.join("providers.json"), root.join("config.toml"))
    }

    fn input() -> ProviderRouteInput {
        ProviderRouteInput {
            id: Some("relay".into()),
            name: "Relay".into(),
            protocol: "openai".into(),
            base_url: "https://relay.example/v1/".into(),
            api_key: Some("secret-key".into()),
            models: vec!["gpt-5.6-sol".into(), "gpt-5.6-sol".into()],
        }
    }

    #[test]
    fn upsert_masks_secret_dedupes_models_and_preserves_key_on_edit() {
        let directory = directory("upsert");
        let view = directory.upsert(input()).unwrap();
        assert_eq!(view.api_key_masked, "****-key");
        assert_eq!(view.models, vec!["gpt-5.6-sol"]);
        let mut edited = input();
        edited.api_key = None;
        edited.name = "Edited".into();
        directory.upsert(edited).unwrap();
        assert_eq!(directory.get("relay").unwrap().api_key, "secret-key");
    }

    #[test]
    fn activation_writes_one_complete_route_and_rolls_selection_forward() {
        let directory = directory("activate");
        directory.upsert(input()).unwrap();
        let route = directory.activate("relay", "gpt-5.6-sol").unwrap();
        assert_eq!(route.selected_model, "gpt-5.6-sol");
        let config = fs::read_to_string(&directory.config_path).unwrap();
        assert!(config.contains("active_provider_id = \"relay\""));
        assert!(config.contains("base_url = \"https://relay.example/v1\""));
        assert!(config.contains("price_in = \"0\""));
        assert!(config.contains("price_out = \"0\""));
        assert!(!directory.views("relay", "ignored").unwrap()[0]
            .api_key_masked
            .contains("secret"));
    }

    #[test]
    fn active_route_cannot_be_deleted_and_unknown_model_cannot_be_selected() {
        let directory = directory("guards");
        directory.upsert(input()).unwrap();
        assert!(directory.delete("relay", "relay").is_err());
        assert!(directory.activate("relay", "other-model").is_err());
    }

    #[test]
    fn preset_upsert_and_activation_commit_one_named_route_with_pricing() {
        let directory = directory("preset-activate");
        let updates = HashMap::from([
            ("price_in".to_string(), "5".to_string()),
            ("price_out".to_string(), "30".to_string()),
            ("price_currency".to_string(), "USD".to_string()),
        ]);
        let route = directory
            .upsert_and_activate(
                ProviderRouteInput {
                    id: Some("preset:openai".into()),
                    name: "OpenAI".into(),
                    protocol: "openai".into(),
                    base_url: "https://api.openai.com/v1".into(),
                    api_key: Some("preset-secret".into()),
                    models: vec!["gpt-5.6-sol".into()],
                },
                "gpt-5.6-sol",
                &updates,
            )
            .unwrap();
        assert_eq!(route.id, "preset:openai");
        assert!(route.active);
        let config = fs::read_to_string(&directory.config_path).unwrap();
        assert!(config.contains("active_provider_id = \"preset:openai\""));
        assert!(config.contains("price_in = \"5\""));
        assert!(config.contains("price_currency = \"USD\""));
    }

    #[test]
    fn failed_preset_snapshot_restores_the_previous_directory() {
        let directory = directory("preset-rollback");
        directory.upsert(input()).unwrap();
        let before = fs::read(directory.path()).unwrap();
        fs::create_dir_all(&directory.config_path).unwrap();
        let error = directory
            .upsert_and_activate(
                ProviderRouteInput {
                    id: Some("preset:deepseek".into()),
                    name: "DeepSeek".into(),
                    protocol: "openai".into(),
                    base_url: "https://api.deepseek.com".into(),
                    api_key: Some("preset-secret".into()),
                    models: vec!["deepseek-chat".into()],
                },
                "deepseek-chat",
                &HashMap::new(),
            )
            .unwrap_err();
        assert!(!error.is_empty());
        assert_eq!(fs::read(directory.path()).unwrap(), before);
    }

    #[test]
    fn curated_model_reconciliation_preserves_token_selection_and_other_routes() {
        let directory = directory("reconcile-models");
        directory
            .upsert(ProviderRouteInput {
                id: Some("preset:deepseek".into()),
                name: "DeepSeek".into(),
                protocol: "openai".into(),
                base_url: "https://api.deepseek.com".into(),
                api_key: Some("deepseek-secret".into()),
                models: vec!["deepseek-v4-flash".into(), "deepseek-v4-pro".into()],
            })
            .unwrap();
        directory
            .upsert(ProviderRouteInput {
                id: Some("custom-relay".into()),
                ..input()
            })
            .unwrap();
        directory
            .activate("preset:deepseek", "deepseek-v4-pro")
            .unwrap();
        let custom_before = directory.get("custom-relay").unwrap();

        let updated = directory
            .reconcile_models(
                "preset:deepseek",
                vec![
                    "deepseek-v4-flash".into(),
                    "deepseek-v4-pro".into(),
                    "deepseek-v4-flash-vision-exp".into(),
                ],
            )
            .unwrap();

        assert_eq!(updated.api_key, "deepseek-secret");
        assert_eq!(updated.selected_model, "deepseek-v4-pro");
        assert_eq!(updated.models.len(), 3);
        assert_eq!(directory.get("custom-relay").unwrap(), custom_before);
        let config = fs::read_to_string(&directory.config_path).unwrap();
        assert!(config.contains(
            "available_models = \"deepseek-v4-flash,deepseek-v4-pro,deepseek-v4-flash-vision-exp\""
        ));
    }
}
