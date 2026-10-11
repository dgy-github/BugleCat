"""Owner composition fixtures are explicitly mocks, never certification samples."""
import copy
import json
from pathlib import Path

import pytest

from evals.verification import artifact_holdout_transport_guard as guard
from evals.verification.calibration import digest, load

ROOT = Path(__file__).resolve().parents[1]


def dump(path, value):
    path.write_text(json.dumps(value), encoding="utf-8")
    return path


@pytest.fixture
def main_fixture(tmp_path, monkeypatch):
    cases = [{"id": f"fixture-{n}", "group_id": f"fixture-group-{n}", "split": "holdout"} for n in range(4)]
    value = {"artifact_hash": "fixture-no-certification", "cases": cases,
             "holdout_lock": {"threshold": 0.95},
             **{field: "fixture" for field in guard.PROFILE_FIELDS}}
    cohort = dump(tmp_path / "cohort.json", value)
    manifest = dump(tmp_path / "manifest.json", {"artifact_cohort_file": str(cohort), "cases": cases[:2],
                                                "authorization_file": "fixture"})
    monkeypatch.setattr(guard.artifact_holdout, "validate", lambda _: copy.deepcopy(value))
    monkeypatch.setattr(guard.artifact_holdout, "_marker", lambda *_: tmp_path / "labels.started")
    monkeypatch.setattr(guard, "_bound_manifest", lambda *_: None)
    monkeypatch.setattr(guard.artifact_cohort, "ledger_snapshot", lambda _: ({"excluded_groups": []}, []))
    return manifest, value, cases


def test_main_owner_proof_fixed_prefix_and_resume_after_prior_member_started(main_fixture, monkeypatch):
    manifest, _, cases = main_fixture
    assert guard.main_proof(manifest)["selected_ids"] == [case["id"] for case in cases[:2]]
    monkeypatch.setattr(guard.artifact_cohort, "ledger_snapshot",
                        lambda _: ({"excluded_groups": [cases[0]["group_id"]]}, []))
    assert guard.main_proof(manifest, cases[1]["id"])["selected_ids"] == [cases[1]["id"]]
    with pytest.raises(ValueError):
        guard.main_proof(manifest, cases[0]["id"])
    with pytest.raises(ValueError):
        guard.main_proof(manifest)


def test_source_reordering_and_owner_failure_fail_closed(main_fixture, monkeypatch):
    manifest, _, cases = main_fixture
    body = load(manifest)
    body["cases"] = [cases[1], cases[0]]
    dump(manifest, body)
    with pytest.raises(ValueError):
        guard.main_proof(manifest)
    monkeypatch.setattr(guard.artifact_holdout, "validate", lambda _: (_ for _ in ()).throw(ValueError("unqualified")))
    with pytest.raises(ValueError, match="unqualified"):
        guard.main_proof(manifest)


def test_main_closed_after_label_registration(main_fixture):
    manifest, _, _ = main_fixture
    (manifest.parent / "labels.started").write_text("fixture", encoding="utf-8")
    with pytest.raises(ValueError, match="closed"):
        guard.main_proof(manifest)


def test_actual_unqualified_calibration_cannot_open_holdout(tmp_path):
    cohort = ROOT / "reports/artifact-profile-cohort-high-20261011-v1.json"
    if not cohort.exists():
        pytest.skip("private registered calibration cohort unavailable")
    manifest = dump(tmp_path / "manifest.json", {"artifact_cohort_file": str(cohort)})
    with pytest.raises(ValueError):
        guard.main_proof(manifest)
    assert list(tmp_path.iterdir()) == [manifest]


def jev_fixture(tmp_path, monkeypatch, supported=150, unsupported=50, unknown=1):
    cases, rows = [], []
    for n, label in enumerate(["supported"] * supported + ["unsupported"] * unsupported + ["unknown"] * unknown):
        status = {"supported": "passed", "unsupported": "failed", "unknown": "unknown"}[label]
        oracle = {"result": {"status": status}}
        oracle["artifact_hash"] = digest(oracle)
        case = {"id": f"fixture-{n}", "group_id": f"fixture-group-{n}", "split": "holdout",
                "author_exposed": False, "call_eligible": label != "unknown", "label": label,
                "candidate_sha256": "fixture-candidate", "main_receipt_sha256": "fixture-main", "oracle": oracle}
        cases.append(case)
        if label != "unknown":
            row = {key: case[key] for key in ("id", "group_id", "split", "candidate_sha256", "main_receipt_sha256")}
            row.update(oracle_sha256=oracle["artifact_hash"], **{field: "fixture" for field in guard.PROFILE_FIELDS})
            row["artifact_sha256"] = digest(row)
            rows.append(row)
    spec = load(ROOT / "evals/verification/reports/artifact_protocol_20261011_v1.json")
    registered = {"artifact_hash": "fixture", "cases": cases, "holdout_lock": {"threshold": 0.95},
                  **{field: "fixture" for field in guard.PROFILE_FIELDS}}
    cohort = dump(tmp_path / "cohort.json", registered)
    prepared = tmp_path / "prepared.jsonl"
    prepared.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
    reg = {"artifact_cohort_file": str(cohort), "prepared_file": str(prepared),
           "prepared_file_sha256": guard.file_hash(prepared), "artifact_hash": "fixture",
           "case_ids": sorted(row["id"] for row in rows), "protocol": spec,
           "label_counts": {"supported": supported, "unsupported": unsupported, "unknown": unknown},
           **{field: "fixture" for field in guard.PROFILE_FIELDS}}
    registration = dump(tmp_path / "registration.json", reg)
    data = {"cases": cases, **{field: "fixture" for field in guard.PROFILE_FIELDS}}
    monkeypatch.setattr(guard.artifact_holdout, "_label_registration", lambda _: (reg, data, rows))
    monkeypatch.setattr(guard.artifact_holdout, "validate", lambda _: registered)
    monkeypatch.setattr(guard.artifact_holdout, "_marker", lambda *_: tmp_path / "evaluation.started")
    monkeypatch.setattr(guard, "verify_dataset_cases", lambda _: None)
    monkeypatch.setattr(guard, "verify_prepared", lambda _: None)
    return prepared, registration, data, rows


def test_jev_full_original_oracle_join_and_unknown_independent(tmp_path, monkeypatch):
    prepared, registration, data, rows = jev_fixture(tmp_path, monkeypatch)
    value = guard.jev_proof(prepared, rows[0]["id"], registration)
    assert value["holdout_lock"]["threshold"] == 0.95 and value["production_authorized"] is False
    assert value["selected_groups"] == [rows[0]["group_id"]]
    data["cases"][0]["label"] = "unsupported"
    with pytest.raises(ValueError):
        guard.jev_proof(prepared, rows[0]["id"], registration)


@pytest.mark.parametrize("positive,negative", [(149, 50), (150, 49)])
def test_jev_insufficient_labels_fail_not_lowered(tmp_path, monkeypatch, positive, negative):
    prepared, registration, _, rows = jev_fixture(tmp_path, monkeypatch, positive, negative)
    with pytest.raises(ValueError, match="quotas"):
        guard.jev_proof(prepared, rows[0]["id"], registration)


def test_jev_source_owner_failure_and_consumed_lock_are_rejected(tmp_path, monkeypatch):
    prepared, registration, _, rows = jev_fixture(tmp_path, monkeypatch)
    monkeypatch.setattr(guard, "verify_dataset_cases", lambda _: (_ for _ in ()).throw(ValueError("raw original missing")))
    with pytest.raises(ValueError, match="raw original"):
        guard.jev_proof(prepared, rows[0]["id"], registration)
    (tmp_path / "evaluation.started").write_text("fixture", encoding="utf-8")
    with pytest.raises(ValueError, match="consumed"):
        guard.jev_proof(prepared, rows[0]["id"], registration)
