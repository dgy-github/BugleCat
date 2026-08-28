use async_trait::async_trait;
use serde_json::json;
use std::cell::Cell;

use crate::{Provider, Summarizer};

/// Shared LLM-backed memory merger. Hosts inject the Provider selected by their
/// Harness runtime, so CLI/GUI do not create parallel model-routing rules.
pub struct ProviderMemorySummarizer {
    provider: Box<dyn Provider>,
    failures: Cell<usize>,
}

impl ProviderMemorySummarizer {
    pub fn new(provider: Box<dyn Provider>) -> Self {
        Self {
            provider,
            failures: Cell::new(0),
        }
    }

    /// Number of calls that failed or returned no usable merged text.
    pub fn failure_count(&self) -> usize {
        self.failures.get()
    }
}

#[async_trait(?Send)]
impl Summarizer for ProviderMemorySummarizer {
    async fn merge(&self, facts: &[String]) -> Option<String> {
        let user = facts
            .iter()
            .enumerate()
            .map(|(index, fact)| format!("{}. {fact}", index + 1))
            .collect::<Vec<_>>()
            .join("\n");
        let messages = vec![
            json!({"role": "system", "content": "Merge these related project notes into ONE concise factual note (at most 2 sentences). Output ONLY the merged note — no preamble, no list, no quotes."}),
            json!({"role": "user", "content": user}),
        ];
        let response = self.provider.chat(&messages, &[], None).await;
        let merged = (response.finish_reason != "error")
            .then(|| response.content.trim().to_string())
            .filter(|content| !content.is_empty());
        if merged.is_none() {
            self.failures.set(self.failures.get().saturating_add(1));
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use ncx_provider::ModelResponse;
    use serde_json::Value;

    use super::*;

    struct CapturingProvider {
        messages: Rc<RefCell<Vec<Value>>>,
        response: ModelResponse,
    }

    #[async_trait(?Send)]
    impl Provider for CapturingProvider {
        fn model(&self) -> &str {
            "fast-model"
        }

        async fn chat(
            &self,
            messages: &[Value],
            _tools: &[Value],
            _reasoning_effort: Option<&str>,
        ) -> ModelResponse {
            self.messages.replace(messages.to_vec());
            self.response.clone()
        }
    }

    #[tokio::test]
    async fn uses_injected_model_and_rejects_errors() {
        let messages = Rc::new(RefCell::new(Vec::new()));
        let summarizer = ProviderMemorySummarizer::new(Box::new(CapturingProvider {
            messages: messages.clone(),
            response: ModelResponse {
                content: "  merged fact  ".into(),
                ..Default::default()
            },
        }));
        assert_eq!(
            summarizer
                .merge(&["first fact".into(), "second fact".into()])
                .await
                .as_deref(),
            Some("merged fact")
        );
        let captured = messages.borrow();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[1]["content"], "1. first fact\n2. second fact");

        let failed = ProviderMemorySummarizer::new(Box::new(CapturingProvider {
            messages: Rc::new(RefCell::new(Vec::new())),
            response: ModelResponse {
                content: "third-party error body".into(),
                finish_reason: "error".into(),
                ..Default::default()
            },
        }));
        assert_eq!(failed.merge(&["fact".into()]).await, None);
        assert_eq!(failed.failure_count(), 1);
    }
}
