//! Declarative metadata used to inspect and compose Harness components.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCapability {
    Core,
    Search,
    Workspace,
    Process,
    Session,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HarnessProfile {
    /// All default components, including process and terminal management.
    #[default]
    Full,
    /// Coding tools without managed background process and terminal components.
    Coding,
    /// Read-oriented component set. Mutation is still enforced by sandbox policy.
    ReadOnly,
    /// Smallest useful local workspace component set.
    Minimal,
}

impl HarnessProfile {
    pub fn enables(self, capability: PluginCapability) -> bool {
        match self {
            Self::Full => true,
            Self::Coding => capability != PluginCapability::Process,
            Self::ReadOnly => matches!(
                capability,
                PluginCapability::Core
                    | PluginCapability::Search
                    | PluginCapability::Workspace
                    | PluginCapability::Session
            ),
            Self::Minimal => matches!(
                capability,
                PluginCapability::Core | PluginCapability::Workspace
            ),
        }
    }
}
