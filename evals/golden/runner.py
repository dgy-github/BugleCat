"""Golden regression runner for BugleCat (buglecat.golden.v1).

Design invariants (do not weaken):
  - Tiered: unit (offline deterministic) / cargo / agent. Tiers not selected
    are reported as skipped with a reason, never as passes.
  - SKIP is not PASS: a skipped critical case fails the gate even offline.
  - Missing evidence fails closed: empty executions cannot produce a pass.
  - Bench tasks must be validated (healthy reference passes + seeded defect
    fails + grader is not vacuous) before any agent-tier claim uses them.
  - Reports separate infrastructure errors from real failures.
  - Reference solutions are pure JSON data (reference/files.json), never
    dynamically executed.

Usage:
    python -m evals.golden.runner                       # unit tier, gate on
    python -m evals.golden.runner --tier unit,cargo
    python -m evals.golden.runner --case G-PATCH-001
    python -m evals.golden.runner --capability bench_task
"""

from __future__ import annotations

import argparse
import json
import py_compile
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
CASES_FILE = Path(__file__).resolve().parent / "golden_cases.json"
DEFAULT_REPORT_DIR = Path(__file__).resolve().parent / "reports"

PYTEST_TIMEOUT_S = 900
CARGO_TIMEOUT_S = 2400
BENCH_CHECK_TIMEOUT_S = 60
GENERATE_TIMEOUT_S = 180


# ── loading ──────────────────────────────────────────────────────────────────

def load_manifest() -> dict:
    return json.loads(CASES_FILE.read_text(encoding="utf-8"))


# ── generic subprocess helper ────────────────────────────────────────────────

def _run(cmd: list[str], *, cwd: Path, timeout_s: int) -> tuple[int, str]:
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_s,
        )
        return proc.returncode, (proc.stdout or "") + (proc.stderr or "")
    except subprocess.TimeoutExpired:
        return 124, f"timeout after {timeout_s}s"
    except FileNotFoundError as exc:
        return 127, f"executable not found: {exc}"


# ── bench task validator (healthy-pass + seeded-fail + non-vacuous) ──────────

def _run_check_in(ws: Path, check_src: Path) -> tuple[bool, str]:
    # Mirror bench/run.py: the grader is copied in as _check.py, then run.
    shutil.copy(check_src, ws / "_check.py")
    rc, output = _run([sys.executable, "_check.py"], cwd=ws, timeout_s=BENCH_CHECK_TIMEOUT_S)
    return rc == 0, output[-400:]


def _reference_files(task_dir: Path) -> dict[str, str] | None:
    """Load reference/files.json (pure data: {"files": {relpath: content}})."""
    payload = task_dir / "reference" / "files.json"
    if not payload.exists():
        return None
    data = json.loads(payload.read_text(encoding="utf-8"))
    files = data.get("files")
    if not isinstance(files, dict) or not files:
        return None
    return {str(k): str(v) for k, v in files.items()}


def _materialize(ws: Path, files: dict[str, str]) -> None:
    for rel, content in files.items():
        target = ws / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")


def validate_bench_tasks(task_filter: str | None = None) -> tuple[bool, list[dict]]:
    """Validate every bench task: grader non-vacuous, seeded defects fail,
    reference solutions pass. Returns (all_valid, per_task_records)."""
    bench_tasks = REPO / "bench" / "tasks"
    records: list[dict] = []
    all_valid = True
    for task_dir in sorted(p for p in bench_tasks.iterdir() if p.is_dir()):
        if task_filter and task_filter not in task_dir.name:
            continue
        record: dict = {"task": task_dir.name, "issues": [], "valid": True}
        check = task_dir / "check.py"
        prompt = task_dir / "prompt.txt"
        if not check.exists() or not prompt.exists() or not prompt.read_text(encoding="utf-8").strip():
            record["issues"].append("missing check.py or empty prompt.txt")
            record["valid"] = False
            records.append(record)
            all_valid = False
            continue
        try:
            py_compile.compile(str(check), doraise=True)
        except py_compile.PyCompileError as exc:
            record["issues"].append(f"check.py does not compile: {exc}")
            record["valid"] = False
            records.append(record)
            all_valid = False
            continue

        seed_files = [
            f for f in task_dir.iterdir()
            if f.name not in {"check.py", "prompt.txt", "reference"} and not f.name.startswith("__")
        ]
        with tempfile.TemporaryDirectory(prefix="golden_bench_") as tmp:
            ws = Path(tmp)
            # 1. Grader must be non-vacuous: on the pre-agent state it fails.
            for f in seed_files:
                (shutil.copytree if f.is_dir() else shutil.copy)(f, ws / f.name)
            pre_ok, _pre_tail = _run_check_in(ws, check)
            record["seeded_fails"] = not pre_ok
            if pre_ok:
                record["issues"].append("grader passes BEFORE any agent edit (vacuous or broken seed)")
                record["valid"] = False

            # 2. Reference solution (if provided) must pass.
            reference = _reference_files(task_dir)
            if reference is not None:
                for stale in ws.iterdir():
                    if stale.name != "_check.py" and stale.is_file():
                        stale.unlink()
                    elif stale.is_dir():
                        shutil.rmtree(stale)
                _materialize(ws, reference)
                ref_ok, ref_tail = _run_check_in(ws, check)
                record["reference_passes"] = ref_ok
                if not ref_ok:
                    record["issues"].append(f"reference solution fails check: {ref_tail}")
                    record["valid"] = False
            else:
                record["reference_passes"] = None  # create-type task without committed reference

        if not record["valid"]:
            all_valid = False
        records.append(record)
    return all_valid, records


# ── self-repair anchor validator ─────────────────────────────────────────────

def validate_self_repair_anchors() -> tuple[bool, list[dict]]:
    """Every mutation anchor must exist exactly once in its target file,
    mirroring cases.mutate()'s own ambiguity refusal."""
    sys.path.insert(0, str(REPO))
    try:
        from evals.self_repair.cases import CASES  # type: ignore
    except Exception as exc:  # pragma: no cover - import environment issue
        return False, [{"case": "<import>", "issues": [str(exc)]}]
    records: list[dict] = []
    all_valid = True
    for case in CASES:
        issues: list[str] = []
        target = REPO / case.target
        if not target.exists():
            issues.append(f"target missing: {case.target}")
        else:
            text = target.read_text(encoding="utf-8")
            for old, _new in case.mutations:
                count = text.count(old)
                if count != 1:
                    issues.append(f"anchor count {count} != 1 for {old[:48]!r}")
        records.append({"case": case.id, "issues": issues})
        if issues:
            all_valid = False
    return all_valid, records


# ── compaction fixture generator validator ───────────────────────────────────

def validate_harness_generate() -> tuple[bool, str]:
    out_dir = Path(tempfile.mkdtemp(prefix="golden_gen_"))
    try:
        rc, output = _run(
            [sys.executable, "-m", "evals.harness", "generate", "--count", "5", "--mutate", "--output", str(out_dir)],
            cwd=REPO,
            timeout_s=GENERATE_TIMEOUT_S,
        )
        manifest = out_dir / "manifest.json"
        if rc != 0:
            return False, f"generate exited {rc}: {output[-300:]}"
        if not manifest.exists():
            return False, "manifest.json not produced"
        payload = json.loads(manifest.read_text(encoding="utf-8"))
        if not payload:
            return False, "manifest is empty"
        return True, "ok"
    finally:
        shutil.rmtree(out_dir, ignore_errors=True)


# ── case execution ───────────────────────────────────────────────────────────

def _cargo_available() -> bool:
    rc, _out = _run(["cargo", "--version"], cwd=REPO, timeout_s=30)
    return rc == 0


def _ncx_binary() -> Path:
    return REPO / "rust" / "target" / "release" / "ncx.exe"


def _provider_configured() -> bool:
    return (Path.home() / ".nanocodex" / "providers.json").exists()


def execute_case(case: dict, selected_tiers: set[str]) -> dict:
    start = time.monotonic()
    outcome: dict = {"id": case["id"], "capability": case["capability"], "tier": case["tier"]}

    if case["tier"] not in selected_tiers:
        outcome.update(status="skipped", reason="tier_not_selected")
        return outcome

    kind = case["kind"]
    if kind == "pytest":
        rc, output = _run(
            [sys.executable, "-m", "pytest", case["target"], "-q", "--no-header"],
            cwd=REPO,
            timeout_s=PYTEST_TIMEOUT_S,
        )
        outcome.update(status="passed" if rc == 0 else "failed", exit_code=rc)
        if rc != 0:
            outcome["tail"] = output[-1500:]
    elif kind == "cargo":
        if not _cargo_available():
            outcome.update(status="skipped", reason="cargo_not_available")
            return outcome
        rc, output = _run(["cargo", *case["args"]], cwd=REPO / "rust", timeout_s=CARGO_TIMEOUT_S)
        outcome.update(status="passed" if rc == 0 else "failed", exit_code=rc)
        if rc != 0:
            outcome["tail"] = output[-1500:]
    elif kind == "agent":
        if not _ncx_binary().exists():
            outcome.update(status="skipped", reason="ncx_binary_missing")
            return outcome
        if not _provider_configured():
            outcome.update(status="skipped", reason="provider_not_configured")
            return outcome
        rc, output = _run(
            [sys.executable, "-m", "evals.harness", "benchmark", *case["args"]],
            cwd=REPO,
            timeout_s=14400,
        )
        outcome.update(status="passed" if rc == 0 else "failed", exit_code=rc)
        if rc != 0:
            outcome["tail"] = output[-1500:]
    elif kind == "builtin":
        validator = case["validator"]
        if validator == "bench_tasks":
            ok, records = validate_bench_tasks()
            outcome.update(status="passed" if ok else "failed")
            if not ok:
                outcome["detail"] = [r for r in records if not r.get("valid")]
        elif validator == "self_repair_anchors":
            ok, records = validate_self_repair_anchors()
            outcome.update(status="passed" if ok else "failed")
            if not ok:
                outcome["detail"] = [r for r in records if r["issues"]]
        elif validator == "harness_generate":
            ok, detail = validate_harness_generate()
            outcome.update(status="passed" if ok else "failed")
            if not ok:
                outcome["detail"] = detail
        else:
            outcome.update(status="error", reason=f"unknown validator {validator}")
    else:
        outcome.update(status="error", reason=f"unknown kind {kind}")

    outcome["seconds"] = round(time.monotonic() - start, 1)
    return outcome


# ── gate & reporting ─────────────────────────────────────────────────────────

def evaluate_gate(results: list[dict]) -> dict:
    executed = [r for r in results if r["status"] in {"passed", "failed", "error"}]
    failed = [r for r in results if r["status"] in {"failed", "error"}]
    skipped = [r for r in results if r["status"] == "skipped"]
    skipped_critical = [r for r in skipped if r.get("priority") == "critical"]
    passed_n = len([r for r in executed if r["status"] == "passed"])
    gate_ok = bool(executed) and not failed and not skipped_critical
    return {
        "executed": len(executed),
        "passed": passed_n,
        "failed": len(failed),
        "skipped": len(skipped),
        "skipped_critical": len(skipped_critical),
        "pass_rate": round(passed_n / len(executed), 4) if executed else None,
        "gate_passed": gate_ok,
    }


def coverage_matrix(results: list[dict]) -> dict:
    matrix: dict[str, dict[str, int]] = {}
    for r in results:
        slot = matrix.setdefault(r["capability"], {"passed": 0, "failed": 0, "skipped": 0})
        slot[r["status"] if r["status"] in {"passed", "failed"} else "skipped"] += 1
    return {"matrix": matrix, "covered_capabilities": sorted(matrix)}


def write_report(results: list[dict], gate: dict, coverage: dict, report_dir: Path, selected_tiers: set[str]) -> tuple[Path, Path]:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    report_dir.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema": "buglecat.golden.report.v1",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "selected_tiers": sorted(selected_tiers),
        "gate": gate,
        "coverage": coverage,
        "results": results,
    }
    json_path = report_dir / f"golden_{stamp}.json"
    json_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")

    lines = [
        "# Golden regression report",
        "",
        f"- generated: {payload['generated_at']}",
        f"- tiers: {', '.join(sorted(selected_tiers))}",
        f"- gate: {'PASS' if gate['gate_passed'] else 'FAIL'}"
        f" (executed={gate['executed']}, passed={gate['passed']}, failed={gate['failed']},"
        f" skipped={gate['skipped']}, skipped_critical={gate['skipped_critical']})",
        "",
        "| case | capability | tier | status | note |",
        "|---|---|---|---|---|",
    ]
    for r in results:
        note = str(r.get("reason") or r.get("tail", ""))[:60].replace("\n", " ").replace("|", "\\|")
        lines.append(f"| {r['id']} | {r['capability']} | {r['tier']} | {r['status']} | {note} |")
    lines += ["", "## Coverage", ""]
    for cap, slot in sorted(coverage["matrix"].items()):
        lines.append(f"- {cap}: passed={slot['passed']} failed={slot['failed']} skipped={slot['skipped']}")
    md_path = report_dir / f"golden_{stamp}.md"
    md_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, md_path


# ── CLI ──────────────────────────────────────────────────────────────────────

def _attach_priority(results: list[dict], manifest: dict) -> list[dict]:
    by_id = {c["id"]: c for c in manifest["cases"]}
    for r in results:
        r["priority"] = by_id.get(r["id"], {}).get("priority", "unknown")
    return results


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="BugleCat golden regression gate")
    parser.add_argument("--tier", default="unit", help="comma list: unit,cargo,agent")
    parser.add_argument("--capability", default=None, help="filter by capability")
    parser.add_argument("--case", default=None, help="filter by case id substring")
    parser.add_argument("--output", default=None, help="report directory override")
    args = parser.parse_args(argv)

    manifest = load_manifest()
    selected_tiers = {t.strip() for t in args.tier.split(",") if t.strip()}
    unknown = selected_tiers - set(manifest["tiers"])
    if unknown:
        print(f"unknown tiers: {sorted(unknown)}", file=sys.stderr)
        return 2

    cases = manifest["cases"]
    if args.capability:
        cases = [c for c in cases if c["capability"] == args.capability]
    if args.case:
        cases = [c for c in cases if args.case in c["id"]]
    if not cases:
        print("no cases matched the filters", file=sys.stderr)
        return 2

    results = []
    for case in cases:
        result = execute_case(case, selected_tiers)
        print(f"[{result['status']:>7}] {result['id']} ({result['capability']}, {result['tier']})"
              + (f" - {result.get('reason', '')}" if result["status"] == "skipped" else ""))
        results.append(result)

    results = _attach_priority(results, manifest)
    gate = evaluate_gate(results)
    coverage = coverage_matrix(results)
    report_dir = Path(args.output) if args.output else DEFAULT_REPORT_DIR
    json_path, md_path = write_report(results, gate, coverage, report_dir, selected_tiers)

    print()
    print(f"gate: {'PASS' if gate['gate_passed'] else 'FAIL'} | executed={gate['executed']} passed={gate['passed']}"
          f" failed={gate['failed']} skipped={gate['skipped']} skipped_critical={gate['skipped_critical']}"
          f" pass_rate={gate['pass_rate']}")
    print(f"reports: {json_path.name} / {md_path.name} (in {report_dir})")
    return 0 if gate["gate_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
