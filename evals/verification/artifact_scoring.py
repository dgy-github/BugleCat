"""Prediction-blind registration and development-only artifact signal scoring.

No native claim certification, production permission or formal holdout here.
Formal source intake must be implemented before this entry point is extended.
"""

import argparse
import json
from pathlib import Path

from .artifact_acceptance import POLICY, TEMPLATE
from .artifact_gate import metrics
from .calibration import create, digest, load, probability, require
from .collection_receipts import read_lines


def sealed(value, key="artifact_hash"):
    require(value.get(key) == digest({k: v for k, v in value.items() if k != key}), "artifact seal mismatch")
    return value


def register(data, rows, spec):
    sealed(data)
    require(data["schema"] == "buglecat.verification.artifact-dataset.v1", "artifact dataset required")
    require(data["scores_hidden"] is True and data["jev_probabilities_read"] is False, "registration follows probabilities")
    require(spec["schema"] == "buglecat.verification.artifact-protocol.v1"
            and spec["policy_version"] == POLICY and spec["template_version"] == TEMPLATE,
            "artifact protocol mismatch")
    require(spec["categories"] == ["artifact_acceptance"], "native claim policy not allowed")
    require(spec["min_supported"] >= 150 and spec["min_unsupported"] >= 50, "group gates cannot be lowered")
    grid = spec["threshold_grid"]
    require(grid == sorted(set(grid)) and grid and all(probability(t) and t >= .5 for t in grid), "invalid threshold grid")
    require(probability(spec["max_false_block_rate"]) and spec["max_false_block_rate"] <= .02,
            "invalid false rejection gate")
    require(probability(spec["min_error_rejection_lower_bound"]) and spec["min_error_rejection_lower_bound"] >= .5,
            "invalid error rejection gate")
    require(spec["confidence"] == .95, "frozen confidence required")
    cases = {c["id"]: c for c in data["cases"]}
    require(len(cases) == len(data["cases"]), "duplicate dataset ID")
    identities, groups = set(), set()
    for row in rows:
        sealed(row, "artifact_sha256")
        require(row["schema"] == "buglecat.verification.prepared-artifact.v1", "artifact prepared row required")
        require(row["split"] == "development" and row["category"] == "artifact_acceptance",
                "only development import currently supported")
        require(row["policy_version"] == POLICY and row["template_version"] == TEMPLATE, "prepared policy mismatch")
        require(row["request_hash"] == digest(row["request"]), "prepared request hash mismatch")
        require(row["id"] not in identities and row["group_id"] not in groups, "one request per independent group")
        identities.add(row["id"])
        groups.add(row["group_id"])
        case = cases[row["id"]]
        require(case["split"] == "development" and case["author_exposed"] is True, "viewed source is development")
        require(case["call_eligible"] is True and case["label"] in {"supported", "unsupported"}, "missing binary oracle fact")
        require(case["group_id"] == row["group_id"] and case["candidate_sha256"] == row["candidate_sha256"]
                and case["main_receipt_sha256"] == row["main_receipt_sha256"], "original source binding mismatch")
        oracle = sealed(case["oracle"])
        require(row["oracle_sha256"] == oracle["artifact_hash"], "independent oracle binding mismatch")
        require(case["label"] == {"passed": "supported", "failed": "unsupported"}.get(oracle["result"]["status"]),
                "label cannot override independent acceptance result")
    require(bool(rows), "empty artifact selection")
    body = {"schema": "buglecat.verification.artifact-registration.v1", "dataset_hash": digest(data),
            "prepared_hash": digest(rows), "protocol": spec, "case_ids": sorted(identities),
            "group_ids": sorted(groups), "development_only": True, "native_claim_certification": False}
    return {**body, "artifact_hash": digest(body)}


def score(data, rows, registration, observations):
    sealed(registration)
    require(registration["development_only"] is True, "formal entry point not implemented")
    spec = registration["protocol"]
    require(register(data, rows, spec) == registration, "registered source changed")
    cases = {c["id"]: c for c in data["cases"]}
    prepared = {r["id"]: r for r in rows}
    seen, votes, failures = set(), {}, {}
    for observed in observations:
        identity = observed["id"]
        require(identity in prepared and identity not in seen, "foreign or repeated observation")
        seen.add(identity)
        row = prepared[identity]
        require(observed["request_hash"] == row["request_hash"], "observation request changed")
        if observed["call_status"] != "ok":
            votes[identity] = None
            failures[identity] = observed["call_status"]
            continue
        require(observed.get("transport_binding_status") == "original_transport_bytes_verified",
                "successful observation needs independent original transport bytes")
        require(observed["actual_request_hash"] == row["request_hash"] and observed.get("response_sha256"),
                "actual transport binding mismatch")
        require(observed["model"] == spec["model"], "actual model version mismatch")
        value = observed["acceptance_violation"]
        require(probability(value), "invalid acceptance probability")
        votes[identity] = value
    require(seen == prepared.keys(), "unfinished/missing attempts cannot disappear")
    trials = []
    selected = [cases[i] for i in registration["case_ids"]]
    supported = [c for c in selected if c["label"] == "supported"]
    unsupported = [c for c in selected if c["label"] == "unsupported"]
    for threshold in spec["threshold_grid"]:
        reject = {i for i, p in votes.items() if p is not None and p >= threshold}
        false = sum(c["id"] in reject for c in supported)
        detected = sum(c["id"] in reject for c in unsupported)
        numeric = metrics(len(supported), len(unsupported), false, detected, threshold, spec)
        numeric.pop("gate_reasons")
        gate = numeric.pop("gate_met")
        trials.append({**numeric, "sample_numeric_gate_met": gate, "reject_signals": len(reject),
                       "unverified_signals": len(selected) - len(reject)})
    body = {"schema": "buglecat.verification.artifact-development-score.v1", "registration_hash": digest(registration),
            "observations_hash": digest(observations), "grid_results": trials, "failed_attempts": failures,
            "failed_attempts_retained_in_denominators": True, "certified_threshold": None,
            "development_only": True, "holdout_status": "not_run", "production_authorized": False,
            "native_claim_certification": False, "baseline_framework_A": "unavailable", "baseline_rules_B": "unavailable"}
    return {**body, "artifact_hash": digest(body)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["register", "score"])
    for name in ("dataset", "prepared", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--protocol", type=Path)
    parser.add_argument("--registration", type=Path)
    parser.add_argument("--observations", type=Path)
    args = parser.parse_args()
    data, rows = load(args.dataset), read_lines(args.prepared)
    if args.action == "register":
        require(args.protocol is not None, "protocol required")
        value = register(data, rows, load(args.protocol))
    else:
        require(args.registration is not None and args.observations is not None, "registration and observations required")
        value = score(data, rows, load(args.registration), read_lines(args.observations))
    create(args.output, value)
    print(json.dumps({"schema": value["schema"], "development_only": True, "paid_calls": 0}))


if __name__ == "__main__":
    main()
