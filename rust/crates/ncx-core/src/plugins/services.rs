//! Service definitions shared by capability plugins and their consumers.

use crate::tools::{ApprovalHandler, ToolContext};
use crate::Provider;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use ncx_config::{
    load_config_with_paths, ConfigPaths, Overrides, ProviderDirectory, ProviderRoute,
    ProviderRouteInput, ProviderRouteView,
};
use ncx_provider::{
    DiscoveredProviderModel, HttpProviderCatalogClient, HttpProviderChatProbeClient,
    ProviderCatalogClient, ProviderCatalogRequest, ProviderChatProbeClient,
    ProviderChatProbeRequest, ProviderChatProbeResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmServiceDescriptor {
    pub model: String,
    pub supports_reasoning: bool,
    pub supports_vision: bool,
}

/// Process-level owner for durable provider routes. Host adapters use this
/// contract instead of duplicating provider persistence and activation rules.
#[derive(Clone, Debug)]
pub struct ProviderDirectoryService {
    directory: ProviderDirectory,
    paths: ConfigPaths,
}

impl Default for ProviderDirectoryService {
    fn default() -> Self {
        Self::from_paths(&ConfigPaths::default())
    }
}

impl ProviderDirectoryService {
    pub fn from_paths(paths: &ConfigPaths) -> Self {
        Self {
            directory: ProviderDirectory::from_paths(paths),
            paths: paths.clone(),
        }
    }

    pub fn list(&self) -> Result<Vec<ProviderRouteView>, String> {
        let cfg = load_config_with_paths(Overrides::default(), &self.paths)
            .map_err(|error| error.to_string())?;
        self.directory.views(&cfg.active_provider_id, &cfg.model)
    }

    pub fn get(&self, id: &str) -> Result<ProviderRoute, String> {
        self.directory.get(id)
    }

    pub fn save(&self, input: ProviderRouteInput) -> Result<ProviderRouteView, String> {
        self.directory.upsert(input)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let active = load_config_with_paths(Overrides::default(), &self.paths)
            .map_err(|error| error.to_string())?
            .active_provider_id;
        self.directory.delete(id, &active)
    }

    pub fn activate(&self, id: &str, model: &str) -> Result<ProviderRoute, String> {
        self.directory.activate(id, model)
    }

    pub fn save_and_activate_preset(
        &self,
        input: ProviderRouteInput,
        model: &str,
        price_in: &str,
        price_out: &str,
        price_currency: &str,
    ) -> Result<ProviderRoute, String> {
        let updates = HashMap::from([
            ("price_in".to_string(), price_in.to_string()),
            ("price_out".to_string(), price_out.to_string()),
            ("price_currency".to_string(), price_currency.to_string()),
        ]);
        self.directory.upsert_and_activate(input, model, &updates)
    }

    pub fn select_model(&self, id: &str, model: &str) -> Result<(), String> {
        self.directory.select_model(id, model)
    }

    pub fn reconcile_models(&self, id: &str, models: Vec<String>) -> Result<ProviderRoute, String> {
        self.directory.reconcile_models(id, models)
    }

    pub fn clear_active_flags(&self) -> Result<(), String> {
        self.directory.clear_active_flags()
    }

    pub fn diagnostics(&self) -> Result<ProviderDirectoryDiagnostics, String> {
        let cfg = load_config_with_paths(Overrides::default(), &self.paths)
            .map_err(|error| error.to_string())?;
        Ok(ProviderDirectoryDiagnostics {
            active_provider_id: cfg.active_provider_id,
            protocol: cfg.provider_protocol,
            base_url: cfg.base_url,
            model: cfg.model,
            has_api_key: !cfg.api_key.trim().is_empty(),
            route_count: self.directory.load()?.len(),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ProviderDirectoryDiagnostics {
    pub active_provider_id: String,
    pub protocol: String,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
    pub route_count: usize,
}

/// Replaceable transport boundary for `/models` discovery. It is deliberately
/// read-only: successful or failed discovery cannot mutate the active route.
#[derive(Clone)]
pub struct ProviderCatalogService {
    client: Arc<dyn ProviderCatalogClient>,
}

/// Explicit chat-plane verification, separate from catalog discovery because
/// it creates a real (one-token) inference request and may be billable.
#[derive(Clone)]
pub struct ProviderChatProbeService {
    client: Arc<dyn ProviderChatProbeClient>,
}

impl Default for ProviderChatProbeService {
    fn default() -> Self {
        Self::new(Arc::new(HttpProviderChatProbeClient))
    }
}

impl ProviderChatProbeService {
    pub fn new(client: Arc<dyn ProviderChatProbeClient>) -> Self {
        Self { client }
    }

    pub fn probe_route(
        &self,
        route: &ProviderRoute,
        model: &str,
    ) -> Result<ProviderChatProbeResult, String> {
        if !route.models.iter().any(|candidate| candidate == model) {
            return Err(format!("模型 {model} 不属于当前模型商 {}", route.name));
        }
        self.client.probe(&ProviderChatProbeRequest::new(
            &route.base_url,
            &route.protocol,
            &route.api_key,
            model,
        ))
    }
}

impl Default for ProviderCatalogService {
    fn default() -> Self {
        Self::new(Arc::new(HttpProviderCatalogClient))
    }
}

impl ProviderCatalogService {
    pub fn new(client: Arc<dyn ProviderCatalogClient>) -> Self {
        Self { client }
    }

    pub fn discover_route(
        &self,
        route: &ProviderRoute,
    ) -> Result<Vec<DiscoveredProviderModel>, String> {
        if route.api_key.trim().is_empty() {
            return Err("请先保存 Token".to_string());
        }
        self.discover(
            &route.base_url,
            &route.protocol,
            Some(route.api_key.clone()),
        )
    }

    /// Validate a complete candidate route without mutating the active
    /// directory or the compatibility config.  Activation callers must run
    /// this before committing, so a bad token/endpoint/model cannot replace a
    /// working runtime route.
    pub fn validate_route_model(&self, route: &ProviderRoute, model: &str) -> Result<(), String> {
        let models = self.discover_route(route)?;
        if models.iter().any(|candidate| candidate.id == model) {
            Ok(())
        } else {
            Err(format!(
                "模型 {model} 不在模型商 {} 返回的目录中",
                route.name
            ))
        }
    }

    pub fn discover_config(
        &self,
        cfg: &ncx_config::Config,
    ) -> Result<Vec<DiscoveredProviderModel>, String> {
        if cfg.api_key.trim().is_empty() {
            return Err("当前 Provider 尚未配置 Token".to_string());
        }
        self.discover(
            &cfg.base_url,
            &cfg.provider_protocol,
            Some(cfg.api_key.clone()),
        )
    }

    pub fn discover_public(&self, base_url: &str) -> Result<Vec<DiscoveredProviderModel>, String> {
        self.discover(base_url, "openai", None)
    }

    fn discover(
        &self,
        base_url: &str,
        protocol: &str,
        api_key: Option<String>,
    ) -> Result<Vec<DiscoveredProviderModel>, String> {
        self.client
            .discover(&ProviderCatalogRequest::new(base_url, protocol, api_key))
    }
}

/// Runtime-owned provider factory exposed through the LLM capability boundary.
pub trait LlmProviderFactory {
    fn primary(&self) -> Box<dyn Provider>;
    fn vision(&self) -> Option<Box<dyn Provider>>;
}

#[derive(Clone)]
pub struct LlmProviderFactoryHandle(pub Rc<dyn LlmProviderFactory>);

#[derive(Clone)]
pub struct InteractionService {
    pub approver: Option<Rc<dyn ApprovalHandler>>,
}

pub use ncx_sandbox::PolicyService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextServiceDescriptor {
    pub workspace: String,
    pub has_memory: bool,
    pub skill_count: usize,
    pub service: ncx_context::ContextService,
}

impl ContextServiceDescriptor {
    pub fn assemble(&self, base: impl Into<String>) -> String {
        self.service.assemble(base)
    }
}

#[derive(Clone)]
pub struct MemoryServiceDescriptor {
    pub enabled: bool,
    pub store: Option<Rc<crate::memory::MemoryStore>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionServiceDescriptor {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct McpServiceDescriptor {
    pub enabled: bool,
    pub configured_servers: usize,
    pub active_tools: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AttachmentServiceDescriptor {
    pub max_bytes: u64,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MediaServiceDescriptor {
    pub vision: bool,
    pub image_generation: bool,
    pub video_generation: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CostTelemetryServiceDescriptor {
    pub currency: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub telemetry_enabled: bool,
}

impl CostTelemetryServiceDescriptor {
    pub fn estimate(&self, prompt_tokens: i64, completion_tokens: i64) -> f64 {
        (prompt_tokens.max(0) as f64 / 1_000_000.0) * self.input_per_million
            + (completion_tokens.max(0) as f64 / 1_000_000.0) * self.output_per_million
    }
}

pub struct CostTelemetryService {
    pub config: CostTelemetryServiceDescriptor,
    turns: Cell<u64>,
    prompt_tokens: Cell<i64>,
    completion_tokens: Cell<i64>,
    media_cost: Cell<f64>,
}

impl CostTelemetryService {
    pub fn new(config: CostTelemetryServiceDescriptor) -> Self {
        Self {
            config,
            turns: Cell::new(0),
            prompt_tokens: Cell::new(0),
            completion_tokens: Cell::new(0),
            media_cost: Cell::new(0.0),
        }
    }
    pub fn record(&self, usage: &std::collections::BTreeMap<String, i64>) {
        if !self.config.telemetry_enabled {
            return;
        }
        self.turns.set(self.turns.get() + 1);
        self.prompt_tokens.set(
            self.prompt_tokens.get() + usage.get("prompt_tokens").copied().unwrap_or(0).max(0),
        );
        self.completion_tokens.set(
            self.completion_tokens.get()
                + usage.get("completion_tokens").copied().unwrap_or(0).max(0),
        );
    }
    pub fn record_media_cost(&self, cost: f64) {
        if self.config.telemetry_enabled && cost.is_finite() && cost > 0.0 {
            self.media_cost.set(self.media_cost.get() + cost);
        }
    }
    pub fn snapshot(&self) -> CostTelemetrySnapshot {
        CostTelemetrySnapshot {
            turns: self.turns.get(),
            prompt_tokens: self.prompt_tokens.get(),
            completion_tokens: self.completion_tokens.get(),
            estimated_cost: self
                .config
                .estimate(self.prompt_tokens.get(), self.completion_tokens.get())
                + self.media_cost.get(),
            media_cost: self.media_cost.get(),
            currency: self.config.currency.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct CostTelemetrySnapshot {
    pub turns: u64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub estimated_cost: f64,
    pub media_cost: f64,
    pub currency: String,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct HarnessDiagnostics {
    pub llm: bool,
    pub interaction: bool,
    pub policy: bool,
    pub context: bool,
    pub memory: bool,
    pub compaction: bool,
    pub mcp: bool,
    pub attachment: bool,
    pub media: bool,
    pub cost_telemetry: bool,
    pub provider_directory: bool,
    pub provider_catalog: bool,
    pub provider_chat_probe: bool,
}

pub fn context_descriptor(ctx: &ToolContext) -> ContextServiceDescriptor {
    ContextServiceDescriptor {
        workspace: ctx.workspace.display().to_string(),
        has_memory: ctx.memory.is_some(),
        skill_count: ctx.skills.len(),
        service: ncx_context::ContextService::new(ctx.context_entries.as_ref().clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct FailingCatalogClient;
    impl ProviderCatalogClient for FailingCatalogClient {
        fn discover(
            &self,
            _request: &ProviderCatalogRequest,
        ) -> Result<Vec<DiscoveredProviderModel>, String> {
            Err("simulated catalog failure".to_string())
        }
    }

    struct FixedCatalogClient;
    impl ProviderCatalogClient for FixedCatalogClient {
        fn discover(
            &self,
            _request: &ProviderCatalogRequest,
        ) -> Result<Vec<DiscoveredProviderModel>, String> {
            Ok(vec![DiscoveredProviderModel {
                id: "gpt-5.6-sol".into(),
                name: "GPT 5.6 Sol".into(),
                context_length: None,
                input_price_per_million: None,
                output_price_per_million: None,
            }])
        }
    }
    #[test]
    fn telemetry_accumulates_usage_and_estimates_cost() {
        let service = CostTelemetryService::new(CostTelemetryServiceDescriptor {
            currency: "CNY".into(),
            input_per_million: 2.0,
            output_per_million: 4.0,
            telemetry_enabled: true,
        });
        let usage = [
            ("prompt_tokens".to_string(), 500_000),
            ("completion_tokens".to_string(), 250_000),
        ]
        .into_iter()
        .collect();
        service.record(&usage);
        service.record_media_cost(1.2);
        let snapshot = service.snapshot();
        assert_eq!(snapshot.turns, 1);
        assert_eq!(snapshot.media_cost, 1.2);
        assert_eq!(snapshot.estimated_cost, 3.2);
    }

    #[test]
    fn provider_directory_service_owns_route_activation_and_safe_diagnostics() {
        let root = crate::test_support::unique_temp_dir("ncx-provider-service");
        let paths = ConfigPaths {
            deepseek: PathBuf::from(&root).join("deepseek.toml"),
            codex: PathBuf::from(&root).join("codex.toml"),
            nanocodex: PathBuf::from(&root).join("config.toml"),
        };
        let service = ProviderDirectoryService::from_paths(&paths);
        service
            .save(ProviderRouteInput {
                id: Some("relay".into()),
                name: "Relay".into(),
                protocol: "openai".into(),
                base_url: "https://relay.example/v1".into(),
                api_key: Some("never-expose-this".into()),
                models: vec!["gpt-5.6-sol".into()],
            })
            .unwrap();
        service.activate("relay", "gpt-5.6-sol").unwrap();

        let diagnostics = service.diagnostics().unwrap();
        assert_eq!(diagnostics.active_provider_id, "relay");
        assert_eq!(diagnostics.model, "gpt-5.6-sol");
        assert!(diagnostics.has_api_key);
        assert_eq!(diagnostics.route_count, 1);
        assert!(!serde_json::to_string(&diagnostics)
            .unwrap()
            .contains("never-expose-this"));
        assert_eq!(service.list().unwrap()[0].api_key_masked, "****this");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_failure_cannot_mutate_the_active_provider_route() {
        let root = crate::test_support::unique_temp_dir("ncx-provider-catalog");
        let paths = ConfigPaths {
            deepseek: root.join("deepseek.toml"),
            codex: root.join("codex.toml"),
            nanocodex: root.join("config.toml"),
        };
        let directory = ProviderDirectoryService::from_paths(&paths);
        directory
            .save(ProviderRouteInput {
                id: Some("relay".into()),
                name: "Relay".into(),
                protocol: "anthropic".into(),
                base_url: "https://relay.example/v1".into(),
                api_key: Some("catalog-test-key".into()),
                models: vec!["claude-sonnet-4-5".into()],
            })
            .unwrap();
        directory.activate("relay", "claude-sonnet-4-5").unwrap();
        let provider_path = paths.nanocodex.with_file_name("providers.json");
        let config_before = std::fs::read(&paths.nanocodex).unwrap();
        let providers_before = std::fs::read(&provider_path).unwrap();

        let catalog = ProviderCatalogService::new(Arc::new(FailingCatalogClient));
        let error = catalog
            .discover_route(&directory.get("relay").unwrap())
            .unwrap_err();
        assert_eq!(error, "simulated catalog failure");
        assert_eq!(std::fs::read(&paths.nanocodex).unwrap(), config_before);
        assert_eq!(std::fs::read(&provider_path).unwrap(), providers_before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn route_validation_requires_the_selected_model_without_mutating_files() {
        let root = crate::test_support::unique_temp_dir("ncx-provider-validation");
        let paths = ConfigPaths {
            deepseek: root.join("deepseek.toml"),
            codex: root.join("codex.toml"),
            nanocodex: root.join("config.toml"),
        };
        let directory = ProviderDirectoryService::from_paths(&paths);
        directory
            .save(ProviderRouteInput {
                id: Some("relay".into()),
                name: "Relay".into(),
                protocol: "openai".into(),
                base_url: "https://relay.example/v1".into(),
                api_key: Some("validation-test-key".into()),
                models: vec!["gpt-5.6-sol".into(), "other-model".into()],
            })
            .unwrap();
        let route = directory.get("relay").unwrap();
        let provider_path = paths.nanocodex.with_file_name("providers.json");
        let providers_before = std::fs::read(&provider_path).unwrap();
        let catalog = ProviderCatalogService::new(Arc::new(FixedCatalogClient));

        catalog.validate_route_model(&route, "gpt-5.6-sol").unwrap();
        let error = catalog
            .validate_route_model(&route, "other-model")
            .unwrap_err();

        assert!(error.contains("other-model"));
        assert!(!paths.nanocodex.exists());
        assert_eq!(std::fs::read(&provider_path).unwrap(), providers_before);
        let _ = std::fs::remove_dir_all(root);
    }
}
