"""Adapt already-paid, previously-viewed v1 fixtures for offline development only.

Never invent request provenance, independent labels, or a new blind holdout from
the old calibration/holdout names. The adapter sends no requests.
"""

import argparse
import hashlib
import json
from pathlib import Path

from .baseline import evaluate
from .contract import VerificationInput
from .review_queue import write_new


def canonical_hash(value) -> str:
    raw = json.dumps(value, sort_keys=True, ensure_ascii=False).encode()
    return hashlib.sha256(raw).hexdigest()


def adapt(cases_path: Path, observations_path: Path) -> tuple[dict, list[dict], dict]:
    raw = cases_path.read_bytes()
    cases = json.loads(raw)
    live = [json.loads(line) for line in observations_path.read_text(encoding="utf-8").splitlines()
            if line.strip()]
    by_id = {row["id"]: row for row in live}
    if len(by_id) != len(live) or len({c["id"] for c in cases}) != len(cases):
        raise ValueError("duplicate fixture or observation")
    if set(by_id) - {c["id"] for c in cases}:
        raise ValueError("foreign observation")
    dataset, converted = [], []
    models = {o["response"]["model"] for o in live if o.get("call_status") == "ok"}
    if len(models) != 1:
        raise ValueError("development adapter requires one actual Jev model version")
    model = next(iter(models))
    for case in cases:
        packet = VerificationInput.model_validate(case["input"])
        claim = packet.claims[0] if packet.claims else None
        # These bindings identify the local fixtures only: the historical paid
        # log did not record request hashes or template version.
        state = {**case["input"], "claim": claim.text if claim else None}
        input_hash = canonical_hash(state)
        candidate_hash = hashlib.sha256(packet.candidate.encode()).hexdigest()
        dataset.append({
            "id": case["id"], "group_id": case["group"], "split": "development",
            "category": claim.kind if claim else "semantic",
            "label": {"pass": "supported", "contradiction": "unsupported",
                      "need_evidence": "unsupported", "unknown": "unknown"}[case["expected"]],
            "label_basis": "old author fixture verdict; not independently reviewed: " + case["rationale"],
            "review_status": "pending", "author_exposed": True,
            "source_kind": "synthetic_previously_viewed_v1_fixture",
            "input_hash": input_hash, "candidate_hash": candidate_hash,
            "input_binding_status": "derived_local_fixture_not_recorded_request",
            "context": "narrative" if claim else "non_asserted",
            "polarity": "positive" if claim and claim.kind != "limitation" else "negative",
            "eligible": bool(claim) and claim.kind == "semantic",
            "legacy_split": case["split"], "legacy_expected": case["expected"],
        })
        observation = by_id.get(case["id"], {})
        answers = observation.get("response", {}).get("answers", {})
        converted.append({
            "id": case["id"], "rules_verdict": evaluate(packet).verdict,
            "call_status": observation.get("call_status", "not_run"), "model": model,
            "policy_version": "python-fixture-baseline-v1",
            "template_version": "unrecorded-legacy-fixture",
            "input_hash": input_hash, "candidate_hash": candidate_hash,
            "questions": [{"name": name, "kind": answer["type"], "noul": answer["noul"]}
                          for name, answer in answers.items()],
            "elapsed_ms": observation.get("elapsed_ms"),
            "usage": observation.get("response", {}).get("usage"),
        })
    data = {"schema": "buglecat.verification.review-dataset.v1", "cases": dataset,
            "purpose": "offline development rehearsal; no real-runtime or independent holdout certification",
            "source_sha256": hashlib.sha256(raw).hexdigest(),
            "observations_source_sha256": hashlib.sha256(observations_path.read_bytes()).hexdigest(),
            "source_method": "v1 fixture baseline; not host-rules-v3.1",
            "prior_observations_reused": True, "new_jev_attempts": 0}
    protocol = {
        "schema": "buglecat.verification.calibration-protocol.v1",
        "model": model, "policy_version": "python-fixture-baseline-v1",
        "template_version": "unrecorded-legacy-fixture", "categories": ["semantic"],
        "threshold_grid": [0.5, 0.6, 0.7, 0.8, 0.9, 0.95, 0.975, 0.99, 1.0],
        "max_false_block_rate": 0.02, "confidence": 0.95,
        "min_supported": 150, "min_unsupported": 50, "min_paired_improvements": 5,
        "purpose": "predeclared exploratory category gate; not approved production policy",
        "required_later": "fresh human/assertion labels, unexposed grouped holdout, v3.1 request bindings and closed-loop success/latency/budget gates",
    }
    return data, converted, protocol


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--observations", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--converted-observations", type=Path, required=True)
    parser.add_argument("--semantic-observations", type=Path)
    parser.add_argument("--protocol", type=Path, required=True)
    args = parser.parse_args()
    outputs = [args.dataset, args.converted_observations, args.protocol]
    if args.semantic_observations:
        outputs.append(args.semantic_observations)
    if len({p.resolve() for p in outputs}) != len(outputs) or any(p.exists() for p in outputs):
        raise FileExistsError("distinct new output paths required")
    data, observations, protocol = adapt(args.cases, args.observations)
    write_new(args.dataset, data)
    with args.converted_observations.open("x", encoding="utf-8") as stream:
        for observation in observations:
            stream.write(json.dumps(observation, ensure_ascii=False, allow_nan=False) + "\n")
    write_new(args.protocol, protocol)
    semantic_ids = {c["id"] for c in data["cases"] if c["category"] == "semantic"}
    if args.semantic_observations:
        with args.semantic_observations.open("x", encoding="utf-8") as stream:
            for observation in observations:
                if observation["id"] in semantic_ids:
                    stream.write(json.dumps(observation, ensure_ascii=False, allow_nan=False) + "\n")
    print(json.dumps({"development_n": len(data["cases"]), "observations_n": len(observations),
                      "semantic_n": len(semantic_ids), "holdout_n": 0, "new_jev_attempts": 0}))


if __name__ == "__main__":
    main()
