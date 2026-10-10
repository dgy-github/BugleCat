import json
import math
import subprocess
import sys

import pytest

from evals.verification.calibration import (
    apply,
    calibrate,
    dataset,
    digest,
    holdout,
    metrics,
    observations,
    protocol,
    register,
    upper_bound,
)


def write(path, value):
    path.write_text(json.dumps(value), encoding="utf-8")
    return path


def spec():
    return {"model": "jev-1.13.0", "policy_version": "host-rules-v3.1",
            "template_version": "jev-single-claim-v2", "categories": ["semantic"],
            "threshold_grid": [.9, .95, .99], "max_false_block_rate": .05,
            "confidence": .95, "min_supported": 60, "min_unsupported": 30,
            "min_paired_improvements": 5}


def case(identity, label="supported", split="calibration"):
    return {"id": identity, "group_id": identity, "split": split, "category": "semantic",
            "label": label, "label_basis": "Independent executable assertion reviewed",
            "review_status": "assertion_reviewed", "source_kind": "anonymized_real_trace",
            "author_exposed": False, "context": "narrative", "polarity": "positive",
            "input_hash": "input-" + identity, "candidate_hash": "candidate-" + identity}


def row(identity, probability=.01, verdict="unknown", status="ok"):
    return {"id": identity, "rules_verdict": verdict, "call_status": status,
            "request_hash": digest({"test_request": identity}),
            "actual_request_hash": digest({"test_request": identity}),
            "transport_binding_status": "actual_transport_hash_recorded",
            "model": "jev-1.13.0", "policy_version": "host-rules-v3.1",
            "template_version": "jev-single-claim-v2", "input_hash": "input-" + identity,
            "candidate_hash": "candidate-" + identity,
            "questions": [{"name": "contradiction", "kind": "noul", "noul": probability},
                          {"name": "unsupported_success", "kind": "noul", "noul": probability}]
            if status == "ok" else []}


def jsonl(path, rows):
    path.write_text("\n".join(json.dumps(r) for r in rows), encoding="utf-8")
    return path


def setup(tmp_path, cases=None):
    cases = cases or [case("s"), case("u", "unsupported")]
    data = write(tmp_path / "data.json", {
        "schema": "buglecat.verification.review-dataset.v1", "cases": cases})
    registration = tmp_path / "registration.json"
    register(data, write(tmp_path / "protocol.json", spec()), registration)
    obs = jsonl(tmp_path / "observations.jsonl", [
        row(c["id"], .98 if c["label"] == "unsupported" else .01)
        for c in cases if c["split"] != "holdout"])
    return data, obs, registration


def test_rust_order_and_reject_only():
    parsed = {**row("x"), "answers": {"contradiction": .95, "unsupported_success": .99}}
    assert apply(parsed, .95) == "contradiction"
    assert apply(parsed, .98) == "need_evidence"
    assert apply(parsed, 1) == "unknown"
    for verdict in ("pass", "need_evidence", "contradiction"):
        assert apply({**parsed, "rules_verdict": verdict}, .5) == verdict
    assert apply({**parsed, "call_status": "timeout"}, .5) == "unknown"


def test_probability_is_not_pass_and_a_missing_is_explicit(tmp_path):
    data, obs, registration = setup(tmp_path)
    result = calibrate(data, obs, registration, tmp_path / "calibration.json")
    assert result["threshold"] == .95
    assert not result["calibration_gate_met"]
    assert result["certified_threshold"] is None
    assert result["threshold_status"] == "candidate_not_calibrated"
    assert not result["production_authorized"]
    score = result["grid_results"][1]
    assert score["A"] is None
    assert score["C"]["counts_by_label_and_verdict"]["supported"]["unknown"] == 1
    assert score["C"]["false_allow"] == score["B"]["false_allow"]
    assert score["C"]["host_delivery_action"]["unsupported_delivery_rate"]["rate"] == 0
    assert score["B"]["host_delivery_action"]["unsupported_delivery_rate"]["rate"] == 1
    with pytest.raises(ValueError, match="calibration gate failed"):
        holdout(data, obs, tmp_path / "calibration.json", tmp_path / "receipt.json")


def test_exact_confidence_bounds_and_no_small_n_certification():
    assert upper_bound(0, 1, .95) == pytest.approx(.95)
    assert upper_bound(0, 60, .95) == pytest.approx(1 - .05 ** (1 / 60))
    assert upper_bound(1, 1, .95) == 1
    assert upper_bound(0, 0, .95) is None


@pytest.mark.parametrize("changes", [
    {"transport_binding_status": "actual_transport_hash_unrecorded_development_only"},
    {"transport_binding_status": None},
    {"actual_request_hash": None},
    {"actual_request_hash": "0" * 64},
    {"request_hash": "not-a-sha", "actual_request_hash": "not-a-sha"},
])
def test_unrecorded_actual_transport_hash_blocks_formal_calibration(tmp_path, changes):
    cases = [case(f"s{i}") for i in range(60)] + [case(f"u{i}", "unsupported") for i in range(30)]
    data, obs, registration = setup(tmp_path, cases)
    records = [json.loads(line) for line in obs.read_text().splitlines()]
    records[0].update(changes)
    jsonl(obs, records)
    result = calibrate(data, obs, registration, tmp_path / "result.json")
    assert result["certified_threshold"] is None
    assert all("actual_transport_hash_unrecorded" in score["gate_reasons"] for score in result["grid_results"])


def test_group_variants_do_not_inflate_confidence():
    cases = [case(str(i)) for i in range(100)]
    for c in cases:
        c["group_id"] = "same-original-task"
    result = metrics(cases, {c["id"]: "unknown" for c in cases}, .95)
    assert result["false_block"]["denominator"] == 100
    assert result["false_block"]["independent_groups"] == 1
    assert result["false_block"]["confidence_upper_bound"] == pytest.approx(.95)


@pytest.mark.parametrize("changes,match", [
    ({"split": "holdout"}, "group crosses"),
    ({"id": "s"}, "duplicate case"),
    ({"author_exposed": "false"}, "author_exposed"),
    ({"review_status": "verified"}, "review status"),
    ({"source_kind": "synthetic"}, "remain development"),
])
def test_dataset_validation(tmp_path, changes, match):
    a, b = case("s"), case("b")
    b["group_id"] = a["group_id"]
    b.update(changes)
    path = write(tmp_path / "data.json", {
        "schema": "buglecat.verification.review-dataset.v1", "cases": [a, b]})
    with pytest.raises(ValueError, match=match):
        dataset(path)


@pytest.mark.parametrize("field,value,match", [
    ("model", "jev-latest", "model"),
    ("policy_version", "host-rules-v3", "policy_version"),
    ("template_version", "other", "template_version"),
    ("candidate_hash", "wrong", "candidate_hash mismatch"),
    ("input_hash", "wrong", "input_hash mismatch"),
    ("rules_verdict", "verified", "verdict"),
])
def test_mismatched_observation(tmp_path, field, value, match):
    r = {**row("s"), field: value}
    with pytest.raises(ValueError, match=match):
        observations(jsonl(tmp_path / "obs.jsonl", [r]), [case("s")], spec())


@pytest.mark.parametrize("p", [-.1, 1.1, math.nan, math.inf, True, "0.95"])
def test_invalid_probabilities(tmp_path, p):
    with pytest.raises(ValueError, match="probability"):
        observations(jsonl(tmp_path / "obs.jsonl", [row("s", p)]), [case("s")], spec())


def test_missing_duplicate_failed_and_leaked_observations(tmp_path):
    path = tmp_path / "obs.jsonl"
    for rows, match in [([], "missing observations"),
                        ([row("s"), row("s")], "duplicate observation"),
                        ([row("holdout")], "unselected split"),
                        ([row("s", status="timeout")], "no successful observations")]:
        with pytest.raises(ValueError, match=match):
            observations(jsonl(path, rows), [case("s")], spec())
    r = row("s")
    r["questions"].append(r["questions"][0])
    with pytest.raises(ValueError, match="duplicate question"):
        observations(jsonl(path, [r]), [case("s")], spec())


def test_unknown_labels_are_separate_unlabeled_cannot_run(tmp_path):
    data, obs, registration = setup(tmp_path, [case("s", "unknown")])
    result = calibrate(data, obs, registration, tmp_path / "result.json")
    assert result["grid_results"][0]["C"]["false_allow"]["denominator"] == 0
    value = json.loads(data.read_text())
    value["cases"][0]["label"] = "unlabeled"
    write(data, value)
    new_reg = tmp_path / "registration2.json"
    register(data, tmp_path / "protocol.json", new_reg)
    with pytest.raises(ValueError, match="unlabeled"):
        calibrate(data, obs, new_reg, tmp_path / "result2.json")


def test_development_unreviewed_is_never_certified(tmp_path):
    cases = [case("s", split="development"), case("u", "unsupported", "development")]
    for c in cases:
        c.update(source_kind="synthetic_author_labeled", review_status="pending", author_exposed=True)
    data, obs, registration = setup(tmp_path, cases)
    with pytest.raises(ValueError, match="no calibration cases"):
        calibrate(data, obs, registration, tmp_path / "default.json")
    result = calibrate(data, obs, registration, tmp_path / "dev.json", development=True)
    assert not result["calibration_gate_met"]
    assert "synthetic_development_only" in result["grid_results"][0]["gate_reasons"]
    with pytest.raises(ValueError, match="development calibration"):
        holdout(data, obs, tmp_path / "dev.json", tmp_path / "receipt.json")


def large_setup(tmp_path):
    cases = [case(f"{split}-s{i}", split=split) for split in ("calibration", "holdout") for i in range(60)]
    cases += [case(f"{split}-u{i}", "unsupported", split) for split in ("calibration", "holdout") for i in range(30)]
    return setup(tmp_path, cases)


def test_holdout_selected_once_without_retuning(tmp_path):
    data, obs, registration = large_setup(tmp_path)
    artifact = tmp_path / "calibration.json"
    result = calibrate(data, obs, registration, artifact)
    assert result["calibration_gate_met"]
    assert result["certified_threshold"] == result["threshold"]
    assert result["threshold_status"] == "calibrated_for_holdout_only"
    cases = [c for c in dataset(data)["cases"] if c["split"] == "holdout"]
    test_obs = jsonl(tmp_path / "holdout.jsonl", [
        row(c["id"], .98 if c["label"] == "unsupported" else .01) for c in cases])
    receipt = tmp_path / "receipt.json"
    outcome = holdout(data, test_obs, artifact, receipt)
    assert outcome["result"]["threshold"] == result["threshold"]
    assert outcome["result"]["gate_met"]
    assert not outcome["production_authorized"]
    with pytest.raises(FileExistsError):
        holdout(data, test_obs, artifact, tmp_path / "different-receipt.json")
    with pytest.raises(FileExistsError):
        calibrate(data, obs, registration, artifact)


def test_holdout_parse_failure_consumes_registration(tmp_path):
    data, obs, registration = large_setup(tmp_path)
    artifact = tmp_path / "calibration.json"
    calibrate(data, obs, registration, artifact)
    test_obs = tmp_path / "broken.jsonl"
    test_obs.write_text("{broken", encoding="utf-8")
    receipt = tmp_path / "receipt.json"
    with pytest.raises(ValueError):
        holdout(data, test_obs, artifact, receipt)
    assert json.loads(receipt.read_text())["status"] == "failed_consumed"
    with pytest.raises(FileExistsError):
        holdout(data, test_obs, artifact, tmp_path / "retry.json")


def test_hash_binding_prevents_dataset_and_artifact_drift(tmp_path):
    data, obs, registration = setup(tmp_path)
    artifact = tmp_path / "calibration.json"
    calibrate(data, obs, registration, artifact)
    value = json.loads(artifact.read_text())
    value["threshold"] = .5
    write(artifact, value)
    with pytest.raises(ValueError, match="artifact hash"):
        holdout(data, obs, artifact, tmp_path / "receipt.json")
    value = json.loads(data.read_text())
    value["cases"][0]["label_basis"] += " altered"
    write(data, value)
    with pytest.raises(ValueError, match="dataset changed"):
        calibrate(data, obs, registration, tmp_path / "different.json")


@pytest.mark.parametrize("grid", [[.95, .9], [.95, .95], [.49], [], [True]])
def test_grid_must_be_preregistered(grid):
    with pytest.raises(ValueError):
        protocol({**spec(), "threshold_grid": grid})


def test_small_n_protocol_cannot_lower_hard_floor():
    with pytest.raises(ValueError, match="independent-group floors"):
        protocol({**spec(), "min_supported": 1, "min_unsupported": 1})


@pytest.mark.parametrize("context,polarity", [("quoted", "positive"), ("code", "positive"),
                                            ("narrative", "negative")])
def test_nonasserted_or_negative_claims_cannot_be_rejected(tmp_path, context, polarity):
    c = {**case("s"), "context": context, "polarity": polarity}
    rows = observations(jsonl(tmp_path / "obs.jsonl", [row("s", 1)]), [c], spec())
    assert apply(rows["s"], .9) == "unknown"


def test_uncertain_labels_and_original_are_not_assumed_success():
    cases = [case("s"), case("u", "unsupported"), case("x", "unknown")]
    result = metrics(cases, {"s": "unknown", "u": "unknown", "x": "contradiction"}, .95)
    assert result["false_block"]["numerator"] == 0
    assert result["false_allow"]["numerator"] == 0
    assert result["counts_by_label_and_verdict"]["unknown"]["contradiction"] == 1


def test_exposed_holdout_refused_before_reading_probabilities(tmp_path):
    data, obs, registration = large_setup(tmp_path)
    value = json.loads(data.read_text())
    for c in value["cases"]:
        if c["split"] == "holdout":
            c["author_exposed"] = True
    write(data, value)
    # New registration represents a genuinely different declaration, not retuning.
    reg = tmp_path / "exposed-registration.json"
    register(data, tmp_path / "protocol.json", reg)
    artifact = tmp_path / "calibration.json"
    calibrate(data, obs, reg, artifact)
    with pytest.raises(ValueError, match="exposure"):
        holdout(data, tmp_path / "does-not-exist.jsonl", artifact, tmp_path / "receipt.json")
    assert not list(tmp_path.glob("holdout-consumed-*"))


def test_derived_bindings_cannot_qualify_by_changing_review_flags(tmp_path):
    cases = [case("s"), case("u", "unsupported")]
    for c in cases:
        c["input_binding_status"] = "derived_local_fixture_not_recorded_request"
    data, obs, registration = setup(tmp_path, cases)
    result = calibrate(data, obs, registration, tmp_path / "result.json")
    assert all("derived_fixture_not_recorded_request" in score["gate_reasons"]
               for score in result["grid_results"])


def test_legacy_template_cannot_qualify_even_with_reviewed_labels(tmp_path):
    data, obs, _ = setup(tmp_path)
    value = spec()
    value["template_version"] = "unrecorded-legacy-fixture"
    registration = tmp_path / "legacy-registration.json"
    register(data, write(tmp_path / "legacy-protocol.json", value), registration)
    rows = [row("s"), row("u", .98)]
    for r in rows:
        r["template_version"] = value["template_version"]
    jsonl(obs, rows)
    result = calibrate(data, obs, registration, tmp_path / "result.json")
    assert all("legacy_unrecorded_request_versions" in score["gate_reasons"]
               for score in result["grid_results"])


def test_cli_and_stable_hash(tmp_path):
    data, obs, registration = setup(tmp_path)
    output = tmp_path / "cli.json"
    result = subprocess.run([sys.executable, "-m", "evals.verification.calibration", "calibrate",
                             "--dataset", str(data), "--observations", str(obs),
                             "--registration", str(registration), "--output", str(output)],
                            capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    assert json.loads(result.stdout)["production_authorized"] is False
    assert digest({"a": 1, "b": 2}) == digest({"b": 2, "a": 1})
