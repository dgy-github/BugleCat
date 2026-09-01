use super::*;

#[tokio::test]
async fn model_receives_the_windows_shell_contract() {
    let ws = tmpdir("windows_shell_contract");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut loop_ = build(&ws, Box::new(CapturingProvider { seen: seen.clone() }));

    loop_.run_turn(json!("inspect files"), None).await;

    assert!(seen.borrow().iter().any(|message| {
        message["role"] == "system"
            && message["content"]
                .as_str()
                .unwrap_or_default()
                .contains("Windows cmd.exe")
    }));
}

#[tokio::test]
async fn repeated_tool_failures_force_a_final_answer_without_more_tools() {
    let ws = tmpdir("long_failure_convergence");
    let converged_without_tools = Rc::new(Cell::new(false));
    let saw_convergence_note = Rc::new(Cell::new(false));
    let provider = LongFailureProvider {
        calls: Cell::new(0),
        converged_without_tools: converged_without_tools.clone(),
        saw_convergence_note: saw_convergence_note.clone(),
    };
    let mut loop_ = build(&ws, Box::new(provider)).with_max_iterations(50);

    let result = loop_.run_turn(json!("research carefully"), None).await;

    assert_eq!(result.stop_reason, "completed");
    assert!(converged_without_tools.get());
    assert!(saw_convergence_note.get());
    assert_eq!(result.tools_used.len(), 40);
}

#[tokio::test]
async fn long_chain_convergence_keeps_tools_until_pdf_is_delivered() {
    let ws = tmpdir("long_pdf_delivery");
    let tools_remained_available = Rc::new(Cell::new(false));
    let provider = LongPdfDeliveryProvider {
        calls: Cell::new(0),
        tools_remained_available: tools_remained_available.clone(),
    };
    let mut loop_ = build(&ws, Box::new(provider)).with_max_iterations(50);

    let result = loop_
        .run_turn(json!("调研完成后生成并验证 PDF"), None)
        .await;

    assert_eq!(result.stop_reason, "completed");
    assert!(tools_remained_available.get());
    assert!(ws.join("final.pdf").is_file());
    assert_eq!(result.tools_used.len(), 41);
}

#[tokio::test]
async fn memory_recall_is_sent_as_query_scoped_system_note() {
    let ws = tmpdir("memory_recall_note");
    let memory = Rc::new(crate::memory::MemoryStore::new(
        ws.join(".ncx").join("memory"),
    ));
    memory
        .remember("Use the GNU target for Windows release builds.", &[], 1)
        .unwrap();
    memory
        .remember("The storyboard panel renders thumbnails.", &[], 2)
        .unwrap();

    let seen = Rc::new(RefCell::new(Vec::new()));
    let policy = SandboxPolicy::new(WORKSPACE_WRITE, &ws);
    let ctx = ToolContext::new(ws.clone(), policy).with_memory(memory);
    let tools = ToolRegistry::new(ctx);
    let session = Session::new("system prompt");
    let mut loop_ = AgentLoop::new(
        Box::new(CapturingProvider { seen: seen.clone() }),
        tools,
        session,
    );

    let r = loop_
        .run_turn(json!("fix the Windows build target"), None)
        .await;

    assert_eq!(r.stop_reason, "completed");
    let messages = seen.borrow();
    let note = messages
        .iter()
        .find(|m| {
            m["role"] == "system"
                && m["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("[memory recall for this prompt]")
        })
        .expect("query-scoped memory recall note is sent");
    assert!(note["content"]
        .as_str()
        .unwrap_or("")
        .contains("GNU target"));
    // Runtime note only — never persisted into the session history.
    assert!(!loop_.session.messages.iter().any(|m| {
        m["content"]
            .as_str()
            .unwrap_or("")
            .contains("[memory recall for this prompt]")
    }));
}

#[tokio::test]
async fn registered_context_provider_is_query_scoped_and_reversible() {
    let ws = tmpdir("context_provider");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut loop_ = build(&ws, Box::new(CapturingProvider { seen: seen.clone() }));
    loop_
        .register_context_provider(Rc::new(StaticContextProvider))
        .unwrap();

    loop_.run_turn(json!("first question"), None).await;
    assert!(seen.borrow().iter().any(|message| {
        message["role"] == "system"
            && message["content"]
                .as_str()
                .unwrap_or_default()
                .contains("[plugin context]\nquery=first question")
    }));

    assert!(loop_.unregister_context_provider("static-test-context"));
    loop_.run_turn(json!("second question"), None).await;
    assert!(!seen.borrow().iter().any(|message| {
        message["role"] == "system"
            && message["content"]
                .as_str()
                .unwrap_or_default()
                .contains("query=second question")
    }));
}

#[tokio::test]
async fn primary_provider_can_be_replaced_without_rebuilding_runtime_state() {
    let ws = tmpdir("replace_provider");
    let calls = Rc::new(Cell::new(0));
    let mut loop_ = build(&ws, Box::new(ScriptedProvider::new(Vec::new())));
    loop_.session.add_assistant("retained", None, "");

    let previous = loop_.replace_provider(Box::new(CountingProvider {
        calls: calls.clone(),
    }));

    assert_eq!(previous.model(), "scripted");
    assert_eq!(loop_.provider_model(), "counting");
    loop_.run_turn(json!("continue"), None).await;
    assert_eq!(calls.get(), 1);
    assert!(loop_
        .session
        .messages
        .iter()
        .any(|message| message["content"] == "retained"));
}

#[tokio::test]
async fn task_budget_is_visible_to_model() {
    let ws = tmpdir("budget_note");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut loop_ = build(&ws, Box::new(CapturingProvider { seen: seen.clone() }))
        .with_task_budget(TaskBudget {
            max_model_calls: 3,
            max_tool_calls: 4,
        });
    let r = loop_.run_turn(json!("do it"), None).await;
    assert_eq!(r.stop_reason, "completed");
    let messages = seen.borrow();
    assert!(messages.iter().any(|m| {
        m["role"] == "system"
            && m["content"]
                .as_str()
                .unwrap_or("")
                .contains("Runtime task budget")
            && m["content"]
                .as_str()
                .unwrap_or("")
                .contains("tool_calls 0/4")
    }));
}

#[tokio::test]
async fn user_prompt_hook_can_block_model_call() {
    let ws = tmpdir("user_prompt_block");
    let calls = Rc::new(Cell::new(0usize));
    let mut loop_ = build_with_hooks(
        &ws,
        Box::new(CountingProvider {
            calls: calls.clone(),
        }),
        vec![HookConfig {
            event: "user_prompt".into(),
            matcher: "*".into(),
            command: "exit 1".into(),
            timeout_s: 20,
        }],
    );

    let r = loop_.run_turn(json!("blocked"), None).await;

    assert_eq!(r.stop_reason, "blocked");
    assert_eq!(calls.get(), 0);
    assert!(r.final_text.contains("blocked by user_prompt hook"));
}

#[tokio::test]
async fn user_prompt_hook_output_is_sent_as_system_note() {
    let ws = tmpdir("user_prompt_note");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut loop_ = build_with_hooks(
        &ws,
        Box::new(CapturingProvider { seen: seen.clone() }),
        vec![HookConfig {
            event: "user_prompt".into(),
            matcher: "*".into(),
            command: "echo prompt-note".into(),
            timeout_s: 20,
        }],
    );

    let r = loop_.run_turn(json!("continue"), None).await;

    assert_eq!(r.stop_reason, "completed");
    let messages = seen.borrow();
    assert!(messages.iter().any(|m| {
        m["role"] == "system" && m["content"].as_str().unwrap_or("").contains("prompt-note")
    }));
}

#[tokio::test]
async fn stop_hook_output_is_appended_to_final_text() {
    let ws = tmpdir("stop_hook_note");
    let mut loop_ = build_with_hooks(
        &ws,
        Box::new(ScriptedProvider::new(vec![ModelResponse {
            content: "done".into(),
            ..Default::default()
        }])),
        vec![HookConfig {
            event: "stop".into(),
            matcher: "*".into(),
            command: "echo stop-ok".into(),
            timeout_s: 3,
        }],
    );

    let r = loop_.run_turn(json!("finish"), None).await;

    assert_eq!(r.stop_reason, "completed");
    assert!(r.final_text.contains("stop-ok"));
    assert!(loop_.session.messages.iter().any(
        |m| m["role"] == "assistant" && m["content"].as_str().unwrap_or("").contains("stop-ok")
    ));
}

#[tokio::test]
async fn tool_budget_stops_and_backfills_unanswered_calls() {
    let p = ScriptedProvider::new(vec![assistant_toolcall(vec![
        tc("r1", "read_file", json!({"path": "none1.txt"})),
        tc("r2", "read_file", json!({"path": "none2.txt"})),
        tc("r3", "read_file", json!({"path": "none3.txt"})),
    ])]);
    let ws = tmpdir("tool_budget");
    let mut loop_ = build(&ws, Box::new(p)).with_task_budget(TaskBudget {
        max_model_calls: 5,
        max_tool_calls: 2,
    });
    let r = loop_.run_turn(json!("read three files"), None).await;
    assert_eq!(r.stop_reason, "task_budget");
    assert_eq!(r.tools_used.len(), 2);
    assert!(answered(&loop_.session.messages));
    assert!(loop_.session.messages.iter().any(|m| {
        m["role"] == "tool"
            && m["tool_call_id"] == "r3"
            && m["content"]
                .as_str()
                .unwrap_or("")
                .contains("task budget exhausted")
    }));
}

#[tokio::test]
async fn cancel_mid_tool_loop_backfills_tool_results() {
    let p = ScriptedProvider::new(vec![assistant_toolcall(vec![
        tc("c1", "read_file", json!({"path": "a.txt"})),
        tc("c2", "read_file", json!({"path": "b.txt"})),
    ])]);
    let ws = tmpdir("cancelmid");
    let mut loop_ = build(&ws, Box::new(p));
    let n = Cell::new(0u32);
    let check = move || {
        let v = n.get();
        n.set(v + 1);
        v >= 1
    };
    let r = loop_.run_turn(json!("read two files"), Some(&check)).await;
    assert_eq!(r.stop_reason, "cancelled");
    assert!(answered(&loop_.session.messages));
    let ids: std::collections::HashSet<&str> = loop_
        .session
        .messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["tool_call_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["c1", "c2"].into_iter().collect());
}

#[tokio::test]
async fn image_turn_routes_to_vision_provider() {
    let main = ScriptedProvider::new(vec![ModelResponse {
        content: "text reply".into(),
        ..Default::default()
    }]);
    let vision = ScriptedProvider::new(vec![ModelResponse {
        content: "vision reply: I see a cat".into(),
        ..Default::default()
    }]);
    let ws = tmpdir("vision");
    let mut loop_ = build(&ws, Box::new(main));
    loop_.vision_provider = Some(Box::new(vision));
    let content = json!([
        {"type": "text", "text": "what's in this image?"},
        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
    ]);
    let r = loop_.run_turn(content, None).await;
    assert_eq!(r.stop_reason, "completed");
    assert_eq!(r.final_text, "vision reply: I see a cat");
}

#[tokio::test]
async fn read_only_calls_run_concurrently() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct SlowReadTool {
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
    }
    #[async_trait(?Send)]
    impl Tool for SlowReadTool {
        fn name(&self) -> &str {
            "slow_read"
        }
        fn description(&self) -> &str {
            "sleeps (test)"
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {"i": {"type": "integer"}}})
        }
        fn read_only(&self) -> bool {
            true
        }
        async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(300)).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            format!(
                "read {}",
                args.get("i").and_then(|v| v.as_i64()).unwrap_or(-1)
            )
        }
    }

    let p = ScriptedProvider::new(vec![
        assistant_toolcall(
            (0..4)
                .map(|i| tc(&format!("c{i}"), "slow_read", json!({"i": i})))
                .collect(),
        ),
        ModelResponse {
            content: "done".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("concurrent");
    let mut loop_ = build(&ws, Box::new(p));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    loop_.tools.register(Box::new(SlowReadTool {
        active,
        peak: peak.clone(),
    }));

    let r = loop_.run_turn(json!("read four things"), None).await;
    assert_eq!(r.stop_reason, "completed");
    assert!(peak.load(Ordering::SeqCst) >= 2);
    let ids: Vec<&str> = loop_
        .session
        .messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["tool_call_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["c0", "c1", "c2", "c3"]);
}

#[tokio::test]
async fn custom_scheduler_receives_read_batches_and_serial_barriers() {
    let patch = "*** Begin Patch\n*** Add File: scheduled.txt\n+ok\n*** End Patch";
    let provider = ScriptedProvider::new(vec![
        assistant_toolcall(vec![
            tc("r1", "read_file", json!({"path": "missing-1.txt"})),
            tc("r2", "read_file", json!({"path": "missing-2.txt"})),
            tc("w1", "apply_patch", json!({"patch": patch})),
        ]),
        ModelResponse {
            content: "done".into(),
            ..Default::default()
        },
    ]);
    let serial_calls = Rc::new(Cell::new(0));
    let read_batches = Rc::new(Cell::new(0));
    let scheduler = RecordingScheduler {
        serial_calls: serial_calls.clone(),
        read_batches: read_batches.clone(),
    };
    let ws = tmpdir("custom_scheduler");
    let mut loop_ = build(&ws, Box::new(provider)).with_tool_scheduler(Box::new(scheduler));

    let result = loop_.run_turn(json!("run scheduled calls"), None).await;

    assert_eq!(result.stop_reason, "completed");
    assert_eq!(read_batches.get(), 1);
    assert_eq!(serial_calls.get(), 1);
    assert_eq!(
        std::fs::read_to_string(ws.join("scheduled.txt")).unwrap(),
        "ok\n"
    );
}

#[tokio::test]
async fn write_between_reads_stays_serial_and_ordered() {
    let patch = "*** Begin Patch\n*** Add File: mid.txt\n+x\n*** End Patch";
    let p = ScriptedProvider::new(vec![
        assistant_toolcall(vec![
            tc("r1", "read_file", json!({"path": "none1.txt"})),
            tc("w1", "apply_patch", json!({"patch": patch})),
            tc("r2", "read_file", json!({"path": "none2.txt"})),
        ]),
        ModelResponse {
            content: "done".into(),
            ..Default::default()
        },
    ]);
    let ws = tmpdir("serial");
    let mut loop_ = build(&ws, Box::new(p));
    let r = loop_.run_turn(json!("read write read"), None).await;
    assert_eq!(r.stop_reason, "completed");
    assert_eq!(std::fs::read_to_string(ws.join("mid.txt")).unwrap(), "x\n");
    let ids: Vec<&str> = loop_
        .session
        .messages
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["tool_call_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["r1", "w1", "r2"]);
    assert_eq!(r.tools_used, vec!["read_file", "apply_patch", "read_file"]);
}

#[tokio::test]
async fn stop_interrupts_a_hanging_tool() {
    struct HangingTool;
    #[async_trait(?Send)]
    impl Tool for HangingTool {
        fn name(&self) -> &str {
            "hang"
        }
        fn description(&self) -> &str {
            "blocks forever (test)"
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _ctx: &ToolContext, _args: &Value) -> String {
            std::future::pending::<()>().await;
            "unreachable".into()
        }
    }

    let p = ScriptedProvider::new(vec![assistant_toolcall(vec![tc("h1", "hang", json!({}))])]);
    let ws = tmpdir("hang");
    let mut loop_ = build(&ws, Box::new(p));
    loop_.tools.register(Box::new(HangingTool));

    let n = Cell::new(0u32);
    let check = move || {
        let v = n.get();
        n.set(v + 1);
        v >= 2
    };
    let r = tokio::time::timeout(
        Duration::from_secs(5),
        loop_.run_turn(json!("do the hang"), Some(&check)),
    )
    .await
    .expect("must finish under 5s");
    assert_eq!(r.stop_reason, "cancelled");
    assert!(loop_
        .session
        .messages
        .iter()
        .any(|m| m["role"] == "tool" && m["tool_call_id"] == "h1"));
}

#[tokio::test]
async fn stop_interrupts_a_hanging_model_request() {
    struct HangingProvider;
    #[async_trait(?Send)]
    impl Provider for HangingProvider {
        fn model(&self) -> &str {
            "hanging"
        }

        async fn chat(&self, _m: &[Value], _t: &[Value], _r: Option<&str>) -> ModelResponse {
            std::future::pending::<ModelResponse>().await
        }
    }

    let ws = tmpdir("hanging_model");
    let mut loop_ = build(&ws, Box::new(HangingProvider));
    let checks = Cell::new(0u32);
    let cancel = || {
        checks.set(checks.get() + 1);
        checks.get() >= 3
    };

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        loop_.run_turn(json!("wait forever"), Some(&cancel)),
    )
    .await
    .expect("cancel must drop the hanging model request");

    assert_eq!(result.stop_reason, "cancelled");
    assert_eq!(result.final_text, "Stopped by user.");
}
