//! Model-provider contract consumed by the agent runtime.

use crate::{DeepSeekProvider, ModelResponse};
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamDelta {
    Content(String),
    Reasoning(String),
}

#[async_trait(?Send)]
pub trait Provider {
    fn model(&self) -> &str;

    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
    ) -> ModelResponse;

    async fn chat_streaming(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning_effort: Option<&str>,
        on_delta: &mut dyn FnMut(StreamDelta),
    ) -> ModelResponse {
        let response = self.chat(messages, tools, reasoning_effort).await;
        if response.finish_reason != "error" && !response.reasoning.is_empty() {
            on_delta(StreamDelta::Reasoning(response.reasoning.clone()));
        }
        if response.finish_reason != "error" && !response.content.is_empty() {
            on_delta(StreamDelta::Content(response.content.clone()));
        }
        response
    }
}

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
        on_delta: &mut dyn FnMut(StreamDelta),
    ) -> ModelResponse {
        let tools = (!tools.is_empty()).then_some(tools);
        let on_delta = std::cell::RefCell::new(on_delta);
        match DeepSeekProvider::chat_stream(
            self,
            messages,
            tools,
            None,
            None,
            reasoning_effort,
            |content| (on_delta.borrow_mut())(StreamDelta::Content(content.to_string())),
            |reasoning| (on_delta.borrow_mut())(StreamDelta::Reasoning(reasoning.to_string())),
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
