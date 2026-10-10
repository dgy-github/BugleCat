import json

import pytest

from evals.verification.annotation_audit import (
    annotate,
    artifact_oracle,
    last_check_output,
    multiplier_is_two,
    within_root,
)
from evals.verification.review_queue import digest


@pytest.mark.parametrize("body,expected", [
    ("def twice(n):\n return n * 2", True),
    ("def twice(n):\n return n + 2", False),
    ("def twice(n):\n print(n)\n return n * 2", False),
    ("def twice(n):\n return n * 2.0", False),
    ("def twice(n):\n return n * 2\ndef twice(n):\n return n+2", False),
    ("def twice(:", False),
])
def test_scoped_ast_proof(body, expected):
    assert multiplier_is_two(body) is expected


def test_containment_uses_complete_resolved_paths(tmp_path):
    root = tmp_path / "root"
    assert within_root(root / "child", root)
    assert not within_root(tmp_path / "root-sibling", root)
    assert not within_root(root / ".." / "outside", root)


def test_original_check_output_join_cannot_use_unrelated_print(tmp_path):
    source = tmp_path / "source.jsonl"
    rows = [
        {"role": "assistant", "tool_calls": [
            {"id": "a", "function": {"name": "shell", "arguments": '{"command":"python check.py"}'}},
            {"id": "b", "function": {"name": "shell", "arguments": '{"command":"echo pretend"}'}}]},
        {"role": "tool", "name": "shell", "tool_call_id": "a", "content": "Exit code: 1\nAssertionError: intentional failing fixture"},
        {"role": "tool", "name": "shell", "tool_call_id": "b", "content": "3 checks passed"},
    ]
    source.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
    assert last_check_output(source).startswith("Exit code: 1")
    assert "3 checks passed" not in last_check_output(source)


def test_oracle_cannot_execute_arbitrary_project(tmp_path):
    manifest = {"cases": [{"id": "x", "files": {"check.py": ""}, "grader": "raise RuntimeError('never')"}]}
    with pytest.raises(ValueError, match="isolated trial fixtures"):
        artifact_oracle({"case_id": "x", "workspace": str(tmp_path)}, manifest)


def test_source_change_during_assertion_cannot_receive_label(tmp_path, monkeypatch):
    source = tmp_path / "source.jsonl"
    source.write_text(json.dumps({"role": "assistant", "content": "Passed"}), encoding="utf-8")
    report = tmp_path / "trial.jsonl"
    report.write_text(json.dumps({"session_log": str(source)}), encoding="utf-8")
    case = {"id": "x", "source_sha256": digest(source.read_bytes()), "source_index": 0,
            "candidate_sha256": digest(b"Passed")}

    def stale_oracle(*args):
        source.write_text(json.dumps({"role": "assistant", "content": "Changed"}), encoding="utf-8")
        return {}

    monkeypatch.setattr("evals.verification.annotation_audit.artifact_oracle", stale_oracle)
    with pytest.raises(ValueError, match="source changed during assertion"):
        annotate({"cases": [case]}, [], [report], {}, {})


def test_changed_seed_or_grader_cannot_reuse_original_trial_hash(tmp_path):
    root = tmp_path / "ncx-jev-trial-mismatch" / "workspace"
    root.mkdir(parents=True)
    manifest = {"cases": [{"id": "x", "files": {}, "grader": "raise RuntimeError('never')"}]}
    with pytest.raises(ValueError, match="frozen trial manifest mismatch"):
        artifact_oracle({"case_id": "x", "workspace": str(root), "manifest_hash": "other"}, manifest)
