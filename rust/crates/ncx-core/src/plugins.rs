//! DeepSeek Harness-style plugin architecture.
//!
//! API contracts, registration, runtime composition and built-in capability
//! implementations are separate layers. Tool execution remains in `tools`.

mod api;
mod builtin;
mod composition;
mod external;
mod manifest;
mod openai_compat;
mod registry;
mod runtime;
mod services;

pub(crate) use api::PluginRuntimeState;
pub use api::{HarnessPlugin, PluginHost};
pub use builtin::{
    AttachmentPlugin, BuiltinToolsPlugin, CompactionPlugin, ContextPlugin, CoreToolsPlugin,
    CostTelemetryPlugin, InteractionPlugin, LlmProviderPlugin, McpPlugin, MediaPlugin,
    MemoryPlugin, PolicyPlugin, ProcessToolsPlugin, SearchToolsPlugin, SessionToolsPlugin,
    WorkspaceToolsPlugin,
};
pub use composition::{
    BundleSpec, HarnessComposition, OverlayEntry, OverlaySpec, PluginEntry, ProfileSpec,
};
pub use external::{ExternalPluginCatalog, ExternalPluginManifest, ExternalPluginRecord};
pub use manifest::{PluginCapability, PluginManifest};
pub use openai_compat::{
    discover_codex_hooks, discover_codex_mcp_servers, discover_marketplaces,
    resolve_local_marketplace_plugin, CodexPluginCatalog, CodexPluginManifest, CodexPluginRecord,
    Marketplace, MarketplacePlugin, MarketplaceSource,
};
pub use registry::{PluginInstallReport, PluginRegistry};
pub use runtime::HarnessRuntimeBuilder;
pub use services::{
    context_descriptor, AttachmentServiceDescriptor, CompactionServiceDescriptor,
    ContextServiceDescriptor, CostTelemetryService, CostTelemetryServiceDescriptor,
    CostTelemetrySnapshot, HarnessDiagnostics, InteractionService, LlmProviderFactory,
    LlmProviderFactoryHandle, LlmServiceDescriptor, McpServiceDescriptor, MediaServiceDescriptor,
    MemoryServiceDescriptor, PolicyService,
};
