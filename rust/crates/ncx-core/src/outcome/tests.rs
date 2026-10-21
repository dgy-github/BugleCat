use super::*;
use crate::agent_state::AuthoritySource;

fn sample_outcome(id: &str, role: TaskRole, category: TaskCategory) -> TaskOutcome {
    TaskOutcome {
        task_id: id.to_string(),
        parent_task_id: None,
        session_id: "s1".to_string(),
        turn_id: 1,
        role,
        authority: AuthoritySource::DirectHuman,
        category,
        query_hash: hash_text("fix the bug in parser.rs"),
        model: "deepseek-chat".to_string(),
        fast_model: None,
        permission_mode: "accept-edits".to_string(),
        route: Some(RouteKey {
            complexity: "Simple".to_string(),
            tier_config: "single".to_string(),
            verify_passed: true,
            verify_rounds: 1,
            shadow_recommended_route: None,
            initial_complexity: None,
        }),
        terminal_state: "Completed".to_string(),
        stop_reason: "completed".to_string(),
        verification: Some("passed".to_string()),
        iterations: 2,
        tool_calls: 2,
        tools_used: vec!["read_file".to_string(), "edit_file".to_string()],
        approvals_requested: 0,
        approvals_denied: 0,
        user_questions: 0,
        recalled_note_ids: vec!["n1".to_string()],
        files_touched: vec!["src/parser.rs".to_string()],
        file_hashes: BTreeMap::new(),
        workspace_revision: Some("abc1234".to_string()),
        usage: Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        },
        duration_ms: 500,
        timestamp_ms: 1_000_000,
        memory_rank_changed: None,
    }
}

fn followup(task_id: &str, signal: FollowupSignal) -> OutcomeFollowup {
    OutcomeFollowup {
        task_id: task_id.to_string(),
        timestamp_ms: 1_000_100,
        signal,
    }
}

fn accepted_for(outcomes: &[TaskOutcome]) -> Vec<OutcomeFollowup> {
    outcomes
        .iter()
        .map(|o| followup(&o.task_id, FollowupSignal::Accepted))
        .collect()
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = crate::test_support::unique_temp_dir(&format!("ncx_outcome_{label}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn task_category_excludes_pure_chat_and_classifies_engineering_turns() {
    assert_eq!(
        TaskCategory::classify("你好，今天天气怎么样", &[], false),
        TaskCategory::Chat
    );
    assert_eq!(
        TaskCategory::classify(
            "修复 parser.rs 里的 panic 报错",
            &["edit_file".to_string()],
            true
        ),
        TaskCategory::Bugfix
    );
    assert_eq!(
        TaskCategory::classify("refactor memory store", &["edit_file".to_string()], true),
        TaskCategory::Refactor
    );
    assert_eq!(
        TaskCategory::classify("run cargo test", &["bash".to_string()], true),
        TaskCategory::Test
    );
    assert_eq!(
        TaskCategory::classify("fixed the failing tests", &["shell".to_string()], true),
        TaskCategory::Bugfix
    );
}

#[test]
fn ascii_keywords_match_on_word_boundaries_not_substrings() {
    let edit = ["apply_patch".to_string()];
    // "ci" inside decide/specific, "test" inside latest, "doc" inside docker,
    // "fix" inside prefix and "add" inside address must not fire.
    assert_eq!(
        TaskCategory::classify("decide a specific layout", &edit, true),
        TaskCategory::Feature
    );
    assert_eq!(
        TaskCategory::classify("show the latest docker address prefix", &edit, true),
        TaskCategory::Feature
    );
    assert_eq!(
        TaskCategory::classify("set up the CI pipeline", &edit, true),
        TaskCategory::Build
    );
    assert_eq!(
        TaskCategory::classify("added a new endpoint", &edit, true),
        TaskCategory::Feature
    );
    assert!(!is_correction_prompt(
        "the terror module needs a prefix",
        &["src/terror.rs".to_string()]
    ));
}

#[test]
fn feature_prompts_with_comment_or_exception_handling_mentions_stay_feature() {
    let edit = ["edit_file".to_string()];
    assert_eq!(
        TaskCategory::classify("实现导出按钮并补充注释说明", &edit, true),
        TaskCategory::Feature
    );
    assert_eq!(
        TaskCategory::classify("新增重试中间件，解决超时异常", &edit, true),
        TaskCategory::Feature
    );
    assert_eq!(
        TaskCategory::classify("更新 README 文档说明", &edit, true),
        TaskCategory::Docs
    );
}

#[test]
fn multilingual_correction_detector_recognizes_zh_en_ja_and_file_mentions() {
    assert!(is_correction_prompt("不对，还是报错了", &[]));
    assert!(is_correction_prompt("That didn't work, revert that", &[]));
    assert!(is_correction_prompt("直っていない、やり直し", &[]));
    assert!(is_correction_prompt(
        "parser.rs 还是有问题",
        &["src/parser.rs".to_string()]
    ));
    assert!(is_correction_prompt(
        "parser.rs still has an error",
        &["src/parser.rs".to_string()]
    ));
    assert!(!is_correction_prompt(
        "接下来帮我写一下 README 文档",
        &["src/parser.rs".to_string()]
    ));
}

#[test]
fn wilson_ci_and_singularity_status_respect_sample_discipline() {
    let ci_zero = wilson_ci(0, 0);
    assert_eq!(ci_zero.lower, 0.0);

    // 25/30 = 83.3%, Wilson 95% lower bound ~ 66.4% >= 60.0%
    let mut outcomes = Vec::new();
    for i in 0..30 {
        let mut o = sample_outcome(&format!("t{i}"), TaskRole::Primary, TaskCategory::Bugfix);
        if i >= 25 {
            o.user_questions = 1; // 5 tasks needed clarification
        }
        outcomes.push(o);
    }
    let followups = accepted_for(&outcomes);
    // Add a chat turn and a worker turn: neither enters the primary ACR denominator.
    outcomes.push(sample_outcome(
        "chat1",
        TaskRole::Primary,
        TaskCategory::Chat,
    ));
    outcomes.push(sample_outcome(
        "worker1",
        TaskRole::OrchestratorWorker,
        TaskCategory::Bugfix,
    ));

    let report = build_outcome_report(
        &outcomes,
        &followups,
        "30d",
        &["category".to_string(), "model".to_string()],
    );
    assert_eq!(report.overall.sample_size, 30);
    assert_eq!(report.chat_turns_excluded, 1);
    assert_eq!(report.worker_turns_excluded, 1);
    assert!(report.overall.acr_strict_ci.lower >= 0.60);
    assert!(report.singularity.reached);
    assert_eq!(report.category_singularity.len(), 1);
    assert_eq!(report.category_singularity[0].category, "bugfix");
    assert!(report.category_singularity[0].status.reached);
}

#[test]
fn singularity_is_judged_per_category() {
    // 30 good bugfix tasks + 30 failing docs tasks: overall lower bound is
    // below 60%, but bugfix alone has crossed.
    let mut outcomes = Vec::new();
    for i in 0..30 {
        outcomes.push(sample_outcome(
            &format!("b{i}"),
            TaskRole::Primary,
            TaskCategory::Bugfix,
        ));
        let mut d = sample_outcome(&format!("d{i}"), TaskRole::Primary, TaskCategory::Docs);
        d.stop_reason = "error".to_string();
        d.terminal_state = "Failed".to_string();
        outcomes.push(d);
    }
    let followups = accepted_for(&outcomes);
    let report = build_outcome_report(&outcomes, &followups, "30d", &[]);
    assert!(!report.singularity.reached);
    let by: BTreeMap<_, _> = report
        .category_singularity
        .iter()
        .map(|c| (c.category.as_str(), c.status.reached))
        .collect();
    assert_eq!(by.get("bugfix"), Some(&true));
    assert_eq!(by.get("docs"), Some(&false));
}

#[test]
fn completed_tasks_without_followup_are_pending_not_successes() {
    let pending = sample_outcome("p1", TaskRole::Primary, TaskCategory::Feature);
    let mut failed = sample_outcome("f1", TaskRole::Primary, TaskCategory::Feature);
    failed.stop_reason = "timeout".to_string();
    failed.terminal_state = "NeedsHuman".to_string();
    let resolved = sample_outcome("r1", TaskRole::Primary, TaskCategory::Feature);

    let report = build_outcome_report(
        &[pending, failed, resolved],
        &[followup("r1", FollowupSignal::Accepted)],
        "30d",
        &[],
    );
    // The failed turn is already determined (I1); the unresolved completed
    // turn is pending and enters no rate.
    assert_eq!(report.overall.sample_size, 2);
    assert_eq!(report.overall.pending_count, 1);
    assert_eq!(report.overall.strict_successes, 1);
}

#[test]
fn followup_signals_drive_strict_and_tolerant_acr_and_explicit_label_overrides() {
    let mut t1 = sample_outcome("t1", TaskRole::Primary, TaskCategory::Feature);
    t1.approvals_requested = 1; // Approved I2: fails strict, passes tolerant

    let t2 = sample_outcome("t2", TaskRole::Primary, TaskCategory::Feature);
    let followups = vec![
        followup("t1", FollowupSignal::Accepted),
        followup(
            "t2",
            FollowupSignal::Correction {
                inferred: true,
                note: None,
            },
        ),
        // Explicit `/outcome good` overrides the inferred correction
        followup(
            "t2",
            FollowupSignal::Label {
                good: true,
                note: Some("actually fine".to_string()),
            },
        ),
    ];

    let report = build_outcome_report(&[t1, t2], &followups, "7d", &[]);
    assert_eq!(report.overall.sample_size, 2);
    assert_eq!(report.overall.strict_successes, 1); // t1 fails strict due to approval
    assert_eq!(report.overall.tolerant_successes, 2);
    assert_eq!(report.overall.rework_count, 0);
    assert_eq!(report.overall.inferred_correction_count, 0);
    assert_eq!(report.overall.status, "insufficient_sample");
}

#[test]
fn followup_polarity_last_label_wins_and_negatives_beat_accepted() {
    assert_eq!(followup_polarity(&[]), None);
    let accepted = followup("t", FollowupSignal::Accepted);
    let rollback = followup(
        "t",
        FollowupSignal::Rollback {
            checkpoint_id: None,
        },
    );
    let good = followup(
        "t",
        FollowupSignal::Label {
            good: true,
            note: None,
        },
    );
    let bad = followup(
        "t",
        FollowupSignal::Label {
            good: false,
            note: None,
        },
    );
    assert_eq!(
        followup_polarity(std::slice::from_ref(&accepted)),
        Some(true)
    );
    assert_eq!(
        followup_polarity(&[accepted.clone(), rollback.clone()]),
        Some(false)
    );
    assert_eq!(
        followup_polarity(&[rollback.clone(), good.clone()]),
        Some(true)
    );
    assert_eq!(followup_polarity(&[good, bad]), Some(false));
}

#[test]
fn routing_stats_monotonic_upgrade_when_wilson_lower_bound_below_threshold() {
    let mut outcomes = Vec::new();
    for i in 0..22 {
        let mut o = sample_outcome(&format!("r{i}"), TaskRole::Primary, TaskCategory::Bugfix);
        if i >= 4 {
            o.stop_reason = "error: verification failed".to_string();
        }
        outcomes.push(o);
    }
    let followups = accepted_for(&outcomes);
    let stats = RoutingStats::from_outcomes(&outcomes, &followups);
    let rec = stats.recommend_complexity_upgrade(TaskCategory::Bugfix, "Simple");
    assert!(rec.is_some());
    assert_eq!(rec.unwrap().0, "Medium");
    // Complex never upgrades further or downgrades.
    assert!(
        stats
            .recommend_complexity_upgrade(TaskCategory::Bugfix, "Complex")
            .is_none()
    );
}

#[test]
fn routing_stats_bucket_on_initial_complexity_and_skip_direct_turns() {
    let mut outcomes = Vec::new();
    for i in 0..22 {
        // Classified Simple, upgraded to Medium by `on`-mode feedback.
        let mut o = sample_outcome(&format!("u{i}"), TaskRole::Primary, TaskCategory::Bugfix);
        if let Some(route) = o.route.as_mut() {
            route.complexity = "Medium".to_string();
            route.initial_complexity = Some("Simple".to_string());
        }
        o.stop_reason = "error".to_string();
        outcomes.push(o);
        // Direct (non-orchestrated) turns carry no route and are ignored.
        let mut direct = sample_outcome(&format!("d{i}"), TaskRole::Primary, TaskCategory::Bugfix);
        direct.route = None;
        outcomes.push(direct);
    }
    let followups = accepted_for(&outcomes);
    let stats = RoutingStats::from_outcomes(&outcomes, &followups);
    assert_eq!(
        stats
            .buckets
            .get(&(TaskCategory::Bugfix, "Simple".to_string()))
            .map(|b| b.sample_size),
        Some(22)
    );
    assert!(
        !stats
            .buckets
            .contains_key(&(TaskCategory::Bugfix, "Medium".to_string()))
    );
}

#[test]
fn shadow_summary_counts_memory_rank_changes_and_route_recommendations() {
    let mut a = sample_outcome("a", TaskRole::Primary, TaskCategory::Bugfix);
    a.memory_rank_changed = Some(true);
    if let Some(route) = a.route.as_mut() {
        route.shadow_recommended_route = Some("Medium (reason)".to_string());
    }
    let mut b = sample_outcome("b", TaskRole::Primary, TaskCategory::Bugfix);
    b.memory_rank_changed = Some(false);
    let mut c = sample_outcome("c", TaskRole::Primary, TaskCategory::Bugfix);
    c.route = None;
    let outcomes = vec![a, b, c];
    let report = build_outcome_report(&outcomes, &accepted_for(&outcomes), "30d", &[]);
    assert_eq!(report.shadow.memory_observed, 2);
    assert_eq!(report.shadow.memory_rank_changed, 1);
    assert_eq!(report.shadow.routing_observed, 2);
    assert_eq!(report.shadow.routing_would_upgrade, 1);
    assert_eq!(report.shadow.routing_would_upgrade_determined, 1);
    assert_eq!(report.shadow.routing_would_upgrade_strict_successes, 1);
}

#[test]
fn context_store_round_trips_outcomes_and_followups() {
    let store = ContextStore::open_in_memory().unwrap();
    let o = sample_outcome("task-99", TaskRole::Primary, TaskCategory::Bugfix);
    append_task_outcome(&store, &o).unwrap();
    let f = OutcomeFollowup {
        task_id: "task-99".to_string(),
        timestamp_ms: 1_000_500,
        signal: FollowupSignal::Rollback {
            checkpoint_id: Some("cp-1".to_string()),
        },
    };
    append_outcome_followup(&store, "s1", "/ws", &f).unwrap();

    let (loaded_o, loaded_f) = load_outcomes_and_followups(&store, None).unwrap();
    assert_eq!(loaded_o.len(), 1);
    assert_eq!(loaded_o[0].task_id, "task-99");
    assert_eq!(loaded_f.len(), 1);
    assert_eq!(loaded_f[0].task_id, "task-99");
    assert_eq!(followups_for_task(&store, "task-99").unwrap().len(), 1);
    assert_eq!(
        latest_primary_outcome(&store, "s1")
            .unwrap()
            .map(|o| o.task_id),
        Some("task-99".to_string())
    );
}

fn memory_with_note(label: &str, text: &str) -> (MemoryStore, String) {
    let ws = temp_dir(label);
    let mut memory = MemoryStore::new(ws.join(".ncx").join("memory"));
    memory.set_feedback_mode("shadow");
    memory.remember(text, &[], 1).unwrap();
    let id = MemoryStore::note_id(text);
    (memory, id)
}

fn stats_of(memory: &MemoryStore, id: &str) -> crate::memory_index::MemoryFeedbackStats {
    crate::memory_index::MemoryIndex::for_store(memory)
        .unwrap()
        .feedback_stat(&memory.workspace_key(), id)
        .unwrap()
}

#[test]
fn memory_is_scored_once_per_task_and_a_disagreeing_label_revises_it() {
    let (memory, id) = memory_with_note("score_once", "run cargo fmt before committing");
    let store = ContextStore::open_in_memory().unwrap();
    let mut o = sample_outcome("t1", TaskRole::Primary, TaskCategory::Bugfix);
    o.recalled_note_ids = vec![id.clone()];
    append_task_outcome(&store, &o).unwrap();
    let fb = || {
        Some(MemoryFeedback {
            store: &memory,
            mode: "shadow",
        })
    };

    record_followup(&store, "/ws", &o, FollowupSignal::Accepted, fb()).unwrap();
    let s = stats_of(&memory, &id);
    assert_eq!((s.injected, s.accepted, s.rejected), (1, 1, 0));

    // A second agreeing signal does not count the task again.
    record_followup(
        &store,
        "/ws",
        &o,
        FollowupSignal::Label {
            good: true,
            note: None,
        },
        fb(),
    )
    .unwrap();
    let s = stats_of(&memory, &id);
    assert_eq!((s.injected, s.accepted, s.rejected), (1, 1, 0));

    // A disagreeing human label flips the single count.
    record_followup(
        &store,
        "/ws",
        &o,
        FollowupSignal::Label {
            good: false,
            note: None,
        },
        fb(),
    )
    .unwrap();
    let s = stats_of(&memory, &id);
    assert_eq!((s.injected, s.accepted, s.rejected), (1, 0, 1));
}

#[test]
fn demotion_only_happens_in_on_mode_and_only_on_rejection() {
    for (mode, expect_demoted) in [("shadow", false), ("on", true)] {
        let (memory, id) =
            memory_with_note(&format!("demote_{mode}"), "always vendor the sqlite crate");
        let store = ContextStore::open_in_memory().unwrap();
        for i in 0..9 {
            let mut o = sample_outcome(&format!("t{i}"), TaskRole::Primary, TaskCategory::Build);
            o.recalled_note_ids = vec![id.clone()];
            append_task_outcome(&store, &o).unwrap();
            record_followup(
                &store,
                "/ws",
                &o,
                FollowupSignal::Correction {
                    inferred: true,
                    note: None,
                },
                Some(MemoryFeedback {
                    store: &memory,
                    mode,
                }),
            )
            .unwrap();
        }
        let s = stats_of(&memory, &id);
        assert_eq!(s.injected, 9, "mode={mode}");
        assert_eq!(s.demoted, expect_demoted, "mode={mode}");
    }

    // Mode off: nothing is scored at all.
    let (memory, id) = memory_with_note("demote_off", "prefer ripgrep");
    let store = ContextStore::open_in_memory().unwrap();
    let mut o = sample_outcome("x", TaskRole::Primary, TaskCategory::Build);
    o.recalled_note_ids = vec![id.clone()];
    record_followup(
        &store,
        "/ws",
        &o,
        FollowupSignal::Accepted,
        Some(MemoryFeedback {
            store: &memory,
            mode: "off",
        }),
    )
    .unwrap();
    assert!(
        crate::memory_index::MemoryIndex::for_store(&memory)
            .unwrap()
            .feedback_stat(&memory.workspace_key(), &id)
            .is_none()
    );
}

#[test]
fn rollback_blames_only_the_latest_task_started_after_the_checkpoint() {
    let ws = temp_dir("rollback");
    let store = ContextStore::open_in_memory().unwrap();
    let mut before = sample_outcome("s1:turn-1", TaskRole::Primary, TaskCategory::Feature);
    before.timestamp_ms = 1_000;
    before.duration_ms = 100;
    let mut after = sample_outcome("s1:turn-2", TaskRole::Primary, TaskCategory::Feature);
    after.turn_id = 2;
    after.timestamp_ms = 5_000;
    after.duration_ms = 500;
    let mut other_session = sample_outcome("s2:turn-1", TaskRole::Primary, TaskCategory::Feature);
    other_session.session_id = "s2".to_string();
    other_session.timestamp_ms = 6_000;
    other_session.duration_ms = 100;
    for o in [&before, &after, &other_session] {
        append_task_outcome(&store, o).unwrap();
    }

    // Checkpoint taken at t=2000: only turn-2 of s1 ran after it.
    let blamed = record_checkpoint_rollback(&store, &ws, "cp-1", 2_000, Some("s1"), None).unwrap();
    assert_eq!(blamed.as_deref(), Some("s1:turn-2"));
    assert!(followups_for_task(&store, "s1:turn-1").unwrap().is_empty());
    assert!(followups_for_task(&store, "s2:turn-1").unwrap().is_empty());

    // A checkpoint newer than every task blames nobody.
    let none = record_checkpoint_rollback(&store, &ws, "cp-2", 9_000, Some("s1"), None).unwrap();
    assert!(none.is_none());
}

#[test]
fn baseline_readiness_requires_two_weeks_two_categories_and_shadow_samples() {
    let day = 86_400_000u64;
    let mut outcomes = Vec::new();
    for i in 0..30u64 {
        for (prefix, category) in [("b", TaskCategory::Bugfix), ("f", TaskCategory::Feature)] {
            let mut o = sample_outcome(&format!("{prefix}{i}"), TaskRole::Primary, category);
            o.timestamp_ms = 1_000_000 + i * day / 2; // spans ~14.5 days
            o.memory_rank_changed = Some(i % 3 == 0);
            outcomes.push(o);
        }
    }
    let report = build_outcome_report(&outcomes, &accepted_for(&outcomes), "30d", &[]);
    let b = &report.baseline;
    assert_eq!(b.days_observed, 14);
    assert_eq!(
        b.categories_ready,
        vec!["bugfix".to_string(), "feature".to_string()]
    );
    assert!(b.memory_ready, "{:?}", b.blockers);
    // No orchestrated task ever got an upgrade recommendation.
    assert!(!b.routing_ready);
    assert!(b.blockers.iter().any(|x| x.starts_with("routing:")));

    // One week of data is never enough, whatever the volume.
    let short: Vec<_> = outcomes
        .iter()
        .cloned()
        .map(|mut o| {
            o.timestamp_ms = 1_000_000 + (o.timestamp_ms - 1_000_000) / 2;
            o
        })
        .collect();
    let report = build_outcome_report(&short, &accepted_for(&short), "30d", &[]);
    assert!(!report.baseline.memory_ready);
    assert!(report.baseline.blockers[0].starts_with("baseline 7/14"));
}
