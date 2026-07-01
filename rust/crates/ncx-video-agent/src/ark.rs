use std::collections::BTreeMap;

use serde_json::Value;

use crate::{Result, VideoAgentError};

pub const ARK_BASE_URL: &str = "https://ark.cn-beijing.volces.com/api/v3";

pub trait ArkTransport {
    fn send(
        &mut self,
        method: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: Option<&Value>,
    ) -> std::result::Result<(u16, String), String>;
}

pub struct ReqwestArkTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestArkTransport {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|err| VideoAgentError::Ark(format!("build HTTP client failed: {err}")))?;
        Ok(Self { client })
    }
}

impl ArkTransport for ReqwestArkTransport {
    fn send(
        &mut self,
        method: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: Option<&Value>,
    ) -> std::result::Result<(u16, String), String> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|err| format!("invalid HTTP method {method}: {err}"))?;
        let mut request = self.client.request(method, url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        let response = request
            .send()
            .map_err(|err| format!("HTTP request failed: {err}"))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|err| format!("read HTTP response body failed: {err}"))?;
        Ok((status, body))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArkTaskStatus {
    pub status: String,
    pub video_url: Option<String>,
    pub usage: Value,
}

pub struct ArkClient<T> {
    api_key: String,
    base_url: String,
    transport: T,
}

impl<T: ArkTransport> ArkClient<T> {
    pub fn new(api_key: impl Into<String>, transport: T) -> Result<Self> {
        Self::with_base_url(api_key, ARK_BASE_URL, transport)
    }

    pub fn with_base_url(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        transport: T,
    ) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(VideoAgentError::Ark(
                "Seedance needs an ARK API key (env ARK_API_KEY).".to_string(),
            ));
        }
        Ok(Self {
            api_key,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            transport,
        })
    }

    pub fn submit(&mut self, payload: &Value) -> Result<String> {
        let (status, body) = self.send("POST", "/contents/generations/tasks", Some(payload))?;
        if status != 200 {
            return Err(VideoAgentError::Ark(format!(
                "submit failed (HTTP {status}): {}",
                preview(&body)
            )));
        }
        let obj: Value = serde_json::from_str(&body).map_err(|err| {
            VideoAgentError::Ark(format!(
                "submit returned non-JSON: {err}: {}",
                preview(&body)
            ))
        })?;
        obj.get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                VideoAgentError::Ark(format!("submit returned no task id: {}", preview(&body)))
            })
    }

    pub fn poll_once(&mut self, task_id: &str) -> Result<ArkTaskStatus> {
        let path = format!("/contents/generations/tasks/{task_id}");
        let (status, body) = self.send("GET", &path, None)?;
        if status != 200 {
            return Err(VideoAgentError::Ark(format!(
                "poll failed (HTTP {status}): {}",
                preview(&body)
            )));
        }
        let obj: Value = serde_json::from_str(&body).map_err(|err| {
            VideoAgentError::Ark(format!("poll returned non-JSON: {err}: {}", preview(&body)))
        })?;
        let status = obj
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let video_url = if status == "succeeded" {
            obj.get("content")
                .and_then(|content| content.get("video_url"))
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty())
                .map(ToOwned::to_owned)
        } else {
            None
        };
        let usage = obj
            .get("usage")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        Ok(ArkTaskStatus {
            status,
            video_url,
            usage,
        })
    }

    fn send(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, String)> {
        let url = format!("{}{}", self.base_url, path);
        let mut headers = BTreeMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", self.api_key),
        );
        self.transport
            .send(method, &url, &headers, body)
            .map_err(VideoAgentError::Ark)
    }
}

fn preview(text: &str) -> String {
    const MAX: usize = 300;
    if text.len() <= MAX {
        text.to_string()
    } else {
        format!("{}...", &text[..MAX])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct FakeTransport {
        calls: Vec<(String, String, Option<Value>)>,
        responses: Vec<(u16, String)>,
    }

    impl ArkTransport for FakeTransport {
        fn send(
            &mut self,
            method: &str,
            url: &str,
            headers: &BTreeMap<String, String>,
            body: Option<&Value>,
        ) -> std::result::Result<(u16, String), String> {
            assert_eq!(
                headers.get("Authorization").map(String::as_str),
                Some("Bearer sk-test")
            );
            self.calls
                .push((method.to_string(), url.to_string(), body.cloned()));
            Ok(self.responses.remove(0))
        }
    }

    #[test]
    fn ark_submit_posts_task_and_reads_id() {
        let transport = FakeTransport {
            responses: vec![(200, json!({"id": "task-1"}).to_string())],
            ..FakeTransport::default()
        };
        let mut client = ArkClient::new("sk-test", transport).unwrap();
        let task = client
            .submit(&json!({"model": "doubao-seedance-2-0-fast-260128"}))
            .unwrap();
        assert_eq!(task, "task-1");
        assert_eq!(client.transport.calls[0].0, "POST");
        assert!(client.transport.calls[0]
            .1
            .ends_with("/contents/generations/tasks"));
    }

    #[test]
    fn ark_poll_reads_succeeded_video_url_and_usage() {
        let transport = FakeTransport {
            responses: vec![(
                200,
                json!({
                    "status": "succeeded",
                    "content": {"video_url": "https://example.test/video.mp4"},
                    "usage": {"total_tokens": 123}
                })
                .to_string(),
            )],
            ..FakeTransport::default()
        };
        let mut client = ArkClient::new("sk-test", transport).unwrap();
        let status = client.poll_once("task-1").unwrap();
        assert_eq!(status.status, "succeeded");
        assert_eq!(
            status.video_url.as_deref(),
            Some("https://example.test/video.mp4")
        );
        assert_eq!(status.usage["total_tokens"], 123);
        assert_eq!(client.transport.calls[0].0, "GET");
        assert!(client.transport.calls[0]
            .1
            .ends_with("/contents/generations/tasks/task-1"));
    }

    #[test]
    fn ark_rejects_empty_key() {
        let err = match ArkClient::new("", FakeTransport::default()) {
            Ok(_) => panic!("empty key should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("ARK API key"));
    }
}
