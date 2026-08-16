//! Ordered, reversible in-process extensions around tool execution.
//!
//! The design adapts DeepSeek Harness's tool pipeline while preserving
//! nanocodex's existing hook, approval, sandbox, and tool ownership.

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::ToolContext;

/// Decision returned before a registered tool is dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolMiddlewareDecision {
    /// Continue into the next registered layer or the tool dispatcher.
    Continue,
    /// Skip dispatch and expose the supplied denial reason to the model.
    Block { reason: String },
}

/// A named layer around tool execution.
///
/// Layers enter in registration order and leave in reverse order. Returning
/// [`ToolMiddlewareDecision::Block`] skips the tool body and all later layers;
/// entered layers still receive [`ToolMiddleware::after_execute`].
#[async_trait(?Send)]
pub trait ToolMiddleware {
    /// Return the stable, non-empty name used for registration and removal.
    fn name(&self) -> &str;

    /// Inspect a call before dispatch and optionally stop the pipeline.
    async fn before_execute(
        &self,
        _ctx: &ToolContext,
        _tool_name: &str,
        _args: &Value,
    ) -> ToolMiddlewareDecision {
        ToolMiddlewareDecision::Continue
    }

    /// Optionally replace the model-facing result after dispatch or blocking.
    async fn after_execute(
        &self,
        _ctx: &ToolContext,
        _tool_name: &str,
        _args: &Value,
        _result: &str,
    ) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    use serde_json::{json, Value};

    use super::*;
    use crate::tools::{Tool, ToolContext, ToolRegistry};

    struct RecordingTool {
        events: Rc<RefCell<Vec<String>>>,
    }

    #[async_trait(?Send)]
    impl Tool for RecordingTool {
        fn name(&self) -> &str {
            "recording"
        }

        fn description(&self) -> &str {
            "Records deterministic middleware tests."
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            self.events.borrow_mut().push("tool".to_string());
            "ok".to_string()
        }
    }

    struct RecordingMiddleware {
        name: &'static str,
        events: Rc<RefCell<Vec<String>>>,
        block: bool,
    }

    #[async_trait(?Send)]
    impl ToolMiddleware for RecordingMiddleware {
        fn name(&self) -> &str {
            self.name
        }

        async fn before_execute(
            &self,
            _ctx: &ToolContext,
            _tool_name: &str,
            _args: &Value,
        ) -> ToolMiddlewareDecision {
            self.events.borrow_mut().push(format!("pre: {}", self.name));
            if self.block {
                ToolMiddlewareDecision::Block {
                    reason: "policy denied".to_string(),
                }
            } else {
                ToolMiddlewareDecision::Continue
            }
        }

        async fn after_execute(
            &self,
            _ctx: &ToolContext,
            _tool_name: &str,
            _args: &Value,
            result: &str,
        ) -> Option<String> {
            self.events
                .borrow_mut()
                .push(format!("post: {}", self.name));
            Some(format!("{result}|{}", self.name))
        }
    }

    fn registry(events: Rc<RefCell<Vec<String>>>) -> ToolRegistry {
        let workspace = PathBuf::from("middleware-test-workspace");
        let policy = SandboxPolicy::new(WORKSPACE_WRITE, &workspace);
        let mut registry = ToolRegistry::empty(ToolContext::new(workspace, policy));
        registry.register(Box::new(RecordingTool { events }));
        registry
    }

    fn layer(
        name: &'static str,
        events: Rc<RefCell<Vec<String>>>,
        block: bool,
    ) -> Rc<dyn ToolMiddleware> {
        Rc::new(RecordingMiddleware {
            name,
            events,
            block,
        })
    }

    #[tokio::test]
    async fn middleware_enters_in_order_and_leaves_in_reverse() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut registry = registry(events.clone());
        registry
            .register_middleware(layer("outer", events.clone(), false))
            .unwrap();
        registry
            .register_middleware(layer("inner", events.clone(), false))
            .unwrap();

        let result = registry.execute("recording", &json!({})).await;

        assert_eq!(result, "ok|inner|outer");
        assert_eq!(
            events.borrow().as_slice(),
            [
                "pre: outer",
                "pre: inner",
                "tool",
                "post: inner",
                "post: outer"
            ]
        );
    }

    #[tokio::test]
    async fn blocking_short_circuits_and_registration_is_reversible() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut registry = registry(events.clone());
        registry
            .register_middleware(layer("outer", events.clone(), false))
            .unwrap();
        registry
            .register_middleware(layer("gate", events.clone(), true))
            .unwrap();
        registry
            .register_middleware(layer("never", events.clone(), false))
            .unwrap();
        assert!(registry
            .register_middleware(layer("gate", events.clone(), false))
            .is_err());

        let blocked = registry.execute("recording", &json!({})).await;

        assert!(blocked.contains("blocked by tool middleware 'gate': policy denied"));
        assert!(blocked.ends_with("|gate|outer"));
        assert_eq!(
            events.borrow().as_slice(),
            ["pre: outer", "pre: gate", "post: gate", "post: outer"]
        );

        events.borrow_mut().clear();
        assert!(registry.unregister_middleware("gate"));
        assert!(!registry.unregister_middleware("gate"));
        let result = registry.execute("recording", &json!({})).await;

        assert_eq!(result, "ok|never|outer");
        assert_eq!(
            events.borrow().as_slice(),
            [
                "pre: outer",
                "pre: never",
                "tool",
                "post: never",
                "post: outer"
            ]
        );
    }
}
