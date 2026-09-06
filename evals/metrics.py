"""Deterministic metrics over recorded agent evidence.

The module never asks the agent to grade itself. It consumes JSON objects from
an event trace and explicit before/after state snapshots; missing evidence is
reported as None instead of being treated as a pass.
"""
from __future__ import annotations

from collections import Counter
from typing import Any, Iterable


def _rate(numerator: int, denominator: int) -> float | None:
    return round(numerator / denominator, 4) if denominator else None


def _events(events: Iterable[dict[str, Any]], kind: str) -> list[dict[str, Any]]:
    return [e for e in events if e.get("kind") == kind]


def evaluate_trace(events: list[dict[str, Any]]) -> dict[str, Any]:
    """Calculate observable tool, verification, recovery and efficiency data."""
    calls = _events(events, "tool_call")
    results = _events(events, "tool_result")
    checks = _events(events, "verification")
    failures = _events(events, "failure")
    recoveries = _events(events, "recovery")
    takeovers = _events(events, "human_takeover")
    completed = _events(events, "task_complete")
    regressions = _events(events, "regression")
    step_events = [e for e in events if e.get("kind") in {"step", "tool_call", "model_call"}]

    successful_results = sum(1 for e in results if e.get("success") is True)
    correctly_selected = sum(
        1 for e in calls if e.get("expected") is True and e.get("args_valid") is not False
    )
    valid_calls = sum(1 for e in calls if e.get("args_valid") is not False)
    verification_passes = sum(1 for e in checks if e.get("passed") is True)
    recovery_successes = sum(1 for e in recoveries if e.get("success") is True)
    duplicate_actions = _duplicate_actions(calls)
    tokens = sum(int(e.get("tokens", 0) or 0) for e in events if e.get("kind") == "model_call")

    return {
        "task_success_rate": _rate(sum(1 for e in completed if e.get("success") is True), len(completed)),
        "tool_success_rate": _rate(successful_results, len(results)),
        "tool_selection_accuracy": _rate(correctly_selected, len(calls)),
        "tool_argument_validity_rate": _rate(valid_calls, len(calls)),
        "verification_pass_rate": _rate(verification_passes, len(checks)),
        "recovery_rate": _rate(recovery_successes, len(recoveries)),
        "failure_recovery_rate": _rate(recovery_successes, len(failures)),
        "regression_pass_rate": _rate(sum(1 for e in regressions if e.get("passed") is True), len(regressions)),
        "duplicate_action_rate": _rate(duplicate_actions, len(calls)),
        "steps": len(step_events),
        "tool_calls": len(calls),
        "tokens": tokens,
        "human_takeover_rate": _rate(len(takeovers), 1 if completed else 0),
        "event_count": len(events),
    }


def _duplicate_actions(calls: list[dict[str, Any]]) -> int:
    keys = [(str(e.get("name", "")), str(e.get("arguments", ""))) for e in calls]
    counts = Counter(keys)
    return sum(max(0, n - 1) for n in counts.values())


def compare_states(before: dict[str, Any], after: dict[str, Any], *,
                   required: tuple[str, ...] =
                   ("goal", "constraints", "pending_tasks", "completed_tasks", "artifacts")) -> dict[str, Any]:
    """Compare compaction snapshots using exact normalized values."""
    preserved: dict[str, bool] = {}
    for key in required:
        if key not in before or key not in after:
            preserved[key] = False
        elif key in {"pending_tasks", "completed_tasks"}:
            preserved[key] = Counter(map(str, before[key])) == Counter(map(str, after[key]))
        else:
            preserved[key] = before[key] == after[key]
    preserved_count = sum(preserved.values())
    return {
        "fields": preserved,
        "state_preservation_rate": _rate(preserved_count, len(required)),
        "context_recovery_accuracy": _rate(sum(key in after for key in required), len(required)),
        "constraint_loss": bool("constraints" in before and preserved.get("constraints") is False),
        "duplicate_completed_tasks": _duplicate_values(after.get("completed_tasks", [])),
        "artifact_state_preserved": preserved.get("artifacts", False),
    }


def _duplicate_values(values: Any) -> int:
    if not isinstance(values, list):
        return 0
    return sum(n - 1 for n in Counter(map(str, values)).values() if n > 1)
