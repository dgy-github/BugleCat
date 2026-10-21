use super::*;
use std::collections::BTreeMap;

fn seed(agent: &mut AgentLoop) {
    agent.session.add_user_text("修复程序，不要恢复旧实现");
    for _ in 0..40 {
        agent
            .session
            .add_assistant(&"调查证据 ".repeat(1_000), None, "");
    }
}

#[tokio::test]
async fn compaction_failure_stops_before_normal_model_call_without_losing_history() {
    let ws = tmpdir("semantic_failure");
    let mut agent = build(
        &ws,
        Box::new(ScriptedProvider::new(vec![ModelResponse {
            content: "partial summary".into(),
            finish_reason: "length".into(),
            ..Default::default()
        }])),
    );
    seed(&mut agent);
    let original = agent.session.messages.clone();
    let result = agent.run_turn(json!("继续"), None).await;
    assert_eq!(result.stop_reason, "compaction_failed");
    assert_eq!(&agent.session.messages[..original.len()], &original);
    assert_eq!(agent.session.messages.len(), original.len() + 1);
    assert!(!result.final_text.contains("partial summary"));
}

#[tokio::test]
async fn compaction_counts_usage_and_does_not_repeat_for_a_small_followup() {
    let ws = tmpdir("semantic_usage");
    let (provider, summary_calls) = ScriptedProvider::new(vec![
        ModelResponse {
            content: "继续完成".into(),
            usage: BTreeMap::from([("prompt_tokens".into(), 5_000)]),
            ..Default::default()
        },
        ModelResponse {
            content: "第二轮完成".into(),
            ..Default::default()
        },
    ])
    .with_summary(ModelResponse {
        content: "调查完成，下一步修复。".into(),
        usage: BTreeMap::from([("prompt_tokens".into(), 1_000)]),
        ..Default::default()
    });
    let mut agent = build(&ws, Box::new(provider));
    seed(&mut agent);
    let result = agent.run_turn(json!("继续"), None).await;
    assert_eq!(result.final_text, "继续完成");
    // Every chunk's summarization cost is charged to the turn, not just the last.
    let chunks = summary_calls.get();
    assert!(chunks >= 1);
    assert_eq!(result.usage["prompt_tokens"], 1_000 * chunks as i64 + 5_000);
    let result = agent.run_turn(json!("继续下一步"), None).await;
    assert_eq!(result.final_text, "第二轮完成");
    assert_eq!(
        agent
            .session
            .messages
            .iter()
            .filter(|m| m.get("_compaction_safety").is_some())
            .count(),
        1
    );
    assert!(
        agent
            .session
            .for_model()
            .iter()
            .all(|m| m.get("_compaction_safety").is_none())
    );
}

#[tokio::test]
async fn cancelled_compaction_keeps_history() {
    let ws = tmpdir("semantic_cancel");
    let mut agent = build(&ws, Box::new(ScriptedProvider::new(vec![])));
    seed(&mut agent);
    let original = agent.session.messages.clone();
    assert!(agent.compact_context(Some(&|| true)).await.is_err());
    assert_eq!(agent.session.messages, original);
}

#[test]
fn compaction_uses_real_prompt_usage_plus_new_messages_and_resets_on_model_change() {
    let ws = tmpdir("semantic_trigger");
    let mut agent = build(&ws, Box::new(ScriptedProvider::new(vec![])));
    agent.context_usage.set(Some((0, 27_000)));
    assert!(!agent.should_compact(&[], ""));
    agent.session.add_user_text(&"中".repeat(4_000));
    assert!(agent.should_compact(&[], ""));
    agent.replace_provider(Box::new(ScriptedProvider::new(vec![])));
    assert!(agent.context_usage.get().is_none());
    agent.context_window = 32_000;
    assert_eq!(agent.compaction_budget(), 28_000);
}

struct SizedProvider(i64);
#[async_trait(?Send)]
impl Provider for SizedProvider {
    fn model(&self) -> &str {
        "same-name-different-route"
    }
    async fn context_window(&self) -> Option<i64> {
        Some(self.0)
    }
    async fn chat(&self, _: &[Value], _: &[Value], _: Option<&str>) -> ModelResponse {
        ModelResponse::default()
    }
}

#[tokio::test]
async fn context_budget_follows_active_model_and_ignores_legacy_character_cap() {
    let ws = tmpdir("dynamic_model_budget");
    let mut agent = build(&ws, Box::new(SizedProvider(128_000)));
    agent.context_edit.max_chars = 1;
    agent.resolve_context_window().await;
    assert_eq!(agent.compaction_budget(), 112_000);
    agent.replace_provider(Box::new(SizedProvider(1_000_000)));
    agent.resolve_context_window().await;
    assert_eq!(agent.compaction_budget(), 875_000);
    agent = agent.with_vision_provider(Some(Box::new(SizedProvider(64_000))));
    agent.use_vision_this_turn = true;
    agent.resolve_context_window().await;
    assert_eq!(agent.compaction_budget(), 56_000);
    agent.use_vision_this_turn = false;
    agent.resolve_context_window().await;
    assert_eq!(agent.compaction_budget(), 875_000);
}

#[tokio::test]
async fn compaction_runs_through_openai_compatible_http_provider() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // A large history is summarized in several bounded requests, so the stub
    // serves every one of them and reports how many it answered.
    let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let served_in_server = served.clone();
    let server = std::thread::spawn(move || {
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let body = loop {
                let mut buffer = [0; 8192];
                let Ok(count) = stream.read(&mut buffer) else {
                    return;
                };
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let size: usize = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + size {
                        break bytes[end + 4..end + 4 + size].to_vec();
                    }
                }
            };
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert!(
                request
                    .get("tools")
                    .is_none_or(|v| v.as_array().is_some_and(Vec::is_empty))
            );
            let prompt = request["messages"][1]["content"].as_str().unwrap();
            assert!(prompt.contains("调查证据"));
            // Every chunk is self-describing and carries tagged source ids.
            assert!(prompt.contains("Part "), "{prompt}");
            assert!(prompt.contains("[S"), "{prompt}");
            served_in_server.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = json!({"choices":[{"message":{"role":"assistant","content":"调查完成；保留禁止恢复旧实现的约束，下一步修复。"},"finish_reason":"stop"}],"usage":{"prompt_tokens":40000,"completion_tokens":40,"total_tokens":40040}}).to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let ws = tmpdir("semantic_http");
    let provider = ncx_provider::DeepSeekProvider::new(
        "local-test-placeholder",
        &format!("http://{address}/v1"),
        "test-model",
    );
    let mut agent = build(&ws, Box::new(provider));
    seed(&mut agent);
    let (stats, usage) = agent.compact_context(None).await.unwrap();
    assert!(stats.edited_chars < stats.original_chars / 2);
    let chunks = served.load(std::sync::atomic::Ordering::SeqCst) as i64;
    assert!(chunks >= 1);
    // Usage is summed across chunks, so a multi-request compaction is not
    // reported as if it had cost one call.
    assert_eq!(usage["prompt_tokens"], 40000 * chunks);
    // The stub loops on accept; dropping the agent ends the test, and the
    // detached thread exits when the listener is closed with the process.
    drop(server);
}
