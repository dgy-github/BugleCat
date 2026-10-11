import json

import pytest

from evals.verification import source_bindings
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash
from evals.verification.public_benchmark import catalog
from evals.verification.source_bindings import bind_prepared, check_unchanged, verify_manifest


def frozen(tmp_path):
    source = tmp_path / "source.json"
    source.write_text(json.dumps([{"task_id": 1, "prompt": "Double a number", "code": "def f(n): return n * 2",
                                 "test_list": ["assert f(2) == 4"], "test_imports": []}]))
    index = catalog(source, "a" * 40)
    paths = {"source": source}
    for key, value in (("catalog", index), ("protocol", {"policy_version": "host-rules-v3.2"}),
                       ("authorization", {"cap": 400})):
        path = tmp_path / f"{key}.json"
        path.write_text(json.dumps(value))
        paths[key] = path
    row = index["tasks"][0]
    manifest = {"schema": "buglecat.verification.mbpp-collection.v1", "model": "deepseek-flash",
                "reasoning_effort": "ultra", "max_tokens": 8192, "timeout_s": 60,
                "cases": [{"id": "mbpp-main-1", "task_id": 1, "group_id": row["group_id"],
                           "split": row["reserved_split"], "public_interface": ["def f(n):"]}]}
    for key, path in paths.items():
        manifest[f"{key}_file"] = str(path)
        manifest[f"{key}_sha256"] = file_hash(path)
    return manifest, paths


def test_source_mutation_rejected_before_oracle(tmp_path):
    manifest, paths = frozen(tmp_path)
    tasks, _ = verify_manifest(manifest)
    assert digest(tasks[1]) == catalog(paths["source"], "a" * 40)["tasks"][0]["task_sha256"]
    paths["source"].write_text(paths["source"].read_text().replace("f(2) == 4", "f(2) == 5"))
    with pytest.raises(ValueError, match="source binding changed"):
        verify_manifest(manifest)


@pytest.mark.parametrize("field,value", [("split", "calibration"), ("group_id", "fake"),
                                        ("public_interface", ["def secret_reference(n):"])])
def test_manifest_cannot_override_frozen_group_split_interface(tmp_path, field, value):
    manifest, _ = frozen(tmp_path)
    manifest["cases"][0][field] = value
    with pytest.raises(ValueError):
        verify_manifest(manifest)


def test_rewritten_catalog_cannot_relabel_group_even_with_new_file_hash(tmp_path):
    manifest, paths = frozen(tmp_path)
    index = json.loads(paths["catalog"].read_text())
    index["tasks"][0]["reserved_split"] = "calibration"
    paths["catalog"].write_text(json.dumps(index))
    manifest["catalog_sha256"] = file_hash(paths["catalog"])
    with pytest.raises(ValueError, match="catalog does not match"):
        verify_manifest(manifest)


def test_source_mutation_during_audit_is_rejected(tmp_path):
    manifest, paths = frozen(tmp_path)
    _, bindings = verify_manifest(manifest)
    paths["protocol"].write_text('{}')
    with pytest.raises(ValueError, match="source binding changed"):
        check_unchanged(bindings)


def test_prepared_cannot_switch_task_or_budget(tmp_path):
    manifest, _ = frozen(tmp_path)
    case = manifest["cases"][0]
    original = {**case, **{k: manifest[k] for k in ("source_sha256", "authorization_sha256", "protocol_sha256")},
                "settings": {k: manifest[k] for k in ("model", "reasoning_effort", "max_tokens", "timeout_s")}}
    original["settings"]["max_retries"] = 0
    bind_prepared(manifest, original)
    original["authorization_sha256"] = "b" * 64
    with pytest.raises(ValueError, match="authorization_sha256 mismatch"):
        bind_prepared(manifest, original)


def test_resource_adjustment_bound_to_nested_actual_cohort(tmp_path):
    manifest, _ = frozen(tmp_path)
    path = tmp_path / "resource.json"
    resource = {"cohort_id": "resource-r1", "previous_settings_sha256": "c" * 64}
    path.write_text(json.dumps(resource))
    manifest.update(resource_adjustment_file=str(path), resource_adjustment_sha256=file_hash(path))
    original = {**manifest["cases"][0],
                **{k: manifest[k] for k in ("source_sha256", "authorization_sha256", "protocol_sha256")},
                "settings": {**{k: manifest[k] for k in ("model", "reasoning_effort", "max_tokens", "timeout_s")},
                             "max_retries": 0},
                "resource_cohort": {**resource, "resource_adjustment_sha256": file_hash(path)}}
    bind_prepared(manifest, original)
    original["resource_cohort"]["cohort_id"] = "other"
    with pytest.raises(ValueError, match="resource cohort mismatch"):
        bind_prepared(manifest, original)


@pytest.mark.parametrize("name", ["source", "catalog", "protocol", "authorization"])
def test_transaction_cache_rehashes_every_bound_file_on_hit(tmp_path, name):
    manifest, paths = frozen(tmp_path)
    with source_bindings.verified_sources():
        verify_manifest(manifest)
        paths[name].write_bytes(paths[name].read_bytes() + b" ")
        with pytest.raises(ValueError, match="binding changed"):
            verify_manifest(manifest)


def test_transaction_is_nested_bounded_and_results_are_not_shared(tmp_path, monkeypatch):
    manifest, _ = frozen(tmp_path)
    original = source_bindings._verify_manifest
    calls = []
    def reconstruct(value):
        calls.append(True)
        return original(value)
    monkeypatch.setattr(source_bindings, "_verify_manifest", reconstruct)
    with source_bindings.verified_sources():
        tasks, bindings = verify_manifest(manifest)
        tasks[1]["code"] = "tampered"
        bindings.clear()
        with source_bindings.verified_sources():
            restored, original_bindings = verify_manifest(manifest)
        assert restored[1]["code"] == "def f(n): return n * 2"
        assert len(original_bindings) == 4 and len(calls) == 1
    with source_bindings.verified_sources():
        verify_manifest(manifest)
    assert len(calls) == 2
    verify_manifest(manifest)
    assert len(calls) == 3


def test_exception_ends_transaction_and_changed_cases_need_full_proof(tmp_path):
    manifest, _ = frozen(tmp_path)
    with pytest.raises(RuntimeError):
        with source_bindings.verified_sources():
            verify_manifest(manifest)
            raise RuntimeError("operation interrupted")
    assert source_bindings._TRANSACTION.get() is None
    with source_bindings.verified_sources():
        verify_manifest(manifest)
        manifest["cases"][0]["public_interface"] = ["def foreign(n):"]
        with pytest.raises(ValueError, match="interface mismatch"):
            verify_manifest(manifest)


def test_source_transaction_does_not_share_cache_across_threads(tmp_path, monkeypatch):
    from concurrent.futures import ThreadPoolExecutor
    from threading import Barrier, Lock
    manifest, _ = frozen(tmp_path)
    original = source_bindings._verify_manifest
    lock, barrier, calls = Lock(), Barrier(2), []
    def reconstruct(value):
        with lock:
            calls.append(True)
        return original(value)
    monkeypatch.setattr(source_bindings, "_verify_manifest", reconstruct)
    @source_bindings.source_transaction
    def operation():
        barrier.wait(timeout=5)
        verify_manifest(manifest)
        verify_manifest(manifest)
    with ThreadPoolExecutor(max_workers=2) as executor:
        results = [executor.submit(operation) for _ in range(2)]
        for result in results:
            result.result(timeout=10)
    assert len(calls) == 2 and source_bindings._TRANSACTION.get() is None


def test_shared_catalog_proof_still_checks_each_distinct_manifest(tmp_path, monkeypatch):
    manifest, _ = frozen(tmp_path)
    original = source_bindings.catalog
    calls = []
    def reconstruct(*args):
        calls.append(True)
        return original(*args)
    monkeypatch.setattr(source_bindings, "catalog", reconstruct)
    with source_bindings.verified_sources():
        verify_manifest(manifest)
        other = {**manifest, "batch_id": "independent-next-batch"}
        verify_manifest(other)
        assert len(calls) == 1
        other["cases"] = [{**manifest["cases"][0], "group_id": "foreign"}]
        with pytest.raises(ValueError, match="split/group mismatch"):
            verify_manifest(other)
        assert len(calls) == 1
