//! Compatibility re-exports for the provider contract now owned by ncx-provider.

pub use ncx_provider::{Provider, StreamDelta};

use async_trait::async_trait;
use ncx_provider::{
    HttpProviderCatalogClient, ModelResponse, ProviderCatalogClient, ProviderCatalogRequest,
};
use serde_json::Value;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// The configured route owns both chat and metadata. Cache belongs to a provider
/// instance, so changing endpoint, key, or model cannot reuse another route's size.
pub(crate) struct CatalogAwareProvider {
    inner: Box<dyn Provider>,
    request: ProviderCatalogRequest,
    capacity: RefCell<Option<(Instant, Option<i64>)>>,
}

impl CatalogAwareProvider {
    pub fn wrap(
        inner: Box<dyn Provider>,
        mut request: ProviderCatalogRequest,
    ) -> Box<dyn Provider> {
        request.timeout = Duration::from_secs(5);
        Box::new(Self {
            inner,
            request,
            capacity: RefCell::new(None),
        })
    }
}

#[async_trait(?Send)]
impl Provider for CatalogAwareProvider {
    fn model(&self) -> &str {
        self.inner.model()
    }
    fn confirmed_model(&self) -> Option<String> {
        self.inner.confirmed_model()
    }

    async fn context_window(&self) -> Option<i64> {
        if let Some((at, value)) = *self.capacity.borrow() {
            if at.elapsed() < Duration::from_secs(if value.is_some() { 300 } else { 30 }) {
                return value;
            }
        }
        let request = self.request.clone();
        let model = self.model().to_string();
        let value = tokio::task::spawn_blocking(move || {
            let models = HttpProviderCatalogClient
                .discover(&request)
                .unwrap_or_default();
            ncx_provider::model_catalog::resolve_context_capacity(
                &models,
                &request.base_url,
                &model,
            )
            .map(|(capacity, _)| capacity)
        })
        .await
        .ok()
        .flatten();
        *self.capacity.borrow_mut() = Some((Instant::now(), value));
        value
    }

    async fn chat(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning: Option<&str>,
    ) -> ModelResponse {
        self.inner.chat(messages, tools, reasoning).await
    }

    async fn chat_streaming(
        &self,
        messages: &[Value],
        tools: &[Value],
        reasoning: Option<&str>,
        on_delta: &mut dyn FnMut(StreamDelta),
    ) -> ModelResponse {
        self.inner
            .chat_streaming(messages, tools, reasoning, on_delta)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    struct Stub;
    #[async_trait(?Send)]
    impl Provider for Stub {
        fn model(&self) -> &str {
            "test-model"
        }
        async fn chat(&self, _: &[Value], _: &[Value], _: Option<&str>) -> ModelResponse {
            ModelResponse::default()
        }
    }

    fn endpoint(capacity: i64) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let mut buffer = [0; 1024];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            assert!(bytes.starts_with(b"GET /v1/models "));
            let body = serde_json::json!({"data":[{"id":"test-model","context_window":capacity}]})
                .to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        });
        (format!("http://{address}/v1"), handle)
    }

    #[tokio::test]
    async fn model_capacity_is_queried_cached_and_scoped_to_route() {
        let (url, server) = endpoint(128_000);
        let provider = CatalogAwareProvider::wrap(
            Box::new(Stub),
            ProviderCatalogRequest::new(url, "openai", None),
        );
        assert_eq!(provider.context_window().await, Some(128_000));
        server.join().unwrap();
        // Server is gone: this can succeed only by reusing the cached metadata.
        assert_eq!(provider.context_window().await, Some(128_000));
        let (url, server) = endpoint(32_000);
        let other = CatalogAwareProvider::wrap(
            Box::new(Stub),
            ProviderCatalogRequest::new(url, "openai", None),
        );
        assert_eq!(other.context_window().await, Some(32_000));
        server.join().unwrap();
    }
}
