"""Original status/Oracle evidence, never synthetic formal observations."""
import json

import pytest

from evals.verification import artifact_v2_formal_intake as owner


def row_fixture(tmp_path):
    return {"id": "case", "group_id": "group", "task_id": 1, "source_kind": "mbpp", "split": "calibration",
            "case_directory": str(tmp_path), "public_interface": ["def f(x):"]}


def test_unstarted_separate_from_original_pending_unknown(tmp_path):
    row = row_fixture(tmp_path)
    task = {"task_id": 1}
    result = owner.outcome(row, task, None, None, [], "unused")
    assert result["label"] == "unlabeled" and result["main_status"] == "main_not_started"
    assert result["call_eligible"] is False
    start = {"event": "attempt_started", "id": "case", "group_id": "group"}
    result = owner.outcome(row, task, start, None, [{**start, "attempt_ordinal": 276}], "unused")
    assert result["label"] == "unknown" and result["reason"] == "main_not_finished"


def test_orphan_receipt_and_changed_start_rejected(tmp_path):
    row = row_fixture(tmp_path)
    (tmp_path / "receipt.json").write_text(json.dumps({"fixture": True}))
    with pytest.raises(ValueError, match="without original"):
        owner.outcome(row, {"task_id": 1}, None, None, [], "unused")
    start = {"event": "attempt_started", "id": "case", "group_id": "group"}
    with pytest.raises(ValueError, match="SQLite start"):
        owner.outcome(row, {"task_id": 1}, start, None, [{**start, "id": "other"}], "unused")


def test_digest_code_uses_raw_utf8_bytes_not_json_string():
    assert owner.hash_text("x\n") != owner.digest("x\n")


def test_legacy_and_development_dataset_rejected_before_any_source_work(tmp_path):
    from evals.verification.calibration import digest
    body = {"schema": "buglecat.verification.artifact-dataset.v1"}
    path = tmp_path / "data.json"
    path.write_text(json.dumps({**body, "artifact_hash": digest(body)}))
    with pytest.raises(ValueError, match="formal v2"):
        owner.verify_dataset(path, tmp_path, tmp_path / "binary.exe")


def batch_fixture(tmp_path, monkeypatch, pending=False):
    case = {"id": "case", "group_id": "group", "task_id": 1, "source_kind": "mbpp", "split": "calibration"}
    task = {"task_id": 1}
    cohort = {"artifact_hash": "scope", "cases": [case], "bindings": [], "oracle_runtime_binding": {"fixture": True}}
    for role in ("authorization", "protocol"):
        path = tmp_path / (role + ".json")
        path.write_text("{}")
        cohort["bindings"].append({"role": role, "file": str(path)})
    cohort_file = tmp_path / "cohort.json"
    cohort_file.write_text("{}")
    binary = tmp_path / "binary.exe"
    binary.write_bytes(b"fake-no-http")
    batch = tmp_path / "batch"
    batch.mkdir()
    manifest = {"cases": [case], "artifact_cohort_hash": "scope"}
    (batch / "collection-manifest.json").write_text(json.dumps(manifest))
    row = {**case, "case_directory": str(batch / "case")}
    (batch / "prepared.jsonl").write_text(json.dumps(row))
    events = [{"id": "case", "event": "attempt_started"}]
    (batch / "attempt-events.jsonl").write_text(json.dumps(events[0]))
    started = {"id": "case", "group_id": "group", "split": "calibration", "artifact_cohort_hash": "scope", "case_directory": str(batch / "case")}
    monkeypatch.setattr(owner, "validate", lambda _: (cohort, [task], {}))
    monkeypatch.setattr(owner, "validate_manifest", lambda _: manifest)
    monkeypatch.setattr(owner, "prepared_gate", lambda *args: None)
    monkeypatch.setattr(owner, "raw_ledger", lambda _: ({"main_started": [{"group_id": "group", "started": started}]}, {}))
    result = {**case, "candidate_code": None, "label": "unknown", "call_eligible": False,
              "main_status": "main_not_finished" if pending else "transport_error", "reason": "fixture"}
    monkeypatch.setattr(owner, "outcome", lambda *args: result.copy())
    collections = [{"directory": str(batch), "manifest_sha256": owner.file_hash(batch / "collection-manifest.json"),
                    "prepared_sha256": owner.file_hash(batch / "prepared.jsonl")}]
    terminal = {"schema": "buglecat.verification.main-batch-terminal.v2", "terminal_main": True,
                "events_sha256": owner.file_hash(batch / "attempt-events.jsonl"),
                "manifest_sha256": collections[0]["manifest_sha256"], "prepared_sha256": collections[0]["prepared_sha256"],
                "artifact_cohort_hash": "scope"}
    return cohort_file, binary, collections, terminal


def test_registration_cannot_happen_between_live_main_requests(tmp_path, monkeypatch):
    cohort, binary, collections, terminal = batch_fixture(tmp_path, monkeypatch)
    with pytest.raises(FileNotFoundError):
        owner.collect_value(cohort, tmp_path, binary, collections)
    terminal["artifact_hash"] = owner.digest(terminal)
    (tmp_path / "batch/batch-terminal.json").write_text(json.dumps(terminal))
    data, _ = owner.collect_value(cohort, tmp_path, binary, collections)
    assert data["collected_groups"] == 1 and data["uncollected_groups"] == 0
    (tmp_path / "batch/attempt-events.jsonl").write_text("[]")
    with pytest.raises(ValueError, match="terminal bytes"):
        owner.collect_value(cohort, tmp_path, binary, collections)


def test_terminal_marker_does_not_promote_pending_or_omit_started_group(tmp_path, monkeypatch):
    cohort, binary, collections, terminal = batch_fixture(tmp_path, monkeypatch, pending=True)
    terminal["artifact_hash"] = owner.digest(terminal)
    (tmp_path / "batch/batch-terminal.json").write_text(json.dumps(terminal))
    with pytest.raises(ValueError, match="in-flight"):
        owner.collect_value(cohort, tmp_path, binary, collections)
    with pytest.raises(ValueError, match="omitted a batch"):
        owner.collect_value(cohort, tmp_path, binary, [])


@pytest.mark.parametrize("tamper", [False, True])
def test_fault_batch_unstarted_suffix_can_resume_without_losing_history(tmp_path, monkeypatch, tamper):
    cohort_file, binary, _, _ = batch_fixture(tmp_path, monkeypatch)
    cohort = owner.validate(cohort_file)[0]
    second_case = {"id": "next", "group_id": "next-group", "task_id": 2, "source_kind": "mbpp", "split": "calibration"}
    cohort["cases"].append(second_case)
    monkeypatch.setattr(owner, "validate", lambda _: (cohort, [{"task_id": 1}, {"task_id": 2}], {}))
    batches = [tmp_path / "batch", tmp_path / "resumed"]
    batches[1].mkdir()
    collections = []
    starts = []
    for case, directory in ((cohort["cases"][0], batches[0]), (second_case, batches[1])):
        starts.append({"group_id": case["group_id"], "started": {**case, "artifact_cohort_hash": "scope", "case_directory": str(directory / case["id"])}})
    monkeypatch.setattr(owner, "raw_ledger", lambda _: ({"main_started": starts}, {}))
    for n, directory in enumerate(batches):
        cases = cohort["cases"] if n == 0 else [second_case]
        rows = []
        for case in cases:
            rows.append({**case, "case_directory": str(directory / case["id"]), "task_sha256": owner.digest(case["task_id"]),
                         "public_interface": ["def f(x):"], "body": {"original": True}, "request_hash": "same",
                         "settings": {"fixture": True}, "oracle_runtime_binding": {"fixture": True}, "artifact_cohort_hash": "scope", "build_info": {"fixture": True}})
        if tamper and n == 1:
            rows[0]["body"] = {"changed": True}
        manifest = {"cases": cases, "artifact_cohort_hash": "scope", "split": "calibration"}
        (directory / "collection-manifest.json").write_text(json.dumps(manifest))
        (directory / "prepared.jsonl").write_text("\n".join(json.dumps(row) for row in rows))
        own = cohort["cases"][0] if n == 0 else second_case
        (directory / "attempt-events.jsonl").write_text(json.dumps({"id": own["id"], "event": "attempt_started"}))
        source = {"directory": str(directory), "manifest_sha256": owner.file_hash(directory / "collection-manifest.json"),
                  "prepared_sha256": owner.file_hash(directory / "prepared.jsonl")}
        terminal = {"schema": "buglecat.verification.main-batch-terminal.v2", "terminal_main": True,
                    "events_sha256": owner.file_hash(directory / "attempt-events.jsonl"), "artifact_cohort_hash": "scope", **{key: source[key] for key in ("manifest_sha256", "prepared_sha256")}}
        (directory / "batch-terminal.json").write_text(json.dumps({**terminal, "artifact_hash": owner.digest(terminal)}))
        collections.append(source)
    monkeypatch.setattr(owner, "validate_manifest", lambda path: json.loads(path.read_text()))
    seen = []
    def outcome(row, task, original, *args):
        seen.append((row["id"], bool(original)))
        return {**{key: row[key] for key in ("id", "group_id", "task_id", "source_kind", "split")},
                "label": "unknown" if original else "unlabeled", "call_eligible": False,
                "candidate_code": None, "main_status": "transport_error" if original else "main_not_started"}
    monkeypatch.setattr(owner, "outcome", outcome)
    if tamper:
        with pytest.raises(ValueError, match="source/body"):
            owner.collect_value(cohort_file, tmp_path, binary, collections)
    else:
        data, rows = owner.collect_value(cohort_file, tmp_path, binary, collections)
        assert seen == [("case", True), ("next", False), ("next", True)]
        assert data["collected_groups"] == 2 and len(data["collections"]) == 2
        assert len(rows) == 2 and all(row["label"] == "unknown" for row in rows)
        with pytest.raises(ValueError, match="omitted a batch"):
            owner.collect_value(cohort_file, tmp_path, binary, collections[:1])
