"""Index externally authored MBPP tasks without executing code or calling models.

References and tests are source material, never an Agent completion label.
Exact/identifier-normalized solution duplicates share a source task group.
"""

import argparse
import ast
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

from .calibration import create, digest, require


class NormalizeIdentifiers(ast.NodeTransformer):
    def __init__(self):
        self.names = {}

    def name(self, value):
        return self.names.setdefault(value, f"id{len(self.names)}")

    def visit_Name(self, node):
        node.id = self.name(node.id)
        return node

    def visit_arg(self, node):
        node.arg = self.name(node.arg)
        return node

    def visit_FunctionDef(self, node):
        node.name = self.name(node.name)
        return self.generic_visit(node)


def solution_fingerprint(code):
    tree = NormalizeIdentifiers().visit(ast.parse(code))
    return hashlib.sha256(ast.dump(tree, include_attributes=False).encode()).hexdigest()


def catalog(source, commit, seed="jev-external-source-20261010-v1"):
    require(bool(re.fullmatch(r"[0-9a-f]{40}", commit)), "source commit must be exact")
    raw = Path(source).read_bytes()
    tasks = json.loads(raw)
    require(isinstance(tasks, list) and bool(tasks), "expected nonempty MBPP array")
    ids, rows, fingerprints = set(), [], {}
    for task in tasks:
        identity = task["task_id"]
        require(type(identity) is int and identity not in ids, "foreign or duplicate task ID")
        ids.add(identity)
        require(all(isinstance(task.get(k), str) and task[k].strip() for k in ("prompt", "code")), "missing prompt/reference")
        tests = task.get("test_list")
        require(isinstance(tests, list) and tests and all(isinstance(t, str) and t for t in tests), "missing tests")
        require(isinstance(task.get("test_imports"), list), "test imports must be explicit")
        ast.parse(task["code"])
        for code in [*task["test_imports"], *tests]:
            ast.parse(code)
        fingerprint = solution_fingerprint(task["code"])
        fingerprints.setdefault(fingerprint, []).append(identity)
        rows.append({"task_id": identity, "task_sha256": digest(task),
                     "prompt_sha256": hashlib.sha256(task["prompt"].encode()).hexdigest(),
                     "reference_sha256": hashlib.sha256(task["code"].encode()).hexdigest(),
                     "solution_fingerprint": fingerprint, "tests_sha256": digest(tests),
                     "test_n": len(tests), "reference_code_executed": False,
                     "candidate_label": "unlabeled", "main_model_candidate": "not_collected"})
    grouped = {}
    for fingerprint, members in fingerprints.items():
        group = "mbpp-" + digest(sorted(members))[:24]
        grouped[group] = sorted(members)
        for row in rows:
            if row["solution_fingerprint"] == fingerprint:
                row["group_id"] = group
    order = sorted(grouped, key=lambda group: digest({"seed": seed, "group": group}))
    # Freeze source-level halves before any candidate or Jev probability exists.
    # Whether each half contains enough actual labels is still unknown.
    calibration_groups = set(order[:len(order) // 2])
    for row in rows:
        row["reserved_split"] = "calibration" if row["group_id"] in calibration_groups else "holdout"
    return {"schema": "buglecat.verification.external-task-catalog.v1",
            "source": "Google Research MBPP sanitized",
            "source_commit": commit, "source_sha256": hashlib.sha256(raw).hexdigest(),
            "source_url": f"https://github.com/google-research/google-research/blob/{commit}/mbpp/sanitized-mbpp.json",
            "official_description": "https://github.com/google-research/google-research/blob/master/mbpp/README.md",
            "task_n": len(rows), "source_group_n": len(grouped), "split_seed": seed,
            "split_group_counts": {"calibration": len(calibration_groups), "holdout": len(order) - len(calibration_groups)},
            "duplicate_solution_groups": {g: members for g, members in grouped.items() if len(members) > 1},
            "original_source_split_counts": dict(Counter("prompting" if t <= 10 else "original_test" if t <= 510 else "validation" if t <= 600 else "training" for t in ids)),
            "scope": "external benchmark source pool only; not real user deployment or a certified labeled dataset",
            "limitations": ["Public benchmark may be present in model training data; new responses are required.",
                            "Reference code/tests are not labels for a future main-model completion.",
                            "Normalized solution duplicates are grouped conservatively; semantic near-duplicates may remain.",
                            "Split group counts alone cannot establish the 150-supported/50-unsupported quotas.",
                            "No generated negative claims, mutations or renamed synthetic cases count as independent real responses."],
            "paid_attempts": 0, "production_authorized": False, "tasks": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    value = catalog(args.source, args.commit)
    create(args.output, value)
    print(json.dumps({k: value[k] for k in ("task_n", "source_group_n", "split_group_counts", "paid_attempts")}))


if __name__ == "__main__":
    main()
