//! Named, reversible context sources for one agent turn.

use std::collections::HashSet;
use std::rc::Rc;

use async_trait::async_trait;
use serde_json::Value;

/// Immutable input exposed to a turn context provider.
#[derive(Debug, Clone)]
pub struct TurnContextRequest {
    pub user_input: Value,
    pub query: String,
}

/// Supplies model-facing system notes for the current turn.
///
/// Providers run in registration order. They cannot mutate the session, tool
/// registry, or sandbox policy; returning no notes is a supported no-op.
#[async_trait(?Send)]
pub trait TurnContextProvider {
    /// Stable registration name used for duplicate checks and removal.
    fn name(&self) -> &str;

    /// Produce zero or more query-scoped notes for this turn.
    async fn provide(&self, request: &TurnContextRequest) -> Vec<String>;
}

struct ContextEntry {
    name: String,
    provider: Rc<dyn TurnContextProvider>,
}

/// Ordered collection of turn context providers.
#[derive(Default)]
pub struct TurnContextRegistry {
    entries: Vec<ContextEntry>,
    names: HashSet<String>,
}

impl TurnContextRegistry {
    /// Register one provider. Names must be unique, non-empty, and normalized.
    pub fn register(&mut self, provider: Rc<dyn TurnContextProvider>) -> Result<(), String> {
        let raw_name = provider.name();
        let name = raw_name.trim();
        if name.is_empty() {
            return Err("turn context provider name cannot be empty".to_string());
        }
        if name != raw_name {
            return Err("turn context provider name cannot contain surrounding whitespace".into());
        }
        if !self.names.insert(name.to_string()) {
            return Err(format!(
                "turn context provider '{name}' is already registered"
            ));
        }
        self.entries.push(ContextEntry {
            name: name.to_string(),
            provider,
        });
        Ok(())
    }

    /// Remove a provider by name and report whether it was present.
    pub fn unregister(&mut self, name: &str) -> bool {
        if !self.names.remove(name) {
            return false;
        }
        self.entries.retain(|entry| entry.name != name);
        true
    }

    pub(crate) async fn collect(&self, request: &TurnContextRequest) -> Vec<String> {
        let mut notes = Vec::new();
        for entry in &self.entries {
            notes.extend(
                entry
                    .provider
                    .provide(request)
                    .await
                    .into_iter()
                    .filter(|note| !note.trim().is_empty()),
            );
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Provider {
        name: &'static str,
        note: &'static str,
        seen: Rc<RefCell<Vec<String>>>,
    }

    #[async_trait(?Send)]
    impl TurnContextProvider for Provider {
        fn name(&self) -> &str {
            self.name
        }

        async fn provide(&self, request: &TurnContextRequest) -> Vec<String> {
            self.seen.borrow_mut().push(self.name.to_string());
            vec![format!("{}: {}", self.note, request.query)]
        }
    }

    fn provider(
        name: &'static str,
        note: &'static str,
        seen: Rc<RefCell<Vec<String>>>,
    ) -> Rc<dyn TurnContextProvider> {
        Rc::new(Provider { name, note, seen })
    }

    #[tokio::test]
    async fn providers_are_ordered_unique_and_reversible() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut registry = TurnContextRegistry::default();
        registry
            .register(provider("first", "one", seen.clone()))
            .unwrap();
        registry
            .register(provider("second", "two", seen.clone()))
            .unwrap();
        assert!(registry
            .register(provider("first", "duplicate", seen.clone()))
            .is_err());

        let request = TurnContextRequest {
            user_input: Value::String("question".into()),
            query: "question".into(),
        };
        assert_eq!(
            registry.collect(&request).await,
            ["one: question", "two: question"]
        );
        assert_eq!(seen.borrow().as_slice(), ["first", "second"]);

        assert!(registry.unregister("first"));
        assert!(!registry.unregister("first"));
        assert_eq!(registry.collect(&request).await, ["two: question"]);
    }
}
