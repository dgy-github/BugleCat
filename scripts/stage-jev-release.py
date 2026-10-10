"""Freeze build inputs and stage a fresh local package; never changes config."""

import argparse
import json
import os
import re
import shutil
import subprocess
from datetime import datetime, timezone
from hashlib import sha256
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
EXCLUDED = {"node_modules", "dist", ".git", "__pycache__", ".nanocodex", ".ncx", "gen"}


def digest(path):
    return sha256(path.read_bytes()).hexdigest()


def graph():
    paths = []
    for base, directories, files in os.walk(REPO / "rust"):
        directories[:] = [name for name in directories
                          if name not in EXCLUDED and not name.startswith("target")
                          and not (Path(base) / name).is_symlink()]
        paths.extend(Path(base) / name for name in files
                     if not (Path(base) / name).is_symlink())
    paths += list((REPO / "scripts").glob("check-protocol-version*.mjs"))
    rows = [{"path": p.relative_to(REPO).as_posix(),
             "bytes": p.stat().st_size, "sha256": digest(p)}
            for p in sorted(paths)]
    raw = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()
    return {"algorithm": "sha256 canonical JSON sorted path/bytes/sha256 inventory",
            "sha256": sha256(raw).hexdigest(), "files": rows}


def create_json(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, ensure_ascii=False)
        stream.write("\n")


def test_results(path):
    raw = path.read_bytes()
    text = raw.decode("utf-16" if raw.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8", errors="replace")
    results = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
    if not results or any(int(failed) for _, failed, _ in results):
        raise ValueError(f"missing passing test evidence: {path.name}")
    return [{"passed": int(passed), "failed": int(failed), "ignored": int(ignored)}
            for passed, failed, ignored in results]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["freeze", "stage", "seal"])
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve()
    if not directory.is_relative_to(REPO / "releases"):
        parser.error("directory must stay under repository releases")
    directory.mkdir(parents=True, exist_ok=True)
    snapshot_path = directory / "source-snapshot.json"
    current = graph()
    if args.action == "freeze":
        create_json(snapshot_path, current)
        print(json.dumps({"source_sha256": current["sha256"], "files": len(current["files"])}))
        return
    frozen = json.loads(snapshot_path.read_text(encoding="utf-8"))
    if current != frozen:
        raise ValueError("build inputs changed since freeze; stage refused")
    artifacts_path = directory / "artifacts.json"
    if args.action == "stage":
        sources = [
            (REPO / "rust/target/release/ncx.exe", "ncx.exe"),
            (REPO / "rust/gui/src-tauri/target/x86_64-pc-windows-msvc/release/ncx-gui.exe", "BugleCat.exe"),
            (REPO / "rust/gui/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/BugleCat_0.1.0_x64-setup.exe", "BugleCat_0.1.0_x64-setup.exe"),
        ]
        for source, name in sources:
            if not source.is_file() or (directory / name).exists():
                raise ValueError(f"missing input or existing artifact: {name}")
        artifacts = {}
        for source, name in sources:
            target = directory / name
            shutil.copy2(source, target)
            artifacts[name] = {"sha256": digest(target), "bytes": target.stat().st_size,
                               "source": str(source), "source_mtime_utc": datetime.fromtimestamp(source.stat().st_mtime, timezone.utc).isoformat()}
        create_json(artifacts_path, artifacts)
        print(json.dumps(artifacts))
        return
    artifacts = json.loads(artifacts_path.read_text(encoding="utf-8"))
    for name, metadata in artifacts.items():
        if digest(directory / name) != metadata["sha256"]:
            raise ValueError(f"staged artifact changed: {name}")
    lifecycle = json.loads((directory / "evidence/lifecycle.json").read_text(encoding="utf-8"))
    smoke = json.loads((directory / "evidence/runtime-smoke.json").read_text(encoding="utf-8"))
    if lifecycle["exeSha256"] != artifacts["BugleCat.exe"]["sha256"] or lifecycle["paidCalls"] != 0:
        raise ValueError("lifecycle is not bound to staged GUI or made paid calls")
    if len(lifecycle["scenarios"]) != 3 or any(not r["cancelCompleted"] or not r["restoredWithoutPass"] or r["durableWriteCalls"] != 1 for r in lifecycle["scenarios"]):
        raise ValueError("lifecycle incomplete")
    if smoke["cliExit"] != 0 or smoke["newLiveJevCalls"] != 0 or not smoke["guiTranscriptRestored"]:
        raise ValueError("runtime smoke incomplete or made paid calls")
    if Path(smoke["cliExe"]).resolve() != directory / "ncx.exe" or Path(smoke["guiExe"]).resolve() != directory / "BugleCat.exe":
        raise ValueError("runtime smoke used different executables")
    session_directory = Path(smoke["root"]) / "workspace/.nanocodex/sessions"
    runtime_records = [json.loads(line) for name in smoke["logs"]
                       for line in (session_directory / name).read_text(encoding="utf-8").splitlines() if line.strip()]
    if not runtime_records or any(row["result"]["policy_version"] != "host-rules-v3.2"
                                  or row["result"].get("attempted_calls", 0) != 0 for row in runtime_records):
        raise ValueError("actual runtime policy differs or attempted Jev")
    create_json(directory / "evidence/runtime-verification-records.json", runtime_records)
    manifest = {"schema": "buglecat.jev.local-release.v3.2", "built_at": datetime.now(timezone.utc).isoformat(),
                "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
                "dirty_sources_included": True, "source_sha256": frozen["sha256"], "source_snapshot": "source-snapshot.json",
                "source_snapshot_sha256": digest(snapshot_path),
                "files": artifacts, "policy_version": "host-rules-v3.2", "package_version": "0.1.0",
                "default_mode": "off", "production_enforce_categories": [], "production_jev_threshold": None,
                "paid_build_smoke_calls": 0, "installer_built": True, "installer_executed": False,
                "runtime_policy_versions": sorted({row["result"]["policy_version"] for row in runtime_records}),
                "effect_certification": "not certified; formal independent calibration and holdout pending data and budget",
                "staging_script_sha256": digest(Path(__file__)),
                "test_results": {"core_provider": test_results(directory / "evidence/core-provider-tests.log"),
                                 "gui": test_results(directory / "evidence/gui-tests.log")},
                "evidence": [{"path": p.relative_to(directory).as_posix(), "sha256": digest(p)}
                             for p in sorted((directory / "evidence").rglob("*")) if p.is_file()]}
    create_json(directory / "manifest.json", manifest)
    with (directory / "SHA256SUMS.txt").open("x", encoding="ascii") as stream:
        for name, metadata in artifacts.items():
            stream.write(f"{metadata['sha256']}  {name}\n")
    print(json.dumps({"directory": str(directory), "source_sha256": frozen["sha256"], "files": artifacts}))


if __name__ == "__main__":
    main()
