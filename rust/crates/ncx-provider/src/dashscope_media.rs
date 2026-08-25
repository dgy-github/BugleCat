use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub const DASHSCOPE_MEDIA_BASE_URL: &str = "https://dashscope.aliyuncs.com/api/v1";
pub const DEFAULT_IMAGE_MODEL: &str = "wan2.2-t2i-flash";
pub const DEFAULT_VIDEO_MODEL: &str = "wan2.1-t2v-turbo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaGenerationRequest {
    pub kind: MediaKind,
    pub prompt: String,
    pub negative_prompt: Option<String>,
    pub size: String,
    pub duration_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MediaGenerationResult {
    pub task_id: String,
    pub model: String,
    pub urls: Vec<String>,
}

#[async_trait(?Send)]
pub trait MediaProvider {
    async fn generate(
        &self,
        request: &MediaGenerationRequest,
    ) -> Result<MediaGenerationResult, String>;
}

pub struct DashScopeMediaProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    image_model: String,
    video_model: String,
    poll_interval: Duration,
    max_polls: usize,
}

impl DashScopeMediaProvider {
    pub fn new(api_key: impl Into<String>, timeout_seconds: u64) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_seconds.max(30)))
                .build()
                .expect("DashScope media HTTP client must build"),
            api_key: api_key.into(),
            base_url: DASHSCOPE_MEDIA_BASE_URL.into(),
            image_model: DEFAULT_IMAGE_MODEL.into(),
            video_model: DEFAULT_VIDEO_MODEL.into(),
            poll_interval: Duration::from_secs(2),
            max_polls: 150,
        }
    }

    #[cfg(test)]
    fn with_models(mut self, image: &str, video: &str) -> Self {
        self.image_model = image.into();
        self.video_model = video.into();
        self
    }

    fn model(&self, kind: MediaKind) -> &str {
        match kind {
            MediaKind::Image => &self.image_model,
            MediaKind::Video => &self.video_model,
        }
    }

    fn endpoint(&self, kind: MediaKind) -> String {
        let path = match kind {
            MediaKind::Image => "services/aigc/text2image/image-synthesis",
            MediaKind::Video => "services/aigc/video-generation/video-synthesis",
        };
        format!("{}/{path}", self.base_url.trim_end_matches('/'))
    }

    fn request_body(&self, request: &MediaGenerationRequest) -> Value {
        let mut input = json!({"prompt": request.prompt});
        if let Some(negative) = request
            .negative_prompt
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            input["negative_prompt"] = Value::String(negative.to_string());
        }
        let parameters = match request.kind {
            MediaKind::Image => json!({"size": request.size, "n": 1}),
            MediaKind::Video => json!({
                "size": request.size,
                "duration": request.duration_seconds,
                "prompt_extend": true,
            }),
        };
        json!({
            "model": self.model(request.kind),
            "input": input,
            "parameters": parameters,
        })
    }

    async fn submit(&self, request: &MediaGenerationRequest) -> Result<String, String> {
        if self.api_key.trim().is_empty() {
            return Err("阿里百炼媒体 API Key 未配置".into());
        }
        let response = self
            .client
            .post(self.endpoint(request.kind))
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .header("X-DashScope-Async", "enable")
            .json(&self.request_body(request))
            .send()
            .await
            .map_err(|error| format!("提交媒体任务失败: {error}"))?;
        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .map_err(|error| format!("媒体任务响应解析失败: {error}"))?;
        if !status.is_success() {
            return Err(api_error(&payload, status.as_u16()));
        }
        payload["output"]["task_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "媒体任务响应缺少 task_id".to_string())
    }

    async fn wait_result(
        &self,
        task_id: &str,
        model: &str,
    ) -> Result<MediaGenerationResult, String> {
        let url = format!("{}/tasks/{task_id}", self.base_url.trim_end_matches('/'));
        for _ in 0..self.max_polls {
            let response = self
                .client
                .get(&url)
                .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
                .send()
                .await
                .map_err(|error| format!("查询媒体任务失败: {error}"))?;
            let status = response.status();
            let payload: Value = response
                .json()
                .await
                .map_err(|error| format!("媒体任务状态解析失败: {error}"))?;
            if !status.is_success() {
                return Err(api_error(&payload, status.as_u16()));
            }
            match payload["output"]["task_status"].as_str().unwrap_or("") {
                "SUCCEEDED" => {
                    let urls = collect_urls(&payload);
                    if urls.is_empty() {
                        return Err("媒体任务成功但没有返回文件 URL".into());
                    }
                    return Ok(MediaGenerationResult {
                        task_id: task_id.into(),
                        model: model.into(),
                        urls,
                    });
                }
                "FAILED" | "CANCELED" | "UNKNOWN" => return Err(api_error(&payload, 200)),
                _ => tokio::time::sleep(self.poll_interval).await,
            }
        }
        Err("媒体任务等待超时，可稍后使用 task_id 查询".into())
    }
}

#[async_trait(?Send)]
impl MediaProvider for DashScopeMediaProvider {
    async fn generate(
        &self,
        request: &MediaGenerationRequest,
    ) -> Result<MediaGenerationResult, String> {
        let model = self.model(request.kind).to_string();
        let task_id = self.submit(request).await?;
        self.wait_result(&task_id, &model).await
    }
}

fn collect_urls(payload: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    for path in [&payload["output"]["video_url"], &payload["output"]["url"]] {
        if let Some(url) = path.as_str() {
            urls.push(url.to_string());
        }
    }
    if let Some(results) = payload["output"]["results"].as_array() {
        for result in results {
            if let Some(url) = result["url"].as_str() {
                urls.push(url.to_string());
            }
        }
    }
    urls
}

fn api_error(payload: &Value, status: u16) -> String {
    let code = payload["code"].as_str().unwrap_or("unknown");
    let message = payload["message"]
        .as_str()
        .or_else(|| payload["output"]["message"].as_str())
        .unwrap_or("未知错误");
    format!("阿里百炼媒体接口错误（HTTP {status}, {code}）：{message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_and_video_requests_use_distinct_models_and_parameters() {
        let provider = DashScopeMediaProvider::new("secret", 30).with_models("image-m", "video-m");
        let image = provider.request_body(&MediaGenerationRequest {
            kind: MediaKind::Image,
            prompt: "cat".into(),
            negative_prompt: None,
            size: "1024*1024".into(),
            duration_seconds: 0,
        });
        assert_eq!(image["model"], "image-m");
        assert_eq!(image["parameters"]["n"], 1);
        let video = provider.request_body(&MediaGenerationRequest {
            kind: MediaKind::Video,
            prompt: "cat runs".into(),
            negative_prompt: Some("blur".into()),
            size: "1280*720".into(),
            duration_seconds: 5,
        });
        assert_eq!(video["model"], "video-m");
        assert_eq!(video["parameters"]["duration"], 5);
        assert_eq!(video["input"]["negative_prompt"], "blur");
    }

    #[test]
    fn result_parser_supports_image_and_video_shapes() {
        assert_eq!(
            collect_urls(&json!({"output":{"results":[{"url":"https://image"}]}})),
            vec!["https://image"]
        );
        assert_eq!(
            collect_urls(&json!({"output":{"video_url":"https://video"}})),
            vec!["https://video"]
        );
    }
}
