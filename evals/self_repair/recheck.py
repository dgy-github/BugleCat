"""Final independent audit of an existing batch; never invokes the model."""
import argparse
import json
import shutil
import subprocess
from pathlib import Path

from .cases import CASES
from .suite import ROOT, write_json
from .suite_grading import enforce_checks, grade
from .suite_support import audit, digest, git, safe_child


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("summary", type=Path)
    parser.add_argument("work_root", type=Path)
    args = parser.parse_args()
    state = json.loads(args.summary.read_text(encoding="utf-8"))
    public = args.summary.parent
    result = {"run_id": state["run_id"], "checks": []}
    for item in state["cases"]:
        if not item.get("attempts"):
            result["checks"].append(dict(id=item["id"], skipped_reason="no_model_attempts",
                                          integrity_issues=[], replayed_attempts=[]))
            write_json(public / "independent_recheck.json", result)
            continue
        case = next(c for c in CASES if c.id == item["id"])
        workspace = args.work_root.resolve() / case.id
        baseline_path = args.work_root / f"{case.id}_baseline.json"
        baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
        issues = audit(workspace, baseline, case.target)
        if git(workspace, "rev-parse", "HEAD").strip() != item["seed_head"]:
            issues.append("seed HEAD changed")
        if digest(json.dumps(baseline, sort_keys=True).encode()) != item["baseline_sha256"]:
            issues.append("external baseline manifest changed")
        if git(workspace, "rev-list", "--count", "HEAD").strip() != "1":
            issues.append("candidate has unexpected history")
        if git(workspace, "remote").strip():
            issues.append("candidate has unexpected remote")
        healthy = grade(case, ROOT, Path(__file__).parent, ROOT / "rust/target")
        if healthy["status"] != "passed":
            issues.append("healthy baseline no longer passes")
        # Publish hashes, not workspace contents or private native sessions.
        shutil.copy2(baseline_path, public / baseline_path.name)
        replayed = []
        seed_checked = {}
        for attempt in item["attempts"]:
            replay = safe_child(args.work_root, f"recheck_{case.id}_{attempt['attempt']}")
            subprocess.run(["git", "clone", "--quiet", "--no-hardlinks", "--no-local",
                            str(workspace), str(replay)], check=True, capture_output=True)
            if not seed_checked:
                seed_checked = grade(case, replay, Path(__file__).parent, ROOT / "rust/target")
                if (seed_checked["status"] != "test_failed" or
                        set(healthy.get("checks", [])) - set(seed_checked.get("checks", []))):
                    issues.append("cold seed does not reproduce failure with all original tests")
                print(f"{case.id} cold seed: {seed_checked['status']}", flush=True)
            patch = public / attempt["patch"]
            if patch.stat().st_size:
                subprocess.run(["git", "-C", str(replay), "apply", str(patch.resolve())],
                               check=True, capture_output=True)
            replay_issues = audit(replay, baseline, case.target)
            # Git normalizes line endings. Compare baseline to seed blob after normalization
            # for unchanged paths before treating a checkout-only conversion as a mutation.
            if replay_issues:
                tracked = git(replay, "diff", "--name-only", "HEAD").splitlines()
                replay_issues = [f"unexpected tracked change: {p}" for p in tracked if p != case.target]
                original_tests = git(workspace, "show", f"HEAD:{case.target}").split("#[cfg(test)]", 1)
                current_tests = (replay / case.target).read_text(encoding="utf-8").split("#[cfg(test)]", 1)
                if len(original_tests) == 2 and (len(current_tests) != 2 or original_tests[1] != current_tests[1]):
                    replay_issues.append("protected inline tests changed")
            validation = (enforce_checks(grade(case, replay, Path(__file__).parent,
                                              ROOT / "rust/target"), healthy) if not replay_issues else
                          {"status": "integrity_failed"})
            replayed.append(dict(attempt=attempt["attempt"], integrity_issues=replay_issues,
                                 original_status=attempt["grader"]["status"], validation=validation,
                                 agrees=validation["status"] == attempt["grader"]["status"]))
            print(f"{case.id} replay {attempt['attempt']}: {validation['status']}", flush=True)
        checked = replayed[-1]["validation"] if replayed else {"status": "not_run"}
        if replayed and ((workspace / case.target).read_text(encoding="utf-8") !=
                         (replay / case.target).read_text(encoding="utf-8")):
            issues.append("saved final patch differs from actual final candidate")
        if issues:
            checked = {"status": "integrity_failed"}
        record = dict(id=case.id, integrity_issues=issues, healthy=healthy, final=checked,
                      seed=seed_checked,
                      replayed_attempts=replayed,
                      final_target_sha256=digest((workspace / case.target).read_bytes()))
        result["checks"].append(record)
        write_json(public / "independent_recheck.json", result)
        print(f"{case.id}: {checked['status']}, integrity issues={len(issues)}", flush=True)
    return int(any(c["integrity_issues"] or any(not a["agrees"] for a in c["replayed_attempts"])
                   for c in result["checks"]))


if __name__ == "__main__":
    raise SystemExit(main())
