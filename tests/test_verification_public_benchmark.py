import json

import pytest

from evals.verification.public_benchmark import catalog, solution_fingerprint


def task(identity, code="def twice(n):\n return n * 2"):
    return {"task_id": identity, "prompt": "Return twice its input", "code": code,
            "test_imports": [], "test_list": ["assert twice(4) == 8"]}


def source(tmp_path, tasks):
    path = tmp_path / "source.json"
    path.write_text(json.dumps(tasks), encoding="utf-8")
    return path


def test_renaming_reference_cannot_inflate_task_groups():
    assert solution_fingerprint("def twice(n):\n return n*2") == solution_fingerprint("def double(x):\n return x*2")
    assert solution_fingerprint("def twice(n):\n return n*2") != solution_fingerprint("def twice(n):\n return n+2")


def test_references_are_never_candidate_completion_labels(tmp_path):
    data = catalog(source(tmp_path, [task(1), task(2)]), "a" * 40)
    assert data["task_n"] == 2 and data["source_group_n"] == 1
    assert all(t["candidate_label"] == "unlabeled" and not t["reference_code_executed"] for t in data["tasks"])
    assert not data["production_authorized"] and data["paid_attempts"] == 0
    assert len({t["reserved_split"] for t in data["tasks"]}) == 1


def test_split_is_fixed_before_model_observations_and_input_order_independent(tmp_path):
    tasks = [task(i, f"def twice(n):\n return n*{i}") for i in range(1, 9)]
    first = catalog(source(tmp_path, tasks), "a" * 40)
    second = catalog(source(tmp_path, tasks[::-1]), "a" * 40)
    assert {t["task_id"]: t["reserved_split"] for t in first["tasks"]} == {t["task_id"]: t["reserved_split"] for t in second["tasks"]}
    assert first["split_group_counts"] == {"calibration": 4, "holdout": 4}


def test_duplicate_ids_or_unpinned_sources_rejected(tmp_path):
    with pytest.raises(ValueError, match="duplicate"):
        catalog(source(tmp_path, [task(1), task(1)]), "a" * 40)
    with pytest.raises(ValueError, match="exact"):
        catalog(source(tmp_path, [task(1)]), "master")
