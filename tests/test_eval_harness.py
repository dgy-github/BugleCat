from evals.metrics import compare_states, evaluate_trace, offline_acr, wilson_ci


def test_trace_metrics_are_evidence_based():
    result = evaluate_trace([
        {"kind": "tool_call", "name": "read", "arguments": "{}", "expected": True},
        {"kind": "tool_result", "success": True},
        {"kind": "failure"},
        {"kind": "recovery", "success": True},
        {"kind": "verification", "passed": True},
        {"kind": "task_complete", "success": True},
    ])
    assert result["task_success_rate"] == 1.0
    assert result["offline_acr"] == 1.0
    assert result["tool_success_rate"] == 1.0
    assert result["failure_recovery_rate"] == 1.0
    assert result["verification_pass_rate"] == 1.0


def test_offline_acr_excludes_feedback_rounds_and_takeovers():
    trace_with_takeover = evaluate_trace([
        {"kind": "human_takeover"},
        {"kind": "task_complete", "success": True},
    ])
    assert trace_with_takeover["task_success_rate"] == 1.0
    assert trace_with_takeover["offline_acr"] == 0.0

    summary = offline_acr([
        {"passed": True, "timed_out": False, "feedback_rounds": 0},
        {"passed": True, "timed_out": False, "feedback_rounds": 1},
        {"passed": True, "timed_out": True, "feedback_rounds": 0},
        {"passed": False, "timed_out": False, "feedback_rounds": 0},
    ])
    assert summary["offline_acr"] == 0.25
    assert summary["autonomous_passes"] == 1
    assert summary["runs"] == 4
    ci = wilson_ci(24, 30)
    assert ci is not None
    assert 0.60 <= ci["lower"] < ci["center"] < ci["upper"] <= 1.0


def test_compaction_fails_closed_and_detects_duplicates():
    before = {"goal": "ship", "constraints": ["tests"], "pending_tasks": ["a"],
              "completed_tasks": ["b"], "artifacts": {"x": "1"}}
    after = {"goal": "ship", "constraints": [], "pending_tasks": ["a"],
             "completed_tasks": ["b", "b"], "artifacts": {"x": "1"}}
    result = compare_states(before, after)
    assert result["state_preservation_rate"] == 0.6
    assert result["constraint_loss"] is True
    assert result["duplicate_completed_tasks"] == 1


def test_empty_trace_has_no_fake_zero_score():
    result = evaluate_trace([])
    assert result["task_success_rate"] is None
    assert result["offline_acr"] is None
    assert result["tool_success_rate"] is None
