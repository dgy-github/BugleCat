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
    CostTelemetryPlugin, ExternalHostPlugin, InteractionPlugin, LlmProviderPlugin, McpPlugin,
    MediaPlugin, MemoryPlugin, PolicyPlugin, ProcessToolsPlugin, ProviderCatalogPlugin,
    ProviderChatProbePlugin, ProviderDirectoryPlugin, SearchToolsPlugin, SessionToolsPlugin,
    WorkspaceToolsPlugin,
};
pub use composition::{
    BundleSpec, HarnessComposition, OverlayEntry, OverlaySpec, PluginEntry, ProfileSpec,
};
pub use external::{
    ExternalPluginCatalog, ExternalPluginHandshake, ExternalPluginManifest, ExternalPluginRecord,
    ExternalPluginRegistration, ExternalProtocolRequest, ExternalProtocolResponse,
    ExternalToolDescriptor,
};
pub use manifest::{PluginCapability, PluginManifest};
pub(crate) use openai_compat::discover_enabled_codex_plugins_with_home;
pub use openai_compat::{
    discover_codex_apps, discover_codex_hooks, discover_codex_mcp_servers, discover_marketplaces,
    resolve_local_marketplace_plugin, CodexAppResource, CodexPluginCatalog, CodexPluginManifest,
    CodexPluginRecord, Marketplace, MarketplacePlugin, MarketplaceSource,
};
pub use registry::{PluginInstallReport, PluginRegistry};
pub use runtime::HarnessRuntimeBuilder;
pub use services::{
    context_descriptor, AttachmentServiceDescriptor, CompactionServiceDescriptor,
    ContextServiceDescriptor, CostTelemetryService, CostTelemetryServiceDescriptor,
    CostTelemetrySnapshot, HarnessDiagnostics, InteractionService, LlmProviderFactory,
    LlmProviderFactoryHandle, LlmServiceDescriptor, McpServiceDescriptor, MediaServiceDescriptor,
    MemoryServiceDescriptor, PolicyService, ProviderCatalogService, ProviderChatProbeService,
    ProviderDirectoryDiagnostics, ProviderDirectoryService,
};
