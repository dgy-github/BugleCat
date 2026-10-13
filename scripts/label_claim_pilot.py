"""Freeze explicit model scope reviews before Jev scores; no automatic labels."""
import argparse
import hashlib
import json
from pathlib import Path


def freeze(root, decisions):
    manifest = json.loads((root / "manifest.json").read_bytes())
    for decision in decisions:
        directory = root / manifest["cases"][decision["index"]]["id"]
        if (directory / "jev-response.json").exists():
            raise ValueError("Jev score already exposed")
        prepared = json.loads((directory / "jev-prepared.json").read_bytes())
        assertions = directory / "assertions.json"
        receipt = directory / "receipt.json"
        if prepared["receipt_sha256"] != hashlib.sha256(receipt.read_bytes()).hexdigest():
            raise ValueError("receipt changed")
        if decision["label"] not in {"supported", "unsupported", "unknown"} or not decision["scope_basis"].strip():
            raise ValueError("explicit label and scope basis required")
        if decision["claim"] != prepared["body"]["state"]["claim"]:
            raise ValueError("reviewed claim differs from prepared request")
        value = {**decision, "request_hash": prepared["request_hash"],
                 "receipt_sha256": prepared["receipt_sha256"],
                 "assertions_sha256": hashlib.sha256(assertions.read_bytes()).hexdigest(),
                 "reviewer_kind": "model", "human_reviewed": False, "jev_scores_read": False,
                 "formal_eligible": False}
        with (directory / "scope-review.json").open("x", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=False, indent=2)
        print(json.dumps({"index": decision["index"], "label": decision["label"], "frozen": True}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("decisions", type=Path)
    args = parser.parse_args()
    freeze(args.root, json.loads(args.decisions.read_bytes()))
