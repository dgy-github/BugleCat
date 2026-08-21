//! DeepSeek Harness-style plugin architecture.
//!
//! API contracts, registration, runtime composition and built-in capability
//! implementations are separate layers. Tool execution remains in `tools`.

mod api;
mod builtin;
mod registry;
mod runtime;

pub use api::{HarnessPlugin, PluginHost};
pub use builtin::{
    BuiltinToolsPlugin, CoreToolsPlugin, ProcessToolsPlugin, SearchToolsPlugin,
    SessionToolsPlugin, WorkspaceToolsPlugin,
};
pub use registry::{PluginInstallReport, PluginRegistry};
pub use runtime::HarnessRuntimeBuilder;
