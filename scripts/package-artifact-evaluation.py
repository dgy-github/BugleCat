"""Stage an audited GUI/CLI parent plus actual, independently attested eval tools.

No configuration edit, installation, real provider request or credential copy.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from evals.verification.artifact_formal_observations import (  # noqa: E402 -- script entrypoint adds repository root
    holdout_owner_sources,
    runner_sources,
)
from evals.verification.calibration import create, load, require  # noqa: E402

PARENT_SHA = "16fab536149aae8e7d1e8645d2b5b63cbc9ad02e47f7a195b715444b53c5819b"
NAMES = {"verification_artifact_profile_collect", "verification_artifact_profile_formal",
         "verification_artifact_holdout_collect", "verification_artifact_holdout_formal"}
SECRET = re.compile(r"apikey_[0-9a-f]{20,}|sk-[A-Za-z0-9_-]{24,}|(?i:bearer)\s+[A-Za-z0-9_.-]{30,}")


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main_sources(name):
    examples = ROOT / "rust/crates/ncx-core/examples"
    files = {
        "runner_source_sha256": examples / f"{name}.rs",
        "support_source_sha256": examples / name / "support.rs",
        "collection_engine_source_sha256": examples / name / "engine.rs",
        "frozen_collector_support_sha256": examples / "verification_artifact_collect/support.rs",
        "legacy_support_source_sha256": examples / "verification_collect/support.rs",
        "request_builder_source_sha256": ROOT / "rust/crates/ncx-provider/src/request.rs",
        "response_parser_source_sha256": ROOT / "rust/crates/ncx-provider/src/response.rs",
    }
    if name == "verification_artifact_holdout_collect":
        files.update(holdout_owner_sources(examples))
        files["profile_collector_validator_source_sha256"] = examples / "verification_artifact_profile_collect/support.rs"
    return files


def stage(parent, component_file, result_file, output):
    parent, output = Path(parent).resolve(), Path(output).resolve()
    require(output.is_relative_to(ROOT / "releases") and not output.exists(), "fresh releases directory required")
    require(sha(parent / "manifest.json") == PARENT_SHA, "audited v3.3 parent manifest changed")
    manifest = load(parent / "manifest.json")
    require(manifest["production_jev_threshold"] is None and manifest["production_enforce_categories"] == []
            and manifest["paid_build_smoke_calls"] == 0, "parent production/smoke scope changed")
    for name, item in manifest["files"].items():
        require(sha(parent / name) == item["sha256"], "parent executable changed")
    for item in manifest["evidence"]:
        require(sha(parent / item["path"]) == item["sha256"], "parent runtime evidence changed")
    require(sha(parent / manifest["source_snapshot"]) == manifest["source_snapshot_sha256"], "parent build snapshot changed")
    components = load(component_file)["components"]
    require({item["name"] for item in components} == NAMES and len(components) == 4, "four frozen artifact entries required")
    result = load(result_file)
    require(result["production_authorized"] is False and result.get("native_claim_certification") is not True,
            "result exceeds artifact evaluation scope")
    if result["schema"] == "buglecat.verification.artifact-holdout-evaluation.v1":
        from evals.verification.artifact_holdout import revalidate_evaluation
        revalidate_evaluation(result_file)
    elif result["schema"] == "buglecat.verification.artifact-formal-calibration.v1":
        from evals.verification.artifact_formal import revalidate_calibration
        revalidate_calibration(result_file)
        require(result["calibration_numeric_gate_met"] is False, "qualified calibration still requires holdout")
    else:
        require(result["schema"] == "buglecat.verification.artifact-evaluation-incomplete.v1"
                and result["certified_threshold"] is None and result["formal_evaluation_completed"] is False,
                "original evaluated result or explicit incomplete source report required")
        from evals.verification.artifact_formal_intake import verify_dataset_cases
        require(sha(result["dataset_file"]) == result["dataset_file_sha256"], "incomplete report dataset changed")
        data = load(result["dataset_file"])
        verify_dataset_cases(data)
        counts = {label: sum(case["label"] == label for case in data["cases"])
                  for label in ("supported", "unsupported", "unknown")}
        require(result["label_counts"] == counts, "incomplete report contradicts independent labels")
    for item in components:
        require(sha(ROOT / item["file"]) == item["sha256"], "frozen sidecar executable changed")
    output.mkdir()
    shutil.copy2(parent / "manifest.json", output / "parent-manifest.json")
    for name in manifest["files"]:
        shutil.copy2(parent / name, output / name)
    shutil.copytree(parent / "evidence", output / "parent-evidence")
    shutil.copy2(parent / "source-snapshot.json", output / "parent-source-snapshot.json")
    # Only code, public benchmark sources and fixed protocols; no session/ledger.
    sources = list((ROOT / "evals/verification").glob("*.py"))
    sources += list((ROOT / "evals/verification/data/mbpp").glob("*.json"))
    sources += list((ROOT / "evals/verification/data/mbpp").glob("*.jsonl"))
    sources += [ROOT / "evals/verification/data/mbpp/ATTRIBUTION.md",
                ROOT / "evals/verification/ARTIFACT_EVALUATION.md",
                ROOT / "evals/verification/ARTIFACT_ACCEPTANCE_PROTOCOL.md",
                ROOT / "evals/verification/reports/artifact_protocol_20261011_v1.json"]
    sources += list((ROOT / "rust/crates").rglob("*.rs"))
    sources = sorted(set(path for path in sources if "target" not in path.parts and not path.is_symlink()))
    for path in sources:
        require(not SECRET.search(path.read_text(encoding="utf-8-sig")), "secret-like source text; package refused")
        destination = output / path.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
    evidence = output / "evidence"
    evidence.mkdir()
    shutil.copy2(result_file, evidence / "evaluation-result.json")
    attestations = []
    env = {**os.environ, "NCX_JEV_API_KEY": "", "NANOCODEX_API_KEY": ""}
    for item in components:
        name = item["name"]
        binary = output / item["file"]
        binary.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / item["file"], binary)
        attempt = evidence / name
        is_main = name.endswith("_collect")
        action = "offline-test" if is_main else "offline-transport-test"
        run = subprocess.run([str(binary), action, str(attempt), "success"], cwd=ROOT, env=env,
                             capture_output=True, encoding="utf-8", timeout=30)
        require(run.returncode == 0, "staged actual sidecar offline smoke failed")
        summary = json.loads(run.stdout)
        require(summary["paid_calls"] == 0 and summary["http_calls" if is_main else "network_calls"] == 0,
                "offline smoke scope violated")
        bindings = main_sources(name) if is_main else runner_sources(ROOT / f"rust/crates/ncx-core/examples/{name}.rs")
        require(all(summary["build_info"][field] == sha(path) for field, path in bindings.items()),
                "staged binary differs from actual compiled source/owner")
        require(summary["build_info"]["executable_sha256" if is_main else "runner_exe_sha256"] == sha(binary),
                "offline smoke ran a different executable")
        create(evidence / f"{name}-summary.json", summary)
        attestations.append({"name": name, "file": item["file"], "sha256": sha(binary),
                             "bytes": binary.stat().st_size,
                             "compiled_source_bindings": [{"field": field, "file": path.relative_to(ROOT).as_posix(), "sha256": sha(path)}
                                                          for field, path in bindings.items()],
                             "staged_actual_offline_smoke": True, "paid_calls": 0})
    files = [{"path": path.relative_to(output).as_posix(), "sha256": sha(path), "bytes": path.stat().st_size}
             for path in sorted(output.rglob("*")) if path.is_file()]
    package = {"schema": "buglecat.artifact-evaluation-package.v1", "parent_manifest_sha256": PARENT_SHA,
               "parent_policy_version": manifest["policy_version"], "components": attestations,
               "component_manifest_sha256": sha(component_file), "evaluation_result_sha256": sha(result_file),
               "evaluation_scope": "artifact acceptance only; see evaluation result",
               "production_authorized": False, "native_claim_certification": False,
               "production_jev_threshold": None, "production_enforce_categories": [],
               "installer_executed": False, "paid_package_smoke_calls": 0,
               "staging_script_sha256": sha(__file__), "files": files}
    create(output / "manifest.json", package)
    print(json.dumps({"package": str(output), "manifest_sha256": sha(output / "manifest.json"),
                      "source_and_artifact_files": len(files), "paid_package_calls": 0}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("parent", "components", "result", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    stage(args.parent, args.components, args.result, args.output)


if __name__ == "__main__":
    main()
