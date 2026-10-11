"""Seal copied V2 tools and a proven v3.3 runtime; default is read-only preflight.

No build, installation, provider call, reservation or evidence regeneration.
--stage runs existing localhost GUI/CLI fixtures, then creates a checked ZIP.
"""
import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from evals.verification.calibration import digest, load, require  # noqa: E402

PARENT_SHA = "16fab536149aae8e7d1e8645d2b5b63cbc9ad02e47f7a195b715444b53c5819b"
MAIN = "verification_artifact_v2_formal_collect"
JEV = "verification_artifact_v2_formal"
NATIVE = {"BugleCat.exe", "ncx.exe", "BugleCat_0.1.0_x64-setup.exe"}
SCOPE = "exact frozen mixed MBPP/HumanEval code acceptance assertions and restricted runtimes"
METRIC_KEYS = ("threshold", "supported_groups", "unsupported_groups", "false_rejected_supported",
               "detected_unsupported", "false_rejection_upper_95", "error_rejection_lower_95", "gate_met",
               "gate_reasons", "failed_attempts_retained_in_denominators", "reject_signals", "unverified_signals",
               "verified_pass_signals")


def public_metrics(value):
    return {**{key: value[key] for key in METRIC_KEYS if key in value},
            "failed_attempt_count": len(value.get("failed_attempts", {}))}


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sealed(value):
    require(value.get("artifact_hash") == digest({k: v for k, v in value.items() if k != "artifact_hash"}),
            "result seal mismatch")
    return value


def safe_file(path, root=None):
    path, root = Path(path), Path(ROOT if root is None else root).resolve()
    require(not path.is_symlink() and path.resolve(strict=True).is_relative_to(root), "unsafe source path")
    require(all(not part.is_symlink() for part in [path, *path.parents] if part != part.parent),
            "symlink source ancestor")
    require(path.is_file(), "regular source file required")
    return path.resolve()


def old_packager():
    spec = importlib.util.spec_from_file_location("audited_parent_packager", ROOT / "scripts/package-artifact-evaluation.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_parent(parent):
    parent = Path(parent).resolve(strict=True)
    require(sha(safe_file(parent / "manifest.json")) == PARENT_SHA, "audited parent seal changed")
    value = load(parent / "manifest.json")
    require(set(value["files"]) == NATIVE and value["policy_version"] == "host-rules-v3.3"
            and value["production_jev_threshold"] is None and value["production_enforce_categories"] == []
            and value["paid_build_smoke_calls"] == 0, "parent runtime scope changed")
    for name, item in value["files"].items():
        require(sha(safe_file(parent / name)) == item["sha256"], "parent executable changed")
    for item in value["evidence"]:
        require(sha(safe_file(parent / item["path"])) == item["sha256"], "parent evidence changed")
    require(sha(safe_file(parent / value["source_snapshot"])) == value["source_snapshot_sha256"],
            "parent source snapshot changed")
    return value


def verify_freeze(path):
    value = load(safe_file(path))
    require(value["schema"] == "buglecat.verification.artifact-formal-freeze.v2", "original V2 freeze required")
    bindings = {}
    for item in value["files"]:
        original, archive = safe_file(item["original"]), safe_file(item["archive"])
        require(original not in bindings and sha(original) == sha(archive) == item["sha256"],
                "frozen original/archive mismatch")
        bindings[original] = item["sha256"]
    binaries = {name: ROOT / f"rust/target/debug/examples/{name}.exe" for name in (MAIN, JEV)}
    require(all(path.resolve() in bindings for path in binaries.values()), "two actual frozen V2 executables required")
    return bindings, binaries


def no_jev_for_allocation(data):
    from evals.verification.artifact_cohort import reservations
    from evals.verification.budget_authorization_v2 import resolve
    chain = resolve(data["authorization_file"])
    rows, _ = reservations(Path(chain["authorization"]["jev"]["ledger_dir"]), "jev", chain)
    groups = {case["group_id"] for case in load(data["cohort_file"])["cases"]}
    require(not groups & {row["group_id"] for row in rows}, "incomplete scope already has Jev reservations")


def verify_result_scope(data, freeze):
    """Join the owner's original data to the requested original executable scope."""
    bindings, _ = verify_freeze(freeze)
    original = load(freeze)
    cohort_file = safe_file(data["cohort_file"])
    cohort = sealed(load(cohort_file))
    require(sha(cohort_file) == data["cohort_sha256"]
            and data["cohort_hash"] == cohort["artifact_hash"] == original["cohort_hash"],
            "original freeze and actual result cohort differ")
    roles = ("source", "catalog", "protocol", "authorization", "profile",
             "formal_statistical_owner", "formal_request_owner", "formal_observation_owner", "request_support_owner")
    selected = {}
    for role in roles:
        matches = [item for item in cohort["bindings"] if item["role"] == role]
        require(len(matches) == 1, "unique original cohort binding required: " + role)
        item = matches[0]
        path = safe_file(item["file"])
        require(bindings.get(path) == item["sha256"] == sha(path),
                "result cohort binding absent or different in original freeze: " + role)
        selected[role] = item
    for role, prefix in (("protocol", "protocol"), ("authorization", "authorization")):
        item = selected[role]
        require(safe_file(data[prefix + "_file"]) == safe_file(item["file"])
                and data[prefix + "_sha256"] == item["sha256"], "actual data binding differs: " + role)
    return {"original_freeze_sha256": sha(freeze), "cohort_hash": cohort["artifact_hash"],
            "cohort_sha256": data["cohort_sha256"],
            "public_source_bindings": [{"role": role, "sha256": selected[role]["sha256"]}
                                       for role in ("source", "catalog", "protocol")]}


def verify_result(path, freeze=None):
    """Only aggregate metadata escapes the original evidence verification."""
    from evals.verification import artifact_v2_formal as formal
    result = sealed(load(safe_file(path)))
    require(result.get("production_authorized") is False and result.get("native_claim_certification") is False
            and result.get("overall_agent_benefit_certification", False) is False, "result exceeds scope")
    schema = result["schema"]
    if schema == "buglecat.verification.artifact-calibration.formal.v2":
        actual, _, data, rows, observations = formal.revalidate_calibration(path)
        require(actual == result and actual["calibration_numeric_gate_met"] is False
                and actual["qualified_for_holdout_threshold"] is None, "qualified calibration requires holdout")
        status, threshold = "calibration_failed_holdout_not_opened", None
    elif schema == "buglecat.verification.artifact-holdout.formal.v2":
        actual = formal.holdout_value(result["registration_file"], result["observations_file"])
        require(actual == result, "holdout differs from original observations")
        registered, data, rows, _ = formal.revalidate(result["registration_file"])
        from evals.verification.budget_authorization_v2 import resolve
        chain = resolve(data["authorization_file"])
        marker = Path(chain["authorization"]["jev"]["ledger_dir"]) / "formal-v2-holdout-evaluations" / (data["cohort_hash"] + ".json")
        claim = sealed(load(marker))
        expected = {"schema": "buglecat.verification.artifact-holdout-evaluation-consumption.v2",
                    "cohort_hash": data["cohort_hash"], "registration_hash": registered["artifact_hash"],
                    "calibration_hash": registered["calibration_hash"], "result_hash": result["artifact_hash"],
                    "observations_sha256": sha(result["observations_file"]), "output_file": str(Path(path).resolve()),
                    "threshold_tuning": False, "holdout_evaluation_consumptions": 1, "new_paid_calls": 0}
        require(claim == {**expected, "artifact_hash": digest(expected)}, "fixed holdout marker differs from result")
        observations = formal.read_lines(result["observations_file"])
        status = "holdout_passed" if actual["gate_met"] else "holdout_failed"
        threshold = actual["certified_threshold"]
    else:
        require(schema == "buglecat.verification.artifact-evaluation-incomplete.v2"
                and result["formal_evaluation_completed"] is False and result["certified_threshold"] is None
                and result.get("registration_file") is None, "explicit unregistered incomplete V2 report required")
        from evals.verification.artifact_v2_formal_intake import verify_dataset
        require(sha(result["dataset_file"]) == result["dataset_file_sha256"], "incomplete original dataset changed")
        source = load(result["dataset_file"])
        data, rows = verify_dataset(result["dataset_file"], source["prepared_directory"], source["collector_executable_file"])
        counts = {name: sum(row["label"] == name for row in rows) for name in ("supported", "unsupported", "unknown")}
        require(result["label_counts"] == counts and (counts["supported"] < 150 or counts["unsupported"] < 50),
                "incomplete quotas differ from original labels or quotas already met")
        no_jev_for_allocation(data)
        observations, status, threshold = [], "incomplete_quotas_not_met", None
    scope_binding = verify_result_scope(data, freeze) if freeze is not None else None
    counts = {name: sum(row["label"] == name for row in rows) for name in ("supported", "unsupported", "unknown")}
    return {"status": status, "scope": SCOPE, "original_result_sha256": sha(path),
            "original_result_hash": result["artifact_hash"], "label_counts": counts,
            "original_scope_binding": scope_binding,
            "collected_groups": data["collected_groups"], "uncollected_groups": data["uncollected_groups"],
            "unlabeled_groups": sum(row["label"] == "unlabeled" for row in rows),
            "allocated_groups": sum(case["split"] == data["split"] for case in load(data["cohort_file"])["cases"]),
            "observations": len(observations), "certified_artifact_threshold": threshold,
            "grid_results": [public_metrics(value) for value in result.get("grid_results", [])],
            "holdout_metrics": public_metrics(result) if status.startswith("holdout_") else None,
            "formal_artifact_certification": status == "holdout_passed", "overall_goal_complete": False,
            "native_claim_certification": False, "overall_agent_benefit_certification": False,
            "production_authorized": False}


def offline_attest(binary, name, directory, bindings, copied_root=None):
    """Actual copied entrypoints, fake transport only; raw fixture files stay private."""
    action = "offline-test" if name == MAIN else "offline-transport-test"
    original_binary = (ROOT / f"rust/target/debug/examples/{name}.exe").resolve()
    require(name in (MAIN, JEV) and sha(binary) == bindings.get(original_binary),
            "copied executable differs from original freeze")
    env = {k: v for k, v in os.environ.items() if not any(token in k.upper() for token in ("KEY", "TOKEN", "SECRET"))}
    run = subprocess.run([str(binary), action, str(directory), "success"], cwd=ROOT, env=env,
                         capture_output=True, encoding="utf-8", timeout=45)
    require(run.returncode == 0, "actual copied V2 offline entrypoint failed")
    summary = json.loads(run.stdout)
    events_path = directory / ("attempt-events.jsonl" if name == MAIN else "attempt/events.jsonl")
    rows = [json.loads(line) for line in events_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    request_path = directory / ("request.json" if name == MAIN else "attempt/request.json")
    receipt_path = directory / ("receipt.json" if name == MAIN else "attempt/receipt.json")
    raw_path = directory / ("raw-response.json" if name == MAIN else "attempt/raw-response.json")
    event_key = "event" if name == MAIN else "status"
    require(len(rows) == 3 and [r[event_key] for r in rows] == ["attempt_started", "request_submitted", "attempt_finished"],
            "offline original event sequence changed")
    require(rows[1]["actual_request_hash"] == sha(request_path), "offline actual body hash differs")
    receipt = load(receipt_path)
    if name == MAIN:
        from evals.verification.artifact_v2_formal_intake import runner_sources
        sources = runner_sources()
        info = summary["build_info"]
        require(summary["paid_calls"] == summary["http_calls"] == 0
                and all(row["build_info"] == info for row in rows) and receipt["build_info"] == info
                and info["executable_sha256"] == sha(binary), "main offline/exe provenance differs")
        require(rows[-1]["receipt_sha256"] == sha(receipt_path), "main receipt bytes differ")
    else:
        sources = {"runner_source_sha256": ROOT / "rust/crates/ncx-core/examples/verification_artifact_v2_formal.rs",
                   "provider_source_sha256": ROOT / "rust/crates/ncx-provider/src/jev.rs",
                   "frozen_support_sha256": ROOT / "rust/crates/ncx-core/examples/verification_artifact_profile_formal/support.rs",
                   "frozen_formal_sha256": ROOT / "rust/crates/ncx-core/examples/verification_artifact_profile_formal/formal.rs"}
        info = rows[0]
        require(summary["new_paid_calls"] == summary["network_calls"] == 0
                and summary["duplicate_reservation_blocked"] is True and info["offline"] is True
                and all(row["runner_exe_sha256"] == sha(binary) for row in rows)
                and receipt == rows[-1], "Jev offline/exe/receipt provenance differs")
        from evals.verification.artifact_v2_formal_observations import PYTHON_OWNERS
        expected = [{"file": "evals/verification/" + name, "sha256": sha(ROOT / "evals/verification" / name)} for name in PYTHON_OWNERS]
        require(info["compiled_python_owners"] == expected, "compiled Python graph differs")
        sources.update({"python:" + name: ROOT / "evals/verification" / name for name in PYTHON_OWNERS})
    attestations = []
    for field, path in sources.items():
        path = path.resolve()
        expected = bindings.get(path, sha(path))
        require(sha(path) == expected, "source differs from original freeze: " + path.relative_to(ROOT).as_posix())
        if not field.startswith("python:"):
            require(info[field] == expected, "embedded source differs from original freeze")
        if copied_root:
            require(sha(Path(copied_root) / path.relative_to(ROOT)) == expected, "copied source differs")
        attestations.append({"field": field, "file": path.relative_to(ROOT).as_posix(), "sha256": expected,
                             "authority": "original_archive_and_frozen_exe" if path in bindings else "embedded_hash_in_original_frozen_exe"})
    require(raw_path.is_file() and receipt["call_status"] == "ok", "offline raw response/success missing")
    # Both parsers preserve a SHA of the unmodified response, not a reserialization.
    response_hash = receipt.get("response_sha256", receipt.get("raw_response_sha256"))
    require(response_hash == sha(raw_path), "offline raw response differs")
    return {"name": name, "sha256": sha(binary), "bytes": binary.stat().st_size,
            "actual_copied_entrypoint": True, "fake_transport_only": True, "paid_calls": 0,
            "original_events_sha256": sha(events_path), "compiled_source_bindings": attestations}


def public_sources(bindings, cohort):
    allowed = {p for p in bindings if p.suffix in {".py", ".rs"} and
               (p.is_relative_to(ROOT / "evals/verification") or p.is_relative_to(ROOT / "rust/crates"))}
    from evals.verification.artifact_v2_formal_intake import runner_sources
    allowed.update(path.resolve() for path in runner_sources().values())
    allowed.update(ROOT / "rust/crates/ncx-core/examples" / name for name in (
        "verification_artifact_profile_formal/support.rs", "verification_artifact_profile_formal/formal.rs"))
    old = old_packager()
    allowed.update(ROOT / "evals/verification/data/mbpp" / name for name in (*old.PUBLIC_MBPP_FILES, "ATTRIBUTION.md"))
    human = ROOT / "evals/verification/data/humaneval"
    record = load(human / "download-6d43fb980f9f.json")
    allowed.add(human / "download-6d43fb980f9f.json")
    for item in record["files"].values():
        path = Path(item["file"])
        require(sha(path) == item["sha256"], "official HumanEval chain changed")
        allowed.add(path)
    allowed.add(human / "humaneval-derived-6d43fb980f9f-v2.json")
    # Only explicit public-source roles, never the cohort's historical ledger snapshot.
    for item in cohort["bindings"]:
        path = Path(item["file"]).resolve()
        if item.get("role") in {"protocol", "source", "catalog"}:
            require(sha(path) == item["sha256"], "public cohort binding changed")
            allowed.add(path)
    return sorted(allowed)


def write_json(path, value):
    with Path(path).open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write("\n")


def checked_zip(directory, zip_path, files):
    require(not zip_path.exists(), "fresh ZIP required")
    expected = {item["path"]: item for item in files}
    expected["manifest.json"] = {"sha256": sha(directory / "manifest.json"), "bytes": (directory / "manifest.json").stat().st_size}
    with zipfile.ZipFile(zip_path, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in expected:
            require(not Path(name).anchor and ".." not in Path(name).parts and ":" not in name,
                    "unsafe ZIP path")
            archive.write(directory / name, name)
    with zipfile.ZipFile(zip_path) as archive:
        require(archive.testzip() is None and len(archive.namelist()) == len(expected)
                and set(archive.namelist()) == expected.keys(), "ZIP entries differ")
        for name, item in expected.items():
            raw = archive.read(name)
            require(hashlib.sha256(raw).hexdigest() == item["sha256"] and len(raw) == item["bytes"], "ZIP bytes differ")


def stage(parent, freeze, result, output):
    output = Path(output).resolve()
    require(output.is_relative_to(ROOT / "releases") and output != ROOT / "releases"
            and not output.exists() and not output.with_suffix(".zip").exists(), "fresh releases directory/ZIP required")
    parent_manifest = verify_parent(parent)
    bindings, binaries = verify_freeze(freeze)
    summary = verify_result(result, freeze)
    original = load(result)
    dataset = load(original["dataset_file"]) if "dataset_file" in original else load(load(original["registration_file"])["dataset_file"])
    cohort = load(dataset["cohort_file"])
    sources = public_sources(bindings, cohort)
    source_snapshot = {path: sha(path) for path in sources}
    old = old_packager()
    output.mkdir()
    for name in NATIVE:
        shutil.copy2(Path(parent) / name, output / name)
    write_json(output / "parent-manifest.json", parent_manifest)
    shutil.copy2(Path(parent) / parent_manifest["source_snapshot"], output / "parent-source-snapshot.json")
    for path in sources:
        path = safe_file(path)
        if not path.read_bytes().startswith(b"\x1f\x8b"):
            require(not old.SECRET.search(path.read_text(encoding="utf-8-sig")), "secret-like public source refused")
        dest = output / path.relative_to(ROOT)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, dest)
    evidence = output / "evidence"
    evidence.mkdir()
    write_json(evidence / "evaluation-summary.json", summary)
    attestations = []
    with tempfile.TemporaryDirectory(prefix="buglecat-v2-package-offline-") as private:
        for name, path in binaries.items():
            destination = output / path.relative_to(ROOT)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
            attestations.append(offline_attest(destination, name, Path(private) / name, bindings, output))
    write_json(evidence / "sidecar-attestations.json", attestations)
    runtime = old.runtime_smoke(output, parent_manifest)
    # Runtime fixture rows contain candidate transcripts; release only aggregate proof.
    records = evidence / "runtime-verification-records.json"
    records.unlink(missing_ok=True)
    (output / "README.md").write_text(
        "# BugleCat V3.3 + V2 artifact evaluation\n\n"
        "Run BugleCat.exe or ncx.exe --help. Installer is supplied, not installed.\n\n"
        f"Evaluation: {summary['status']}. Scope: {SCOPE}.\n"
        "Native claims and overall agent benefit are unqualified; production is off, categories empty, threshold null.\n"
        "A non-rejection is unverified. Benchmark contamination is not ruled out.\n"
        "The two tools are source-registered evaluation entrypoints, not a portable automatic certification service.\n"
        "No real fee ledger, session, candidate or raw provider response is bundled.\n", encoding="utf-8")
    require(verify_freeze(freeze)[0] == bindings, "source freeze changed while packaging")
    require(verify_result(result, freeze) == summary, "original evaluation changed while packaging")
    require(all(sha(path) == expected == sha(output / path.relative_to(ROOT))
                for path, expected in source_snapshot.items()), "public source bytes changed while packaging")
    verify_parent(parent)
    files = [{"path": p.relative_to(output).as_posix(), "sha256": sha(p), "bytes": p.stat().st_size}
             for p in sorted(output.rglob("*")) if p.is_file()]
    for item in files:
        p = output / item["path"]
        if p.suffix in {".json", ".py", ".rs", ".md", ".txt", ".jsonl"}:
            require(not old.SECRET.search(p.read_text(encoding="utf-8-sig")), "secret-like package text refused")
    manifest = {"schema": "buglecat.artifact-evaluation-package.v2", "parent_manifest_sha256": PARENT_SHA,
                "original_freeze_sha256": sha(freeze), "evaluation_summary": summary, "components": attestations,
                "production_authorized": False, "production_jev_threshold": None, "production_enforce_categories": [],
                "native_claim_certification": False, "overall_agent_benefit_certification": False,
                "overall_goal_complete": False, "installer_executed": False, "paid_package_calls": 0,
                "runtime_validation": runtime, "staging_script_sha256": sha(__file__), "files": files}
    write_json(output / "manifest.json", manifest)
    checked_zip(output, output.with_suffix(".zip"), files)
    return {"package": str(output), "manifest_sha256": sha(output / "manifest.json"),
            "zip_sha256": sha(output.with_suffix(".zip")), "paid_calls": 0, "status": summary["status"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("parent", "freeze", "result", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--stage", action="store_true", help="copy, run localhost runtime fixtures, seal and ZIP")
    args = parser.parse_args()
    if args.stage:
        value = stage(args.parent, args.freeze, args.result, args.output)
    else:
        verify_parent(args.parent)
        verify_freeze(args.freeze)
        value = {"status": "read_only_preflight", "evaluation": verify_result(args.result, args.freeze),
                 "runtime_smoke_not_run": True, "paid_calls": 0}
    print(json.dumps(value, ensure_ascii=False))


if __name__ == "__main__":
    main()
