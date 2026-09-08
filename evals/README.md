# Evidence-based evaluation Harness

[Six-category real-source self-repair suite](self_repair/README.md) ·
[Chinese self-test report](../docs/reports/self_repair_20260908.md)

This Harness produces scores from observable artifacts, never from an agent's
self-reported score. It has three offline entry points:

    python -m evals.harness benchmark --agent nanocodex --repeats 3 --output evals/reports/bench.json
    python -m evals.harness trace path\to\trace.jsonl --output evals/reports/trace.json
    python -m evals.harness session path\to\.nanocodex\session.jsonl
    python -m evals.harness compaction before.json after.json
    python -m evals.harness generate --count 50 --mutate --output evals/generated

For a one-click Windows run (build Agent, replay benchmark, generate 50
compaction fixtures), double-click scripts/run-eval.cmd, or run
scripts/run-eval.ps1. Use -OpenReport to open the JSON report automatically;
use -SkipBuild when the release binary is already current.

Trace events are JSON objects, one per line. Supported kinds include
tool_call, tool_result, verification, failure, recovery, task_complete,
model_call, step, and human_takeover. Tool results carry a boolean success;
verification and recovery events carry passed/success. Regression replay events
use kind regression with a boolean passed field.

Snapshots are JSON objects with explicit goal, constraints, pending_tasks,
completed_tasks, and artifacts fields. Missing fields fail closed. Reports use
schema buglecat.eval.v1.

The session command adapts a persisted chat log into the trace contract.
The generate command creates reproducible compaction cases and a manifest;
mutate adds known loss/duplicate cases to prove the evaluator catches regressions.

The benchmark path also records structured post-edit evidence through bench.run_once_detailed: expected/created/modified files, Python compile checks, timeout state, failure kind, grader note, and sanitized output tails. The default agent timeout is 240 seconds; post-edit checks use a separate 15-second limit.
