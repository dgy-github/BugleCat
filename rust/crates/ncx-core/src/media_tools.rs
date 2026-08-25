use crate::plugins::CostTelemetryService;
use crate::tools::{Tool, ToolContext};
use async_trait::async_trait;
use ncx_provider::{MediaGenerationRequest, MediaKind, MediaProvider};
use serde_json::{json, Value};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct MediaPrice {
    pub amount: f64,
    pub unit: String,
    pub currency: String,
    pub source: String,
    pub audited_at: String,
}

pub struct MediaGenerationService {
    pub provider: Rc<dyn MediaProvider>,
    pub image: MediaPrice,
    pub video: MediaPrice,
    pub telemetry: Option<Rc<CostTelemetryService>>,
}

impl MediaGenerationService {
    fn cost(&self, kind: MediaKind, duration_seconds: u32) -> (f64, &MediaPrice) {
        match kind {
            MediaKind::Image => (self.image.amount, &self.image),
            MediaKind::Video => (self.video.amount * f64::from(duration_seconds), &self.video),
        }
    }
}

pub struct GenerateImageTool(Rc<MediaGenerationService>);
impl GenerateImageTool {
    pub fn new(service: Rc<MediaGenerationService>) -> Self {
        Self(service)
    }
}

#[async_trait(?Send)]
impl Tool for GenerateImageTool {
    fn name(&self) -> &str {
        "generate_image"
    }
    fn description(&self) -> &str {
        "使用当前 Harness 媒体 Provider 根据文字生成图片；返回任务、文件 URL 与本次预估费用。"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"prompt":{"type":"string"},"negative_prompt":{"type":"string"},"size":{"type":"string","default":"1024*1024"}},"required":["prompt"]})
    }
    async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
        execute_generation(&self.0, MediaKind::Image, args).await
    }
}

pub struct GenerateVideoTool(Rc<MediaGenerationService>);
impl GenerateVideoTool {
    pub fn new(service: Rc<MediaGenerationService>) -> Self {
        Self(service)
    }
}

#[async_trait(?Send)]
impl Tool for GenerateVideoTool {
    fn name(&self) -> &str {
        "generate_video"
    }
    fn description(&self) -> &str {
        "使用当前 Harness 媒体 Provider 根据文字生成短视频；返回任务、文件 URL 与按秒预估费用。"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"prompt":{"type":"string"},"negative_prompt":{"type":"string"},"size":{"type":"string","default":"1280*720"},"duration_seconds":{"type":"integer","minimum":1,"maximum":10,"default":5}},"required":["prompt"]})
    }
    async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
        execute_generation(&self.0, MediaKind::Video, args).await
    }
}

async fn execute_generation(
    service: &MediaGenerationService,
    kind: MediaKind,
    args: &Value,
) -> String {
    let prompt = args["prompt"].as_str().unwrap_or("").trim();
    if prompt.is_empty() {
        return "Error: prompt 不能为空".into();
    }
    let duration_seconds = if kind == MediaKind::Video {
        args["duration_seconds"].as_u64().unwrap_or(5).clamp(1, 10) as u32
    } else {
        0
    };
    let default_size = if kind == MediaKind::Image {
        "1024*1024"
    } else {
        "1280*720"
    };
    let request = MediaGenerationRequest {
        kind,
        prompt: prompt.to_string(),
        negative_prompt: args["negative_prompt"].as_str().map(str::to_string),
        size: args["size"].as_str().unwrap_or(default_size).to_string(),
        duration_seconds,
    };
    match service.provider.generate(&request).await {
        Ok(result) => {
            let (cost, price) = service.cost(kind, duration_seconds);
            if let Some(telemetry) = &service.telemetry {
                telemetry.record_media_cost(cost);
            }
            json!({"status":"succeeded","task_id":result.task_id,"model":result.model,"urls":result.urls,"estimated_cost":cost,"currency":price.currency,"price_unit":price.unit,"price_source":price.source,"price_audited_at":price.audited_at}).to_string()
        }
        Err(error) => format!("Error: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ncx_provider::MediaGenerationResult;
    use ncx_sandbox::{SandboxPolicy, WORKSPACE_WRITE};
    struct FakeProvider;
    #[async_trait(?Send)]
    impl MediaProvider for FakeProvider {
        async fn generate(
            &self,
            request: &MediaGenerationRequest,
        ) -> Result<MediaGenerationResult, String> {
            Ok(MediaGenerationResult {
                task_id: "task-1".into(),
                model: if request.kind == MediaKind::Image {
                    "image-model"
                } else {
                    "video-model"
                }
                .into(),
                urls: vec!["https://result".into()],
            })
        }
    }
    fn service() -> Rc<MediaGenerationService> {
        Rc::new(MediaGenerationService {
            provider: Rc::new(FakeProvider),
            image: MediaPrice {
                amount: 0.14,
                unit: "张".into(),
                currency: "CNY".into(),
                source: "catalog".into(),
                audited_at: "2026-08-25".into(),
            },
            video: MediaPrice {
                amount: 0.24,
                unit: "秒".into(),
                currency: "CNY".into(),
                source: "catalog".into(),
                audited_at: "2026-08-25".into(),
            },
            telemetry: None,
        })
    }
    fn context() -> ToolContext {
        let workspace = std::path::PathBuf::from("media-tool-test");
        ToolContext::new(
            workspace.clone(),
            SandboxPolicy::new(WORKSPACE_WRITE, workspace),
        )
    }
    #[tokio::test]
    async fn results_include_explicit_cost_units() {
        let image = GenerateImageTool::new(service())
            .execute(&context(), &json!({"prompt":"cat"}))
            .await;
        assert!(image.contains(r#""estimated_cost":0.14"#), "{image}");
        let video = GenerateVideoTool::new(service())
            .execute(
                &context(),
                &json!({"prompt":"cat runs","duration_seconds":5}),
            )
            .await;
        assert!(video.contains(r#""estimated_cost":1.2"#), "{video}");
        assert!(video.contains(r#""price_unit":"秒""#), "{video}");
    }
}
