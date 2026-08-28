//! CLI assembly for the shared Harness-backed memory summarizer.

use ncx_config::Config;
use ncx_core::{AgentRuntimeProfile, ConfiguredHarnessRuntime, ProviderMemorySummarizer};

pub fn memory_summarizer(cfg: &Config) -> ProviderMemorySummarizer {
    let model = if cfg.fast_model.is_empty() {
        cfg.model.clone()
    } else {
        cfg.fast_model.clone()
    };
    let runtime = ConfiguredHarnessRuntime::new(
        cfg.clone(),
        model,
        AgentRuntimeProfile::from_legacy_permissions(cfg),
    );
    ProviderMemorySummarizer::new(runtime.primary_provider())
}
