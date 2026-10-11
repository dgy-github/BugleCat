"""Offline diagnostic contract fixtures; original proof owner is mocked explicitly."""

import copy
import json
from pathlib import Path

import pytest

from evals.verification import artifact_diagnostics as diagnostic
from evals.verification.artifact_acceptance import request
from evals.verification.artifact_gate import calibrate
from evals.verification.calibration import digest


@pytest.fixture
def proof(tmp_path, monkeypatch):
    source, manifest, cohort, calibration = [tmp_path / name for name in ("source", "manifest", "cohort", "calibration")]
    tasks = [{"task_id": n, "prompt": "fixture-public-task", "test_imports": [],
              "test_list": ["assert f(1) == 1"]} for n in (1, 2, 3)]
    source.write_text(json.dumps(tasks), encoding="utf-8")
    manifest.write_text(json.dumps({"cases": [{"id": str(n), "public_interface": ["def f(x):"]} for n in (1, 2, 3)]}))
    cohort.write_text(json.dumps({"cases": [{} for _ in range(5)], "artifact_hash": "fixture-cohort"}))
    calibration.write_text("{}")
    rows, observations, cases = [], [], []
    for n, label, status, p, error in [(1, "supported", "ok", .6, None),
                                      (2, "unsupported", "ok", .2, "AssertionError"),
                                      (3, "unsupported", "timeout", None, "TypeError")]:
        folder = tmp_path / str(n)
        folder.mkdir()
        code = "def f(x):\n    return x\n# candidate-private-marker"
        body = request(tasks[n - 1], ["def f(x):"], code)
        raw = json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
        (folder / "request.json").write_bytes(raw)
        rows.append({"id": str(n), "group_id": f"group-{n}", "task_id": n, "source_file": str(source),
                     "collection_manifest_file": str(manifest), "request": body, "request_hash": digest(body)})
        observations.append({"id": str(n), "call_status": status, "acceptance_violation": p, "model": "jev-1.13.0",
                             "transport_binding_status": "original_transport_bytes_verified", "actual_request_hash": digest(body),
                             "request_hash": digest(body), "response_sha256": "fixture-response", "usage": {"input_tokens": 10},
                             "original_events_file": str(folder / "events.jsonl")})
        result = {"status": "passed"} if label == "supported" else {
            "status": "failed", "error_kind": error, "tests_passed_before_failure": 0, "reference": {"status": "passed"}}
        cases.append({"id": str(n), "group_id": f"group-{n}", "label": label, "oracle": {"result": result}})
    cases.append({"id": "unknown", "group_id": "unknown", "label": "unknown", "oracle": {
        "result": {"status": "unknown", "reason": "frozen_reference_does_not_pass"}}})
    spec = {"schema": "buglecat.verification.artifact-protocol.v1", "policy_version": "artifact-acceptance-v1",
            "template_version": "jev-artifact-acceptance-v1", "categories": ["artifact_acceptance"], "model": "jev-1.13.0",
            "min_supported": 150, "min_unsupported": 50, "max_false_block_rate": .02, "confidence": .95,
            "min_error_rejection_lower_bound": .5, "threshold_grid": [.5, .6, .7, .8, .9, .95, .975, .99, 1.0]}
    value = {**calibrate(cases[:3], observations, spec), "artifact_hash": "fixture-result",
             "registration_file": "fixture-registration", "registration_file_sha256": "fixture-registration-sha",
             "registration_hash": "fixture-registration", "observations_file": "fixture-observations",
             "observations_file_sha256": "fixture-observation-sha", "observations_hash": "fixture-observations-hash",
             "runner_bindings": []}
    registration = {"artifact_cohort_file": str(cohort)}
    monkeypatch.setattr(diagnostic, "revalidate_calibration", lambda _: (value, registration, {"cases": cases}, rows, observations))
    return calibration, value, cases, rows, observations


def test_original_gate_grid_error_taxonomy_and_full_denominators_preserved(proof):
    path, _, _, _, _ = proof
    aggregate, private = diagnostic.diagnose(path)
    assert aggregate["allocated_groups"] == 5 and aggregate["collected_groups"] == 4
    assert aggregate["uncollected_groups"] == 1 and aggregate["eligible_observations"] == 3
    assert aggregate["label_counts"] == {"supported": 1, "unsupported": 2, "unknown": 1}
    assert aggregate["unknown_reasons"] == {"frozen_reference_does_not_pass": 1}
    assert aggregate["error_taxonomy"] == {"supported:passed": 1, "unsupported:AssertionError": 1, "unsupported:TypeError": 1}
    assert len(aggregate["grid"]) == 9 and aggregate["failed_observations"] == 1
    assert all(row["unsupported_groups"] == 2 and row["failed_attempts"] == 1 for row in aggregate["grid"])
    failed_taxon = aggregate["error_taxonomy_by_threshold"]["unsupported:TypeError"]
    assert failed_taxon["total_groups"] == failed_taxon["failed_prediction_groups"] == 1
    assert all(row["reject_signals"] == 0 for row in failed_taxon["rejected_by_threshold"])
    assert aggregate["input_integrity"]["checks_passed"] is True
    assert len(private["case_metadata_only"]) == 3
    assert aggregate["certified_threshold"] is None and aggregate["production_authorized"] is False


def test_no_candidate_code_reference_or_reasoning_in_public_or_private_output(proof):
    aggregate, private = diagnostic.diagnose(proof[0])
    encoded = json.dumps({"aggregate": aggregate, "private": private})
    assert "candidate-private-marker" not in encoded and "return x" not in encoded
    assert "reference" not in private["case_metadata_only"][0]
    assert "hypotheses" in aggregate["findings"] and "confirmed" in aggregate["findings"]
    assert aggregate["new_blind_test"] is False and aggregate["new_paid_calls"] == 0


def test_original_proof_failure_aborts_before_any_input_diagnostics(proof, monkeypatch):
    def reject(_):
        raise ValueError("fixture raw evidence mismatch")
    monkeypatch.setattr(diagnostic, "revalidate_calibration", reject)
    monkeypatch.setattr(diagnostic, "input_checks", lambda *_: pytest.fail("unverified input"))
    with pytest.raises(ValueError, match="evidence mismatch"):
        diagnostic.diagnose(proof[0])


@pytest.mark.parametrize("mutation", ["missing_assertions", "wrong_imports", "label_in_state", "question_direction",
                                      "wrong_task", "truncated_code", "different_model", "wrong_submit_hash"])
def test_input_defects_reported_as_checks_not_assumed_model_inability(proof, mutation):
    path, _, _, rows, observations = proof
    state = rows[0]["request"]["state"]
    if mutation == "missing_assertions":
        state["acceptance_assertions"] = []
    elif mutation == "wrong_imports":
        state["test_imports"] = ["import math"]
    elif mutation == "label_in_state":
        state["label"] = "supported"
    elif mutation == "question_direction":
        rows[0]["request"]["questions"]["acceptance_violation"]["instructions"] = "is success likely?"
    elif mutation == "wrong_task":
        state["task_prompt"] = "wrong-task"
    elif mutation == "truncated_code":
        state["candidate_code"] = "def f(x):"
    elif mutation == "different_model":
        observations[0]["model"] = "foreign"
    else:
        observations[0]["actual_request_hash"] = "foreign"
    aggregate, private = diagnostic.diagnose(path)
    assert aggregate["input_integrity"]["checks_passed"] is False
    assert aggregate["input_integrity"]["defect_count"] > 0 and private["input_defects"]
    assert aggregate["certified_threshold"] is None


def test_syntax_error_negative_is_supported_taxonomy_not_an_analysis_crash(proof):
    path, _, cases, rows, observations = proof
    rows[1]["request"]["state"]["candidate_code"] = "def f(:"
    cases[1]["oracle"]["result"] = {"status": "failed", "error_kind": "SyntaxError", "reference": {"status": "passed"}}
    rows[1]["request_hash"] = digest(rows[1]["request"])
    observation = observations[1]
    observation["actual_request_hash"] = observation["request_hash"] = rows[1]["request_hash"]
    Path(observation["original_events_file"]).with_name("request.json").write_text(
        json.dumps(rows[1]["request"], sort_keys=True, separators=(",", ":")), encoding="utf-8")
    aggregate, private = diagnostic.diagnose(path)
    assert aggregate["error_taxonomy"]["unsupported:SyntaxError"] == 1
    assert private["case_metadata_only"][1]["python_ast_parse"] == "syntax_error"
    assert aggregate["input_integrity"]["checks_passed"] is True


def test_missing_raw_request_cannot_claim_complete_input(proof):
    path, _, _, _, observations = proof
    Path(observations[0]["original_events_file"]).with_name("request.json").unlink()
    aggregate, private = diagnostic.diagnose(path)
    assert aggregate["input_integrity"]["checks_passed"] is False
    assert {"id": "1", "check": "raw_request_file"} in private["input_defects"]


def test_duplicate_missing_observation_not_dropped_or_filled(proof):
    path, _, _, _, observations = proof
    observations.pop()
    with pytest.raises(ValueError, match="denominator"):
        diagnostic.diagnose(path)


def test_diagnosis_does_not_modify_original_evidence(proof):
    path, value, cases, rows, observations = proof
    before = copy.deepcopy((value, cases, rows, observations))
    diagnostic.diagnose(path)
    assert (value, cases, rows, observations) == before


def test_cli_stdout_is_aggregate_and_private_output_create_new_only(proof, monkeypatch, capsys, tmp_path):
    monkeypatch.setattr(diagnostic, "diagnose", lambda _: diagnostic_result)
    diagnostic_result = ({"schema": diagnostic.SCHEMA, "aggregate_only": True}, {"case_metadata_only": []})
    private = tmp_path / ".nanocodex" / "private.json"
    monkeypatch.setattr("sys.argv", ["diagnostic", "--calibration", str(proof[0]), "--private-output", str(private)])
    diagnostic.main()
    assert json.loads(capsys.readouterr().out) == diagnostic_result[0] and private.exists()
    with pytest.raises(SystemExit) as exit_info:
        diagnostic.main()
    assert exit_info.value.code == 2


def test_private_case_metadata_not_written_to_public_path(proof, monkeypatch, tmp_path):
    monkeypatch.setattr(diagnostic, "diagnose", lambda _: ({}, {}))
    destination = tmp_path / "public.json"
    monkeypatch.setattr("sys.argv", ["diagnostic", "--calibration", str(proof[0]), "--private-output", str(destination)])
    with pytest.raises(SystemExit):
        diagnostic.main()
    assert not destination.exists()
