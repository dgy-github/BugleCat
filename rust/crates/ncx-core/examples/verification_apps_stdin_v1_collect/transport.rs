use super::*;
use futures_util::StreamExt;
use std::time::Duration;
#[async_trait(?Send)]
pub(super) trait Transport {
    async fn post(&self, key: &str, endpoint: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String>;
}
pub(super) struct HttpTransport;
#[async_trait(?Send)]
impl Transport for HttpTransport {
    async fn post(&self, key: &str, endpoint: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| "client_error")?;
        let response = client
            .post(endpoint)
            .bearer_auth(key)
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| "transport_error")?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
        {
            return Err("response_budget".into());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "transport_error")?;
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err("response_budget".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, bytes))
    }
}

pub(super) async fn bounded(
    transport: &dyn Transport,
    key: &str,
    endpoint: &str,
    bytes: &[u8],
    timeout: Duration,
    cancel: Option<&Path>,
) -> Result<(u16, Vec<u8>), String> {
    let request = tokio::time::timeout(timeout, transport.post(key, endpoint, bytes));
    tokio::pin!(request);
    loop {
        tokio::select! {
            result = &mut request => return result.map_err(|_| "timeout")?,
            _ = tokio::time::sleep(Duration::from_millis(20)) => {
                if cancel.is_some_and(Path::exists) { return Err("cancelled".into()); }
            }
        }
    }
}
