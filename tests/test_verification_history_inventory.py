import json

from evals.verification.history_inventory import inventory


def test_inventory_never_exports_secrets_or_prose(tmp_path):
    source = tmp_path / "secret-session.jsonl"
    messages = [
        {"role": "user", "content": "private-person apikey_SECRET"},
        {"role": "assistant", "tool_calls": [{"id": "a", "function": {"name": "shell"}}]},
        {"role": "tool", "name": "shell", "tool_call_id": "a", "content": "private API key\nExit code: 1"},
        {"role": "tool", "name": "read_file", "tool_call_id": "b", "content": "Exit code: 0"},
    ]
    source.write_text("\n".join(json.dumps(m) for m in messages), encoding="utf-8")
    report = inventory(source)
    serialized = json.dumps(report)
    assert "private" not in serialized and "SECRET" not in serialized
    assert "secret-session" not in serialized
    assert report["execution_counts"] == {"failed": 1, "unknown": 1}
    assert report["rows"][0]["same_turn_reference"]
    assert report["rows"][1]["completion_label"] == "unlabeled"


def test_ambiguous_exit_status_is_unknown(tmp_path):
    source = tmp_path / "session.jsonl"
    source.write_text(json.dumps({"role": "tool", "name": "shell", "content":
                                  "Exit code: 0\nExit code: 1"}), encoding="utf-8")
    assert inventory(source)["rows"][0]["execution_label"] == "unknown"


def test_unlinked_or_foreign_turn_result_cannot_supply_execution_label(tmp_path):
    source = tmp_path / "session.jsonl"
    messages = [
        {"role": "user", "content": "first"},
        {"role": "assistant", "tool_calls": [{"id": "a", "function": {"name": "shell"}}]},
        {"role": "user", "content": "second"},
        {"role": "tool", "name": "shell", "tool_call_id": "a", "content": "Exit code: 0"},
        {"role": "tool", "name": "shell", "tool_call_id": "missing", "content": "Exit code: 0"},
    ]
    source.write_text("\n".join(json.dumps(m) for m in messages), encoding="utf-8")
    report = inventory(source)
    assert report["execution_counts"] == {"unknown": 2}
