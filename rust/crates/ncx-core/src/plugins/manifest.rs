//! Declarative metadata used to inspect and compose Harness components.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCapability {
    Core,
    Search,
    Workspace,
    Process,
    Session,
    Llm,
    Interaction,
    Policy,
    Context,
    Memory,
    Compaction,
    Mcp,
    Attachment,
    Media,
    CostTelemetry,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub capability: PluginCapability,
    pub dependencies: &'static [&'static str],
    pub default_enabled: bool,
}

impl PluginManifest {
    pub const fn new(id: &'static str, name: &'static str, capability: PluginCapability) -> Self {
        Self {
            id,
            name,
            version: env!("CARGO_PKG_VERSION"),
            capability,
            dependencies: &[],
            default_enabled: true,
        }
    }
}
