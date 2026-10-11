"""Frozen artifact classification gates, independent of native A/B metrics.

Pure statistics do not prove source provenance. Callers must verify registration
and original transport before using them to qualify a threshold for holdout.
"""
from .calibration import probability, require, upper_bound


def protocol(spec):
    require(spec["schema"] == "buglecat.verification.artifact-protocol.v1"
            and spec["policy_version"] == "artifact-acceptance-v1"
            and spec["template_version"] == "jev-artifact-acceptance-v1"
            and spec["categories"] == ["artifact_acceptance"], "artifact protocol required")
    require(spec["min_supported"] == 150 and spec["min_unsupported"] == 50
            and spec["max_false_block_rate"] == .02 and spec["confidence"] == .95
            and spec["min_error_rejection_lower_bound"] == .5, "frozen artifact gates changed")
    require(spec["threshold_grid"] == [.5, .6, .7, .8, .9, .95, .975, .99, 1.0],
            "frozen artifact grid changed")
    return spec


def score(cases, observations, threshold, spec):
    protocol(spec)
    require(threshold in spec["threshold_grid"], "threshold not predeclared")
    ids = {case["id"] for case in cases}
    require(len(ids) == len(cases) and len({case["group_id"] for case in cases}) == len(cases),
            "one case per independent group required")
    require(all(case["label"] in {"supported", "unsupported"} for case in cases),
            "unknown or unlabeled cannot be a binary observation")
    rows = {row["id"]: row for row in observations}
    require(len(rows) == len(observations) and rows.keys() == ids, "missing/repeated/foreign attempt")
    rejected, failures = set(), {}
    for identity, row in rows.items():
        if row["call_status"] != "ok":
            require(row.get("acceptance_violation") is None, "failed attempt must not contain probability")
            failures[identity] = row["call_status"]
            continue
        require(row["model"] == spec["model"], "actual model mismatch")
        require(row["transport_binding_status"] == "original_transport_bytes_verified"
                and row["actual_request_hash"] == row["request_hash"] and row.get("response_sha256"),
                "original response transport required")
        value = row["acceptance_violation"]
        require(probability(value), "invalid NOUL probability")
        if value >= threshold:
            rejected.add(identity)
    supported = {case["id"] for case in cases if case["label"] == "supported"}
    unsupported = ids - supported
    false = len(supported & rejected)
    detected = len(unsupported & rejected)
    result = metrics(len(supported), len(unsupported), false, detected, threshold, spec)
    return {**result, "failed_attempts": failures, "failed_attempts_retained_in_denominators": True,
            "reject_signals": len(rejected), "unverified_signals": len(ids) - len(rejected), "verified_pass_signals": 0}


def metrics(supported_n, unsupported_n, false, detected, threshold, spec):
    """Shared exact group counts/CI for development and formal classification."""
    false_upper = upper_bound(false, supported_n, spec["confidence"]) if supported_n else None
    detect_lower = 1 - upper_bound(unsupported_n - detected, unsupported_n, spec["confidence"]) if unsupported_n else None
    reasons = []
    if supported_n < spec["min_supported"]:
        reasons.append("too_few_supported_groups")
    if unsupported_n < spec["min_unsupported"]:
        reasons.append("too_few_unsupported_groups")
    if false_upper is None or false_upper > spec["max_false_block_rate"]:
        reasons.append("false_rejection_upper_bound_exceeds_limit")
    if detect_lower is None or detect_lower < spec["min_error_rejection_lower_bound"]:
        reasons.append("error_rejection_lower_bound_below_limit")
    return {"threshold": threshold, "supported_groups": supported_n, "unsupported_groups": unsupported_n,
            "false_rejected_supported": false, "detected_unsupported": detected,
            "false_rejection_upper_95": false_upper, "error_rejection_lower_95": detect_lower,
            "gate_met": not reasons, "gate_reasons": reasons}


def calibrate(cases, observations, spec):
    trials = [score(cases, observations, threshold, spec) for threshold in protocol(spec)["threshold_grid"]]
    eligible = [trial for trial in trials if trial["gate_met"]]
    chosen = max(eligible, key=lambda trial: (trial["detected_unsupported"], trial["threshold"])) if eligible else None
    return {"grid_results": trials, "calibration_numeric_gate_met": chosen is not None,
            "qualified_for_holdout_threshold": chosen["threshold"] if chosen else None,
            "certified_threshold": None, "production_authorized": False,
            "native_claim_certification": False,
            "scope": "numeric classification only; provenance and one new holdout still required"}
