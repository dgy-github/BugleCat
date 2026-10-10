import json
from pathlib import Path

import pytest

from evals.verification.calibration_inputs import adapt

ROOT = Path(__file__).resolve().parents[1]


def test_old_holdout_is_all_development_and_no_provenance_invented():
    data, observations, protocol = adapt(
        ROOT / "evals/verification/cases.json",
        ROOT / "evals/verification/reports/jev_live_20261010.jsonl",
    )
    assert len(data["cases"]) == len(observations) == 18
    assert {c["split"] for c in data["cases"]} == {"development"}
    assert all(c["author_exposed"] and c["review_status"] == "pending" for c in data["cases"])
    assert {o["template_version"] for o in observations} == {"unrecorded-legacy-fixture"}
    assert data["new_jev_attempts"] == 0
    assert protocol["categories"] == ["semantic"]
    assert all(c["input_binding_status"].startswith("derived_local") for c in data["cases"])
    assert all(not c["eligible"] for c in data["cases"] if c["context"] == "non_asserted")


def test_duplicate_paid_observation_cannot_be_double_counted(tmp_path):
    source = ROOT / "evals/verification/reports/jev_live_20261010.jsonl"
    lines = source.read_text(encoding="utf-8").splitlines()
    output = tmp_path / "duplicated.jsonl"
    output.write_text("\n".join([*lines, lines[0]]), encoding="utf-8")
    with pytest.raises(ValueError, match="duplicate"):
        adapt(ROOT / "evals/verification/cases.json", output)


def test_model_versions_cannot_be_pooled(tmp_path):
    source = ROOT / "evals/verification/reports/jev_live_20261010.jsonl"
    rows = [json.loads(line) for line in source.read_text(encoding="utf-8").splitlines()]
    rows[0]["response"]["model"] = "other-version"
    output = tmp_path / "mixed.jsonl"
    output.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
    with pytest.raises(ValueError, match="one actual"):
        adapt(ROOT / "evals/verification/cases.json", output)
