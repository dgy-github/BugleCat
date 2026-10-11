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
PUBLIC_MBPP_FILES = (
    "mbpp-expanded-20261011-v2.json",
    "mbpp-full-f82046ba5aabbbb427dbfd38a254d26bff08b533.jsonl",
    "sanitized-mbpp-f82046ba5aabbbb427dbfd38a254d26bff08b533.json",
)
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


def runtime_smoke(output, parent_manifest):
    """Run the copied GUI/CLI against existing localhost fixtures before seal."""
    evidence = output / "evidence"
    env = {**os.environ, "NCX_JEV_API_KEY": "", "NANOCODEX_API_KEY": "",
           "NCX_JEV_GUI_EXE": str(output / "BugleCat.exe"),
           "NCX_JEV_CLI_EXE": str(output / "ncx.exe"),
           "NCX_JEV_SMOKE_REPORT": str(evidence / "runtime-smoke.json"),
           "NCX_JEV_LIFECYCLE_REPORT": str(evidence / "lifecycle.json")}
    env.pop("NCX_JEV_RESTORE_ROOT", None)
    for name in ("verification.mjs", "verification-lifecycle.mjs"):
        run = subprocess.run(["node", str(ROOT / "rust/gui/e2e" / name)], cwd=ROOT, env=env,
                             capture_output=True, encoding="utf-8", timeout=480)
        require(run.returncode == 0, f"copied runtime localhost smoke failed: {name}")
    smoke = load(evidence / "runtime-smoke.json")
    lifecycle = load(evidence / "lifecycle.json")
    require(smoke["cliExit"] == 0 and smoke["newLiveJevCalls"] == 0
            and smoke["liveJevCalls"] == 0 and smoke["guiTranscriptRestored"] is True
            and smoke["unverifiedLabelVisible"] is True, "copied runtime smoke incomplete or paid")
    require(Path(smoke["cliExe"]).resolve() == output / "ncx.exe"
            and Path(smoke["guiExe"]).resolve() == output / "BugleCat.exe",
            "runtime smoke used other executables")
    require(lifecycle["paidCalls"] == 0 and lifecycle["exeSha256"] == sha(output / "BugleCat.exe")
            and Path(lifecycle["exe"]).resolve() == output / "BugleCat.exe",
            "lifecycle used another GUI or made paid calls")
    scenarios = lifecycle["scenarios"]
    require(len(scenarios) == 3 and {row["mode"] for row in scenarios} == {"off", "shadow", "enforce"}
            and all(row["cancelCompleted"] and row["restoredWithoutPass"]
                    and row["durableWriteCalls"] == 1 and row["newJevAttempts"] == 0
                    and row["passedLabels"] == 0 for row in scenarios), "copied lifecycle incomplete")
    sessions = Path(smoke["root"]) / "workspace/.nanocodex/sessions"
    records = [json.loads(line) for name in smoke["logs"]
               for line in (sessions / name).read_text(encoding="utf-8").splitlines() if line.strip()]
    require(records and all(row["result"]["policy_version"] == parent_manifest["policy_version"]
                            and row["result"].get("attempted_calls", 0) == 0 for row in records),
            "copied runtime policy changed or attempted Jev")
    create(evidence / "runtime-verification-records.json", records)
    for name, metadata in parent_manifest["files"].items():
        require(sha(output / name) == metadata["sha256"], "copied parent bytes changed during smoke")
    return {"actual_copied_runtime_smoke": True, "policy_version": parent_manifest["policy_version"],
            "modes": sorted(row["mode"] for row in scenarios), "paid_calls": 0,
            "installer_executed": False}


def package_readme(output, result):
    """Plain startup instructions with the actual result's bounded scope."""
    if result["schema"] == "buglecat.verification.artifact-holdout-evaluation.v1":
        status = "留出门槛通过" if result["artifact_benchmark_certified"] else "留出门槛未通过"
    elif result["schema"] == "buglecat.verification.artifact-formal-calibration.v1":
        status = "校准门槛未通过，未打开留出"
    else:
        status = "本次正式评测未完成"
    text = f"""# BugleCat 本地运行包

本包包含 v3.3 GUI/CLI 修复及四个独立代码产物评测程序。

## 启动

- 双击 `BugleCat.exe` 启动桌面应用；使用已有应用设置配置主模型。
- 命令行入口为 `ncx.exe`，运行 `ncx.exe --help` 查看参数。
- `BugleCat_0.1.0_x64-setup.exe` 为 v3.3 桌面安装程序。独立评测工具保存在本包 `rust/target/debug/examples/`，使用说明见 `evals/verification/ARTIFACT_EVALUATION.md`。

## 本次评测

状态：{status}。完整结果见 `evidence/evaluation-result.json`。
结论范围为冻结 MBPP 代码产物断言。原生声明及整体 agent 决策收益仍未认证，生产校验默认关闭，未自动启用评测阈值。
实际复制 GUI/CLI 的启动、恢复、取消和三模式故障检查见 `evidence/runtime-smoke.json` 及 `evidence/lifecycle.json`；检查使用本地夹具，零收费。安装程序未实际安装。
`manifest.json` 记录程序、源、结果和验证文件的 SHA256。费用台账、原始会话、候选及密钥保留本地。
"""
    (output / "README.md").write_text(text, encoding="utf-8")


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
    # A directory glob can include abandoned drafts or private local additions.
    sources += [ROOT / "evals/verification/data/mbpp" / name for name in PUBLIC_MBPP_FILES]
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
    runtime = runtime_smoke(output, manifest)
    package_readme(output, result)
    files = [{"path": path.relative_to(output).as_posix(), "sha256": sha(path), "bytes": path.stat().st_size}
             for path in sorted(output.rglob("*")) if path.is_file()]
    package = {"schema": "buglecat.artifact-evaluation-package.v1", "parent_manifest_sha256": PARENT_SHA,
               "parent_policy_version": manifest["policy_version"], "components": attestations,
               "component_manifest_sha256": sha(component_file), "evaluation_result_sha256": sha(result_file),
               "evaluation_scope": "artifact acceptance only; see evaluation result",
               "production_authorized": False, "native_claim_certification": False,
               "production_jev_threshold": None, "production_enforce_categories": [],
               "installer_executed": False, "paid_package_smoke_calls": 0,
               "runtime_validation": runtime,
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
