"""Export counts from the bounded pilot without publishing private transcripts."""
import argparse
import json
import sqlite3
from pathlib import Path


def summarize(root):
    manifest = json.loads((root / "manifest.json").read_bytes())
    result = {"schema": "buglecat.claim-pilot.summary.v1", "formal_eligible": False,
              "threshold": None, "holdout_started": False, "cases": [], "budgets": {}}
    for service, cap in (("main", 32), ("jev", 8)):
        database = (root / service / "main-attempts.sqlite3").resolve()
        with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
            count, unfinished = connection.execute(
                "SELECT COUNT(*), COALESCE(SUM(finished_json IS NULL),0) FROM attempts"
            ).fetchone()
        if count > cap:
            raise ValueError("budget exceeded")
        result["budgets"][service] = {"reserved": count, "unfinished": unfinished,
                                      "cap": cap, "unused": cap - count}
    for index, case in enumerate(manifest["cases"]):
        directory = root / case["id"]
        receipt = json.loads((directory / "receipt.json").read_bytes())
        assertions = json.loads((directory / "assertions.json").read_bytes())
        row = {"index": index, "group_id": case["id"], "task_id": case["task_id"],
               "stop_reason": receipt["stop_reason"], "iterations": receipt["iterations"],
               "artifact_result": assertions["oracle"]["result"]["status"],
               "protected_check_unchanged": receipt["protected_check_unchanged"],
               "semantic_label": "unlabeled", "author_exposed": True}
        if (directory / "scope-review.json").exists():
            review = json.loads((directory / "scope-review.json").read_bytes())
            row["semantic_label"] = review["label"]
        if (directory / "jev-response.json").exists():
            scored = json.loads((directory / "jev-response.json").read_bytes())
            row["jev_status"] = scored["status"]
            row["jev_model"] = scored.get("response", {}).get("model")
            row["scores"] = scored.get("response", {}).get("answers")
        result["cases"].append(row)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    report = summarize(args.root)
    with args.output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2)
    print(json.dumps(report["budgets"]))
