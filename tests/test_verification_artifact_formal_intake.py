import json
from pathlib import Path

import pytest

from evals.verification.artifact_formal import register
from evals.verification.artifact_formal_intake import cohort, ledger_start, verify_dataset_cases
from evals.verification.calibration import digest


def write(path, body):
    value = {**body, "artifact_hash": digest(body)}
    path.write_text(json.dumps(value), encoding="utf-8")
    return value


def source(tmp_path):
    path = tmp_path / "cohort.json"
    write(path, {"schema": "buglecat.verification.artifact-cohort.v1", "registration_kind": "collection_preregistration",
                 "status": "registered", "formal_registration": True, "full_campaign_source_gate": True,
                 "split": "calibration", "before_main_requests": True, "scores_hidden": True,
                 "jev_probabilities_read": False, "author_exposed": False, "bindings": []})
    return path


def test_rejected_or_fixture_source_cannot_register_formal(tmp_path):
    path = source(tmp_path)
    value = json.loads(path.read_text())
    for status in ("rejected", "fixture_only"):
        body = {key: val for key, val in value.items() if key != "artifact_hash"}
        body["status"] = status
        write(path, body)
        with pytest.raises(ValueError, match="not registered"):
            cohort(path)


def test_development_dataset_rejected_before_predictions_or_source_execution(tmp_path):
    path = tmp_path / "development.json"
    write(path, {"schema": "buglecat.verification.artifact-dataset.v1", "scores_hidden": True,
                 "jev_probabilities_read": False})
    with pytest.raises(ValueError, match="formal original dataset"):
        register(path, tmp_path / "missing-prepared", tmp_path / "output")
    assert not (tmp_path / "output").exists()


def test_real_old_start_cannot_be_upgraded_with_new_cohort_file(tmp_path):
    prepared = Path("reports/benchmark-collection-batch02-20261010-resource-r1/prepared.jsonl")
    if not prepared.exists():
        pytest.skip("original private local collection not present")
    original = next(json.loads(line) for line in prepared.read_text().splitlines()
                    if json.loads(line)["id"] == "mbpp-main-104")
    original["authorization_file"] = "evals/verification/reports/jev_budget_extension_20261011_100.json"
    with pytest.raises(ValueError, match="lacks preregistered cohort"):
        ledger_start(original, source(tmp_path))


def original_reviewed():
    path = Path("reports/artifact-ultra-first3-reviewed-dataset-20261011.json")
    if not path.is_file():
        pytest.skip("private original reviewed receipt set unavailable")
    return json.loads(path.read_text(encoding="utf-8"))


def test_original_started_group_cannot_be_dropped_from_formal_denominator():
    data = original_reviewed()
    data["cases"] = data["cases"][1:]
    with pytest.raises(ValueError, match="omitted an actual main attempt"):
        verify_dataset_cases(data)


def test_original_valid_label_cannot_be_hidden_as_unknown():
    data = original_reviewed()
    case = next(case for case in data["cases"] if case["label"] == "supported")
    case.update(label="unknown", call_eligible=False)
    with pytest.raises(ValueError, match="overrides original Oracle"):
        verify_dataset_cases(data)
