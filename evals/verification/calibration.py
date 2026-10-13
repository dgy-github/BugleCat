"""Offline, reject-only Jev calibration. No transport, keys or production config.

One row is one positive narrative claim, not a whole-task verdict. Review labels
and exposure declarations are supplied attestations, not independently proven by
this tool. Freeze a registration before observing calibration probabilities.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path

DATA_SCHEMA = "buglecat.verification.review-dataset.v1"
REG_SCHEMA = "buglecat.verification.calibration-registration.v1"
CAL_SCHEMA = "buglecat.verification.calibration.v1"
REJECT = {"need_evidence", "contradiction"}
VERDICTS = REJECT | {"pass", "unknown"}
LABELS = {"supported", "unsupported", "unknown", "unlabeled"}
REVIEWS = {"pending", "assertion_reviewed", "human_reviewed", "model_reviewed"}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=False, allow_nan=False).encode()).hexdigest()


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def create(path, value):
    """Never overwrite an artifact or consume a previously opened holdout."""
    with Path(path).open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def probability(value):
    return (isinstance(value, (float, int)) and not isinstance(value, bool)
            and math.isfinite(value) and 0 <= value <= 1)


def dataset(path):
    data = load(path)
    require(data.get("schema") == DATA_SCHEMA, "invalid dataset schema")
    cases = data.get("cases")
    require(isinstance(cases, list) and cases, "empty dataset")
    ids, groups = set(), {}
    for case in cases:
        require(isinstance(case, dict), "case must be object")
        for field in ("id", "group_id", "category", "source_kind"):
            require(nonempty(case.get(field)), f"missing {field}")
        require(case["id"] not in ids, "duplicate case ID")
        ids.add(case["id"])
        split = case.get("split")
        require(split in {"development", "calibration", "holdout"}, "invalid split")
        old = groups.setdefault(case["group_id"], split)
        require(old == split, "group crosses splits")
        require(case.get("label") in LABELS, "invalid label")
        if "synthetic" in case["source_kind"].lower():
            require(split == "development", "synthetic cases must remain development")
        require(case.get("review_status") in REVIEWS, "invalid review status")
        require(isinstance(case.get("author_exposed"), bool), "missing author_exposed")
        require(isinstance(case.get("label_basis"), str), "missing label_basis")
        if case["label"] != "unlabeled":
            require(nonempty(case["label_basis"]), "label requires basis")
        if "original_verdict" in case:
            require(case["original_verdict"] in VERDICTS, "invalid original verdict")
    return data


def protocol(value):
    for field in ("model", "policy_version", "template_version"):
        require(nonempty(value.get(field)), f"protocol missing {field}")
    grid = value.get("threshold_grid")
    require(isinstance(grid, list) and grid, "missing threshold_grid")
    require(all(probability(t) and t >= .5 for t in grid), "invalid threshold")
    require(grid == sorted(set(grid)), "grid must be unique and ascending")
    require(value.get("categories") == ["semantic"], "only semantic category supported")
    for field in ("max_false_block_rate", "confidence"):
        require(probability(value.get(field)), f"invalid {field}")
    require(0 < value["confidence"] < 1, "confidence must be between 0 and 1")
    if "min_detection_rate" in value:
        require(probability(value["min_detection_rate"]) and value["min_detection_rate"] > 0,
                "invalid minimum detection rate")
    for field in ("min_supported", "min_unsupported", "min_paired_improvements"):
        require(type(value.get(field)) is int and value[field] >= 1, f"invalid {field}")
    require(value["min_supported"] >= 30 and value["min_unsupported"] >= 20,
            "minimum independent-group floors are 30 supported and 20 unsupported")
    return value


def seal(value):
    return {**value, "artifact_hash": digest(value)}


def unseal(path, schema):
    value = load(path)
    require(value.get("schema") == schema, "invalid artifact schema")
    body = {k: v for k, v in value.items() if k != "artifact_hash"}
    require(value.get("artifact_hash") == digest(body), "artifact hash mismatch")
    return value


def register(data_path, protocol_path, output):
    data = dataset(data_path)
    spec = protocol(load(protocol_path))
    value = seal({"schema": REG_SCHEMA, "dataset_hash": digest(data), "protocol": spec,
                  "protocol_hash": digest(spec),
                  "declaration": "Must be frozen before viewing probabilities; hash is not proof of timing."})
    create(output, value)
    return value


def selected(data, split, spec):
    cases = [c for c in data["cases"] if c["split"] == split
             and c["category"] in spec["categories"]]
    require(cases, f"no {split} cases in registered categories")
    require(all(c["label"] != "unlabeled" for c in cases), "unlabeled cases: not run")
    return cases


def observations(path, cases, spec):
    expected = {c["id"]: c for c in cases}
    rows = {}
    for line in Path(path).read_text(encoding="utf-8-sig").splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        require(isinstance(row, dict), "observation must be object")
        identity = row.get("id")
        require(identity in expected, "observation from unselected split/category")
        require(identity not in rows, "duplicate observation ID")
        require(row.get("rules_verdict") in VERDICTS, "invalid rules verdict")
        require(nonempty(row.get("call_status")), "missing call_status")
        for field in ("policy_version", "template_version"):
            require(row.get(field) == spec[field], f"mixed or mismatched {field}")
        if row.get("model") is not None:
            require(row["model"] == spec["model"], "mixed or mismatched model")
        for field in ("input_hash", "candidate_hash"):
            require(nonempty(row.get(field)), f"missing {field}")
            if field in expected[identity]:
                require(row[field] == expected[identity][field], f"{field} mismatch")
        questions = row.get("questions")
        require(isinstance(questions, list), "questions must be list")
        answers = {}
        for question in questions:
            require(isinstance(question, dict) and nonempty(question.get("name")), "invalid question")
            name = question["name"]
            require(name not in answers, "duplicate question")
            require(question.get("kind") == "noul" and probability(question.get("noul")),
                    "illegal probability or question kind")
            answers[name] = question["noul"]
        if row["call_status"] == "ok":
            require(row.get("model") == spec["model"], "success missing model")
            require({"contradiction", "unsupported_success"} <= answers.keys(),
                    "successful observation missing policy questions")
        else:
            require(not answers, "failed call cannot carry probabilities")
        case = expected[identity]
        rows[identity] = {**row, "answers": answers,
                          "eligible": case.get("context", "narrative") == "narrative"
                          and case.get("polarity", "positive") == "positive"}
    require(rows.keys() == expected.keys(), "missing observations: not run")
    require(any(r["call_status"] == "ok" for r in rows.values()), "no successful observations: not run")
    return rows


def apply(row, threshold):
    """Mirror Rust apply_experimental_policy ordering; never manufacture Pass."""
    verdict = row["rules_verdict"]
    if verdict != "unknown" or row["call_status"] != "ok" or not row.get("eligible", True):
        return verdict
    for name, result in (("contradiction", "contradiction"),
                         ("unsupported_success", "need_evidence")):
        if row["answers"][name] >= threshold:
            return result
    return verdict


def binomial_cdf(k, n, p):
    if p == 0:
        return 1.0
    if p == 1:
        return float(k == n)
    return min(1.0, sum(math.exp(math.lgamma(n + 1) - math.lgamma(i + 1)
                               - math.lgamma(n - i + 1) + i * math.log(p)
                               + (n - i) * math.log1p(-p)) for i in range(k + 1)))


def upper_bound(errors, n, confidence):
    """One-sided exact Clopper-Pearson bound; n is independent groups."""
    if not n:
        return None
    if errors == n:
        return 1.0
    lo, hi = 0.0, 1.0
    for _ in range(70):
        mid = (lo + hi) / 2
        if binomial_cdf(errors, n, mid) > 1 - confidence:
            lo = mid
        else:
            hi = mid
    return hi


def metrics(cases, predictions, confidence):
    counts = {label: {v: 0 for v in sorted(VERDICTS)} for label in LABELS}
    for case in cases:
        counts[case["label"]][predictions[case["id"]]] += 1
    supported = counts["supported"]
    unsupported = counts["unsupported"]
    n_supported, n_unsupported = sum(supported.values()), sum(unsupported.values())
    groups = {c["group_id"] for c in cases if c["label"] == "supported"}
    blocked = {c["group_id"] for c in cases if c["label"] == "supported"
               and predictions[c["id"]] in REJECT}
    return {"counts_by_label_and_verdict": counts,
            "host_delivery_action": {
                "mapping": {"pass": "deliver_verified", "unknown": "deliver_unverified",
                            "need_evidence": "correction_or_block", "contradiction": "correction_or_block"},
                "unsupported_delivery_rate": {
                    "numerator": unsupported["pass"] + unsupported["unknown"],
                    "denominator": n_unsupported,
                    "rate": (unsupported["pass"] + unsupported["unknown"]) / n_unsupported
                    if n_unsupported else None},
                "scope": "Fixed candidate action only; correction may change eventual task outcome."},
            "false_allow": {"numerator": unsupported["pass"], "denominator": n_unsupported,
                            "rate": unsupported["pass"] / n_unsupported if n_unsupported else None},
            "false_pass_definition": "unsupported AND factual_verdict=pass; reject-only C cannot improve B false_pass",
            "false_block": {"numerator": sum(supported[v] for v in REJECT),
                            "denominator": n_supported,
                            "rate": sum(supported[v] for v in REJECT) / n_supported if n_supported else None,
                            "independent_group_errors": len(blocked), "independent_groups": len(groups),
                            "confidence_upper_bound": upper_bound(len(blocked), len(groups), confidence)},
            "unknown_is_not_pass": True}


def paired(cases, rules, augmented):
    # Count distinct groups; a group with both benefit and harm is conservative harm.
    gains, harms = set(), set()
    for case in cases:
        identity, group = case["id"], case["group_id"]
        if case["label"] == "unsupported" and rules[identity] not in REJECT and augmented[identity] in REJECT:
            gains.add(group)
        if case["label"] == "supported" and rules[identity] not in REJECT and augmented[identity] in REJECT:
            harms.add(group)
    gains -= harms
    n = len(gains) + len(harms)
    p = sum(math.comb(n, k) * .5 ** n for k in range(len(gains), n + 1)) if n else 1.0
    return {"improved_unsupported_rejection_groups": len(gains),
            "new_false_block_groups": len(harms), "one_sided_sign_test_p": p,
            "interpretation": "Rejection discrimination only; Unknown is never certified success."}


def review_issues(cases):
    issues = set()
    for case in cases:
        if case["review_status"] not in {"human_reviewed", "assertion_reviewed"}:
            issues.add("labels_not_independently_reviewed")
        if case["author_exposed"]:
            issues.add("author_exposed_not_blind")
        if "synthetic" in case["source_kind"].lower():
            issues.add("synthetic_development_only")
        if any(not nonempty(case.get(f)) for f in ("input_hash", "candidate_hash")):
            issues.add("labels_not_bound_to_input_and_candidate")
        if case.get("input_binding_status") == "derived_local_fixture_not_recorded_request":
            issues.add("derived_fixture_not_recorded_request")
        if case.get("context") != "narrative" or case.get("polarity") != "positive":
            issues.add("not_positive_narrative")
    return sorted(issues)


def score(cases, rows, threshold, spec):
    rules = {i: r["rules_verdict"] for i, r in rows.items()}
    augmented = {i: apply(r, threshold) for i, r in rows.items()}
    c_metrics = metrics(cases, augmented, spec["confidence"])
    comparison = paired(cases, rules, augmented)
    # One task is one statistical unit. A partially detected task is a miss;
    # repeating several claims from it cannot inflate the sample size.
    negative_groups = {}
    for case in cases:
        if case["label"] == "unsupported":
            negative_groups.setdefault(case["group_id"], []).append(augmented[case["id"]] in REJECT)
    detected = sum(all(values) for values in negative_groups.values())
    negative_n = len(negative_groups)
    detection_lower = (1 - upper_bound(negative_n - detected, negative_n, spec["confidence"])) if negative_n else None
    supported_n = c_metrics["false_block"]["independent_groups"]
    unsupported_n = len({c["group_id"] for c in cases if c["label"] == "unsupported"})
    bound = c_metrics["false_block"]["confidence_upper_bound"]
    reasons = review_issues(cases)
    if ("unrecorded" in spec["template_version"].lower()
            or "legacy-fixture" in spec["template_version"].lower()
            or spec["policy_version"] == "python-fixture-baseline-v1"):
        reasons.append("legacy_unrecorded_request_versions")
    if supported_n < spec["min_supported"]:
        reasons.append("too_few_supported_groups")
    if unsupported_n < spec["min_unsupported"]:
        reasons.append("too_few_unsupported_groups")
    if bound is None or bound > spec["max_false_block_rate"]:
        reasons.append("false_block_confidence_bound_exceeds_limit")
    if "min_detection_rate" in spec and (detection_lower is None or detection_lower < spec["min_detection_rate"]):
        reasons.append("detection_confidence_bound_below_limit")
    if comparison["improved_unsupported_rejection_groups"] < spec["min_paired_improvements"]:
        reasons.append("insufficient_paired_improvement")
    if comparison["one_sided_sign_test_p"] > 1 - spec["confidence"]:
        reasons.append("paired_improvement_not_significant")
    if any(row["call_status"] != "ok" for row in rows.values()):
        reasons.append("incomplete_successful_observations")
    if any(row["call_status"] == "ok" and (
            row.get("transport_binding_status") != "actual_transport_hash_recorded"
            or not isinstance(row.get("request_hash"), str)
            or len(row["request_hash"]) != 64
            or any(char not in "0123456789abcdef" for char in row["request_hash"])
            or row.get("actual_request_hash") != row.get("request_hash"))
           for row in rows.values()):
        reasons.append("actual_transport_hash_unrecorded")
    original = ({c["id"]: c["original_verdict"] for c in cases}
                if all("original_verdict" in c for c in cases) else None)
    return {"threshold": threshold, "A": metrics(cases, original, spec["confidence"]) if original else None,
            "A_status": "paired_provided" if original else "unavailable_no_original_framework_evidence",
            "B": metrics(cases, rules, spec["confidence"]), "C": c_metrics,
            "detection": {"independent_groups": negative_n, "detected_groups": detected,
                          "confidence_lower_bound": detection_lower,
                          "minimum_required": spec.get("min_detection_rate"),
                          "partial_group_detection_counts_as_miss": True},
            "paired_C_vs_B": comparison, "gate_met": not reasons, "gate_reasons": reasons}


def calibrate(data_path, observation_path, registration_path, output, development=False):
    registration = unseal(registration_path, REG_SCHEMA)
    data = dataset(data_path)
    require(digest(data) == registration["dataset_hash"], "dataset changed after registration")
    spec = protocol(registration["protocol"])
    split = "development" if development else "calibration"
    cases = selected(data, split, spec)
    rows = observations(observation_path, cases, spec)
    trials = [score(cases, rows, t, spec) for t in spec["threshold_grid"]]
    # Fixed tie break: most unsupported groups rejected, then highest threshold.
    eligible = [t for t in trials if t["gate_met"]]
    pool = eligible or trials
    chosen = max(pool, key=lambda t: (t["paired_C_vs_B"]["improved_unsupported_rejection_groups"],
                                     t["threshold"]))
    gate_met = chosen["gate_met"] and not development
    value = seal({"schema": CAL_SCHEMA, "registration": registration,
                  "registration_path": str(Path(registration_path).resolve()),
                  "dataset_hash": digest(data), "observations_hash": digest(rows), "split": split,
                  "case_ids": [c["id"] for c in cases], "threshold": chosen["threshold"],
                  "certified_threshold": chosen["threshold"] if gate_met else None,
                  "threshold_status": "calibrated_for_holdout_only" if gate_met else "candidate_not_calibrated",
                  "calibration_gate_met": gate_met,
                  "development_only": development, "grid_results": trials,
                  "production_authorized": False})
    create(output, value)
    return value


def holdout(data_path, observation_path, calibration_path, receipt):
    calibration = unseal(calibration_path, CAL_SCHEMA)
    registration = calibration["registration"]
    require(digest({k: v for k, v in registration.items() if k != "artifact_hash"})
            == registration["artifact_hash"], "registration hash mismatch")
    require(not calibration["development_only"], "development calibration cannot open holdout")
    require(calibration["calibration_gate_met"], "calibration gate failed: holdout not run")
    require(calibration.get("certified_threshold") == calibration["threshold"],
            "calibration threshold not qualified")
    data = dataset(data_path)
    require(digest(data) == calibration["dataset_hash"], "holdout dataset changed")
    spec = protocol(registration["protocol"])
    require(calibration["threshold"] in spec["threshold_grid"], "threshold not registered")
    cases = selected(data, "holdout", spec)
    require(not review_issues(cases), "holdout review/exposure/binding requirements not met")
    # Persist consumption BEFORE reading holdout probabilities. A parse error also
    # consumes this registration's holdout; changing output path cannot retry it.
    registration_path = Path(calibration["registration_path"])
    require(unseal(registration_path, REG_SCHEMA) == registration, "registration file changed")
    require(not Path(receipt).exists(), "receipt already exists")
    lock = registration_path.parent / ("holdout-consumed-" + registration["dataset_hash"] + ".json")
    marker = {"schema": "buglecat.verification.holdout-consumption.v1",
              "dataset_hash": registration["dataset_hash"], "calibration_hash": calibration["artifact_hash"],
              "receipt": str(Path(receipt).resolve()), "status": "consumed_before_read"}
    create(lock, marker)
    try:
        rows = observations(observation_path, cases, spec)
        result = score(cases, rows, calibration["threshold"], spec)
        value = {"schema": "buglecat.verification.holdout-receipt.v1", "status": "completed",
                 "calibration_hash": calibration["artifact_hash"], "dataset_hash": digest(data),
                 "observations_hash": digest(rows), "result": result,
                 "blindness": "Declared unexposed; tool cannot prove independent annotation or prior access.",
                 "production_authorized": False}
    except (ValueError, OSError, TypeError, KeyError) as error:
        create(receipt, seal({**marker, "status": "failed_consumed", "error": str(error)}))
        raise
    create(receipt, seal(value))
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    registration = commands.add_parser("register")
    registration.add_argument("--dataset", required=True)
    registration.add_argument("--protocol", required=True)
    registration.add_argument("--output", required=True)
    calibration = commands.add_parser("calibrate")
    for name in ("dataset", "observations", "registration", "output"):
        calibration.add_argument("--" + name, required=True)
    calibration.add_argument("--development", action="store_true")
    evaluation = commands.add_parser("holdout")
    for name in ("dataset", "observations", "calibration", "receipt"):
        evaluation.add_argument("--" + name, required=True)
    args = vars(parser.parse_args())
    command = args.pop("command")
    try:
        if command == "register":
            result = register(args["dataset"], args["protocol"], args["output"])
        elif command == "calibrate":
            result = calibrate(args["dataset"], args["observations"], args["registration"],
                               args["output"], args["development"])
        else:
            result = holdout(args["dataset"], args["observations"], args["calibration"], args["receipt"])
    except (ValueError, OSError, TypeError, KeyError) as error:
        parser.exit(2, f"not run: {error}\n")
    print(json.dumps({"artifact_hash": result.get("artifact_hash"),
                      "threshold": result.get("threshold"), "status": result.get("status"),
                      "certified_threshold": result.get("certified_threshold"),
                      "threshold_status": result.get("threshold_status"),
                      "calibration_gate_met": result.get("calibration_gate_met"),
                      "production_authorized": False}))


if __name__ == "__main__":
    main()
