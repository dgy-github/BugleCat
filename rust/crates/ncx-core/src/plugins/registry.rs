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
    plugins: Vec<RegisteredPlugin>,
}

struct RegisteredPlugin {
    plugin: Rc<dyn HarnessPlugin>,
    config: toml::Value,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Rc<dyn HarnessPlugin>) -> Result<(), String> {
        self.register_configured(plugin, toml::Value::Table(Default::default()))
    }

    pub fn register_configured(
        &mut self,
        plugin: Rc<dyn HarnessPlugin>,
        config: toml::Value,
    ) -> Result<(), String> {
        let id = plugin.id().trim();
        if id.is_empty() {
            return Err("harness plugin id must not be empty".to_string());
        }
        if self.plugins.iter().any(|current| current.plugin.id() == id) {
            return Err(format!("harness plugin '{id}' is already registered"));
        }
        if plugin.manifest().id != id {
            return Err(format!(
                "harness plugin id '{id}' does not match manifest id '{}'",
                plugin.manifest().id
            ));
        }
        self.plugins.push(RegisteredPlugin { plugin, config });
        Ok(())
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|entry| entry.plugin.id())
    }

    pub fn manifests(&self) -> impl Iterator<Item = PluginManifest> + '_ {
        self.plugins.iter().map(|entry| entry.plugin.manifest())
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
                    plugin
                        .plugin
                        .inject()
                        .iter()
                        .all(|name| host.has_service(name))
                };
                if !ready {
                    waiting.push(plugin);
                    continue;
                }
                plugin
                    .plugin
                    .install(&mut PluginHost::new(registry), &plugin.config)?;
                installed.push(plugin.plugin.id().to_string());
            }
            pending = waiting;
            if pending.is_empty() || pending.len() == before {
                break;
            }
        }
        if !pending.is_empty() {
            let details = pending
                .iter()
                .map(|entry| {
                    let missing = entry
                        .plugin
                        .inject()
                        .iter()
                        .filter(|name| !PluginHost::new(registry).has_service(name))
                        .copied()
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{} requires [{}]", entry.plugin.id(), missing)
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!(
                "Harness plugin dependencies are unresolved: {details}"
            ));
        }
        Ok(PluginInstallReport {
            installed,
            pending: pending
                .iter()
                .map(|plugin| plugin.plugin.id().to_string())
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct EmptyPlugin(&'static str);

    impl HarnessPlugin for EmptyPlugin {
        fn id(&self) -> &str {
            self.0
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new(self.0, self.0, super::super::PluginCapability::Core)
        }
        fn install(&self, _host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
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
        fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
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
        fn install(&self, host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
            let message = host
                .service::<String>("message")
                .ok_or_else(|| "missing injected message service".to_string())?;
            if message.as_str() != "ready" {
                return Err("unexpected service value".to_string());
            }
            Ok(())
        }
    }

    struct MissingDependencyPlugin;

    impl HarnessPlugin for MissingDependencyPlugin {
        fn id(&self) -> &str {
            "missing-consumer"
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new(
                "missing-consumer",
                "Missing Consumer",
                super::super::PluginCapability::Core,
            )
        }
        fn inject(&self) -> &[&str] {
            &["never-provided"]
        }
        fn install(&self, _host: &mut PluginHost<'_>, _config: &toml::Value) -> Result<(), String> {
            Ok(())
        }
    }

    struct ConfigPlugin(Rc<RefCell<Option<String>>>);

    impl HarnessPlugin for ConfigPlugin {
        fn id(&self) -> &str {
            "configured"
        }
        fn manifest(&self) -> PluginManifest {
            PluginManifest::new(
                "configured",
                "Configured",
                super::super::PluginCapability::Core,
            )
        }
        fn install(&self, _host: &mut PluginHost<'_>, config: &toml::Value) -> Result<(), String> {
            let label = config
                .get("label")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| "configured plugin requires label".to_string())?;
            *self.0.borrow_mut() = Some(label.to_string());
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

    #[test]
    fn unresolved_service_dependency_fails_loud() {
        use crate::tools::{ToolContext, ToolRegistry};
        use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
        use std::path::PathBuf;

        let mut plugins = PluginRegistry::new();
        plugins.register(Rc::new(MissingDependencyPlugin)).unwrap();
        let workspace = PathBuf::from("plugin-missing-dependency-test");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let mut tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
        let error = plugins.install_into(&mut tools).unwrap_err();
        assert!(error.contains("never-provided"));
    }

    #[test]
    fn file_composition_config_reaches_plugin_installation() {
        use crate::tools::{ToolContext, ToolRegistry};
        use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
        use std::path::PathBuf;

        let observed = Rc::new(RefCell::new(None));
        let mut plugins = PluginRegistry::new();
        let config = "label = \"from-overlay\"".parse::<toml::Value>().unwrap();
        plugins
            .register_configured(Rc::new(ConfigPlugin(observed.clone())), config)
            .unwrap();
        let workspace = PathBuf::from("plugin-config-test");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let mut tools = ToolRegistry::empty(ToolContext::new(workspace, policy));
        plugins.install_into(&mut tools).unwrap();
        assert_eq!(observed.borrow().as_deref(), Some("from-overlay"));
    }
}
