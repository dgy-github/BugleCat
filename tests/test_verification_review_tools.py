"""Offline regressions for the review tools; never use credentials or transport."""
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / f"{name}.py")
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


reviewer = module("run_jev_certification")
replay = module("replay_verification")


def cohort(tmp_path, positives=150, negatives=40):
    labels = [{"id": str(i), "new_label": "supported" if i < positives else "unsupported"}
              for i in range(positives + negatives)]
    path = tmp_path / "labels.jsonl"
    path.write_text("\n".join(json.dumps(r) for r in labels), encoding="utf-8")
    calls = tmp_path / "calls"
    calls.mkdir()
    for row in labels:
        score = 0 if row["new_label"] == "supported" else 1
        (calls / f"{row['id']}.json").write_text(json.dumps({
            "ok": True, "noul": score,
            "raw": {"answers": {"acceptance_violation": {"noul": score}}},
        }), encoding="utf-8")
    return path, calls


def test_perfect_scores_never_certify_or_invent_historical_attempts(tmp_path):
    report = reviewer.review(*cohort(tmp_path))
    assert report["status"] == "complete_exploratory"
    assert any(r["exploratory_bounds_met"] for r in report["grid_evaluation"])
    assert report["certified_qualified_thresholds"] == []
    assert report["historical_network_attempts"] is None
    assert report["new_network_attempts"] == 0


@pytest.mark.parametrize("bad", [None, {"ok": False}, {"ok": True, "noul": True},
                                 {"ok": True, "noul": float("nan")},
                                 {"ok": True, "noul": 1.2}, {"ok": True, "noul": .5}])
def test_failure_keeps_cohort_denominator_and_disables_grid(tmp_path, bad):
    labels, calls = cohort(tmp_path)
    target = calls / "189.json"
    if bad is None:
        target.unlink()
    else:
        target.write_text(json.dumps(bad), encoding="utf-8")
    before = {p.name: p.read_bytes() for p in calls.iterdir()}
    report = reviewer.review(labels, calls)
    assert report["cohort_counts"]["unsupported"] == 40
    assert report["scored_counts"]["unsupported"] == 39
    assert len(report["unresolved"]) == 1
    assert report["grid_evaluation"] == []
    assert before == {p.name: p.read_bytes() for p in calls.iterdir()}


def record(verdict="pass", reason="supported_host_claim"):
    return json.dumps({"schema": "buglecat.verification.record.v2", "result": {
        "verdict": verdict, "decisions": [{"verdict": verdict, "reason_code": reason}]}})


@pytest.mark.parametrize("content", ["", "{broken", "{}", "[]", '{"schema":"wrong"}'])
def test_strict_rejects_empty_or_invalid_logs(tmp_path, content):
    path = tmp_path / "bad.verification.jsonl"
    path.write_text(content, encoding="utf-8")
    result = subprocess.run([sys.executable, str(ROOT / "scripts/replay_verification.py"),
                             str(path), "--strict"], capture_output=True)
    assert result.returncode == 2


def test_replay_continues_after_bad_line(tmp_path):
    path = tmp_path / "mixed.verification.jsonl"
    path.write_text(record() + "\n{broken\n" + record("need_evidence", "foreign_evidence"), encoding="utf-8")
    report = replay.analyze_files([path])
    assert report["total_records"] == 2
    assert report["input_errors"][0]["line"] == 2
    assert report["intervention_records"] == 1


def test_strict_missing_and_need_evidence(tmp_path):
    path = tmp_path / "missing.verification.jsonl"
    cmd = [sys.executable, str(ROOT / "scripts/replay_verification.py"), str(path), "--strict"]
    assert subprocess.run(cmd, capture_output=True).returncode == 2
    path.write_text(record("need_evidence", "incomplete_evidence"), encoding="utf-8")
    assert subprocess.run(cmd, capture_output=True).returncode == 1
    path.write_text(record(), encoding="utf-8")
    assert subprocess.run(cmd, capture_output=True).returncode == 0


def test_no_direct_http_or_retry_entrypoint():
    assert not hasattr(reviewer, "call_jev_single")
    assert not hasattr(reviewer, "get_cached_attempts")


def test_accounting_inventory_does_not_count_responses_as_attempts(tmp_path):
    auditor = module("audit_jev_history")
    history, ledger = tmp_path / "history", tmp_path / "ledger"
    (history / "jev_calls").mkdir(parents=True)
    (ledger / "reservations").mkdir(parents=True)
    (history / "jev_calls/one.json").write_text('{"ok":true}', encoding="utf-8")
    (history / "jev_calls/two.json").write_text('{"ok":true,"source":"stdin_dev_cached"}', encoding="utf-8")
    (ledger / "reservations/one.json").write_text('{"group_id":"one"}', encoding="utf-8")
    result = auditor.audit(history, ledger)
    assert result["saved_response_files"] == 2
    assert result["sources"] == {"legacy_direct_http": 1, "stdin_dev_cached": 1}
    assert result["shared_ledger_reservations"] == 1
    assert result["actual_network_attempts"] is None
    assert result["charged_amount"] is None
    assert result["ledger_modified"] is False
