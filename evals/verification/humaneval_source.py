"""Pinned official HumanEval source adapter, never a formal registration.

All 164 source entries and complete original tests are retained. No candidate
exists yet, and source/reference admission is not a positive or negative label.
New IDs are explicitly namespaced; original MBPP source splits stay untouched.
"""
import argparse
import ast
import gzip
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

from .calibration import create, digest, load, require
from .collection_manifest import file_hash, interfaces
from .humaneval_oracle_worker import reviewed_tree
from .public_benchmark import solution_fingerprint

COMMIT = "6d43fb980f9fee3c892a914eda09951f772ad10d"
PINNED = {"data/HumanEval.jsonl.gz": "b796127e635a67f93fb35c04f4cb03cf06f38c8072ee7cee8833d7bee06979ef",
          "LICENSE": "bcba3de214851cce46ed5af42d6698044616eeace887c3231bc7a20474ab639e",
          "human_eval/data.py": "e1351f0ded89b072490180ce94ce33b88bac4e38b26f08eb26ab0714586585ab"}
SOURCE_SCHEMA = "buglecat.verification.humaneval-source-inventory.v1"
SCOPE = "complete pinned official HumanEval check(entry_point) only; not full task semantics"


def official_rows(manifest_file):
    manifest = load(manifest_file)
    require(manifest.get("commit") == COMMIT and set(manifest.get("files", {})) == set(PINNED), "official snapshot required")
    for name, expected in PINNED.items():
        binding = manifest["files"][name]
        require(binding["sha256"] == expected and file_hash(binding["file"]) == expected
                and binding["url"] == f"https://raw.githubusercontent.com/openai/human-eval/{COMMIT}/{name}", "official source binding changed")
    raw = Path(manifest["files"]["data/HumanEval.jsonl.gz"]["file"]).read_bytes()
    rows = [json.loads(line) for line in gzip.decompress(raw).decode("utf-8").splitlines() if line.strip()]
    require(len(rows) == 164 and {r["task_id"] for r in rows} == {f"HumanEval/{n}" for n in range(164)}, "complete official source required")
    return rows, manifest


def adapt(row):
    identity = row["task_id"]
    require(identity.startswith("HumanEval/") and identity[10:].isdigit(), "invalid source identity")
    entry = row["entry_point"]
    require(entry.isidentifier() and not entry.startswith("_"), "invalid entry point")
    reference = row["prompt"] + row["canonical_solution"]
    tree = ast.parse(row["test"])
    checks = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "check"]
    require(len(checks) == 1 and len(checks[0].args.args) == 1
            and not checks[0].args.vararg and not checks[0].args.kwarg
            and any(isinstance(node, ast.Assert) for node in ast.walk(checks[0])), "complete check(candidate) function required")
    require(any(isinstance(node, ast.FunctionDef) and node.name == entry for node in ast.parse(reference).body), "reference entry point missing")
    return {"task_id": 1_000_000 + int(identity[10:]), "source_task_id": identity,
            "source_kind": "official_humaneval", "prompt": row["prompt"], "code": reference,
            "entry_point": entry, "test_imports": [], "test_list": [row["test"], f"check({entry})"],
            "original_test_sha256": hashlib.sha256(row["test"].encode()).hexdigest(),
            "source_row_sha256": digest(row), "oracle_scope": SCOPE}


def implementation_fingerprint(code):
    """Conservative extra duplicate check across differently documented suites.

    Ignore annotations/import preambles/docstrings for this exclusion check only.
    Original executable source and assertions are never transformed or executed
    from this AST. Defaults and all algorithm statements remain represented.
    """
    tree = ast.parse(code)
    class NormalizePresentation(ast.NodeTransformer):
        def visit_FunctionDef(self, node):
            node.returns = None
            node.type_comment = None
            if node.body and isinstance(node.body[0], ast.Expr) and isinstance(node.body[0].value, ast.Constant) and isinstance(node.body[0].value.value, str):
                node.body = node.body[1:]
            if not node.body:
                node.body = [ast.Pass()]
            return self.generic_visit(node)
        def visit_arg(self, node):
            node.annotation = None
            node.type_comment = None
            return node
    tree.body = [node for node in tree.body if not isinstance(node, (ast.Import, ast.ImportFrom))]
    tree = NormalizePresentation().visit(tree)
    return solution_fingerprint(ast.unparse(tree))


def inventory(rows, mbpp_tasks=()):
    tasks, cases, seen, grouped = [], [], set(), {}
    mbpp_fingerprints = {solution_fingerprint(task["code"]) for task in mbpp_tasks}
    mbpp_implementations = {implementation_fingerprint(task["code"]) for task in mbpp_tasks}
    for row in sorted(rows, key=lambda r: int(r["task_id"][10:])):
        require(row["task_id"] not in seen, "duplicate source identity")
        seen.add(row["task_id"])
        task = adapt(row)
        fingerprint = solution_fingerprint(task["code"])
        implementation = implementation_fingerprint(task["code"])
        status, reason, interface = "static_supported", None, None
        try:
            interface = interfaces(task["code"])
            for text in [task["code"], *task["test_list"]]:
                reviewed_tree(text)
        except (ValueError, SyntaxError) as error:
            status, reason = "unknown", str(error).split(":", 1)[0]
        if fingerprint in mbpp_fingerprints or implementation in mbpp_implementations:
            status, reason = "excluded_duplicate", "normalized_reference_matches_MBPP"
        group = "humaneval-" + implementation[:24]
        grouped.setdefault(group, []).append(task["task_id"])
        tasks.append(task)
        cases.append({"task_id": task["task_id"], "source_task_id": task["source_task_id"], "group_id": group,
                      "task_sha256": digest(task), "original_test_sha256": task["original_test_sha256"],
                      "solution_fingerprint": fingerprint, "implementation_fingerprint": implementation,
                      "public_interface": interface, "source_admission": status, "source_admission_reason": reason,
                      "reserved_split": "unassigned_new_source", "candidate_label": "unlabeled", "reference_executed": False})
    body = {"schema": SOURCE_SCHEMA, "source_commit": COMMIT, "tasks": cases,
            "task_count": len(tasks), "source_group_count": len(grouped),
            "static_admission_counts": dict(Counter(case["source_admission"] for case in cases)),
            "duplicate_groups": {group: ids for group, ids in grouped.items() if len(ids) > 1},
            "scope": SCOPE, "inventory_only": True, "collection_preregistered": False,
            "source_splits_not_assigned": True, "old_MBPP_reserved_split_changed": False,
            "no_future_candidate_labels": True, "new_paid_calls": 0, "production_authorized": False,
            "limitations": ["Public benchmarks may occur in model training data.",
                            "Static support and reference pass do not label an uncollected candidate.",
                            "Normalized implementation grouping ignores docstrings/annotations/import preambles only for conservative exclusion; semantic near-duplicates may remain.",
                            "Unsupported environments and reference failures must stay Unknown."]}
    return tasks, {**body, "artifact_hash": digest(body)}


def audit(task, candidate_code, timeout=5):
    require(type(timeout) is int and 1 <= timeout <= 10, "bounded timeout required")
    worker = Path(__file__).with_name("humaneval_oracle_worker.py").resolve()
    legacy = worker.with_name("benchmark_oracle_worker.py")
    bindings = [{"file": str(path), "sha256": file_hash(path)} for path in (worker, legacy)]
    payload = {"reference": task["code"], "imports": task["test_imports"], "tests": task["test_list"], "candidate": candidate_code}
    environment = {key: os.environ[key] for key in ("SYSTEMROOT", "WINDIR", "TEMP", "TMP") if key in os.environ}
    with tempfile.TemporaryDirectory(prefix="ncx-humaneval-oracle-") as work:
        try:
            child = subprocess.run([sys.executable, "-I", "-S", "-B", str(worker)], input=json.dumps(payload),
                                   text=True, capture_output=True, cwd=work, env=environment, timeout=timeout, check=False)
            result = json.loads(child.stdout) if child.returncode == 0 else {"status": "unknown", "reason": "worker_failed"}
        except (subprocess.TimeoutExpired, json.JSONDecodeError):
            result = {"status": "unknown", "reason": "worker_timeout_or_invalid_output"}
    require(all(file_hash(binding["file"]) == binding["sha256"] for binding in bindings), "oracle source changed")
    body = {"schema": "buglecat.verification.humaneval-assertion-receipt.v1", "task_id": task["task_id"],
            "source_task_id": task["source_task_id"], "task_sha256": digest(task), "original_test_sha256": task["original_test_sha256"],
            "candidate_code_sha256": hashlib.sha256(candidate_code.encode()).hexdigest(), "oracle_bindings": bindings,
            "oracle_scope": SCOPE, "timeout_s": timeout, "test_block_count": len(task["test_list"]), "result": result,
            "independent_of_jev": True, "secrets_removed": True, "paid_attempts": 0}
    return {**body, "artifact_hash": digest(body)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--official-manifest", type=Path, required=True)
    parser.add_argument("--mbpp-source", type=Path, required=True)
    parser.add_argument("--output-source", type=Path, required=True)
    parser.add_argument("--output-inventory", type=Path, required=True)
    args = parser.parse_args()
    require(not args.output_source.exists() and not args.output_inventory.exists(), "create-new outputs required")
    rows, manifest = official_rows(args.official_manifest)
    tasks, value = inventory(rows, load(args.mbpp_source))
    body = {key: val for key, val in value.items() if key != "artifact_hash"}
    body["bindings"] = [{"file": str(args.official_manifest.resolve()), "sha256": file_hash(args.official_manifest)},
                        {"file": str(args.mbpp_source.resolve()), "sha256": file_hash(args.mbpp_source)}, *manifest["files"].values()]
    create(args.output_source, tasks)
    body["derived_source_file"] = str(args.output_source.resolve())
    body["derived_source_sha256"] = file_hash(args.output_source)
    create(args.output_inventory, {**body, "artifact_hash": digest(body)})
    print(json.dumps({key: body[key] for key in ("task_count", "source_group_count", "static_admission_counts", "new_paid_calls")}))


if __name__ == "__main__":
    main()
