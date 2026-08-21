//! Ordered plugin registration and installation reporting.

use std::rc::Rc;

use super::{HarnessPlugin, PluginHost, PluginManifest};
use crate::tools::ToolRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallReport {
    pub installed: Vec<String>,
    pub pending: Vec<String>,
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

    pub fn install_into(&self, registry: &mut ToolRegistry) -> Result<PluginInstallReport, String> {
        let mut installed = Vec::with_capacity(self.plugins.len());
        let mut pending = self.plugins.iter().collect::<Vec<_>>();
        loop {
            let before = pending.len();
            let mut waiting = Vec::new();
            for plugin in pending {
                let ready = {
                    let host = PluginHost::new(registry);
                    plugin.inject().iter().all(|name| host.has_service(name))
                };
                if !ready {
                    waiting.push(plugin);
                    continue;
                }
                plugin.install(&mut PluginHost::new(registry))?;
                installed.push(plugin.id().to_string());
            }
            pending = waiting;
            if pending.is_empty() || pending.len() == before {
                break;
            }
        }
        Ok(PluginInstallReport {
            installed,
            pending: pending
                .iter()
                .map(|plugin| plugin.id().to_string())
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct EmptyPlugin(&'static str);

    impl HarnessPlugin for EmptyPlugin {
        fn id(&self) -> &str {
            self.0
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new(self.0, self.0, super::super::PluginCapability::Core)
        }
        fn install(&self, _host: &mut PluginHost<'_>) -> Result<(), String> {
            Ok(())
        }
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

    struct ServicePlugin {
        disposed: Rc<Cell<bool>>,
    }

    impl HarnessPlugin for ServicePlugin {
        fn id(&self) -> &str {
            "service"
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new("service", "Service", super::super::PluginCapability::Core)
        }
        fn install(&self, host: &mut PluginHost<'_>) -> Result<(), String> {
            host.provide("message", Rc::new(String::from("ready")))?;
            let disposed = self.disposed.clone();
            host.effect(move || disposed.set(true));
            Ok(())
        }
    }

    struct ConsumerPlugin;

    impl HarnessPlugin for ConsumerPlugin {
        fn id(&self) -> &str {
            "consumer"
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new("consumer", "Consumer", super::super::PluginCapability::Core)
        }
        fn inject(&self) -> &[&str] {
            &["message"]
        }
        fn install(&self, host: &mut PluginHost<'_>) -> Result<(), String> {
            let message = host
                .service::<String>("message")
                .ok_or_else(|| "missing injected message service".to_string())?;
            if message.as_str() != "ready" {
                return Err("unexpected service value".to_string());
            }
            Ok(())
        }
    }

    #[test]
    fn dependencies_activate_by_service_and_effects_dispose_with_runtime() {
        use crate::tools::{ToolContext, ToolRegistry};
        use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
        use std::path::PathBuf;

        let disposed = Rc::new(Cell::new(false));
        let mut plugins = PluginRegistry::new();
        plugins.register(Rc::new(ConsumerPlugin)).unwrap();
        plugins
            .register(Rc::new(ServicePlugin {
                disposed: disposed.clone(),
            }))
            .unwrap();
        let workspace = PathBuf::from("plugin-service-test");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let mut tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
        let report = plugins.install_into(&mut tools).unwrap();
        assert_eq!(report.installed, vec!["service", "consumer"]);
        assert!(report.pending.is_empty());
        drop(tools);
        assert!(disposed.get());
    }
}
