"""Run independent frozen assertions before reading any Jev probability."""
import argparse
import hashlib
import json
from pathlib import Path

from evals.verification.benchmark_oracle import audit


def review(root, index):
    manifest = json.loads((root / "manifest.json").read_bytes())
    case = manifest["cases"][index]
    directory = root / case["id"]
    receipt = json.loads((directory / "receipt.json").read_bytes())
    code = (directory / "workspace/solution.py").read_text(encoding="utf-8")
    oracle = audit(case["task"], code)
    artifact = directory / "solution-final.py"
    with artifact.open("x", encoding="utf-8") as stream:
        stream.write(code)
    value = {"schema": "buglecat.claim-pilot.assertions.v1", "group_id": case["id"],
             "receipt_sha256": hashlib.sha256((directory / "receipt.json").read_bytes()).hexdigest(),
             "candidate_sha256": hashlib.sha256(receipt["candidate"].encode()).hexdigest(),
             "artifact_sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
             "oracle": oracle, "protected_check_unchanged": receipt["protected_check_unchanged"],
             "natural_final_answer": receipt["stop_reason"] == "completed",
             "semantic_label": "unlabeled", "semantic_scope_review_required": True,
             "jev_scores_read": False, "formal_eligible": False,
             "scope": "Frozen artifact assertions only; cannot automatically label a natural-language completion claim."}
    with (directory / "assertions.json").open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
    print(json.dumps({"index": index, "artifact_result": oracle["result"],
                      "natural_final_answer": value["natural_final_answer"], "semantic_label": "unlabeled"}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("index", type=int)
    args = parser.parse_args()
    review(args.root, args.index)
