//! Ordered plugin registration and installation reporting.

use std::rc::Rc;

use super::{HarnessPlugin, PluginHost, PluginManifest};
use crate::tools::ToolRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallReport {
    pub installed: Vec<String>,
}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Rc<dyn HarnessPlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Rc<dyn HarnessPlugin>) -> Result<(), String> {
        let id = plugin.id().trim();
        if id.is_empty() {
            return Err("harness plugin id must not be empty".to_string());
        }
        if self.plugins.iter().any(|current| current.id() == id) {
            return Err(format!("harness plugin '{id}' is already registered"));
        }
        if plugin.manifest().id != id {
            return Err(format!(
                "harness plugin id '{id}' does not match manifest id '{}'",
                plugin.manifest().id
            ));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|plugin| plugin.id())
    }

    pub fn manifests(&self) -> impl Iterator<Item = PluginManifest> + '_ {
        self.plugins.iter().map(|plugin| plugin.manifest())
    }

    pub fn install_into(&self, registry: &mut ToolRegistry) -> PluginInstallReport {
        let mut installed = Vec::with_capacity(self.plugins.len());
        for plugin in &self.plugins {
            plugin.install(&mut PluginHost::new(registry));
            installed.push(plugin.id().to_string());
        }
        PluginInstallReport { installed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EmptyPlugin(&'static str);

    impl HarnessPlugin for EmptyPlugin {
        fn id(&self) -> &str {
            self.0
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new(self.0, self.0, super::super::PluginCapability::Core)
        }
        fn install(&self, _host: &mut PluginHost<'_>) {}
    }

    #[test]
    fn ids_are_stable_unique_and_ordered() {
        let mut registry = PluginRegistry::new();
        registry.register(Rc::new(EmptyPlugin("first"))).unwrap();
        registry.register(Rc::new(EmptyPlugin("second"))).unwrap();
        assert!(registry.register(Rc::new(EmptyPlugin("first"))).is_err());
        assert!(registry.register(Rc::new(EmptyPlugin("  "))).is_err());
        assert_eq!(registry.ids().collect::<Vec<_>>(), vec!["first", "second"]);
    }
}
