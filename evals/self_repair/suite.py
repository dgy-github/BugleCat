"""One-shot real-source self-repair suite; no scheduler or automatic source merge.

python -m evals.self_repair.suite --work-root D:/BugleCat-selftest-UNIQUE
Add --validate-only for offline healthy/seed checks without model calls.
"""
import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

from .cases import CASES, mutate
from .suite_grading import enforce_checks, grade, invalidate_workspace_artifacts
from .suite_support import (
    audit,
    digest,
    export_workspace,
    fingerprint,
    git,
    grading_env,
    initialize_seed,
    owned_command,
    safe_child,
    sanitize_value,
    sanitized,
)
from .toolchain import install_case_wrapper, minimal_environment

ROOT = Path(__file__).resolve().parents[2]
MODEL = "deepseek-v4-pro"


def configured_environment():
    routes = json.loads((Path.home() / ".nanocodex/providers.json").read_text(encoding="utf-8-sig"))
    route = next(r for r in routes if r["id"] == "preset:deepseek")
    if MODEL not in route["models"]:
        raise ValueError("DeepSeek Pro is not in the configured route")
    env = dict(os.environ)
    env.update(NANOCODEX_API_KEY=route["api_key"], DEEPSEEK_API_KEY=route["api_key"],
               NANOCODEX_BASE_URL=route["base_url"], DEEPSEEK_BASE_URL=route["base_url"],
               NANOCODEX_PROVIDER_PROTOCOL=route["protocol"], NANOCODEX_MODEL=MODEL,
               NANOCODEX_TURN_TIMEOUT_S="600")
    return env, route["api_key"]


def select_cases(value):
    names = value.split(",") if value else [c.id for c in CASES]
    if len(set(names)) != len(names) or set(names) - {c.id for c in CASES}:
        raise ValueError("Use unique exact case IDs")
    return [c for c in CASES if c.id in names]


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def prompt_for(case):
    tests = "python self_repair_check.py"
    return (f"BugleCat self-repair task {case.id}: repair a regression in your real source. "
            f"Only change {case.target}. Contract: {case.contract}\n"
            "Existing tests, self_repair_check.py and build/config files are frozen: do not edit, "
            "remove or disable them. self_repair_check.py restores the preflight-verified Windows "
            "build environment and runs the relevant tests. Use that exact entrypoint instead of "
            "raw cargo, toolchain discovery or installation. "
            "You may add offline tests under tests/test_self_repair_*.py or the owning crate's "
            "tests/self_repair_*.rs. Do not change git history, commit or push. Do not access "
            "other workspaces, reference solutions, credentials or the network. Do not rebuild "
            "the CLI. Use the local files and test failures to find the repair. "
            f"Run {tests}. After editing Python run py_compile immediately; resolve errors before "
            "finishing. Finish immediately after verification succeeds. Total turn budget 600s, "
            "30 model calls, 80 tool calls. Avoid broad exploration.")


def run_case(case, workspace, baseline, seed_head, binary, cache, env, credential, public, healthy):
    attempts = []
    prompt = prompt_for(case)
    for number in (1, 2):
        try:
            result = owned_command([str(binary), "-w", str(workspace), "-s", "workspace-write",
                                    "--max-iterations", "30", "--max-tool-calls", "80", prompt],
                                   workspace, 630, env)
        except OSError as error:
            attempts.append(dict(attempt=number, elapsed_s=0, returncode=None, timed_out=False,
                                 grader={"status": "infrastructure_error", "passed": 0,
                                         "failed": 0, "elapsed_s": 0}, patch="",
                                 integrity_issues=[type(error).__name__]))
            write_json(public / f"{case.id}_attempts.json", attempts)
            break
        record = dict(attempt=number, elapsed_s=result["elapsed_s"],
                      returncode=result["returncode"], timed_out=result["timed_out"],
                      grader={"status": "pending", "passed": 0, "failed": 0, "elapsed_s": 0})
        attempts.append(record)
        write_json(public / f"{case.id}_attempts.json", attempts)
        try:
            issues = audit(workspace, baseline, case.target)
            if git(workspace, "rev-parse", "HEAD").strip() != seed_head:
                issues.append("seed git history changed")
            if git(workspace, "remote").strip():
                issues.append("unexpected git remote")
            # Never run modified checkers/build configuration after a scope violation.
            checked = (enforce_checks(grade(case, workspace, Path(__file__).parent, cache), healthy)
                       if not issues else
                       {"status": "integrity_failed", "passed": 0, "failed": 0, "elapsed_s": 0})
            patch = git(workspace, "diff", "HEAD", "--", case.target)
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            issues = [f"validation unavailable: {type(error).__name__}"]
            checked = {"status": "infrastructure_error", "passed": 0, "failed": 0, "elapsed_s": 0}
            patch = ""
        diagnostics = []
        for line in result["stderr"].splitlines():
            if line.startswith("[ncx-provider] "):
                try:
                    raw = json.loads(line.removeprefix("[ncx-provider] "))
                    diagnostics.append({k: raw[k] for k in
                                        ("model", "status", "kind", "http_status", "elapsed_ms", "attempts",
                                         "retries", "error_kind", "timeout_kind") if k in raw})
                except ValueError:
                    pass
        patch_name = f"{case.id}_attempt{number}.patch"
        (public / patch_name).write_text(sanitized(patch, (credential,)), encoding="utf-8")
        record.update(grader=checked, integrity_issues=issues, patch=patch_name,
                      provider_diagnostics=diagnostics)
        # Apply credential redaction even to allowlisted diagnostics and grader errors.
        record = sanitize_value(record, (credential,))
        attempts[-1] = record
        write_json(public / f"{case.id}_attempts.json", attempts)
        print(f"{case.id} attempt {number}: {checked['status']} ({result['elapsed_s']}s)", flush=True)
        if checked["status"] == "passed" or issues:
            break
        prompt = (prompt_for(case) + "\nOne final feedback turn. Independent validation failed:\n"
                  + checked.get("output_tail", checked["status"])[-10000:])
    return attempts


def execute_case(case, work, cache, env, credential, public, binary):
    item = dict(id=case.id, title=case.title, target=case.target, attempts=[],
                status="invalid_environment", healthy={"status": "not_run"})
    try:
        print(f"{case.id}: checking healthy source", flush=True)
        healthy = grade(case, ROOT, Path(__file__).parent, cache)
        item["healthy"] = healthy
        if healthy["status"] != "passed":
            item["status"] = "invalid_healthy_baseline"
            return item
        ws = safe_child(work, case.id)
        export_workspace(ROOT, ws)
        install_case_wrapper(case, ws, cache)
        if case.crate:
            cleanup = invalidate_workspace_artifacts(ws, cache, grading_env())
            if cleanup["returncode"] != 0 or cleanup["timed_out"]:
                item["status"] = "invalid_agent_environment"
                return item
        preflight = owned_command([sys.executable, str(ws / "self_repair_check.py")], ws, 600,
                                  minimal_environment())
        item["agent_shell_preflight"] = {
            "passed": preflight["returncode"] == 0 and not preflight["timed_out"],
            "elapsed_s": preflight["elapsed_s"], "returncode": preflight["returncode"]}
        if not item["agent_shell_preflight"]["passed"]:
            item["status"] = "invalid_agent_environment"
            return item
        target = ws / case.target
        item["healthy_target_sha256"] = digest(target.read_bytes())
        target.write_text(mutate(case, target.read_text(encoding="utf-8")), encoding="utf-8")
        seed_head = initialize_seed(ws)
        baseline = fingerprint(ws)
        write_json(work / f"{case.id}_baseline.json", baseline)
        write_json(public / f"{case.id}_baseline.json", baseline)
        item["seed_head"] = seed_head
        item["baseline_sha256"] = digest(json.dumps(baseline, sort_keys=True).encode())
        item["seed"] = grade(case, ws, Path(__file__).parent, cache)
        item["status"] = "validated" if item["seed"]["status"] == "test_failed" else "invalid_seed"
        print(f"{case.id}: healthy={healthy['status']} seed={item['seed']['status']}", flush=True)
        write_json(public / f"{case.id}_case.json", item)
        if item["status"] == "validated" and credential:
            item["attempts"] = run_case(case, ws, baseline, seed_head, binary, cache,
                                        env, credential, public, healthy)
            item["status"] = item["attempts"][-1]["grader"]["status"]
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        item["status"] = "invalid_environment"
        item["error_type"] = type(error).__name__
        attempt_path = public / f"{case.id}_attempts.json"
        if attempt_path.exists():
            item["attempts"] = json.loads(attempt_path.read_text(encoding="utf-8"))
    finally:
        write_json(public / f"{case.id}_case.json", item)
    return item


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--cases", default="")
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    cases = select_cases(args.cases)
    work = args.work_root.resolve()
    if work.exists() or work.is_relative_to(ROOT) or ROOT.is_relative_to(work):
        raise ValueError("Use a new dedicated directory outside the source repository")
    binary = ROOT / "rust/target/release/ncx.exe"
    env, credential = ({}, "") if args.validate_only else configured_environment()
    cache = ROOT / "rust/target"  # serial builds only; reuse local dependencies
    env["CARGO_TARGET_DIR"] = str(cache)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    public = Path(__file__).parent / "results" / stamp
    public.mkdir(parents=True, exist_ok=False)
    work.mkdir(parents=True)
    state = dict(schema_version=1, run_id=stamp, model=MODEL, validate_only=args.validate_only,
                 source_head=git(ROOT, "rev-parse", "HEAD").strip(),
                 binary_sha256=digest(binary.read_bytes()), cases=[])
    write_json(public / "summary.json", state)
    for case in cases:
        state["cases"].append(execute_case(case, work, cache, env, credential, public, binary))
        write_json(public / "summary.json", state)
    print(f"Evidence: {public / 'summary.json'}", flush=True)
    return int(any(c["status"] not in ("passed", "validated") for c in state["cases"]))


if __name__ == "__main__":
    raise SystemExit(main())
