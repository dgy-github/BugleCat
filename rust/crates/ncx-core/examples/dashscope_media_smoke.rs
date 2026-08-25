use ncx_provider::{DashScopeMediaProvider, MediaGenerationRequest, MediaKind, MediaProvider};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let key_file = std::env::args()
        .nth(1)
        .expect("usage: dashscope_media_smoke <key-file> [line]");
    let key_index = std::env::args()
        .nth(2)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(2)
        .saturating_sub(1);
    let text = std::fs::read_to_string(key_file).expect("cannot read key file");
    let key = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .nth(key_index)
        .and_then(|line| line.split_whitespace().last())
        .filter(|value| value.len() >= 20)
        .expect("second key was not found")
        .to_string();
    let provider = DashScopeMediaProvider::new(key, 120);
    let result = provider
        .generate(&MediaGenerationRequest {
            kind: MediaKind::Image,
            prompt: "一只戴着红色围巾的白猫，苹果系统极简插画风格，纯色背景".into(),
            negative_prompt: Some("文字，水印，模糊".into()),
            size: "1024*1024".into(),
            duration_seconds: 0,
        })
        .await
        .expect("DashScope image smoke failed");
    println!(
        "media smoke ok: model={}, task_id={}, urls={}",
        result.model,
        result.task_id,
        result.urls.len()
    );
    for url in result.urls {
        println!("result_url={url}");
    }
}
