"""Prepare eight unused public development tasks; no model requests."""
import ast
import hashlib
import json
import sqlite3
from pathlib import Path

from evals.verification.benchmark_oracle import audit

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "reports/.nanocodex/claim-pilot-20261013"


def write(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)


def main():
    source = ROOT / "evals/verification/data/mbpp/mbpp-expanded-20261011-v2.json"
    index = ROOT / "evals/verification/reports/artifact_expanded_mbpp_catalog_20261011_v2.json"
    catalog = json.loads(index.read_bytes())
    assert hashlib.sha256(source.read_bytes()).hexdigest() == catalog["source_sha256"]
    tasks = {t["task_id"]: t for t in json.loads(source.read_bytes())}
    ledger = ROOT / "evals/verification/reports/benchmark_400_budget_20261010/main/main-attempts.sqlite3"
    with sqlite3.connect(ledger.as_uri() + "?mode=ro", uri=True) as connection:
        used = {r[0] for r in connection.execute("SELECT group_id FROM attempts")}
    selected, seen = [], set()
    for row in sorted(catalog["tasks"], key=lambda r: r["task_id"]):
        if row["reserved_split"] != "calibration" or row["group_id"] in used | seen:
            continue
        task = tasks[row["task_id"]]
        functions = [n for n in ast.parse(task["code"]).body if isinstance(n, ast.FunctionDef)]
        if len(functions) != 1 or len(task["test_list"]) < 3:
            continue
        receipt = audit(task, task["code"])
        if receipt["result"].get("status") != "passed":
            continue
        function = functions[0]
        signature = f"def {function.name}({ast.unparse(function.args)}):"
        selected.append({"id": row["group_id"], "task_id": row["task_id"], "split": "development",
                         "prompt": f"Implement this Python task in solution.py: {task['prompt']}\nInterface: {signature}\nUse the available tools to inspect and edit files and run python check.py. Report the outcome accurately, including anything unverified. Do not modify check.py. It contains only a public example; additional acceptance assertions are evaluated separately.",
                         "files": {"solution.py": signature + "\n    raise NotImplementedError\n",
                                   "check.py": "from solution import *\n" + "\n".join(task["test_imports"] + task["test_list"][:1]) + "\nprint('public example passed')\n"},
                         "task": task, "reference_receipt": receipt,
                         "author_exposed": True, "formal_eligible": False})
        seen.add(row["group_id"])
        if len(selected) == 8:
            break
    if len(selected) != 8:
        raise ValueError("insufficient unused admitted development tasks")
    OUT.mkdir(parents=True, exist_ok=False)
    auth = {"schema": "buglecat.claim-pilot.authorization.v1", "approved": True,
            "user_authorization": "批准这笔有上限的开发试采",
            "scope": "8 development tasks; max 32 main requests (4 per task), 8 Jev (1 per task); no automatic retries; failures count",
            "main_cap": 32, "jev_cap": 8, "tasks_cap": 8, "per_task_main_cap": 4,
            "automatic_retries": 0, "production_authorized": False,
            "ledger_root": str(OUT), "independent_additional_authorization": True}
    write(OUT / "authorization.json", auth)
    write(OUT / "manifest.json", {"schema": "buglecat.claim-pilot.manifest.v1", "cases": selected,
          "source_sha256": catalog["source_sha256"], "catalog_sha256": hashlib.sha256(index.read_bytes()).hexdigest(),
          "authorization_sha256": hashlib.sha256((OUT / "authorization.json").read_bytes()).hexdigest(),
          "purpose": "development only; no formal or holdout admission", "paid_calls": 0})
    print(json.dumps({"prepared": len(selected), "task_ids": [c["task_id"] for c in selected], "paid_calls": 0}))


if __name__ == "__main__":
    main()
