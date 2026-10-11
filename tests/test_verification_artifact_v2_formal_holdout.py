"""No real holdout opening: temporary markers and mocked original proof only."""
import json

import pytest

from evals.verification import artifact_v2_formal as statistical
from evals.verification import artifact_v2_formal_cohort as sources
from evals.verification import artifact_v2_formal_holdout as owner


def fixture(tmp_path, monkeypatch):
    cohort_file = tmp_path / "cohort.json"
    cohort_file.write_text("original cohort bytes")
    calibration_file = tmp_path / "calibration.json"
    calibration_file.write_text("original calibration bytes")
    registration_file = tmp_path / "registration.json"
    registration_file.write_text("original label bytes")
    observations_file = tmp_path / "observations.jsonl"
    observations_file.write_text("original observation bytes")
    cal = [{"id": f"cal-{n}", "group_id": f"cal-{n}", "split": "calibration"} for n in range(235)]
    hold = [{"id": f"hold-{n}", "group_id": f"hold-{n}", "split": "holdout"} for n in range(240)]
    cohort = {"artifact_hash": "fixed-source", "cases": cal + hold, "excluded_groups": ["old-whole-group"],
              "bindings": [{"role": "authorization", "file": "fixture-auth"}]}
    result = {"artifact_hash": "qualified-calibration", "qualified_for_holdout_threshold": .9,
              "calibration_numeric_gate_met": True, "registration_file": str(registration_file),
              "observations_file": str(observations_file)}
    data = {"cohort_file": str(cohort_file), "cohort_hash": "fixed-source", "cohort_sha256": owner.file_hash(cohort_file),
            "split": "calibration", "cases": cal, "collected_groups": 235}
    registration = {"artifact_hash": "registered-before-jev"}
    monkeypatch.setattr(statistical, "revalidate_calibration", lambda _: (result, registration, data, [], []))
    monkeypatch.setattr(sources, "validate", lambda _: (cohort, [], {}))
    monkeypatch.setattr(sources, "raw_ledger", lambda _: ({"excluded_groups": [], "remaining_main_attempts": 240, "remaining_jev_attempts": 328}, {}))
    monkeypatch.setattr(owner, "resolve", lambda _: {"authorization": {"main_model": {"ledger_dir": str(tmp_path / "ledger")}}})
    return calibration_file, cohort_file, result, data, cohort


@pytest.mark.parametrize("fault", ["no_threshold", "false_gate", "uncollected", "missing_cal_group", "holdout_overlap"])
def test_unqualified_or_incomplete_proof_never_consumes_source(tmp_path, monkeypatch, fault):
    calibration, cohort_file, result, data, cohort = fixture(tmp_path, monkeypatch)
    if fault == "no_threshold":
        result["qualified_for_holdout_threshold"] = None
    elif fault == "false_gate":
        result["calibration_numeric_gate_met"] = False
    elif fault == "uncollected":
        data["collected_groups"] = 234
    elif fault == "missing_cal_group":
        data["cases"] = data["cases"][:-1]
    else:
        cohort["cases"][-1]["group_id"] = "cal-0"
    with pytest.raises(ValueError):
        owner.open_source(calibration, cohort_file, tmp_path / "opening.json")
    assert not (tmp_path / "ledger").exists()


def test_shared_marker_once_and_fixed_complete_allocation(tmp_path, monkeypatch):
    calibration, cohort_file, _, _, _ = fixture(tmp_path, monkeypatch)
    output = tmp_path / "opening.json"
    value = owner.open_source(calibration, cohort_file, output)
    assert owner.validate(output) == value
    assert len(value["cases"]) == 240 and len(value["excluded_groups"]) == 236
    assert value["threshold"] == .9 and value["production_authorized"] is False
    with pytest.raises(FileExistsError):
        owner.open_source(calibration, cohort_file, tmp_path / "alternate.json")
    assert not (tmp_path / "alternate.json").exists()


def test_partial_write_burns_source_marker_and_preserves_original(tmp_path, monkeypatch):
    calibration, cohort_file, _, _, _ = fixture(tmp_path, monkeypatch)
    output = tmp_path / "existing.json"
    output.write_text("preserved")
    with pytest.raises(FileExistsError):
        owner.open_source(calibration, cohort_file, output)
    assert output.read_text() == "preserved"
    assert len(list((tmp_path / "ledger/formal-v2-holdout-source").glob("*.json"))) == 1
    with pytest.raises(FileExistsError):
        owner.open_source(calibration, cohort_file, tmp_path / "retry.json")


def test_resealed_threshold_or_marker_tampering_rejected(tmp_path, monkeypatch):
    calibration, cohort_file, _, _, _ = fixture(tmp_path, monkeypatch)
    output = tmp_path / "opening.json"
    value = owner.open_source(calibration, cohort_file, output)
    body = {key: item for key, item in value.items() if key != "artifact_hash"}
    body["threshold"] = .95
    output.write_text(json.dumps({**body, "artifact_hash": owner.digest(body)}))
    with pytest.raises(ValueError, match="threshold"):
        owner.validate(output)
