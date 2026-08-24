//! DeepSeek Harness-style plugin architecture.
//!
//! API contracts, registration, runtime composition and built-in capability
//! implementations are separate layers. Tool execution remains in `tools`.

mod api;
mod builtin;
mod composition;
mod external;
mod manifest;
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
pub use registry::{PluginInstallReport, PluginRegistry};
pub use runtime::HarnessRuntimeBuilder;
pub use services::{
    context_descriptor, AttachmentServiceDescriptor, CompactionServiceDescriptor,
    ContextServiceDescriptor, CostTelemetryService, CostTelemetryServiceDescriptor,
    CostTelemetrySnapshot, HarnessDiagnostics, InteractionService, LlmProviderFactory,
    LlmProviderFactoryHandle, LlmServiceDescriptor, McpServiceDescriptor, MediaServiceDescriptor,
    MemoryServiceDescriptor, PolicyService,
};
