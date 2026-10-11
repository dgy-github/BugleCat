"""Zero-network packaging gates, including both actual copied entrypoints."""
import importlib.util
import json
import shutil
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("package_v2", ROOT / "scripts/package-artifact-evaluation-v2.py")
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


def write(path, body):
    path.write_text(json.dumps({**body, "artifact_hash": package.digest(body)}), encoding="utf-8")
    return path


@pytest.fixture
def original(tmp_path, monkeypatch):
    from evals.verification import artifact_v2_formal as formal
    monkeypatch.setattr(package, "safe_file", lambda path, root=None: Path(path))
    cohort = tmp_path / "cohort.json"
    cohort.write_text(json.dumps({"cases": [{"group_id": str(n), "split": "calibration"} for n in range(265)]
                                 + [{"group_id": "hold" + str(n), "split": "holdout"} for n in range(240)]}))
    data = {"cases": [{"id": str(n)} for n in range(232)], "cohort_file": str(cohort), "split": "calibration",
            "collected_groups": 232, "uncollected_groups": 33}
    rows = [{"label": "supported"}] * 162 + [{"label": "unsupported"}] * 53 + [{"label": "unknown"}] * 17
    value = {"schema": "buglecat.verification.artifact-calibration.formal.v2",
             "production_authorized": False, "native_claim_certification": False,
             "calibration_numeric_gate_met": False, "qualified_for_holdout_threshold": None}
    path = write(tmp_path / "result.json", value)
    monkeypatch.setattr(formal, "revalidate_calibration", lambda _: (package.load(path), {}, data, rows, [{}] * 215))
    return path, data, rows, formal


def test_failed_calibration_projection_does_not_certify(original):
    path, _, _, _ = original
    value = package.verify_result(path)
    assert value["allocated_groups"] == 265
    assert value["collected_groups"] == 232
    assert value["uncollected_groups"] == 33
    assert value["label_counts"]["unknown"] == 17
    assert value["observations"] == 215
    assert value["certified_artifact_threshold"] is None
    assert value["overall_goal_complete"] is False
    assert value["native_claim_certification"] is False


def test_qualified_calibration_without_holdout_rejected(original):
    path, _, _, _ = original
    value = package.load(path)
    value.pop("artifact_hash")
    value.update(calibration_numeric_gate_met=True, qualified_for_holdout_threshold=.9)
    write(path, value)
    with pytest.raises(ValueError, match="requires holdout"):
        package.verify_result(path)


def test_forged_result_seal_rejected(original):
    path, _, _, _ = original
    value = package.load(path)
    value["qualified_for_holdout_threshold"] = .9
    path.write_text(json.dumps(value))
    with pytest.raises(ValueError, match="seal"):
        package.verify_result(path)


def test_public_aggregate_does_not_leak_case_failures(original):
    path, _, _, _ = original
    body = package.load(path)
    body.pop("artifact_hash")
    body["grid_results"] = [{"threshold": .9, "failed_attempts": {"private-case": "timeout"},
                             "private_candidate": "never publish", "detected_unsupported": 10}]
    write(path, body)
    actual = package.verify_result(path)
    assert actual["grid_results"][0] == {"threshold": .9, "detected_unsupported": 10, "failed_attempt_count": 1}
    assert "private-case" not in json.dumps(actual)


def test_stage_uses_only_explicit_public_files_and_runtime_gate(tmp_path, monkeypatch):
    old = package.old_packager()
    monkeypatch.setattr(package, "ROOT", tmp_path)
    parent = tmp_path / "parent"
    parent.mkdir()
    for name in package.NATIVE:
        (parent / name).write_bytes(b"native fixture")
    (parent / "source-snapshot.json").write_text("{}")
    manifest = {"files": {name: {"sha256": package.sha(parent / name)} for name in package.NATIVE},
                "source_snapshot": "source-snapshot.json"}
    monkeypatch.setattr(package, "verify_parent", lambda _: manifest)
    bindings, binaries = {}, {}
    for name in (package.MAIN, package.JEV):
        path = tmp_path / f"rust/target/debug/examples/{name}.exe"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"sidecar fixture")
        bindings[path] = package.sha(path)
        binaries[name] = path
    freeze = tmp_path / "freeze.json"
    freeze.write_text("{}")
    cohort = tmp_path / "private-cohort.json"
    cohort.write_text("{}")
    dataset = tmp_path / "private-dataset.json"
    dataset.write_text(json.dumps({"cohort_file": str(cohort)}))
    result = tmp_path / "private-result.json"
    result.write_text(json.dumps({"dataset_file": str(dataset)}))
    monkeypatch.setattr(package, "verify_freeze", lambda _: (bindings, binaries))
    result_proofs = []
    monkeypatch.setattr(package, "verify_result", lambda path, scope: result_proofs.append((path, scope))
                        or {"status": "incomplete_quotas_not_met"})
    public = tmp_path / "evals/verification/public.py"
    public.parent.mkdir(parents=True)
    public.write_text("# no private evidence")
    monkeypatch.setattr(package, "public_sources", lambda *_: [public])
    calls = []
    monkeypatch.setattr(package, "offline_attest", lambda *a: calls.append(a) or {"name": a[1], "paid_calls": 0})
    def runtime(output, _):
        calls.append("runtime")
        (output / "evidence/runtime-verification-records.json").write_text("private raw fixture transcript")
        return {"paid_calls": 0, "actual_copied_runtime_smoke": True}
    monkeypatch.setattr(old, "runtime_smoke", runtime)
    monkeypatch.setattr(package, "old_packager", lambda: old)
    output = tmp_path / "releases/fresh"
    output.parent.mkdir()
    value = package.stage(parent, freeze, result, output)
    assert value["status"] == "incomplete_quotas_not_met"
    assert len(calls) == 3 and calls[-1] == "runtime"
    assert result_proofs == [(result, freeze), (result, freeze)]
    assert not (output / "private-cohort.json").exists()
    assert not (output / "evidence/runtime-verification-records.json").exists()
    assert {p.name for p in output.rglob("*.exe")} == package.NATIVE | {name + ".exe" for name in binaries}
    actual_manifest = package.load(output / "manifest.json")
    assert actual_manifest["overall_goal_complete"] is False
    assert actual_manifest["production_authorized"] is False


def test_original_revalidation_error_not_swallowed(original, monkeypatch):
    path, _, _, formal = original
    def reject(_):
        raise ValueError("raw response changed")
    monkeypatch.setattr(formal, "revalidate_calibration", reject)
    with pytest.raises(ValueError, match="raw response changed"):
        package.verify_result(path)


@pytest.mark.parametrize("gate", [False, True])
def test_holdout_requires_exact_fixed_marker(original, monkeypatch, tmp_path, gate):
    from evals.verification import budget_authorization_v2 as budget
    path, data, rows, formal = original
    observations = tmp_path / "observations.jsonl"
    observations.write_text("{}\n")
    regfile = tmp_path / "reg.json"
    regfile.write_text("{}")
    value = {"schema": "buglecat.verification.artifact-holdout.formal.v2", "production_authorized": False,
             "native_claim_certification": False, "overall_agent_benefit_certification": False,
             "registration_file": str(regfile), "observations_file": str(observations),
             "gate_met": gate, "certified_threshold": .9 if gate else None}
    write(path, value)
    data.update(authorization_file="auth", cohort_hash="cohort")
    reg = {"artifact_hash": "reg", "calibration_hash": "cal"}
    monkeypatch.setattr(formal, "holdout_value", lambda *_: package.load(path))
    monkeypatch.setattr(formal, "revalidate", lambda _: (reg, data, rows, []))
    monkeypatch.setattr(budget, "resolve", lambda _: {"authorization": {"jev": {"ledger_dir": str(tmp_path)}}})
    marker = tmp_path / "formal-v2-holdout-evaluations/cohort.json"
    marker.parent.mkdir()
    claim = {"schema": "buglecat.verification.artifact-holdout-evaluation-consumption.v2", "cohort_hash": "cohort",
             "registration_hash": "reg", "calibration_hash": "cal", "result_hash": package.load(path)["artifact_hash"],
             "observations_sha256": package.sha(observations), "output_file": str(path.resolve()),
             "threshold_tuning": False, "holdout_evaluation_consumptions": 1, "new_paid_calls": 0}
    write(marker, claim)
    actual = package.verify_result(path)
    assert actual["formal_artifact_certification"] is gate
    assert actual["production_authorized"] is False
    claim["result_hash"] = "tampered"
    write(marker, claim)
    with pytest.raises(ValueError, match="fixed holdout marker"):
        package.verify_result(path)


def incomplete(original, tmp_path, monkeypatch):
    from evals.verification import artifact_v2_formal_intake as intake
    path, data, rows, _ = original
    rows = rows[:160] + [{"label": "unknown"}] * 72
    dataset = tmp_path / "dataset.json"
    dataset.write_text(json.dumps({"prepared_directory": "original-prepared", "collector_executable_file": "actual-exe"}))
    calls = []
    monkeypatch.setattr(intake, "verify_dataset", lambda *args: (calls.append(args) or (data, rows)))
    monkeypatch.setattr(package, "no_jev_for_allocation", lambda value: calls.append(value))
    body = {"schema": "buglecat.verification.artifact-evaluation-incomplete.v2", "production_authorized": False,
            "native_claim_certification": False, "formal_evaluation_completed": False,
            "certified_threshold": None, "registration_file": None, "dataset_file": str(dataset),
            "dataset_file_sha256": package.sha(dataset), "label_counts": {"supported": 160, "unsupported": 0, "unknown": 72}}
    write(path, body)
    return path, calls


def test_incomplete_uses_actual_original_dataset(original, tmp_path, monkeypatch):
    path, calls = incomplete(original, tmp_path, monkeypatch)
    actual = package.verify_result(path)
    assert actual["status"] == "incomplete_quotas_not_met"
    assert actual["observations"] == 0
    assert calls[0][1:] == ("original-prepared", "actual-exe")
    assert len(calls) == 2


def test_incomplete_fake_labels_rejected(original, tmp_path, monkeypatch):
    path, _ = incomplete(original, tmp_path, monkeypatch)
    value = package.load(path)
    value.pop("artifact_hash")
    value["label_counts"]["unsupported"] = 50
    write(path, value)
    with pytest.raises(ValueError, match="quotas differ"):
        package.verify_result(path)


def test_zip_exact_bytes_and_no_extra_private_files(tmp_path):
    source = tmp_path / "package"
    source.mkdir()
    (source / "public.txt").write_text("safe")
    (source / "manifest.json").write_text("{}")
    (source / "private-ledger.json").write_text("not included")
    target = tmp_path / "bundle.zip"
    package.checked_zip(source, target, [{"path": "public.txt", "sha256": package.sha(source / "public.txt"), "bytes": 4}])
    import zipfile
    with zipfile.ZipFile(target) as archive:
        assert set(archive.namelist()) == {"manifest.json", "public.txt"}
    with pytest.raises(ValueError, match="fresh ZIP"):
        package.checked_zip(source, target, [])


@pytest.mark.parametrize("path", ["../outside", "/absolute"])
def test_zip_path_escape_rejected(tmp_path, path):
    (tmp_path / "manifest.json").write_text("{}")
    with pytest.raises(ValueError, match="unsafe ZIP path"):
        package.checked_zip(tmp_path, tmp_path / "bundle.zip", [{"path": path, "sha256": "", "bytes": 0}])


def test_actual_parent_and_freeze_unchanged():
    assert package.verify_parent(ROOT / "releases/jev-v33-20261010-235623")["policy_version"] == "host-rules-v3.3"
    bindings, binaries = package.verify_freeze(ROOT / "reports/.nanocodex/artifact-v2-formal-20261011-r2/freeze.json")
    assert len(binaries) == 2
    assert all(path.resolve() in bindings for path in binaries.values())


@pytest.mark.parametrize("name", [package.MAIN, package.JEV])
def test_actual_copied_offline_entrypoint(tmp_path, name):
    bindings, binaries = package.verify_freeze(ROOT / "reports/.nanocodex/artifact-v2-formal-20261011-r2/freeze.json")
    cohort = package.load(ROOT / "reports/.nanocodex/artifact-v2-formal-20261011-r2/cohort.json")
    for path in package.public_sources(bindings, cohort):
        target = tmp_path / "copied-source" / path.relative_to(ROOT)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
    copied = tmp_path / (name + ".exe")
    shutil.copy2(binaries[name], copied)
    actual = package.offline_attest(copied, name, tmp_path / "private-fixture", bindings, tmp_path / "copied-source")
    assert actual["actual_copied_entrypoint"] is True
    assert actual["paid_calls"] == 0
    assert actual["sha256"] == package.sha(binaries[name])


def test_offline_rejects_unfrozen_exe_before_execution(tmp_path, monkeypatch):
    fake = tmp_path / "fake.exe"
    fake.write_bytes(b"not a frozen executable")
    monkeypatch.setattr(package.subprocess, "run", lambda *a, **k: pytest.fail("must not execute"))
    with pytest.raises(ValueError, match="original freeze"):
        package.offline_attest(fake, package.MAIN, tmp_path / "fixture", {})


def test_public_allowlist_excludes_real_cohort_and_history():
    bindings, _ = package.verify_freeze(ROOT / "reports/.nanocodex/artifact-v2-formal-20261011-r2/freeze.json")
    cohort_path = ROOT / "reports/.nanocodex/artifact-v2-formal-20261011-r2/cohort.json"
    sources = package.public_sources(bindings, package.load(cohort_path))
    assert cohort_path not in sources
    assert not any("ledger" in p.name or p.suffix in {".sqlite3", ".exe", ".started"} for p in sources)
    assert any(p.name.startswith("LICENSE.") for p in sources)
    assert any(p.name == "artifact_protocol_20261011_v2_r2.json" for p in sources)
    assert any(p.name == "mixed-source.json" for p in sources)


def test_no_jev_even_reservation_without_response_is_incomplete_blocker(tmp_path, monkeypatch):
    from evals.verification import artifact_cohort, budget_authorization_v2
    cohort = tmp_path / "cohort.json"
    cohort.write_text(json.dumps({"cases": [{"group_id": "g"}]}))
    monkeypatch.setattr(budget_authorization_v2, "resolve", lambda _: {"authorization": {"jev": {"ledger_dir": str(tmp_path)}}})
    monkeypatch.setattr(artifact_cohort, "reservations", lambda *a: ([{"group_id": "g", "started": False}], []))
    with pytest.raises(ValueError, match="reservations"):
        package.no_jev_for_allocation({"authorization_file": "auth", "cohort_file": str(cohort)})


@pytest.fixture
def scope_fixture(tmp_path, monkeypatch):
    monkeypatch.setattr(package, "safe_file", lambda path, root=None: Path(path).resolve())
    bindings, items = {}, []
    roles = ("source", "catalog", "protocol", "authorization", "profile", "formal_statistical_owner",
             "formal_request_owner", "formal_observation_owner", "request_support_owner")
    for role in roles:
        file = tmp_path / (role + '.json')
        file.write_text('{}')
        bindings[file] = package.sha(file)
        items.append({"role": role, "file": str(file), "sha256": package.sha(file)})
    cohort = write(tmp_path / 'cohort.json', {"bindings": items})
    freeze = tmp_path / 'freeze.json'
    freeze.write_text(json.dumps({"cohort_hash": package.load(cohort)["artifact_hash"]}))
    data = {"cohort_file": str(cohort), "cohort_sha256": package.sha(cohort), "cohort_hash": package.load(cohort)["artifact_hash"],
            "protocol_file": str(tmp_path / 'protocol.json'), "protocol_sha256": package.sha(tmp_path / 'protocol.json'),
            "authorization_file": str(tmp_path / 'authorization.json'),
            "authorization_sha256": package.sha(tmp_path / 'authorization.json')}
    monkeypatch.setattr(package, "verify_freeze", lambda _: (bindings, {}))
    return data, freeze, bindings


def test_result_scope_same_frozen_cohort_allowed(scope_fixture):
    data, freeze, _ = scope_fixture
    proof = package.verify_result_scope(data, freeze)
    assert proof['cohort_hash'] == data['cohort_hash']
    assert {row['role'] for row in proof['public_source_bindings']} == {'source', 'catalog', 'protocol'}


def test_cross_cohort_rejected_even_with_same_actual_exe(scope_fixture):
    data, freeze, _ = scope_fixture
    freeze.write_text(json.dumps({"cohort_hash": "other_valid_cohort"}))
    with pytest.raises(ValueError, match='cohort differ'):
        package.verify_result_scope(data, freeze)


@pytest.mark.parametrize('role', ['source', 'catalog', 'protocol', 'authorization', 'formal_request_owner'])
def test_actual_binding_must_have_original_archive(scope_fixture, role):
    data, freeze, bindings = scope_fixture
    target = Path(data['cohort_file']).parent / (role + '.json')
    del bindings[target]
    with pytest.raises(ValueError, match='absent or different'):
        package.verify_result_scope(data, freeze)


def test_same_cohort_protocol_field_cannot_point_to_other_bytes(scope_fixture, tmp_path):
    data, freeze, _ = scope_fixture
    other = tmp_path / 'other-protocol.json'
    other.write_text('{}')
    data['protocol_file'] = str(other)
    with pytest.raises(ValueError, match='actual data binding differs'):
        package.verify_result_scope(data, freeze)


def test_readonly_preflight_passes_freeze_to_original_result_proof(monkeypatch):
    monkeypatch.setattr(package, 'verify_parent', lambda _: {})
    monkeypatch.setattr(package, 'verify_freeze', lambda _: ({}, {}))
    proofs = []
    monkeypatch.setattr(package, 'verify_result', lambda result, freeze: proofs.append((result, freeze)) or {})
    monkeypatch.setattr(package, 'stage', lambda *a: pytest.fail('readonly must not stage'))
    monkeypatch.setattr(package.sys, 'argv', ['package', '--parent', 'parent', '--freeze', 'freeze',
                                            '--result', 'result', '--output', 'out'])
    package.main()
    assert proofs == [(Path('result'), Path('freeze'))]


def test_actual_scope_binding_current_source_freeze_no_model_calls():
    cohort_file = ROOT / 'reports/.nanocodex/artifact-v2-formal-20261011-r2/cohort.json'
    cohort = package.load(cohort_file)
    roles = {row['role']: row for row in cohort['bindings']}
    data = {"cohort_file": str(cohort_file), "cohort_sha256": package.sha(cohort_file), "cohort_hash": cohort['artifact_hash']}
    for prefix in ('protocol', 'authorization'):
        data[prefix + '_file'] = roles[prefix]['file']
        data[prefix + '_sha256'] = roles[prefix]['sha256']
    actual = package.verify_result_scope(data, ROOT / 'reports/.nanocodex/artifact-v2-formal-20261011-r2/freeze.json')
    assert actual['cohort_hash'] == cohort['artifact_hash']


@pytest.mark.parametrize("name", ["existing", "outside"])
def test_stage_bad_output_refused_before_proofs(tmp_path, monkeypatch, name):
    monkeypatch.setattr(package, "ROOT", tmp_path)
    target = tmp_path / ("releases/existing" if name == "existing" else "outside")
    if name == "existing":
        target.mkdir(parents=True)
    monkeypatch.setattr(package, "verify_result", lambda _: pytest.fail("must not prove"))
    with pytest.raises(ValueError, match="fresh releases"):
        package.stage("parent", "freeze", "result", target)
