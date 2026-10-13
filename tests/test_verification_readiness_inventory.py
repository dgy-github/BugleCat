import importlib.util
import json
from pathlib import Path

import pytest

SPEC = importlib.util.spec_from_file_location(
    "readiness", Path(__file__).resolve().parents[1] / "scripts/verification_readiness_inventory.py")
readiness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(readiness)


def test_aggregate_export_never_proves_request_reconciliation(tmp_path):
    path = tmp_path / "usage.csv"
    path.write_text("day,api_key_id,user_email,requests,input_tokens,output_tokens\n"
                    "2026-10-13T02:00:00+00:00,private-key,private-email,313,1257312,7825\n")
    result = readiness.provider_summary(path)
    assert result["totals"]["requests"] == 313
    assert result["per_request_reconciled"] is False
    assert result["automatic_retries"] is None
    assert "private-" not in json.dumps(result)


@pytest.mark.parametrize("value", ["-1", "1.2", "nan"])
def test_invalid_count_refused(tmp_path, value):
    path = tmp_path / "usage.csv"
    path.write_text(f"day,requests,input_tokens,output_tokens\ndate,{value},1,1\n")
    with pytest.raises(ValueError):
        readiness.provider_summary(path)


def test_thread_inventory_is_only_unreviewed_upper_bound(tmp_path):
    items = [{"type": "assistantMessage", "text": "private-answer"}, {"type": "toolResult"}]
    value = {"threads": {
        "a": {"metadata": {"workspace": "real-project"}, "turns": [{"items": items}]},
        "b": {"metadata": {"workspace": "fixture-e2e"}, "turns": [{"items": items}]},
        "c": {"metadata": {"workspace": "real-project"}, "turns": []}}}
    path = tmp_path / "threads.json"
    path.write_text(json.dumps(value))
    result = readiness.thread_inventory(path)
    assert result["counts"] == {"unreviewed_candidate": 1, "fixture_marker": 1, "missing_answer_or_tool": 1}
    assert result["certification_eligible_groups"] is None
    assert "private-answer" not in json.dumps(result)
    assert "real-project" not in json.dumps(result)
