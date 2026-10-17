"""Command-line evidence harness.

Examples:
  python -m evals.harness benchmark --tasks t1,t2 --repeats 1
  python -m evals.harness trace trace.jsonl
  python -m evals.harness compaction before.json after.json
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .metrics import compare_states, evaluate_trace


def _load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def _load_jsonl(path: Path) -> list[dict[str, Any]]:
    events = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SystemExit(f"invalid JSONL at {path}:{line_no}: {exc}") from exc
        if not isinstance(value, dict):
            raise SystemExit(f"trace event at {path}:{line_no} is not an object")
        events.append(value)
    return events


def _session_events(path: Path) -> list[dict[str, Any]]:
    """Project the persisted chat log into the evaluator's evidence schema."""
    events: list[dict[str, Any]] = []
    for message in _load_jsonl(path):
        role = message.get("role")
        if role == "assistant":
            for call in message.get("tool_calls") or []:
                fn = call.get("function") or {}
                args = fn.get("arguments", "{}")
                events.append({"kind": "tool_call", "name": fn.get("name", ""),
                               "arguments": args, "args_valid": isinstance(args, (str, dict))})
            if message.get("content"):
                events.append({"kind": "step", "role": role})
        elif role == "tool":
            content = message.get("content", "")
            events.append({"kind": "tool_result", "success": not str(content).startswith("[error]"),
                           "tool_call_id": message.get("tool_call_id")})
    return events


def _generate_compaction(args: argparse.Namespace) -> dict[str, Any]:
    root = args.output
    root.mkdir(parents=True, exist_ok=True)
    manifest = []
    for index in range(max(1, args.count)):
        before = {
            "goal": f"complete benchmark task {index + 1}",
            "constraints": ["run tests", "preserve public API"],
            "pending_tasks": [f"task-{index + 1}-verify"],
            "completed_tasks": [f"task-{index + 1}-implement"],
            "artifacts": {f"artifact-{index + 1}": "present"},
        }
        after = json.loads(json.dumps(before))
        if args.mutate and index % 2:
            after["constraints"] = []
            after["completed_tasks"].append(after["completed_tasks"][0])
        before_path = root / f"case_{index + 1:03d}_before.json"
        after_path = root / f"case_{index + 1:03d}_after.json"
        before_path.write_text(json.dumps(before, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        after_path.write_text(json.dumps(after, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        manifest.append({"id": f"case-{index + 1}", "before": before_path.name, "after": after_path.name,
                         "expected": compare_states(before, after)})
    manifest_path = root / "manifest.json"
    manifest_path.write_text(json.dumps({"schema": "buglecat.eval.v1", "cases": manifest},
                                        ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return {"generated": {"directory": str(root), "cases": len(manifest), "manifest": str(manifest_path)}}


def _write_report(data: dict[str, Any], output: Path | None) -> None:
    data = {"schema": "buglecat.eval.v1", "generated_at": datetime.now(timezone.utc).isoformat(), **data}
    text = json.dumps(data, ensure_ascii=False, indent=2) + "\n"
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(text, encoding="utf-8")
        print(f"report written: {output}")
    else:
        print(text, end="")


def _benchmark(args: argparse.Namespace) -> dict[str, Any]:
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "bench"))
    import run as bench  # type: ignore[import-not-found]

    selected = bench.tasks(args.tasks)
    rows: list[dict[str, Any]] = []
    for task in selected:
        passes = clean = timeouts = 0
        durations: list[float] = []
        for _ in range(max(1, args.repeats)):
            evidence = bench.run_once_detailed(args.agent, task, args.timeout)
            ok, elapsed = evidence["passed"], evidence["elapsed_s"]
            timed_out = bool(evidence.get("timed_out", False))
            passes += int(ok)
            clean += int(ok and not timed_out)
            timeouts += int(timed_out)
            durations.append(elapsed)
        rows.append({"task": task.name, "passes": passes, "clean_passes": clean,
                     "timeouts": timeouts, "runs": len(durations),
                     "mean_seconds": round(sum(durations) / len(durations), 3),
                     "last_failure_kind": evidence.get("failure_kind") if not ok else None,
                     "last_timed_out": evidence.get("timed_out", False)})
    return {"benchmark": summarize_benchmark(args.agent, rows)}


def summarize_benchmark(agent: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    total_runs = sum(r["runs"] for r in rows)
    total_passes = sum(r["passes"] for r in rows)
    total_clean = sum(r.get("clean_passes", 0) for r in rows)
    total_autonomous = sum(
        r.get("autonomous_passes", r.get("clean_passes", 0)) for r in rows
    )

    def rate(n: int) -> float | None:
        return round(n / total_runs, 4) if total_runs else None

    return {"agent": agent, "tasks": rows,
            "task_success_rate": rate(total_passes),
            # A run killed at the timeout may still leave a passing file behind;
            # it did not finish. clean = passed AND exited on its own.
            "clean_pass_rate": rate(total_clean),
            "offline_acr": rate(total_autonomous),
            "runs": total_runs, "passes": total_passes, "clean_passes": total_clean,
            "timeouts": sum(r.get("timeouts", 0) for r in rows)}


GATE_METRICS = {
    "acr": "offline_acr",
    "clean": "clean_pass_rate",
    "pass": "task_success_rate",
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Evidence-based BugleCat evaluation harness")
    sub = parser.add_subparsers(dest="command", required=True)

    trace = sub.add_parser("trace", help="calculate metrics from a JSONL event trace")
    trace.add_argument("path", type=Path)
    trace.add_argument("--output", type=Path)
    session = sub.add_parser("session", help="score a persisted .nanocodex/session.jsonl")
    session.add_argument("path", type=Path)
    session.add_argument("--output", type=Path)

    compact = sub.add_parser("compaction", help="compare before/after state snapshots")
    compact.add_argument("before", type=Path)
    compact.add_argument("after", type=Path)
    compact.add_argument("--output", type=Path)

    generate = sub.add_parser("generate", help="generate deterministic compaction benchmark fixtures")
    generate.add_argument("--output", type=Path, default=Path("evals/generated"))
    generate.add_argument("--count", type=int, default=50)
    generate.add_argument("--mutate", action="store_true",
                          help="include deliberate losses/duplicates in every other case")

    bench = sub.add_parser("benchmark", help="replay hidden-check coding tasks")
    bench.add_argument("--agent", default="nanocodex")
    bench.add_argument("--tasks", default="")
    bench.add_argument("--repeats", type=int, default=1)
    bench.add_argument("--timeout", type=int, default=240)
    bench.add_argument("--output", type=Path)
    bench.add_argument("--min-pass-rate", type=float, default=1.0,
                       help="exit non-zero when the gate metric is below this (0..1); "
                            "an empty run always fails")
    bench.add_argument("--gate-metric", choices=sorted(GATE_METRICS), default="clean",
                       help="clean = passed and not timed out (default); pass = grader passed")

    args = parser.parse_args(argv)
    if args.command == "trace":
        _write_report({"trace": evaluate_trace(_load_jsonl(args.path))}, args.output)
    elif args.command == "session":
        _write_report({"trace": evaluate_trace(_session_events(args.path))}, args.output)
    elif args.command == "compaction":
        _write_report({"compaction": compare_states(_load_json(args.before), _load_json(args.after))}, args.output)
    elif args.command == "generate":
        _write_report(_generate_compaction(args), None)
    else:
        if not 0.0 <= args.min_pass_rate <= 1.0:
            parser.error("--min-pass-rate must be within [0, 1]")
        report = _benchmark(args)
        gate = benchmark_gate(report["benchmark"], args.min_pass_rate, GATE_METRICS[args.gate_metric])
        report["benchmark"]["gate"] = gate
        _write_report(report, args.output)
        if not gate["passed"]:
            print(f"benchmark gate FAILED: {gate['reason']}", file=sys.stderr)
            return 1
    return 0


def benchmark_gate(summary: dict[str, Any], min_pass_rate: float,
                   metric: str = "task_success_rate") -> dict[str, Any]:
    """Fail closed: no executed runs (or a missing metric) never passes."""
    rate = summary.get(metric)
    base = {"metric": metric, "min_pass_rate": min_pass_rate}
    if not summary.get("runs") or rate is None:
        return {"passed": False, **base, "reason": f"no benchmark runs executed or {metric} missing"}
    if rate < min_pass_rate:
        return {"passed": False, **base, "reason": f"{metric} {rate} < min_pass_rate {min_pass_rate}"}
    return {"passed": True, **base, "reason": "ok"}


if __name__ == "__main__":
    raise SystemExit(main())
