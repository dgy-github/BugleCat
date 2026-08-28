use super::*;

#[tokio::test]
async fn returns_final_text_without_tools() {
    let p = ScriptedProvider::new(vec![ModelResponse {
        content: "All done.".into(),
        ..Default::default()
    }]);
    let ws = tmpdir("notools");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("say hi"), None).await;
    assert_eq!(r.stop_reason, "completed");
    assert_eq!(r.final_text, "All done.");
    assert_eq!(r.iterations, 1);
}

#[tokio::test]
async fn suggests_a_short_task_oriented_session_title() {
    let p = ScriptedProvider::new(vec![ModelResponse {
        content: "标题：**整理大模型架构资料 PDF。**\n不要输出这一行".into(),
        ..Default::default()
    }]);
    let ws = tmpdir("session_title");
    let loop_ = build(&ws, Box::new(p));

    let title = loop_
        .suggest_title(
            "这里是一大段 MoE、GRPO、Engram 等背景资料，帮我分析并上网搜集详细资料，整理成通俗易懂的 PDF。",
        )
        .await;

    assert_eq!(title.as_deref(), Some("整理大模型架构资料 PDF"));
}

#[tokio::test]
async fn rejects_an_invalid_generated_session_title() {
    let p = ScriptedProvider::new(vec![ModelResponse {
        content: "这是一段超过限制而且没有遵守只输出短标题要求的模型回答，它不应该被保存为会话标题，因为侧栏会再次变得很难阅读".into(),
        ..Default::default()
    }]);
    let ws = tmpdir("invalid_session_title");
    let loop_ = build(&ws, Box::new(p));

    assert_eq!(loop_.suggest_title("修复登录问题").await, None);
}

#[tokio::test]
async fn executes_apply_patch_then_finishes() {
    let patch = "*** Begin Patch\n*** Add File: out.txt\n+hello\n*** End Patch";
    let p = ScriptedProvider::new(vec![
        assistant_toolcall(vec![tc("c1", "apply_patch", json!({"patch": patch}))]),
        ModelResponse {
            content: "Created out.txt.".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("applypatch");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("create out.txt"), None).await;
    assert_eq!(
        std::fs::read_to_string(ws.join("out.txt")).unwrap(),
        "hello\n"
    );
    assert_eq!(r.stop_reason, "completed");
    assert!(r.tools_used.contains(&"apply_patch".to_string()));
    // Second call saw a tool message in history.
    assert!(loop_.session.messages.iter().any(|m| m["role"] == "tool"));
}

#[tokio::test]
async fn pdf_creation_request_cannot_finish_until_a_new_valid_pdf_exists() {
    let ws = tmpdir("pdf_delivery_gate");
    std::fs::write(ws.join("old.pdf"), b"%PDF-1.4\nold\n%%EOF").unwrap();
    let patch =
        "*** Begin Patch\n*** Add File: report.pdf\n+%PDF-1.4\n+/Type /Page\n+%%EOF\n*** End Patch";
    let provider = ScriptedProvider::new(vec![
        ModelResponse {
            content: "资料已经整理好了。".into(),
            ..Default::default()
        },
        assistant_toolcall(vec![tc("pdf-1", "apply_patch", json!({"patch": patch}))]),
        ModelResponse {
            content: "PDF 已生成：report.pdf".into(),
            ..Default::default()
        },
    ]);
    let mut loop_ = build(&ws, Box::new(provider));

    let result = loop_
        .run_turn(json!("调研这些资料，整理成个 PDF 给我"), None)
        .await;

    assert_eq!(result.stop_reason, "completed");
    assert_eq!(result.iterations, 3);
    assert!(ws.join("report.pdf").is_file());
    assert_eq!(result.final_text, "PDF 已生成：report.pdf");
}

#[tokio::test]
async fn pdf_read_request_does_not_require_creating_a_new_pdf() {
    let ws = tmpdir("pdf_read_without_delivery");
    std::fs::write(ws.join("input.pdf"), b"%PDF-1.4\ninput\n%%EOF").unwrap();
    let provider = ScriptedProvider::new(vec![ModelResponse {
        content: "PDF 内容已读取。".into(),
        ..Default::default()
    }]);
    let mut loop_ = build(&ws, Box::new(provider));

    let result = loop_
        .run_turn(json!("读取并分析 input.pdf 的内容"), None)
        .await;

    assert_eq!(result.stop_reason, "completed");
    assert_eq!(result.iterations, 1);
}

#[tokio::test]
async fn emits_events_for_tool_turn() {
    let patch = "*** Begin Patch\n*** Add File: ev.txt\n+hi\n*** End Patch";
    let p = ScriptedProvider::new(vec![
        assistant_toolcall(vec![tc("c1", "apply_patch", json!({"patch": patch}))]),
        ModelResponse {
            content: "done".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("events");
    let mut loop_ = build(&ws, Box::new(p));
    let events = std::rc::Rc::new(RefCell::new(Vec::<LoopEvent>::new()));
    let sink = events.clone();
    loop_.set_event_sink(Box::new(move |e| sink.borrow_mut().push(e)));
    loop_.run_turn(json!("create ev.txt"), None).await;
    let evs = events.borrow();
    assert!(evs
        .iter()
        .any(|e| matches!(e, LoopEvent::ToolStart { name, .. } if name == "apply_patch")));
    assert!(evs
        .iter()
        .any(|e| matches!(e, LoopEvent::ToolResult { name, .. } if name == "apply_patch")));
    assert!(evs
        .iter()
        .any(|e| matches!(e, LoopEvent::AssistantText { text, model, .. } if text == "done" && model == "scripted")));
}

#[tokio::test]
async fn persists_reasoning_on_tool_call_turn() {
    let patch = "*** Begin Patch\n*** Add File: reasoned.txt\n+ok\n*** End Patch";
    let mut first = assistant_toolcall(vec![tc("c1", "apply_patch", json!({"patch": patch}))]);
    first.reasoning = "I need to create a file before answering.".into();
    let p = ScriptedProvider::new(vec![
        first,
        ModelResponse {
            content: "Created reasoned.txt.".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("reasoning");
    let mut loop_ = build(&ws, Box::new(p));
    let events = Rc::new(RefCell::new(Vec::<LoopEvent>::new()));
    let sink = events.clone();
    loop_.set_event_sink(Box::new(move |event| sink.borrow_mut().push(event)));
    loop_.run_turn(json!("create reasoned.txt"), None).await;
    let m = loop_
        .session
        .messages
        .iter()
        .find(|m| m["role"] == "assistant" && m.get("tool_calls").is_some())
        .unwrap();
    assert_eq!(
        m["reasoning_content"],
        "I need to create a file before answering."
    );
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        LoopEvent::ReasoningDelta(text)
            if text == "I need to create a file before answering."
    )));
}

#[tokio::test]
async fn long_history_is_automatically_compacted_before_the_next_model_call() {
    let ws = tmpdir("automatic_context_compaction");
    let provider = ScriptedProvider::new(vec![ModelResponse {
        content: "已承接压缩后的上下文。".into(),
        ..Default::default()
    }]);
    let mut loop_ = build(&ws, Box::new(provider));
    for i in 0..20 {
        loop_.session.add_user_text(&format!(
            "旧任务 {i}：生成并验证交付物 {}",
            "背景".repeat(30)
        ));
        loop_.session.add_assistant(
            &format!("旧任务 {i} 已完成，文件位于 report-{i}.pdf"),
            None,
            "",
        );
    }
    loop_.context_edit = ContextEditPolicy {
        enabled: true,
        max_chars: 900,
        keep_recent_messages: 4,
        max_tool_result_chars: 40,
    };
    let events = Rc::new(RefCell::new(Vec::<LoopEvent>::new()));
    let sink = events.clone();
    loop_.set_event_sink(Box::new(move |event| sink.borrow_mut().push(event)));
    let before = loop_.session.messages.len();

    let result = loop_.run_turn(json!("继续处理刚才的交付物"), None).await;

    assert_eq!(result.stop_reason, "completed");
    assert!(
        loop_.session.messages.len() < before,
        "超过阈值后应把压缩结果写回会话，而不是只生成临时发送视图"
    );
    let rendered = serde_json::to_string(&loop_.session.messages).unwrap();
    assert!(rendered.contains("压缩后保留的会话里程碑"));
    assert!(rendered.contains("report-"));
    assert!(events.borrow().iter().any(|event| matches!(
        event,
        LoopEvent::ContextCompacted(stats)
            if stats.dropped_messages > 0 && stats.edited_chars < stats.original_chars
    )));
}

#[tokio::test]
async fn compact_hooks_wrap_persisted_compaction_and_feed_runtime_notes() {
    let ws = tmpdir("compact_hooks");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut loop_ = build_with_hooks(
        &ws,
        Box::new(CapturingProvider { seen: seen.clone() }),
        vec![
            HookConfig {
                event: "pre_compact".into(),
                matcher: "*".into(),
                command: "echo before-compact".into(),
                timeout_s: 3,
            },
            HookConfig {
                event: "post_compact".into(),
                matcher: "*".into(),
                command: "echo after-compact".into(),
                timeout_s: 3,
            },
        ],
    );
    for i in 0..16 {
        loop_
            .session
            .add_user_text(&format!("old request {i} {}", "x".repeat(80)));
        loop_
            .session
            .add_assistant(&format!("old result {i}"), None, "");
    }
    loop_.context_edit = ContextEditPolicy {
        enabled: true,
        max_chars: 700,
        keep_recent_messages: 4,
        max_tool_result_chars: 40,
    };

    let result = loop_.run_turn(json!("continue"), None).await;

    assert_eq!(result.stop_reason, "completed");
    let messages = seen.borrow();
    let system_notes = messages
        .iter()
        .filter(|message| message["role"] == "system")
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(system_notes.contains("before-compact"), "{system_notes}");
    assert!(system_notes.contains("after-compact"), "{system_notes}");
}

#[tokio::test]
async fn runs_update_plan_and_records_state() {
    let p = ScriptedProvider::new(vec![
        {
            let mut r = assistant_toolcall(vec![tc(
                "p1",
                "update_plan",
                json!({"plan": [
                    {"step": "write file", "status": "in_progress"},
                    {"step": "verify", "status": "pending"},
                ]}),
            )]);
            r.content = "planning".into();
            r
        },
        ModelResponse {
            content: "half done".into(),
            ..Default::default()
        },
        assistant_toolcall(vec![tc(
            "p2",
            "update_plan",
            json!({"plan": [
                {"step": "write file", "status": "completed"},
                {"step": "verify", "status": "completed"},
            ]}),
        )]),
        ModelResponse {
            content: "all done".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("plan");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("two step task"), None).await;
    assert_eq!(r.stop_reason, "completed");
    assert_eq!(r.final_text, "all done");
    assert_eq!(r.iterations, 4);
    let plan = loop_.tools.ctx.plan.borrow();
    assert_eq!(plan[0]["step"], "write file");
    assert_eq!(plan[0]["status"], "completed");
    assert_eq!(plan[1]["status"], "completed");
}

#[tokio::test]
async fn unfinished_plan_from_previous_turn_does_not_block_a_new_request() {
    let p = ScriptedProvider::new(vec![
        assistant_toolcall(vec![tc(
            "old-plan",
            "update_plan",
            json!({"plan": [
                {"step": "old task", "status": "in_progress"},
            ]}),
        )]),
        ModelResponse {
            content: "new request done".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("plan_is_scoped_to_turn");
    let mut loop_ = build(&ws, Box::new(p)).with_max_iterations(2);
    let checks = Cell::new(0u32);
    let cancel_after_plan_tool = || {
        let current = checks.get();
        checks.set(current + 1);
        current >= 2
    };

    let first = loop_
        .run_turn(json!("old request"), Some(&cancel_after_plan_tool))
        .await;
    assert_eq!(first.stop_reason, "cancelled");
    assert!(
        loop_.tools.ctx.plan.borrow().is_empty(),
        "a cancelled turn must retire its plan"
    );

    let second = loop_.run_turn(json!("new request"), None).await;
    assert_eq!(second.stop_reason, "completed");
    assert_eq!(second.final_text, "new request done");
    assert_eq!(second.iterations, 1);
}

#[tokio::test]
async fn retries_an_empty_response_before_completing() {
    let p = ScriptedProvider::new(vec![
        ModelResponse::default(),
        ModelResponse {
            content: "done after retry".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("empty_response_retry");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("finish this task"), None).await;

    assert_eq!(r.stop_reason, "completed");
    assert_eq!(r.final_text, "done after retry");
    assert_eq!(r.iterations, 2);
}

#[tokio::test]
async fn retries_a_transport_error_before_completing_the_same_turn() {
    let p = ScriptedProvider::new(vec![
        ModelResponse {
            content: "RequestError: error sending request for url (https://api.deepseek.com/chat/completions)".into(),
            finish_reason: "error".into(),
            ..Default::default()
        },
        ModelResponse {
            content: "可以，我可以制作 PPT。".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("transport_error_retry");
    let mut loop_ = build(&ws, Box::new(p));

    let result = loop_.run_turn(json!("你能否做 PPT"), None).await;

    assert_eq!(result.stop_reason, "completed");
    assert_eq!(result.final_text, "可以，我可以制作 PPT。");
    assert_eq!(result.iterations, 2);
    assert!(!loop_.session.messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["content"]
                .as_str()
                .is_some_and(|text| text.contains("RequestError"))
    }));
}

#[tokio::test]
async fn retries_a_stream_decode_error_before_completing_the_same_turn() {
    let p = ScriptedProvider::new(vec![
        ModelResponse {
            content: "StreamError: error decoding response body".into(),
            finish_reason: "error".into(),
            ..Default::default()
        },
        ModelResponse {
            content: "已恢复并继续完成任务。".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("stream_decode_error_retry");
    let mut loop_ = build(&ws, Box::new(p));

    let result = loop_.run_turn(json!("继续执行当前任务"), None).await;

    assert_eq!(result.stop_reason, "completed");
    assert_eq!(result.final_text, "已恢复并继续完成任务。");
    assert_eq!(result.iterations, 2);
    assert!(!loop_.session.messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["content"]
                .as_str()
                .is_some_and(|text| text.contains("StreamError"))
    }));
}

#[tokio::test]
async fn repeated_transport_errors_end_with_a_chinese_recoverable_message() {
    let error = || {
        ModelResponse {
        content: "RequestError: error sending request for url (https://api.deepseek.com/chat/completions)".into(),
        finish_reason: "error".into(),
        ..Default::default()
    }
    };
    let p = ScriptedProvider::new(vec![
        error(),
        error(),
        error(),
        ModelResponse {
            content: "must not be reached".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("transport_error_exhausted");
    let mut loop_ = build(&ws, Box::new(p));

    let result = loop_.run_turn(json!("你能否做 PPT"), None).await;

    assert_eq!(result.stop_reason, "error");
    assert_eq!(result.iterations, 3);
    assert!(result.final_text.contains("连接模型服务失败"));
    assert!(result.final_text.contains("可以直接重试"));
    assert!(!result.final_text.contains("RequestError"));
}

#[tokio::test]
async fn stops_with_an_error_after_three_empty_responses() {
    let p = ScriptedProvider::new(vec![
        ModelResponse::default(),
        ModelResponse::default(),
        ModelResponse::default(),
        ModelResponse {
            content: "must not be reached".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("repeated_empty_response");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("finish this task"), None).await;

    assert_eq!(r.stop_reason, "error");
    assert_eq!(r.iterations, 3);
    assert!(!r.final_text.trim().is_empty());
}

#[tokio::test]
async fn update_plan_cannot_drop_an_unfinished_step() {
    let ws = tmpdir("plan_keeps_unfinished_steps");
    let loop_ = build(&ws, Box::new(ScriptedProvider::new(vec![])));
    let first = loop_
        .tools
        .execute_with_recovery(
            "update_plan",
            &json!({"plan": [
                {"step": "generate PDF", "status": "in_progress"},
            ]}),
        )
        .await;
    assert!(!first.starts_with("Error:"), "{first}");

    let replacement = loop_
        .tools
        .execute_with_recovery(
            "update_plan",
            &json!({"plan": [
                {"step": "write Markdown", "status": "completed"},
            ]}),
        )
        .await;

    assert!(replacement.starts_with("Error:"), "{replacement}");
    let plan = loop_.tools.ctx.plan.borrow();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0]["step"], "generate PDF");
    assert_eq!(plan[0]["status"], "in_progress");
}

#[tokio::test]
async fn stops_at_max_iterations() {
    let looping: Vec<ModelResponse> = (0..20)
        .map(|i| {
            assistant_toolcall(vec![tc(
                &format!("c{i}"),
                "read_file",
                json!({"path": "nope.txt"}),
            )])
        })
        .collect();
    let p = ScriptedProvider::new(looping);
    let ws = tmpdir("maxiter");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("loop forever"), None).await;
    assert_eq!(r.stop_reason, "task_budget");
    assert_eq!(r.iterations, 10);
}
