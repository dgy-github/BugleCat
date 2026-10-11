"""Code fixtures exercise holdout guards, never genuine calibration evidence."""

import json
from pathlib import Path

import pytest

from evals.verification import artifact_formal
from evals.verification import artifact_holdout as holdout
from evals.verification.calibration import digest, load
from evals.verification.collection_manifest import file_hash
from evals.verification.paid_budget import durable_new

ROOT = Path(__file__).resolve().parents[1]
SPEC = ROOT / "evals/verification/reports/artifact_protocol_20261011_v1.json"


def seal(path, body):
    value = {**body, "artifact_hash": digest(body)}
    durable_new(path, value)
    return value


def fixture(tmp_path, monkeypatch):
    """Mock only previously verified provenance; test real gate/ordering/files."""
    source, catalog, auth, spec = [tmp_path / name for name in ("source.json", "catalog.json", "auth.json", "spec.json")]
    spec.write_bytes(SPEC.read_bytes())
    tasks = [{"task_id": n, "code": f"def f(x): return x * {n}"} for n in range(1, 223)]
    source.write_text(json.dumps(tasks), encoding="utf-8")
    index = {"source_sha256": file_hash(source), "split_seed": "fixed-fixture-seed", "tasks": [
        {"task_id": task["task_id"], "group_id": f"g{task['task_id']}", "task_sha256": digest(task),
         "reserved_split": "calibration" if task["task_id"] <= 10 else "holdout"} for task in tasks]}
    catalog.write_text(json.dumps(index), encoding="utf-8")
    auth.write_text("{}", encoding="utf-8")
    original_file = tmp_path / "calibration-cohort.json"
    baseline = {"main_ledger_file": str(tmp_path / "main-attempts.sqlite3"),
                "main_started": [], "main_reservations": [], "jev_reservations": [], "main_metadata": [],
                "excluded_groups": []}
    original = seal(original_file, {"schema": "buglecat.verification.artifact-cohort.v1",
        "registration_kind": "collection_preregistration", "status": "registered", "formal_registration": True,
        "full_campaign_source_gate": True, "fixture_only": False, "split": "calibration",
        "before_main_requests": True, "scores_hidden": True, "jev_probabilities_read": False,
        "author_exposed": False, "holdout_opened": False,
        "cases": [{"group_id": f"g{n}"} for n in range(1, 11)], "ledger_snapshot": baseline,
        "main_collection_settings": {"max_retries": 0}, "main_system_prompt_sha256": "a" * 64,
        "bindings": [holdout._bound(role, path) for role, path in zip(
            ("source", "catalog", "authorization", "protocol"), (source, catalog, auth, spec))]})
    registration_file = tmp_path / "calibration-registration.json"
    registration = seal(registration_file, {"artifact_cohort_file": str(original_file), "protocol": load(spec)})
    observations = tmp_path / "calibration-observations.jsonl"
    observations.write_text("[]\n", encoding="utf-8")
    calibration_file = tmp_path / "calibration.json"
    value = seal(calibration_file, {"schema": "buglecat.verification.artifact-formal-calibration.v1",
        "calibration_numeric_gate_met": True, "qualified_for_holdout_threshold": .9,
        "certified_threshold": None, "production_authorized": False, "holdout_status": "not_run",
        "registration_file": str(registration_file), "registration_file_sha256": file_hash(registration_file),
        "registration_hash": registration["artifact_hash"], "observations_file": str(observations),
        "observations_file_sha256": file_hash(observations), "observations_hash": digest([]),
        "calibration_groups": ["g1", "g2"]})
    # No fixture can pass the production shared verifier: explicitly replaced here.
    def verified(path):
        candidate = load(path)
        holdout.sealed(candidate)
        assert file_hash(observations) == candidate["observations_file_sha256"]
        return candidate, registration, {}, [], []
    monkeypatch.setattr(artifact_formal, "revalidate_calibration", verified)
    monkeypatch.setattr(holdout.artifact_cohort, "external_bindings", lambda *_: [])
    monkeypatch.setattr(holdout.artifact_cohort.artifact_source_registry, "inventory", lambda *_: {})
    snapshot = {**baseline,
                "excluded_groups": ["g11", "g12"], "remaining_main_attempts": 202, "remaining_jev_reservations": 203}
    monkeypatch.setattr(holdout.artifact_cohort, "ledger_snapshot", lambda _: (snapshot, []))
    return calibration_file, value, original, snapshot


def test_frozen_source_order_disjoint_full_calibration_and_prior_groups(tmp_path, monkeypatch):
    path, value, original, _ = fixture(tmp_path, monkeypatch)
    output = tmp_path / "holdout.json"
    result = holdout.register_source(path, "fixture-gate-unit", output)
    groups = {case["group_id"] for case in result["cases"]}
    assert len(groups) == 202 and groups.isdisjoint({"g11", "g12"})
    assert groups.isdisjoint({case["group_id"] for case in original["cases"]})
    assert result["holdout_lock"]["calibration_allocation_groups"] == sorted(f"g{n}" for n in range(1, 11))
    assert result["holdout_lock"]["threshold"] == .9
    assert result["holdout_lock"]["observations_sha256"] == value["observations_file_sha256"]
    assert result["collection_authorized"] is False and result["production_authorized"] is False
    assert result["certified_threshold"] is None and result["new_paid_calls"] == 0
    assert all(case["label"] == "unlabeled" and case["split"] == "holdout" for case in result["cases"])
    assert not any("test_list" in case or "code" in case or "noul" in case for case in result["cases"])
    assert holdout.validate(output) == result


def test_open_once_survives_input_copy_and_alternate_output(tmp_path, monkeypatch):
    path, _, _, _ = fixture(tmp_path, monkeypatch)
    output = tmp_path / "holdout.json"
    holdout.register_source(path, "one", output)
    alias = tmp_path / "copied-calibration.json"
    alias.write_bytes(path.read_bytes())
    with pytest.raises(FileExistsError):
        holdout.register_source(alias, "two", tmp_path / "alternate.json")
    assert not (tmp_path / "alternate.json").exists()
    copy = tmp_path / "copied-cohort.json"
    copy.write_bytes(output.read_bytes())
    with pytest.raises(ValueError, match="reopening"):
        holdout.validate(copy)


@pytest.mark.parametrize("mutation", ["unqualified", "threshold", "observations", "fixture", "budget"])
def test_gate_fails_before_opening_marker(tmp_path, monkeypatch, mutation):
    path, value, original, snapshot = fixture(tmp_path, monkeypatch)
    if mutation in {"unqualified", "threshold"}:
        body = {key: val for key, val in value.items() if key != "artifact_hash"}
        body["calibration_numeric_gate_met" if mutation == "unqualified" else "qualified_for_holdout_threshold"] = (
            False if mutation == "unqualified" else .123)
        path.unlink()
        seal(path, body)
    elif mutation == "observations":
        Path(value["observations_file"]).write_text("tampered", encoding="utf-8")
    elif mutation == "fixture":
        original_path = tmp_path / "calibration-cohort.json"
        original_path.unlink()
        seal(original_path, {**{key: val for key, val in original.items() if key != "artifact_hash"}, "fixture_only": True})
    else:
        snapshot["remaining_main_attempts"] = 199
    with pytest.raises((ValueError, AssertionError)):
        holdout.register_source(path, "bad", tmp_path / "holdout.json")
    assert not (tmp_path / "holdout-gates").exists()
    assert not (tmp_path / "holdout.json").exists()


def test_legacy_state_string_without_original_calibration_is_rejected(tmp_path):
    path = tmp_path / "legacy.json"
    seal(path, {"schema": "buglecat.verification.artifact-formal-calibration.v1",
                "calibration_numeric_gate_met": True, "qualified_for_holdout_threshold": .9})
    with pytest.raises((ValueError, KeyError)):
        holdout.register_source(path, "legacy", tmp_path / "holdout.json")
    assert not (tmp_path / "holdout.json").exists()


def registration_fixture(tmp_path, monkeypatch, supported_n=150):
    path, _, _, _ = fixture(tmp_path, monkeypatch)
    cohort_file = tmp_path / "holdout.json"
    registered = holdout.register_source(path, "fixture-gate-unit", cohort_file)
    cases, rows = [], []
    for n, source_case in enumerate(registered["cases"]):
        label = "supported" if n < supported_n else "unsupported" if n < 200 else "unknown"
        case = {**source_case, "label": label, "call_eligible": label != "unknown",
                "source_kind": "source_preregistered_original_model_code", "candidate_sha256": "a" * 64,
                "main_receipt_sha256": "b" * 64}
        if label != "unknown":
            oracle_body = {"result": {"status": "passed" if label == "supported" else "failed"}}
            oracle = {**oracle_body, "artifact_hash": digest(oracle_body)}
            case["oracle"] = oracle
            row_body = {"id": case["id"], "group_id": case["group_id"], "split": "holdout",
                        "artifact_cohort_hash": registered["artifact_hash"], "candidate_sha256": case["candidate_sha256"],
                        "main_receipt_sha256": case["main_receipt_sha256"], "oracle_sha256": oracle["artifact_hash"]}
            rows.append({**row_body, "artifact_sha256": digest(row_body)})
        cases.append(case)
    dataset_file, prepared_file = tmp_path / "labels.json", tmp_path / "prepared.jsonl"
    seal(dataset_file, {"schema": "buglecat.verification.artifact-formal-dataset.v1", "cases": cases,
        "scores_hidden": True, "jev_probabilities_read": False, "artifact_cohort_file": str(cohort_file),
        "artifact_cohort_sha256": file_hash(cohort_file), "artifact_cohort_hash": registered["artifact_hash"]})
    prepared_file.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
    monkeypatch.setattr(holdout, "verify_prepared", lambda _: registered)
    # These fixtures test the one-use gate only; actual main provenance is
    # independently exercised against retained original receipts by intake tests.
    monkeypatch.setattr(holdout, "verify_dataset_cases", lambda _: registered)
    reg_file = tmp_path / "holdout-registration.json"
    reg = holdout.register_labels(cohort_file, dataset_file, prepared_file, reg_file)
    return cohort_file, reg_file, reg, cases, rows


def test_labels_freeze_once_preserves_unknown_and_cannot_relabel(tmp_path, monkeypatch):
    cohort_file, reg_file, reg, cases, _ = registration_fixture(tmp_path, monkeypatch)
    assert len(reg["case_ids"]) == 200 and sum(c["label"] == "unknown" for c in cases) == 2
    assert reg["threshold"] == .9
    with pytest.raises(ValueError, match="already frozen"):
        holdout.register_labels(cohort_file, reg["dataset_file"], reg["prepared_file"], tmp_path / "reg2.json")
    Path(reg["dataset_file"]).write_text("tampered", encoding="utf-8")
    with pytest.raises(ValueError, match="labels/requests changed"):
        holdout._label_registration(reg_file)


def test_holdout_label_quota_must_pass_before_any_jev_registration(tmp_path, monkeypatch):
    with pytest.raises(ValueError, match="binary label quotas"):
        registration_fixture(tmp_path, monkeypatch, supported_n=149)
    assert not (tmp_path / "holdout-registration.json").exists()
    assert not list((tmp_path / "holdout-gates").glob("*.labels.started"))


@pytest.mark.parametrize("failed_consumption", [False, True])
def test_observations_consumed_once_even_failure_cannot_change_output(tmp_path, monkeypatch, failed_consumption):
    from evals.verification import artifact_formal_observations
    cohort_file, reg_file, reg, cases, rows = registration_fixture(tmp_path, monkeypatch)
    obs_file = tmp_path / "observations.jsonl"
    by_id = {case["id"]: case for case in cases}
    observations = [{"id": row["id"], "call_status": "ok", "model": reg["protocol"]["model"],
        "acceptance_violation": 0 if by_id[row["id"]]["label"] == "supported" else 1,
        "request_hash": "c" * 64, "actual_request_hash": "c" * 64, "response_sha256": "d" * 64,
        "transport_binding_status": "original_transport_bytes_verified", "original_events_file": "fixture",
        "reservation_file": "fixture", "runner_executable_file": "fixture", "runner_source_file": "fixture"}
        for row in rows]
    obs_file.write_text("".join(json.dumps(row) + "\n" for row in observations), encoding="utf-8")
    observed_by_id = {row["id"]: row for row in observations}
    def verified(row, *_):
        if failed_consumption:
            raise ValueError("original transport invalid fixture")
        return observed_by_id[row["id"]]
    monkeypatch.setattr(artifact_formal_observations, "observe", verified)
    output = tmp_path / "result.json"
    if failed_consumption:
        with pytest.raises(ValueError, match="original transport invalid"):
            holdout.consume(reg_file, obs_file, output)
        assert not output.exists()
    else:
        result = holdout.consume(reg_file, obs_file, output)
        assert result["threshold"] == .9 and result["holdout_numeric_gate_met"] is True
        assert result["unknown_source_groups"] == 2 and result["allocated_groups"] == 202
        assert result["verified_pass_signals"] == 0 and result["certified_threshold"] is None
        assert not result["production_authorized"]
        assert result["artifact_benchmark_certified"] is True
        assert result["certified_artifact_threshold"] == .9
        assert result["overall_agent_decision_benefit_certified"] is False
        assert holdout.revalidate_evaluation(output) == result
        modified = {key: value for key, value in result.items() if key != "artifact_hash"}
        modified["certified_artifact_threshold"] = .5
        output.write_text(json.dumps({**modified, "artifact_hash": digest(modified)}), encoding="utf-8")
        with pytest.raises(ValueError, match="differs from original"):
            holdout.revalidate_evaluation(output)
    assert holdout._marker(load(cohort_file), ".evaluation.started").exists()
    with pytest.raises(FileExistsError):
        holdout.consume(reg_file, obs_file, tmp_path / "another-result.json")
    assert not (tmp_path / "another-result.json").exists()


def test_failed_jev_attempt_retained_in_unsupported_denominator(tmp_path, monkeypatch):
    # Exact statistical owner is reused; missing observations are never imputed.
    spec = load(SPEC)
    cases = [{"id": f"c{n}", "group_id": f"g{n}", "label": "supported" if n < 150 else "unsupported"}
             for n in range(200)]
    rows = [{"id": case["id"], "call_status": "timeout", "acceptance_violation": None} for case in cases]
    result = holdout.score(cases, rows, .9, spec)
    assert result["unsupported_groups"] == 50 and result["supported_groups"] == 150
    assert result["detected_unsupported"] == 0 and result["verified_pass_signals"] == 0
    assert not result["gate_met"] and len(result["failed_attempts"]) == 200


@pytest.mark.parametrize("reset", ["main_started", "main_reservations", "jev_reservations", "main_metadata", "root", "excluded"])
def test_old_failures_reservations_or_metadata_cannot_reset(reset):
    baseline = {"main_ledger_file": "main.sqlite", "main_started": [{"group_id": "old", "started": {"failed": True}}],
                "main_reservations": [{"group_id": "reserved-main"}], "jev_reservations": [{"group_id": "reserved-jev"}],
                "main_metadata": [["authorization_sha256", "original"]], "excluded_groups": ["old"]}
    current = {**baseline}
    if reset == "root":
        current["main_ledger_file"] = "different.sqlite"
    elif reset == "excluded":
        current["excluded_groups"] = []
    else:
        current[reset] = []
    with pytest.raises(ValueError, match="root changed|reset|restored"):
        holdout._preserve_ledger(baseline, current)
