"""No-fee mixed source, immutable history and bounded allocation gates."""
import json
from pathlib import Path

import pytest

from evals.verification import artifact_v2_formal_cohort as owner
from evals.verification.calibration import digest


def catalog_fixture():
    return {"tasks": [{"task_id": n, "group_id": f"g{n}", "task_sha256": digest(n), "source_kind": "mbpp",
                       "public_interface": [f"def f{n}(x):"], "source_available": True,
                       "reserved_split": "calibration" if n < 300 else "holdout"} for n in range(650)]}


def test_fixed_split_whole_allocation_and_order_no_fee():
    index = catalog_fixture()
    excluded = {f"g{n}" for n in range(20)}
    cases = owner.allocation(index, excluded, 235, 240, 475, 500)
    assert len(cases) == 475 and not {row["group_id"] for row in cases} & excluded
    assert cases == owner.allocation(index, excluded, 235, 240, 475, 500)
    assert all(row["task_id"] < 300 for row in cases if row["split"] == "calibration")
    assert all(row["task_id"] >= 300 for row in cases if row["split"] == "holdout")


@pytest.mark.parametrize("cal,hold,main,jev", [(199, 240, 500, 500), (235, 199, 500, 500),
                                             (True, 240, 500, 500), (235, 240, 474, 500), (235, 240, 500, 474)])
def test_allocation_cannot_expand_cap_or_lower_source_floor(cal, hold, main, jev):
    with pytest.raises(ValueError):
        owner.allocation(catalog_fixture(), set(), cal, hold, main, jev)


def test_fresh_cal_insufficient_never_borrows_existing_holdout():
    with pytest.raises(ValueError, match="fresh calibration"):
        owner.allocation(catalog_fixture(), {f"g{n}" for n in range(100)}, 235, 240, 500, 500)


def test_new_human_split_freezes_seed_without_rewriting_mbpp():
    groups = {f"human-{n}" for n in range(152)}
    split = owner.split_new_groups(groups, 90)
    assert sum(value == "calibration" for value in split.values()) == 90
    assert sum(value == "holdout" for value in split.values()) == 62
    assert split == owner.split_new_groups(groups, 90)
    with pytest.raises(ValueError):
        owner.split_new_groups(groups, True)


def history_fixture(tmp_path):
    reservation = tmp_path / "reservation.json"
    reservation.write_text("original immutable reservation")
    return {"main_ledger_file": "original.sqlite3", "raw_main_rows": [[1, "group", '{"original":true}', '{"finished":true}'],
                                                                  [2, "pending", '{"pending":true}', None]],
            "main_metadata": [["authorization_sha256", "original"]],
            "reservation_bindings": [{"file": str(reservation), "sha256": owner.file_hash(reservation)}]}


def test_history_append_only_preserves_pending_and_all_original_bytes(tmp_path):
    original = history_fixture(tmp_path)
    current = json.loads(json.dumps(original))
    current["raw_main_rows"].append([3, "new", '{"auth750":true}', None])
    current["main_metadata"].append(["new_scope", "new_bind"])
    owner.history_gate(original, current)
    assert original["raw_main_rows"][1][-1] is None


@pytest.mark.parametrize("mutation", ["delete", "reformat_start", "terminal_finish", "reset_auth", "root", "reservation"])
def test_history_mutations_are_not_a_budget_reset_bridge(tmp_path, mutation):
    original = history_fixture(tmp_path)
    current = json.loads(json.dumps(original))
    if mutation == "delete":
        current["raw_main_rows"].pop()
    elif mutation == "reformat_start":
        current["raw_main_rows"][0][2] = '{ "original": true }'
    elif mutation == "terminal_finish":
        current["raw_main_rows"][0][3] = '{"finished":false}'
    elif mutation == "reset_auth":
        current["main_metadata"][0][1] = "reset"
    elif mutation == "root":
        current["main_ledger_file"] = "other.sqlite3"
    else:
        Path(original["reservation_bindings"][0]["file"]).write_text("modified")
    with pytest.raises(ValueError):
        owner.history_gate(original, current)


def test_readonly_real_pinned_sources_no_oracle_or_model_calls():
    root = owner.ROOT
    review = root / "reports/.nanocodex/artifact-calibration-review"
    if not review.exists():
        pytest.skip("local official snapshot inventory unavailable")
    tasks, value = owner.source_value(
        root / "evals/verification/data/mbpp/mbpp-expanded-20261011-v2.json",
        root / "evals/verification/reports/artifact_expanded_mbpp_catalog_20261011_v2.json",
        root / "evals/verification/data/humaneval/download-6d43fb980f9f.json",
        root / "evals/verification/data/humaneval/humaneval-derived-6d43fb980f9f-v2.json",
        review / "humaneval-source-inventory-v2.json", review / "humaneval-reference-admission-v2.json",
        human_calibration_groups=90)
    assert len(tasks) == len(value["tasks"]) == 1099
    humans = {row["group_id"]: row for row in value["tasks"] if row["source_kind"] == "humaneval" and row["source_available"]}
    assert len(humans) == 152 and sum(row["reserved_split"] == "calibration" for row in humans.values()) == 90
    assert value["old_mbpp_splits_changed"] is False and value["new_paid_calls"] == 0


@pytest.mark.parametrize("reference,timeout", [(False, 5), (True, 10)])
def test_runtime_wrapper_binds_actual_interpreter_and_separate_timeouts(monkeypatch, reference, timeout):
    task = {"task_id": 1}
    calls = []
    runtime = {"executable_sha256": "a" * 64, "version_info": [3, 12, 0]}
    monkeypatch.setattr(owner, "runtime_info", lambda: runtime.copy())
    def audit(task, code, **kwargs):
        calls.append(kwargs)
        return {"result": {"status": "passed"}}
    monkeypatch.setattr(owner, "mbpp_audit", audit)
    monkeypatch.setattr(owner, "oracle_gate", lambda receipt, *args, **kwargs: receipt)
    value = owner.oracle_wrapper(task, "original code", "mbpp", reference=reference)
    assert calls == [{"timeout": timeout}]
    assert value["candidate_labels_assigned"] is (not reference)
    assert value["oracle_runtime_binding"] == runtime and value["new_paid_calls"] == 0
    owner.wrapped_oracle_gate(value, task, "original code", "mbpp", runtime, reference=reference)
    body = {key: item for key, item in value.items() if key != "artifact_hash"}
    body["oracle_runtime_binding"] = {**runtime, "executable_sha256": "b" * 64}
    with pytest.raises(ValueError, match="runtime"):
        owner.wrapped_oracle_gate({**body, "artifact_hash": digest(body)}, task, "original code", "mbpp", runtime, reference=reference)


def test_reference_pool_keeps_unknown_sources_out_without_split_borrowing():
    root = owner.ROOT
    pool = root / "reports/.nanocodex/artifact-v2-formal-source-review/reference-pool.json"
    if not pool.exists():
        pytest.skip("local source-only CPU admission unavailable")
    review = root / "reports/.nanocodex/artifact-calibration-review"
    _, value = owner.source_value(
        root / "evals/verification/data/mbpp/mbpp-expanded-20261011-v2.json",
        root / "evals/verification/reports/artifact_expanded_mbpp_catalog_20261011_v2.json",
        root / "evals/verification/data/humaneval/download-6d43fb980f9f.json",
        root / "evals/verification/data/humaneval/humaneval-derived-6d43fb980f9f-v2.json",
        review / "humaneval-source-inventory-v2.json", review / "humaneval-reference-admission-v2.json",
        human_calibration_groups=93, reference_pool_file=pool)
    proof = owner.sealed(owner.load(pool))
    unknown_ids = {case["task_id"] for case in proof["cases"] if case["reference_receipt"]["receipt"]["result"]["status"] == "unknown"}
    assert len(unknown_ids) == 3
    assert all(not row["source_available"] for row in value["tasks"] if row["task_id"] in unknown_ids)
    assert all(row["reserved_split"] == "calibration" for row in value["tasks"] if row["task_id"] in unknown_ids)
    humans = {row["group_id"]: row for row in value["tasks"] if row["source_kind"] == "humaneval" and row["source_available"]}
    assert sum(row["reserved_split"] == "calibration" for row in humans.values()) == 93


def test_mixed_source_cache_is_transaction_local_deepcopied_and_rehashes(tmp_path, monkeypatch):
    from evals.verification.source_bindings import verified_sources
    source = tmp_path / "source.json"
    catalog = tmp_path / "catalog.json"
    bound = tmp_path / "original.txt"
    source.write_text(json.dumps([{"task_id": 1}]))
    bound.write_text("immutable")
    body = {"schema": owner.SOURCE_SCHEMA, "seed": owner.SEED, "human_calibration_groups": 93,
            "bindings": [{"role": role, "file": str(bound), "sha256": owner.file_hash(bound)} for role in
                         ("mbpp_source", "mbpp_catalog", "human_manifest", "human_source", "human_reference_admission")]}
    human_catalog = tmp_path / "human.json"
    human_catalog.write_text(json.dumps({"bindings": []}))
    body["bindings"].append({"role": "human_catalog", "file": str(human_catalog), "sha256": owner.file_hash(human_catalog)})
    value = {**body, "artifact_hash": owner.digest(body)}
    catalog.write_text(json.dumps(value))
    calls = []
    def build(*args, **kwargs):
        calls.append(True)
        return [{"task_id": 1}], value.copy()
    monkeypatch.setattr(owner, "source_value", build)
    with verified_sources():
        tasks, first = owner.rebuild_sources(source, catalog)
        tasks[0]["task_id"] = 999
        first["seed"] = "mutated copy"
        assert owner.rebuild_sources(source, catalog)[0] == [{"task_id": 1}]
        assert len(calls) == 1
        bound.write_text("changed between hits")
        with pytest.raises(ValueError, match="binding changed"):
            owner.rebuild_sources(source, catalog)
    bound.write_text("immutable")
    with verified_sources():
        owner.rebuild_sources(source, catalog)
    assert len(calls) == 2
