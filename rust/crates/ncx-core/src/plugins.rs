//! DeepSeek Harness-style plugin architecture.
//!
//! API contracts, registration, runtime composition and built-in capability
//! implementations are separate layers. Tool execution remains in `tools`.

mod api;
mod builtin;
mod composition;
mod manifest;
mod registry;
mod runtime;
mod services;

pub(crate) use api::PluginRuntimeState;
pub use api::{HarnessPlugin, PluginHost};
pub use builtin::{
    BuiltinToolsPlugin, CompactionPlugin, ContextPlugin, CoreToolsPlugin, InteractionPlugin,
    LlmProviderPlugin, MemoryPlugin, PolicyPlugin, ProcessToolsPlugin, SearchToolsPlugin,
    SessionToolsPlugin, WorkspaceToolsPlugin,
};
pub use composition::{
    BundleSpec, HarnessComposition, OverlayEntry, OverlaySpec, PluginEntry, ProfileSpec,
};
pub use manifest::{PluginCapability, PluginManifest};
pub use registry::{PluginInstallReport, PluginRegistry};
pub use runtime::HarnessRuntimeBuilder;
pub use services::{
    context_descriptor, ContextServiceDescriptor, InteractionService, LlmServiceDescriptor,
    PolicyService,
};
