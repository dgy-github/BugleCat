"""Zero-network diagnostics of original artifact calibration, never certification.

The proof owner reconstructs original source/transport/statistics first. Public
output is aggregate only. Optional private detail contains identities and check
names, never candidate code, model reasoning, keys or original response bodies.
"""

import argparse
import ast
import json
import statistics
import warnings
from collections import Counter, defaultdict
from pathlib import Path

from .artifact_acceptance import QUESTION, request
from .artifact_formal import revalidate_calibration
from .calibration import digest, load, require
from .collection_manifest import file_hash
from .paid_budget import durable_new
from .source_bindings import source_transaction

SCHEMA = "buglecat.verification.artifact-diagnostics.v1"
STATE_FIELDS = {"schema", "statement_origin", "scope", "task_prompt", "public_interface",
                "candidate_code", "acceptance_assertions", "test_imports"}


def distribution(values):
    return {"n": len(values), "min": min(values) if values else None, "max": max(values) if values else None,
            "mean": statistics.mean(values) if values else None,
            "median": statistics.median(values) if values else None,
            "quartiles": statistics.quantiles(values, n=4, method="inclusive") if len(values) > 1 else []}


def syntax_metadata(code):
    try:
        tree = ast.parse(code)
    except SyntaxError:
        return {"python_ast_parse": "syntax_error", "ast_node_count": None}
    return {"python_ast_parse": "parsed", "ast_node_count": len(list(ast.walk(tree)))}


def input_checks(row, observation, tasks):
    """Compare source fields and exact frozen raw submission without execution."""
    body = row["request"]
    state = body.get("state", {})
    issues = []
    def check(name, passed):
        if not passed:
            issues.append(name)
    check("state_fields", set(state) == STATE_FIELDS)
    check("question_contract", body.get("questions") == {
        "acceptance_violation": {"type": "noul", "instructions": QUESTION}})
    check("requested_model", body.get("model") == "jev-latest")
    check("state_schema_scope_origin", state.get("schema") == "buglecat.jev.artifact-acceptance.v1"
          and state.get("scope") == "frozen_acceptance_assertions_only"
          and state.get("statement_origin") == "host_artifact_hypothesis")
    for name in ("candidate_code", "task_prompt", "public_interface", "acceptance_assertions"):
        check(f"nonempty_{name}", bool(state.get(name)))
    check("imports_present", isinstance(state.get("test_imports"), list))
    manifest = load(row["collection_manifest_file"])
    source_case = next(case for case in manifest["cases"] if case["id"] == row["id"])
    task = tasks[row["task_id"]]
    check("source_body_exact", body == request(task, source_case["public_interface"], state.get("candidate_code")))
    check("request_hash", digest(body) == row["request_hash"])
    canonical_size = len(json.dumps(body, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode())
    check("input_within_48KB", canonical_size <= 48_000)
    request_file = Path(observation["original_events_file"]).with_name("request.json")
    if observation["call_status"] == "ok":
        check("actual_model", observation.get("model") == "jev-1.13.0")
        check("raw_request_file", request_file.exists())
        if request_file.exists():
            check("raw_request_body", load(request_file) == body)
            check("actual_submitted_hash", file_hash(request_file) == row["request_hash"] == observation.get("actual_request_hash"))
    else:
        check("failure_probability_absent", observation.get("acceptance_violation") is None)
        # Cancellation before submission legitimately has no raw request/submit.
        # The original proof owner, not these checks, authenticates that path.
    return issues, canonical_size


@source_transaction
def diagnose(calibration_file):
    """Require genuine proof; return (aggregate, private metadata) without writes."""
    original_hash = file_hash(calibration_file)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", SyntaxWarning)
        value, registration, data, rows, observations = revalidate_calibration(calibration_file)
        cases, observed = {case["id"]: case for case in data["cases"]}, {row["id"]: row for row in observations}
        require(len(cases) == len(data["cases"]) and len(observed) == len(observations)
                and set(observed) == {row["id"] for row in rows}, "complete unique original denominator required")
        sizes, token_counts, defects, details = [], [], [], []
        taxonomy, unknown, scores = Counter(), Counter(), defaultdict(list)
        source_tasks = {}
        for row in rows:
            source = row["source_file"]
            if source not in source_tasks:
                source_tasks[source] = {task["task_id"]: task for task in load(source)}
            case, observation = cases[row["id"]], observed[row["id"]]
            checks, size = input_checks(row, observation, source_tasks[source])
            sizes.append(size)
            token_counts.append(observation.get("usage", {}).get("input_tokens") if observation.get("usage") else None)
            defects.extend({"id": row["id"], "check": name} for name in checks)
            result = case["oracle"]["result"]
            error = result.get("error_kind", "passed" if result["status"] == "passed" else "unspecified_failed_kind")
            taxonomy[f'{case["label"]}:{error}'] += 1
            if observation["call_status"] == "ok":
                scores[f'{case["label"]}:{error}'].append(observation["acceptance_violation"])
            details.append({"id": row["id"], "group_id": row["group_id"], "task_id": row["task_id"],
                            "label": case["label"], "oracle_error_kind": error,
                            "tests_passed_before_failure": result.get("tests_passed_before_failure"),
                            "call_status": observation["call_status"], "input_checks_failed": checks,
                            "request_hash": row["request_hash"], **syntax_metadata(row["request"]["state"].get("candidate_code", ""))})
        for case in data["cases"]:
            if case["label"] == "unknown":
                reason = case.get("reason") or case.get("oracle", {}).get("result", {}).get("reason", "unspecified_unknown")
                unknown[reason] += 1
    cohort = load(registration["artifact_cohort_file"])
    grid = [{key: trial[key] for key in ("threshold", "supported_groups", "unsupported_groups", "false_rejected_supported",
                "detected_unsupported", "false_rejection_upper_95", "error_rejection_lower_95", "gate_met", "gate_reasons")}
            | {"failed_attempts": len(trial["failed_attempts"]), "failures_retained_in_denominators": trial["failed_attempts_retained_in_denominators"]}
            for trial in value["grid_results"]]
    require(len(grid) == 9 and file_hash(calibration_file) == original_hash, "original calibration/grid changed")
    bindings = {"calibration": {"file": str(Path(calibration_file).resolve()), "sha256": original_hash, "hash": value["artifact_hash"]},
                "registration": {"file": value["registration_file"], "sha256": value["registration_file_sha256"], "hash": value["registration_hash"]},
                "observations": {"file": value["observations_file"], "sha256": value["observations_file_sha256"], "hash": value["observations_hash"]},
                "runner_bindings": value["runner_bindings"], "source_protocol_authorization": cohort.get("bindings", []),
                "cohort": {"file": registration["artifact_cohort_file"],
                    "sha256": file_hash(registration["artifact_cohort_file"]), "hash": cohort["artifact_hash"]}}
    report = {"schema": SCHEMA, "original_proof_revalidated": True, "bindings": bindings,
              "allocated_groups": len(cohort["cases"]), "collected_groups": len(data["cases"]),
              "uncollected_groups": len(cohort["cases"]) - len(data["cases"]), "eligible_observations": len(rows),
              "label_counts": dict(Counter(case["label"] for case in data["cases"])), "unknown_reasons": dict(unknown),
              "failed_observations": sum(row["call_status"] != "ok" for row in observations), "error_taxonomy": dict(taxonomy),
              "error_taxonomy_by_threshold": {key: {"total_groups": count,
                    "successful_prediction_groups": len(scores[key]), "failed_prediction_groups": count - len(scores[key]),
                    "rejected_by_threshold": [{"threshold": trial["threshold"],
                        "reject_signals": sum(p >= trial["threshold"] for p in scores[key])} for trial in grid]}
                    for key, count in taxonomy.items()},
              "grid": grid, "input_integrity": {"defect_count": len(defects), "checks_passed": not defects,
                    "request_bytes": distribution(sizes), "jev_input_tokens": distribution([n for n in token_counts if n is not None]),
                    "missing_usage_count": sum(n is None for n in token_counts)},
              "score_distributions": {key: distribution(values) for key, values in scores.items()},
              "findings": {"confirmed": "Original proof and explicit input checks ran; only checks_passed establishes absence of these input defects.",
                           "hypotheses": ["Literal assertion reasoning, execution semantics or model discrimination may contribute; scores alone cannot establish cause.",
                                          "Assertion-specific prompts may help; max aggregation may also increase false rejections. Requires new independent evaluation."]},
              "already_viewed_calibration_only": True, "new_blind_test": False, "certified_threshold": None,
              "production_authorized": False, "native_claim_certification": False, "new_paid_calls": 0}
    private = {"schema": SCHEMA + ".private", "aggregate_hash": digest(report), "input_defects": defects,
               "case_metadata_only": details, "candidate_code_or_reasoning_included": False}
    return {**report, "artifact_hash": digest(report)}, {**private, "artifact_hash": digest(private)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--calibration", type=Path, required=True)
    parser.add_argument("--private-output", type=Path)
    args = parser.parse_args()
    try:
        aggregate, private = diagnose(args.calibration)
        if args.private_output:
            require(".nanocodex" in args.private_output.resolve().parts, "private metadata must stay in ignored .nanocodex")
            args.private_output.parent.mkdir(parents=True, exist_ok=True)
            durable_new(args.private_output, private)
    except Exception as error:
        parser.exit(2, f"artifact_diagnostics_rejected:{type(error).__name__}; no paid action\n")
    print(json.dumps(aggregate, ensure_ascii=False, allow_nan=False))


if __name__ == "__main__":
    main()
