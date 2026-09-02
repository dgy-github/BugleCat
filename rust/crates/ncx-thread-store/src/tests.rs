use super::*;
use ncx_protocol::{ItemId, ThreadItem};
use std::fs;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_store(name: &str) -> JsonThreadStore {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    JsonThreadStore::open(std::env::temp_dir().join(format!("ncx-{name}-{unique}.json"))).unwrap()
}

fn thread(id: &str) -> Thread {
    Thread {
        metadata: ThreadMetadata {
            id: ThreadId::new(id).unwrap(),
            workspace: "workspace".into(),
            title: "title".into(),
            archived: false,
            harness_profile: "full".into(),
            created_at: 1,
            updated_at: 1,
        },
        turns: Vec::new(),
    }
}

fn goal(id: &str, revision: u64, objective: &str) -> GoalSnapshot {
    GoalSnapshot {
        id: ncx_protocol::GoalId::new(id).unwrap(),
        revision,
        objective: objective.into(),
        phase: ncx_protocol::GoalPhase::Active,
        blocked_reason: None,
        max_goal_rounds: 8,
        rounds_started: 0,
        created_at: 10,
        updated_at: 10 + revision as i64,
    }
}

fn write_epoch(store: &JsonThreadStore, id: &ThreadId) -> u64 {
    store
        .inspect(|persisted| {
            persisted
                .runtime_activation_epochs
                .get(id.as_str())
                .copied()
                .unwrap_or_default()
        })
        .unwrap()
}

#[test]
fn goal_compare_and_set_is_durable_and_rejects_stale_revision_without_writing() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-goal-cas-{unique}.json"));
    let id = ThreadId::new("thread").unwrap();
    let store = JsonThreadStore::open(&path).unwrap();
    store.create(thread("thread")).unwrap();
    let first = goal("goal", 1, "first");
    assert_eq!(
        store
            .compare_and_set_goal(&id, GoalExpectation::Absent, Some(first.clone()))
            .unwrap(),
        Some(first.clone())
    );

    let second = goal("goal", 2, "second");
    store
        .compare_and_set_goal(
            &id,
            GoalExpectation::Exact(GoalRef {
                id: first.id.clone(),
                revision: 1,
            }),
            Some(second.clone()),
        )
        .unwrap();
    let before_stale = fs::read(&path).unwrap();
    let stale = store.compare_and_set_goal(
        &id,
        GoalExpectation::Exact(GoalRef {
            id: first.id,
            revision: 1,
        }),
        Some(goal("goal", 3, "must not persist")),
    );
    assert!(matches!(stale, Err(ThreadStoreError::StaleGoal { .. })));
    assert_eq!(fs::read(&path).unwrap(), before_stale);

    let reopened = JsonThreadStore::open(&path).unwrap();
    assert_eq!(reopened.read_goal(&id).unwrap(), Some(second));
}

#[test]
fn fork_copies_durable_goal_snapshot() {
    let store = temp_store("goal-fork");
    let source = ThreadId::new("source").unwrap();
    let target = ThreadId::new("target").unwrap();
    store.create(thread("source")).unwrap();
    let snapshot = goal("goal", 3, "continue after fork only when re-armed");
    store
        .compare_and_set_goal(&source, GoalExpectation::Absent, Some(snapshot.clone()))
        .unwrap();
    store.fork(&source, target.clone()).unwrap();
    assert_eq!(store.read_goal(&target).unwrap(), Some(snapshot));
}

#[test]
fn legacy_store_without_goal_map_opens_with_no_goal() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-goal-legacy-{unique}.json"));
    let store = JsonThreadStore::open(&path).unwrap();
    store.create(thread("legacy")).unwrap();
    drop(store);
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    json.as_object_mut().unwrap().remove("goals");
    fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let reopened = JsonThreadStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .read_goal(&ThreadId::new("legacy").unwrap())
            .unwrap(),
        None
    );
}

#[test]
fn read_with_goal_returns_the_thread_and_goal_from_one_persisted_snapshot() {
    let store = temp_store("read-with-goal");
    let thread_id = ThreadId::new("thread").unwrap();
    store.create(thread("thread")).unwrap();

    // A Thread can exist without a Goal; the tuple still has to be present so
    // callers can distinguish that state from a missing Thread.
    let (stored_thread, stored_goal) = store
        .read_with_goal(&thread_id)
        .unwrap()
        .expect("thread should be returned");
    assert_eq!(stored_thread, thread("thread"));
    assert_eq!(stored_goal, None);

    let first = goal("goal-a", 1, "first objective");
    store
        .compare_and_set_goal(&thread_id, GoalExpectation::Absent, Some(first.clone()))
        .unwrap();
    let (stored_thread, stored_goal) = store
        .read_with_goal(&thread_id)
        .unwrap()
        .expect("thread with goal should be returned");
    assert_eq!(stored_thread.metadata.id, thread_id);
    assert_eq!(stored_goal, Some(first.clone()));

    let second = goal("goal-b", 1, "replacement objective");
    store
        .compare_and_set_goal(
            &thread_id,
            GoalExpectation::Exact(GoalRef {
                id: first.id,
                revision: first.revision,
            }),
            Some(second.clone()),
        )
        .unwrap();
    let (_, stored_goal) = store
        .read_with_goal(&thread_id)
        .unwrap()
        .expect("replacement thread should be returned");
    assert_eq!(stored_goal, Some(second));

    let missing = ThreadId::new("missing").unwrap();
    assert_eq!(store.read_with_goal(&missing).unwrap(), None);
}

fn turn(id: &str) -> Turn {
    Turn {
        id: TurnId::new(id).unwrap(),
        status: TurnStatus::Running,
        execution_mode: ncx_protocol::ExecutionMode::Agent,
        items: Vec::new(),
        started_at: 2,
        completed_at: None,
        error: None,
        usage: TurnUsage::default(),
    }
}

fn goal_turn(id: &str, snapshot: &GoalSnapshot, round: u32) -> Turn {
    Turn {
        items: vec![ThreadItem::GoalMessage {
            id: ItemId::new(format!("message-{id}")).unwrap(),
            text: format!("continue round {round}"),
            goal_id: snapshot.id.clone(),
            revision: snapshot.revision,
            round,
        }],
        ..turn(id)
    }
}

#[test]
fn goal_round_admission_atomically_claims_turn_and_increments_counter() {
    let store = temp_store("goal-round");
    let thread_id = ThreadId::new("thread").unwrap();
    store.create(thread("thread")).unwrap();
    let snapshot = goal("goal", 2, "continue");
    store
        .compare_and_set_goal(&thread_id, GoalExpectation::Absent, Some(snapshot.clone()))
        .unwrap();
    let admitted = store
        .claim_goal_round(
            &thread_id,
            GoalRef {
                id: snapshot.id.clone(),
                revision: snapshot.revision,
            },
            1,
            goal_turn("round-1", &snapshot, 1),
        )
        .unwrap();
    assert_eq!(admitted.rounds_started, 1);
    let stored = store.read(&thread_id).unwrap().unwrap();
    assert_eq!(stored.turns.len(), 1);
    assert!(matches!(
        stored.turns[0].items.as_slice(),
        [ThreadItem::GoalMessage { round: 1, .. }]
    ));
    assert_eq!(
        store.read_goal(&thread_id).unwrap().unwrap().rounds_started,
        1
    );
}

#[test]
fn rejected_goal_round_changes_neither_turn_nor_counter() {
    let store = temp_store("goal-round-reject");
    let thread_id = ThreadId::new("thread").unwrap();
    store.create(thread("thread")).unwrap();
    let snapshot = goal("goal", 2, "continue");
    store
        .compare_and_set_goal(&thread_id, GoalExpectation::Absent, Some(snapshot.clone()))
        .unwrap();
    let result = store.claim_goal_round(
        &thread_id,
        GoalRef {
            id: snapshot.id.clone(),
            revision: snapshot.revision,
        },
        2,
        goal_turn("skipped", &snapshot, 2),
    );
    assert!(matches!(result, Err(ThreadStoreError::InvalidGoalRound(_))));
    assert!(store.read(&thread_id).unwrap().unwrap().turns.is_empty());
    assert_eq!(store.read_goal(&thread_id).unwrap(), Some(snapshot));
}

#[test]
fn one_thread_accepts_only_one_active_turn() {
    let store = temp_store("ownership");
    let id = ThreadId::new("thread").unwrap();
    store.create(thread("thread")).unwrap();
    store.claim_turn(&id, turn("turn-1")).unwrap();
    assert!(matches!(
        store.claim_turn(&id, turn("turn-2")),
        Err(ThreadStoreError::Busy { .. })
    ));
    store
        .finish_turn(
            &id,
            &TurnId::new("turn-1").unwrap(),
            TurnStatus::Completed,
            3,
            None,
            TurnUsage::default(),
        )
        .unwrap();
    store.claim_turn(&id, turn("turn-2")).unwrap();
}

#[test]
fn harness_profile_update_rechecks_turns_inside_the_atomic_store_transaction() {
    let store = temp_store("profile-first-turn-race");
    let id = ThreadId::new("thread").unwrap();
    store.create(thread("thread")).unwrap();

    // This represents the caller's earlier validation/read. A first TurnStart
    // may complete before the caller reaches the profile write, so the store
    // must check the durable state again inside its mutation transaction.
    let observed = store.read(&id).unwrap().unwrap();
    assert!(observed.turns.is_empty());
    store.claim_turn(&id, turn("first-turn")).unwrap();

    assert_eq!(
        store
            .set_harness_profile_if_idle(&id, "readonly".into(), 3)
            .unwrap(),
        None
    );
    let stored = store.read(&id).unwrap().unwrap();
    assert_eq!(stored.metadata.harness_profile, "full");
    assert_eq!(stored.turns.len(), 1);
}

#[test]
fn different_threads_hold_active_turns_concurrently() {
    let store = temp_store("parallel-ownership");
    let first = ThreadId::new("first").unwrap();
    let second = ThreadId::new("second").unwrap();
    store.create(thread("first")).unwrap();
    store.create(thread("second")).unwrap();
    store.claim_turn(&first, turn("turn-first")).unwrap();
    store.claim_turn(&second, turn("turn-second")).unwrap();
    assert_eq!(
        store.read(&first).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
    assert_eq!(
        store.read(&second).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
}

#[test]
fn items_are_owned_by_the_claimed_turn_and_persisted() {
    let store = temp_store("items");
    let thread_id = ThreadId::new("thread").unwrap();
    let turn_id = TurnId::new("turn").unwrap();
    store.create(thread("thread")).unwrap();
    store.claim_turn(&thread_id, turn("turn")).unwrap();
    store
        .append_item(
            &thread_id,
            &turn_id,
            ThreadItem::UserMessage {
                id: ItemId::new("item").unwrap(),
                text: "hello".into(),
            },
            3,
        )
        .unwrap();
    assert_eq!(
        store.read(&thread_id).unwrap().unwrap().turns[0]
            .items
            .len(),
        1
    );
}

#[test]
fn fork_keeps_history_but_changes_durable_identity() {
    let store = temp_store("fork");
    store.create(thread("source")).unwrap();
    store
        .claim_turn(&ThreadId::new("source").unwrap(), turn("running"))
        .unwrap();
    let forked = store
        .fork(
            &ThreadId::new("source").unwrap(),
            ThreadId::new("target").unwrap(),
        )
        .unwrap();
    assert_eq!(forked.metadata.id.as_str(), "target");
    assert_eq!(forked.turns[0].status, TurnStatus::Cancelled);
    assert!(store
        .read(&ThreadId::new("source").unwrap())
        .unwrap()
        .is_some());
}

#[test]
fn completed_turn_survives_store_reopen() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-reopen-{unique}.json"));
    let thread_id = ThreadId::new("thread").unwrap();
    let turn_id = TurnId::new("turn").unwrap();
    {
        let store = JsonThreadStore::open(&path).unwrap();
        store.create(thread("thread")).unwrap();
        store.claim_turn(&thread_id, turn("turn")).unwrap();
        let usage = TurnUsage {
            tokens: [
                ("prompt_tokens".to_string(), 12),
                ("completion_tokens".to_string(), 3),
            ]
            .into_iter()
            .collect(),
            estimated_cost: Some(0.02),
            currency: Some("CNY".into()),
        };
        store
            .finish_turn(&thread_id, &turn_id, TurnStatus::Completed, 3, None, usage)
            .unwrap();
    }
    let reopened = JsonThreadStore::open(&path).unwrap();
    let stored = reopened.read(&thread_id).unwrap().unwrap();
    assert_eq!(stored.turns[0].status, TurnStatus::Completed);
    assert_eq!(stored.turns[0].usage.tokens["prompt_tokens"], 12);
    assert_eq!(stored.turns[0].usage.estimated_cost, Some(0.02));
}

#[test]
fn compacted_model_context_is_replaced_and_survives_reopen() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-model-context-{unique}.json"));
    let thread_id = ThreadId::new("thread").unwrap();
    {
        let store = JsonThreadStore::open(&path).unwrap();
        store.create(thread("thread")).unwrap();
        store
            .replace_model_context(
                &thread_id,
                vec![serde_json::json!({"role":"user","content":"summary"})],
                7,
            )
            .unwrap();
    }
    let reopened = JsonThreadStore::open(&path).unwrap();
    let context = reopened.read_model_context(&thread_id).unwrap().unwrap();
    assert_eq!(context.updated_at, 7);
    assert_eq!(context.messages[0]["content"], "summary");
}

#[test]
fn fork_copies_model_context_under_the_new_thread_identity() {
    let store = temp_store("fork-model-context");
    let source = ThreadId::new("source").unwrap();
    let target = ThreadId::new("target").unwrap();
    store.create(thread("source")).unwrap();
    store
        .replace_model_context(
            &source,
            vec![serde_json::json!({"role":"assistant","content":"done"})],
            4,
        )
        .unwrap();
    store.fork(&source, target.clone()).unwrap();
    let copied = store.read_model_context(&target).unwrap().unwrap();
    assert_eq!(copied.thread_id, target);
    assert_eq!(copied.messages[0]["content"], "done");
}

#[test]
fn rollback_receipt_discards_an_unchanged_fork_and_all_of_its_side_domains() {
    let store = temp_store("rollback-fork");
    let source = ThreadId::new("source").unwrap();
    let target = ThreadId::new("target").unwrap();
    let snapshot = goal("goal", 1, "finish the migration");
    store.create(thread("source")).unwrap();
    store
        .replace_model_context(
            &source,
            vec![serde_json::json!({"role":"assistant","content":"done"})],
            4,
        )
        .unwrap();
    store
        .compare_and_set_goal(&source, GoalExpectation::Absent, Some(snapshot.clone()))
        .unwrap();
    let (_, receipt) = store
        .fork_with_rollback(&source, target.clone(), 10, 10)
        .unwrap();
    assert!(store.discard_if_unchanged(&receipt).unwrap());
    assert!(store.read(&target).unwrap().is_none());
    assert!(store.read_model_context(&target).unwrap().is_none());
    assert!(store.read_goal(&target).unwrap().is_none());
    assert!(store.read(&source).unwrap().is_some());
    assert!(store.read_model_context(&source).unwrap().is_some());
    assert_eq!(store.read_goal(&source).unwrap(), Some(snapshot));

    // A failed runtime activation must not permanently reserve the generated
    // target ID; the same request can be retried safely.
    assert!(store.fork(&source, target).is_ok());
}

#[test]
fn rollback_receipt_never_discards_a_target_changed_after_provisioning() {
    let store = temp_store("rollback-fence");
    let target = ThreadId::new("target").unwrap();
    let receipt = store.create_with_rollback(thread("target")).unwrap();
    store
        .replace_model_context(
            &target,
            vec![serde_json::json!({"role":"user","content":"keep me"})],
            2,
        )
        .unwrap();

    assert!(!store.discard_if_unchanged(&receipt).unwrap());
    assert!(store.read(&target).unwrap().is_some());
    assert!(store.read_model_context(&target).unwrap().is_some());
}

#[test]
fn rollback_receipt_rejects_an_aba_write_that_restores_the_original_thread() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-rollback-aba-{unique}.json"));
    let first = JsonThreadStore::open(&path).unwrap();
    let target = ThreadId::new("target").unwrap();
    let receipt = first.create_with_rollback(thread("target")).unwrap();
    let original = first.read(&target).unwrap().unwrap();

    // A second process changes the target and then restores every visible
    // field. Equality-only rollback checks would now incorrectly delete it.
    let second = JsonThreadStore::open(&path).unwrap();
    let mut changed = original.metadata.clone();
    changed.title = "temporary change".into();
    second.update_metadata(changed).unwrap();
    second.update_metadata(original.metadata.clone()).unwrap();
    drop(second);

    assert!(!first.discard_if_unchanged(&receipt).unwrap());
    assert_eq!(first.read(&target).unwrap(), Some(original));
}

#[test]
fn rollback_receipt_rejects_aba_writes_in_side_domains() {
    let store = temp_store("rollback-aba-side-domain");
    let source = ThreadId::new("source").unwrap();
    let target = ThreadId::new("target").unwrap();
    let context = vec![serde_json::json!({"role":"user","content":"same"})];
    store.create(thread("source")).unwrap();
    store.replace_model_context(&source, context, 2).unwrap();
    let (_, receipt) = store
        .fork_with_rollback(&source, target.clone(), 3, 3)
        .unwrap();
    let original_context = store.read_model_context(&target).unwrap().unwrap();
    store
        .replace_model_context(
            &target,
            vec![serde_json::json!({"role":"user","content":"other"})],
            4,
        )
        .unwrap();
    store
        .replace_model_context(
            &target,
            original_context.messages.clone(),
            original_context.updated_at,
        )
        .unwrap();

    assert!(!store.discard_if_unchanged(&receipt).unwrap());
    assert!(store.read(&target).unwrap().is_some());
    assert_eq!(
        store.read_model_context(&target).unwrap(),
        Some(original_context)
    );
}

#[test]
fn all_successful_thread_write_apis_advance_the_durable_epoch() {
    let store = temp_store("write-epochs");
    let id = ThreadId::new("thread").unwrap();
    let turn_id = TurnId::new("turn").unwrap();

    store.create(thread("thread")).unwrap();
    assert_eq!(write_epoch(&store, &id), 1);

    let mut metadata = store.read(&id).unwrap().unwrap().metadata;
    metadata.title = "renamed".into();
    store.update_metadata(metadata).unwrap();
    assert_eq!(write_epoch(&store, &id), 2);

    assert!(store
        .set_harness_profile_if_idle(&id, "readonly".into(), 3)
        .unwrap()
        .is_some());
    assert_eq!(write_epoch(&store, &id), 3);

    store
        .replace_model_context(&id, vec![serde_json::json!({"role":"user"})], 4)
        .unwrap();
    assert_eq!(write_epoch(&store, &id), 4);

    store
        .compare_and_set_goal(
            &id,
            GoalExpectation::Absent,
            Some(goal("goal", 1, "do work")),
        )
        .unwrap();
    assert_eq!(write_epoch(&store, &id), 5);

    store.claim_turn(&id, turn("turn")).unwrap();
    assert_eq!(write_epoch(&store, &id), 6);

    store
        .append_item(
            &id,
            &turn_id,
            ThreadItem::UserMessage {
                id: ItemId::new("item").unwrap(),
                text: "hello".into(),
            },
            5,
        )
        .unwrap();
    assert_eq!(write_epoch(&store, &id), 7);

    store
        .finish_turn(
            &id,
            &turn_id,
            TurnStatus::Completed,
            6,
            None,
            TurnUsage::default(),
        )
        .unwrap();
    assert_eq!(write_epoch(&store, &id), 8);

    store.mark_runtime_activation(&id).unwrap();
    assert_eq!(write_epoch(&store, &id), 9);
}

#[test]
fn provisioning_and_goal_round_writes_initialize_or_advance_epochs() {
    let store = temp_store("provisioning-write-epochs");
    let first = ThreadId::new("first").unwrap();
    let second = ThreadId::new("second").unwrap();
    store
        .create_many(vec![thread("first"), thread("second")])
        .unwrap();
    assert_eq!(write_epoch(&store, &first), 1);
    assert_eq!(write_epoch(&store, &second), 1);

    let source = ThreadId::new("source").unwrap();
    let fork_target = ThreadId::new("fork-target").unwrap();
    let rollback_target = ThreadId::new("rollback-target").unwrap();
    store.create(thread("source")).unwrap();
    store.fork(&source, fork_target.clone()).unwrap();
    assert_eq!(write_epoch(&store, &fork_target), 1);
    let (_, receipt) = store
        .fork_with_rollback(&source, rollback_target.clone(), 4, 4)
        .unwrap();
    assert_eq!(receipt.runtime_activation_epoch, 1);
    assert_eq!(write_epoch(&store, &rollback_target), 1);

    let created_with_receipt = ThreadId::new("created-with-receipt").unwrap();
    let receipt = store
        .create_with_rollback(thread("created-with-receipt"))
        .unwrap();
    assert_eq!(receipt.runtime_activation_epoch, 1);
    assert_eq!(write_epoch(&store, &created_with_receipt), 1);

    let round_thread = ThreadId::new("round-thread").unwrap();
    let round_goal = goal("round-goal", 1, "finish it");
    store.create(thread("round-thread")).unwrap();
    store
        .compare_and_set_goal(
            &round_thread,
            GoalExpectation::Absent,
            Some(round_goal.clone()),
        )
        .unwrap();
    assert_eq!(write_epoch(&store, &round_thread), 2);
    store
        .claim_goal_round(
            &round_thread,
            GoalRef {
                id: round_goal.id.clone(),
                revision: round_goal.revision,
            },
            1,
            goal_turn("round-turn", &round_goal, 1),
        )
        .unwrap();
    assert_eq!(write_epoch(&store, &round_thread), 3);
}

#[test]
fn running_turn_is_recovered_as_failed_after_restart() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-recover-running-{unique}.json"));
    let thread_id = ThreadId::new("thread").unwrap();
    {
        let store = JsonThreadStore::open(&path).unwrap();
        store.create(thread("thread")).unwrap();
        store.claim_turn(&thread_id, turn("turn")).unwrap();
    }
    let reopened = JsonThreadStore::open(&path).unwrap();
    let stored = reopened.read(&thread_id).unwrap().unwrap();
    assert_eq!(stored.turns[0].status, TurnStatus::Failed);
    assert!(stored.turns[0]
        .error
        .as_deref()
        .unwrap()
        .contains("runtime restarted"));
    reopened.claim_turn(&thread_id, turn("next")).unwrap();
}

#[test]
fn second_store_does_not_recover_or_overwrite_a_live_owner() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-cross-process-{unique}.json"));
    let first = JsonThreadStore::open(&path).unwrap();
    first.create(thread("first")).unwrap();
    let first_id = ThreadId::new("first").unwrap();
    first.claim_turn(&first_id, turn("turn-first")).unwrap();

    let second = JsonThreadStore::open(&path).unwrap();
    assert_eq!(
        second.read(&first_id).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
    assert!(matches!(
        second.claim_turn(&first_id, turn("overlap")),
        Err(ThreadStoreError::Busy { .. })
    ));
    second.create(thread("second")).unwrap();
    assert!(first
        .read(&ThreadId::new("second").unwrap())
        .unwrap()
        .is_some());
    assert_eq!(
        first.read(&first_id).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );

    drop(first);
    assert_eq!(
        second.read(&first_id).unwrap().unwrap().turns[0].status,
        TurnStatus::Failed
    );
    second.claim_turn(&first_id, turn("next")).unwrap();
}

#[test]
fn cross_process_lease_helper() {
    let Ok(path) = std::env::var("NCX_THREAD_STORE_HELPER_PATH") else {
        return;
    };
    let ready = PathBuf::from(format!("{path}.ready"));
    let release = PathBuf::from(format!("{path}.release"));
    let store = JsonThreadStore::open(&path).unwrap();
    let id = ThreadId::new("owned").unwrap();
    store.create(thread("owned")).unwrap();
    store.claim_turn(&id, turn("child-turn")).unwrap();
    fs::write(&ready, b"ready").unwrap();
    for _ in 0..500 {
        if release.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("parent did not release helper");
}

#[test]
fn live_turn_lease_is_respected_across_processes() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-process-lease-{unique}.json"));
    let path_text = path.display().to_string();
    let ready = PathBuf::from(format!("{path_text}.ready"));
    let release = PathBuf::from(format!("{path_text}.release"));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::cross_process_lease_helper",
            "--nocapture",
        ])
        .env("NCX_THREAD_STORE_HELPER_PATH", &path_text)
        .spawn()
        .unwrap();
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        assert!(child.try_wait().unwrap().is_none(), "helper exited early");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "helper did not acquire the turn lease");

    let observer = JsonThreadStore::open(&path).unwrap();
    let id = ThreadId::new("owned").unwrap();
    assert_eq!(
        observer.read(&id).unwrap().unwrap().turns[0].status,
        TurnStatus::Running
    );
    assert!(matches!(
        observer.claim_turn(&id, turn("overlap")),
        Err(ThreadStoreError::Busy { .. })
    ));
    observer.create(thread("observer-write")).unwrap();
    fs::write(&release, b"release").unwrap();
    assert!(child.wait().unwrap().success());

    assert_eq!(
        observer.read(&id).unwrap().unwrap().turns[0].status,
        TurnStatus::Failed
    );
    assert!(observer
        .read(&ThreadId::new("observer-write").unwrap())
        .unwrap()
        .is_some());
    let _ = fs::remove_file(ready);
    let _ = fs::remove_file(release);
}

#[test]
fn corrupt_primary_recovers_from_last_backup() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ncx-recover-backup-{unique}.json"));
    let store = JsonThreadStore::open(&path).unwrap();
    store.create(thread("thread")).unwrap();
    fs::copy(&path, path.with_extension("bak")).unwrap();
    fs::write(&path, "not-json").unwrap();

    let reopened = JsonThreadStore::open(&path).unwrap();
    assert!(reopened
        .read(&ThreadId::new("thread").unwrap())
        .unwrap()
        .is_some());
}
