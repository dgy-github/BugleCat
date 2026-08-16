//! Model interface and adapters used by the agent runtime.

use async_trait::async_trait;
use ncx_provider::{DeepSeekProvider, ModelResponse};
use serde_json::Value;

/// Minimal async chat interface driven by [`crate::AgentLoop`].
///
/// `?Send` allows implementations to hold single-threaded providers and test
/// doubles used by the interactive runtime.
#[async_trait(?Send)]
pub trait Provider {
    fn model(&self) -> &str;

    /// Return one completion, mapping transport failures into a model response.
    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse;

    /// Stream assistant text, falling back to one non-streaming completion.
    async fn chat_streaming(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
        on_content: &mut dyn FnMut(String),
    ) -> ModelResponse {
        let response = self.chat(messages, tools, reasoning_effort).await;
        if response.finish_reason != "error" && !response.content.is_empty() {
            on_content(response.content.clone());
        }
        response
    }
}

/// Adapt the HTTP provider to the loop contract by representing transport
/// failures as responses with `finish_reason == "error"`.
#[async_trait(?Send)]
impl Provider for DeepSeekProvider {
    fn model(&self) -> &str {
        &self.model
    }

    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse {
        let tools = (!tools.is_empty()).then_some(tools);
        match DeepSeekProvider::chat(self, messages, tools, None, None, reasoning_effort).await {
            Ok(response) => response,
            Err(error) => provider_error(error),
        }
    }

    async fn chat_streaming(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
        on_content: &mut dyn FnMut(String),
    ) -> ModelResponse {
        let tools = (!tools.is_empty()).then_some(tools);
        match DeepSeekProvider::chat_stream(
            self,
            messages,
            tools,
            None,
            None,
            reasoning_effort,
            |content: &str| on_content(content.to_string()),
            |_| {},
        )
        .await
        {
            Ok(response) => response,
            Err(error) => provider_error(error),
        }
    }
}

fn provider_error(error: impl std::fmt::Display) -> ModelResponse {
    ModelResponse {
        content: error.to_string(),
        finish_reason: "error".to_string(),
        ..Default::default()
    }
}
