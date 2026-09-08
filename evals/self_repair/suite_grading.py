"""Run frozen regression checks independently of the repairing model."""
import re
import sys
from pathlib import Path

from .suite_support import grading_env, owned_command, sanitized


def invalidate_workspace_artifacts(workspace, cache, env):
    """Cargo freshness is mtime-based across source exports sharing one target dir.

    Remove only these packages' rebuildable dev artifacts. Registry dependencies,
    source files and the release executor remain intact. Call serially.
    """
    command = ["cargo", "clean", "--profile", "dev", "--offline", "--locked",
               "--manifest-path", str(workspace / "rust/Cargo.toml"), "--target-dir", str(cache)]
    for package in ("ncx-cli", "ncx-core", "ncx-provider", "ncx-tools"):
        command.extend(["-p", package])
    return owned_command(command, workspace, 60, env)


def classify(result: dict, rust: bool) -> dict:
    output = result["stdout"] + "\n" + result["stderr"]
    if rust:
        counts = re.findall(r"test result: .*? (\d+) passed; (\d+) failed;", output)
        passed = sum(int(p) for p, _ in counts)
        failed = sum(int(f) for _, f in counts)
        checks = re.findall(r"^test (\S+) \.\.\. (?:ok|FAILED)$", output, re.M)
    else:
        passed = len(re.findall(r"^PASS:", output, re.M))
        failed = len(re.findall(r"^FAIL:", output, re.M))
        checks = [line.split(": ", 1)[1] if line.startswith("PASS: ") else line.rsplit(": ", 1)[1]
                  for line in output.splitlines() if line.startswith(("PASS: ", "FAIL: "))]
    if result["timed_out"]:
        status = "grader_timeout"
    elif result["returncode"] == 0 and passed > 0 and failed == 0:
        status = "passed"
    elif failed > 0 and result["returncode"] != 0:
        status = "test_failed"
    elif result["returncode"] != 0 and re.search(
        r"(?m)^(?:error\[E\d+\]|error: (?:expected|unexpected|mismatched|this file contains|unterminated|unknown start)|"
        r"(?:SyntaxError|IndentationError|TabError):)", output
    ):
        status = "compile_failed"
    else:
        status = "grader_error"
    return {"status": status, "passed": passed, "failed": failed,
            "checks": sorted(checks),
            "elapsed_s": result["elapsed_s"], "returncode": result["returncode"],
            "output_tail": sanitized(output[-16000:])}


def enforce_checks(result, healthy):
    """Require the original checks, not merely any nonzero passing test count."""
    missing = set(healthy.get("checks", [])) - set(result.get("checks", []))
    expected_count = healthy.get("passed", 0) + healthy.get("failed", 0)
    if result["status"] == "passed" and (missing or result["passed"] < expected_count):
        result = dict(result, status="integrity_failed", missing_checks=sorted(missing))
    return result


def grade(case, workspace: Path, evaluator: Path, cache: Path, timeout=600) -> dict:
    env = grading_env()
    env["CARGO_TARGET_DIR"] = str(cache)
    if case.crate:
        cleanup = invalidate_workspace_artifacts(workspace, cache, env)
        if cleanup["returncode"] != 0 or cleanup["timed_out"]:
            return dict(classify(cleanup, True), phase="cache_invalidation")
        command = ["cargo", "test", "--offline", "--locked", "--manifest-path",
                   str(workspace / "rust/Cargo.toml"), "-p", case.crate,
                   case.test_filter, "--", "--test-threads=1"]
    else:
        command = [sys.executable, str(evaluator / "acceptance.py"), str(workspace)]
    result = classify(owned_command(command, workspace, timeout, env), bool(case.crate))
    if case.crate:
        result["cache_invalidation_s"] = cleanup["elapsed_s"]
        result["elapsed_s"] = round(result["elapsed_s"] + cleanup["elapsed_s"], 2)
    return result
