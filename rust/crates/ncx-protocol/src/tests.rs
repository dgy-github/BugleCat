use super::*;

#[test]
fn event_round_trip_preserves_thread_and_turn_ownership() {
    let event = EventEnvelope::new(
        7,
        ThreadId::new("thread-1").unwrap(),
        Some(TurnId::new("turn-2").unwrap()),
        Event::TurnStarted {
            status: TurnStatus::Running,
        },
    );
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(serde_json::from_str::<EventEnvelope>(&json).unwrap(), event);
    assert!(json.contains("\"threadId\":\"thread-1\""));
    assert!(json.contains("\"turnId\":\"turn-2\""));
}

#[test]
fn durable_ids_reject_empty_values() {
    assert!(ThreadId::new("  ").is_err());
    assert!(TurnId::new("").is_err());
    assert!(ItemId::new("item").is_ok());
    assert!(GoalId::new("  ").is_err());
}

#[test]
fn legacy_turn_and_submit_default_to_agent_mode() {
    let turn: Turn = serde_json::from_value(serde_json::json!({
        "id": "turn-legacy", "status": "completed", "items": [],
        "startedAt": 1, "completedAt": 2, "error": null, "usage": {}
    }))
    .unwrap();
    assert_eq!(turn.execution_mode, ExecutionMode::Agent);

    let request: ClientRequest = serde_json::from_value(serde_json::json!({
        "method": "turnSubmit",
        "params": { "threadId": "thread-legacy", "text": "hello", "images": [] }
    }))
    .unwrap();
    assert!(matches!(
        request,
        ClientRequest::TurnSubmit {
            execution_mode: ExecutionMode::Agent,
            ..
        }
    ));
}

fn completed_thread(items: Vec<ThreadItem>) -> Thread {
    Thread {
        metadata: ThreadMetadata {
            id: ThreadId::new("thread-1").unwrap(),
            workspace: "workspace".into(),
            title: "title".into(),
            archived: false,
            harness_profile: "full".into(),
            created_at: 1,
            updated_at: 2,
        },
        turns: vec![Turn {
            id: TurnId::new("turn-1").unwrap(),
            status: TurnStatus::Completed,
            execution_mode: ExecutionMode::Agent,
            items,
            started_at: 1,
            completed_at: Some(2),
            error: None,
            usage: TurnUsage::default(),
        }],
    }
}

#[test]
fn visible_projection_keeps_each_request_and_only_the_final_answer() {
    let visible = completed_thread(vec![
        ThreadItem::UserMessage {
            id: ItemId::new("u").unwrap(),
            text: "request".into(),
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("a1").unwrap(),
            text: "progress".into(),
            model: None,
            confirmed_model: None,
        },
        ThreadItem::GoalMessage {
            id: ItemId::new("goal-message").unwrap(),
            text: "hidden automatic continuation".into(),
            goal_id: GoalId::new("goal").unwrap(),
            revision: 2,
            round: 1,
        },
        ThreadItem::ToolResult {
            id: ItemId::new("r").unwrap(),
            call_id: ItemId::new("c").unwrap(),
            output: "secret".into(),
            success: true,
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("a2").unwrap(),
            text: "done".into(),
            model: Some("requested-model".into()),
            confirmed_model: Some("confirmed-model".into()),
        },
    ])
    .into_visible();
    assert_eq!(visible.turns[0].items.len(), 2);
    assert!(
        matches!(&visible.turns[0].items[1], ThreadItem::AssistantMessage { text, model: Some(model), confirmed_model: Some(confirmed), .. } if text == "done" && model == "requested-model" && confirmed == "confirmed-model")
    );
}

#[test]
fn visible_projection_preserves_generated_artifacts() {
    let visible = completed_thread(vec![
        ThreadItem::UserMessage {
            id: ItemId::new("u").unwrap(),
            text: "生成图片".into(),
        },
        ThreadItem::Artifact {
            id: ItemId::new("artifact-1").unwrap(),
            kind: "image".into(),
            name: "生成图片 1".into(),
            url: "https://example.com/image.png".into(),
        },
        ThreadItem::AssistantMessage {
            id: ItemId::new("a").unwrap(),
            text: "完成".into(),
            model: None,
            confirmed_model: None,
        },
    ])
    .into_visible();
    assert!(visible.turns[0].items.iter().any(|item| matches!(item, ThreadItem::Artifact { kind, url, .. } if kind == "image" && url == "https://example.com/image.png")));
}

fn assert_round_trip(request: ClientRequest, expected: &[&str]) {
    let json = serde_json::to_string(&request).unwrap();
    for fragment in expected {
        assert!(json.contains(fragment), "{json}");
    }
    assert_eq!(
        serde_json::from_str::<ClientRequest>(&json).unwrap(),
        request
    );
}

#[test]
fn client_request_and_item_fields_use_frontend_camel_case() {
    assert_round_trip(
        ClientRequest::GoalRoundStart {
            thread_id: ThreadId::new("thread-goal").unwrap(),
            turn_id: TurnId::new("turn-goal").unwrap(),
            goal: GoalRef {
                id: GoalId::new("goal-1").unwrap(),
                revision: 4,
            },
            round: 3,
            prompt: "continue".into(),
        },
        &[
            "\"method\":\"goalRoundStart\"",
            "\"turnId\":\"turn-goal\"",
            "\"round\":3",
        ],
    );
    assert_round_trip(
        ClientRequest::GoalEdit {
            thread_id: ThreadId::new("thread-goal").unwrap(),
            goal: GoalRef {
                id: GoalId::new("goal-1").unwrap(),
                revision: 4,
            },
            objective: "ship safely".into(),
            max_goal_rounds: 12,
        },
        &[
            "\"method\":\"goalEdit\"",
            "\"goal\":{\"id\":\"goal-1\",\"revision\":4}",
            "\"maxGoalRounds\":12",
        ],
    );
    assert_round_trip(
        ClientRequest::ThreadHarnessProfileSet {
            thread_id: ThreadId::new("thread-profile").unwrap(),
            harness_profile: "coding".into(),
        },
        &[
            "\"method\":\"threadHarnessProfileSet\"",
            "\"harnessProfile\":\"coding\"",
        ],
    );
    assert_round_trip(
        ClientRequest::ItemAppend {
            thread_id: ThreadId::new("thread-1").unwrap(),
            turn_id: TurnId::new("turn-1").unwrap(),
            item: ThreadItem::ToolResult {
                id: ItemId::new("result-1").unwrap(),
                call_id: ItemId::new("call-1").unwrap(),
                output: "ok".into(),
                success: true,
            },
        },
        &["\"threadId\"", "\"turnId\"", "\"callId\""],
    );
    assert_round_trip(
        ClientRequest::MarketplacePluginInstall {
            marketplace_path: "marketplace.json".into(),
            plugin_name: "demo".into(),
            upgrade: true,
        },
        &[
            "\"method\":\"marketplacePluginInstall\"",
            "\"marketplacePath\"",
            "\"pluginName\"",
        ],
    );
    assert_round_trip(
        ClientRequest::DshMarketplaceSearch {
            source: "dshfind".into(),
            manifest_url: None,
            query: "memory".into(),
        },
        &[
            "\"method\":\"dshMarketplaceSearch\"",
            "\"manifestUrl\":null",
        ],
    );
    assert_round_trip(
        ClientRequest::InteractionAnswer {
            thread_id: Some(ThreadId::new("thread-2").unwrap()),
            id: 9,
            answer: Some("继续".into()),
        },
        &[
            "\"method\":\"interactionAnswer\"",
            "\"threadId\":\"thread-2\"",
        ],
    );
    assert_round_trip(
        ClientRequest::SettingsUpdate {
            updates: BTreeMap::from([("reasoning_effort".into(), "high".into())]),
        },
        &[
            "\"method\":\"settingsUpdate\"",
            "\"reasoning_effort\":\"high\"",
        ],
    );
}

#[test]
fn legacy_thread_metadata_defaults_harness_profile_to_full() {
    let metadata: ThreadMetadata = serde_json::from_value(serde_json::json!({
        "id": "legacy-thread",
        "workspace": "workspace",
        "title": "legacy",
        "archived": false,
        "createdAt": 1,
        "updatedAt": 2
    }))
    .unwrap();
    assert_eq!(metadata.harness_profile, "full");
}
