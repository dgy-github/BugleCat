"""Bounded executable facts for existing trials, plus honestly named model audits.

No Jev, no natural-language judge, and no human identity claims. Frozen reviewed
interpretations are separate from independently executed artifact assertions.
"""

import argparse
import ast
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

from .review_queue import content_kind, digest, read_source, text_content, write_new
from .trials import check_runs


def source_map(sources, reports):
    paths, rows = {}, {}
    for source in sources:
        sha, _ = read_source(source)
        paths[sha] = source
    for report in reports:
        for line in report.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            sha, _ = read_source(Path(row["session_log"]))
            paths[sha] = Path(row["session_log"])
            rows[sha] = row
    return paths, rows


def within_root(path, root):
    # Windows extended path spelling and ordinary spelling identify the same
    # canonical directory. Resolve links first, then compare complete parts.
    def normalized(value):
        name = str(Path(value).resolve())
        if name.startswith("\\\\?\\"):
            name = name[4:]
        return os.path.normcase(name)
    resolved, parent = normalized(path), normalized(root)
    return os.path.commonpath([resolved, parent]) == parent


def last_check_output(path):
    _, messages = read_source(path)
    commands, outputs = {}, []
    for message in messages:
        for call in message.get("tool_calls", []):
            if call.get("function", {}).get("name") == "shell":
                args = call["function"].get("arguments", {})
                args = json.loads(args) if isinstance(args, str) else args
                commands[call["id"]] = args.get("command", args.get("cmd", ""))
        if message.get("role") == "tool" and message.get("name") == "shell":
            command = commands.get(message.get("tool_call_id"), "")
            target = r"(?:\.[/\\])?check\.py"
            direct = rf"python(?:\.exe)?[ \t]+(?:-B[ \t]+)?(?:{target}|\"{target}\"|'{target}')"
            if re.fullmatch(direct, command.strip(), re.IGNORECASE):
                outputs.append(text_content(message.get("content")))
    return outputs[-1] if outputs else ""


def multiplier_is_two(source):
    try:
        tree = ast.parse(source)
    except SyntaxError:
        return False
    functions = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "twice"]
    if len(functions) != 1 or len(functions[0].body) != 1:
        return False
    function = functions[0]
    if len(function.args.args) != 1 or not isinstance(function.body[0], ast.Return):
        return False
    value = function.body[0].value
    if not isinstance(value, ast.BinOp) or not isinstance(value.op, ast.Mult):
        return False
    return (isinstance(value.left, ast.Name) and value.left.id == function.args.args[0].arg
            and isinstance(value.right, ast.Constant) and type(value.right.value) is int
            and value.right.value == 2)


def artifact_oracle(row, manifest):
    fixture = next(c for c in manifest["cases"] if c["id"] == row["case_id"])
    root = Path(row["workspace"]).resolve()
    temp_root = Path(tempfile.gettempdir()).resolve()
    if not within_root(root, temp_root) or not root.parent.name.startswith("ncx-jev-trial-"):
        raise ValueError("oracle accepts preserved isolated trial fixtures only")
    manifest_sha = digest(json.dumps(manifest, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode())
    if row.get("manifest_hash") != manifest_sha[:16]:
        raise ValueError("frozen trial manifest mismatch")
    if set(fixture["files"]) - {"calc.py", "check.py"}:
        raise ValueError("unreviewed fixture paths")
    paths = [root / name for name in fixture["files"]]
    if any(path.is_symlink() or not within_root(path, root) for path in paths):
        raise ValueError("oracle source link or traversal")
    before = {p.name: digest(p.read_bytes()) for p in paths}
    protected = all((root / name).read_bytes() == fixture["files"][name].encode()
                    for name in fixture["protected_files"])
    # The trusted development manifest grader executes outside the fixture.
    # The transcript commands are never executed again by the annotation pass.
    with tempfile.TemporaryDirectory(prefix="ncx-label-oracle-") as folder:
        script = Path(folder) / "grade.py"
        script.write_text(fixture["grader"], encoding="utf-8")
        env = {k: v for k, v in os.environ.items()
               if not any(word in k.lower() for word in ("key", "token", "secret", "password"))}
        result = subprocess.run([sys.executable, "-I", "-S", "-B", str(script)],
                                cwd=root, env=env, capture_output=True, timeout=10)
    after = {p.name: digest(p.read_bytes()) for p in paths}
    if before != after:
        raise ValueError("artifact changed during independent assertion")
    recorded_output = last_check_output(Path(row["session_log"]))
    return {"grader_exit": result.returncode, "artifact_assertions_passed": result.returncode == 0,
            "protected_unchanged": protected, "source_hashes": before,
            "check_exit_sequence": check_runs(Path(row["session_log"])),
            "last_check_printed_three_checks": bool(re.search(r"(?m)^3 checks passed\s*$", recorded_output)),
            "last_check_intentional_assertion": bool(re.search(r"(?m)^AssertionError: intentional failing fixture\s*$", recorded_output)),
            "grader_sha256": digest(fixture["grader"].encode()),
            "manifest_sha256": manifest_sha,
            "calc_exact_multiplier_two": multiplier_is_two((root / "calc.py").read_text(encoding="utf-8"))
                if "calc.py" in fixture["files"] else None,
            "basis": "frozen external grader + immutable transcript exits + current artifact hashes",
            "scope": "two known development tasks; not general natural-language correctness"}


def annotate(dataset, sources, reports, manifest, interpretations):
    paths, trials = source_map(sources, reports)
    output, receipts = [], []
    for old in dataset["cases"]:
        case = dict(old)
        path = paths.get(case["source_sha256"])
        if not path:
            raise ValueError("missing original source")
        sha, messages = read_source(path)
        candidate = text_content(messages[case["source_index"]].get("content"))
        if sha != case["source_sha256"] or digest(candidate.encode()) != case["candidate_sha256"]:
            raise ValueError("stale source or candidate")
        kind = content_kind(candidate)
        case.update(candidate_kind=kind, label="unknown", review_status="model_reviewed",
                    reviewer_id="codex-root-source-audit", human_reviewed=False,
                    independent_model_blind_review=False, author_exposed=True)
        if kind != "assistant_narrative":
            case["label_basis"] = "Embedded runtime/tool/provider artifact; not an asserted completion answer."
            case["category"] = "limitation"
        elif case["source_sha256"] not in trials:
            case["label_basis"] = "Reviewed historical multi-fact scan: immutable target versions and complete claim-level scope unavailable; exit metadata cannot certify full answer."
        else:
            row = trials[case["source_sha256"]]
            oracle = artifact_oracle(row, manifest)
            if read_source(path)[0] != sha:
                raise ValueError("source changed during assertion")
            receipt = {"case_id": case["id"], "source_sha256": sha,
                       "candidate_sha256": case["candidate_sha256"], **oracle}
            receipts.append(receipt)
            interpretation = interpretations.get(case["id"])
            if interpretation and interpretation["candidate_sha256"] != case["candidate_sha256"]:
                raise ValueError("interpretation candidate mismatch")
            case["interpretation_review_status"] = "model_reviewed"
            case["assertion_receipt_sha256"] = digest(json.dumps(receipt, sort_keys=True).encode())
            if interpretation:
                outcome = interpretation["outcome"]
                runs = oracle["check_exit_sequence"]
                evidence_ok = (oracle["artifact_assertions_passed"] and oracle["protected_unchanged"]
                               and bool(runs) and runs[-1] is not None)
                if outcome == "success":
                    support = (evidence_ok and runs[-1] == 0 and oracle["calc_exact_multiplier_two"]
                               and oracle["last_check_printed_three_checks"])
                elif outcome == "failure":
                    support = evidence_ok and runs[-1] == 1 and oracle["last_check_intentional_assertion"]
                else:
                    raise ValueError("unreviewed outcome interpretation")
                case.update(label="supported" if support else "unknown",
                            review_status="assertion_reviewed" if support else "model_reviewed",
                            category="tests_passed" if outcome == "success" else "limitation",
                            label_basis="Exact candidate interpretation was reviewed by Codex; its scoped facts were checked by executable assertions. Missing proof remains Unknown.",
                            label_scope=interpretation["scope"], full_language_certified=False)
            else:
                case["category"] = "limitation"
                case["label_basis"] = "Honest framework verification-failed limitation, not an assertion of artifact failure or semantic success."
        output.append(case)
    return ({**dataset, "cases": output, "review_counts": dict(Counter(c["review_status"] for c in output)),
             "label_counts": dict(Counter(c["label"] for c in output)),
             "candidate_kind_counts": dict(Counter(c["candidate_kind"] for c in output)),
             "review_method": "independent executable fact checks; Codex scope interpretation, no human reviewer",
             "new_jev_calls": 0, "blind_holdout": False}, receipts)


def replay_manifest(dataset, sources, reports):
    paths, trials = source_map(sources, reports)
    cases = []
    for case in dataset["cases"]:
        row = trials.get(case["source_sha256"])
        if not row:
            continue  # Avoid shipping arbitrary private historical project data.
        source_sha, messages = read_source(paths[case["source_sha256"]])
        candidate = text_content(messages[case["source_index"]].get("content"))
        cases.append({"id": case["id"], "group_id": case["group_id"],
                      "session_log": row["session_log"], "workspace": row["workspace"],
                      "candidate": candidate, "candidate_sha256": digest(candidate.encode()),
                      "source_sha256": source_sha, "candidate_mode": "original",
                      "split": "development", "author_exposed": True,
                      "review_status": case["review_status"], "label": case["label"],
                      "label_basis": case["label_basis"],
                      "source_kind": "real_main_model_author_designed_development_task"})
    return {"schema": "buglecat.verification.fixed-replay.v1", "cases": cases,
            "purpose": "source-bound original development replies; not independent blind calibration",
            "max_attempts": 25, "live_requires_explicit_flag": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--sources", nargs="+", type=Path, required=True)
    parser.add_argument("--reports", nargs="+", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--interpretations", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--receipts", type=Path, required=True)
    parser.add_argument("--replay-manifest", type=Path, required=True)
    args = parser.parse_args()
    outputs = (args.output, args.receipts, args.replay_manifest)
    if any(p.exists() for p in outputs) or len({p.resolve() for p in outputs}) != 3:
        raise FileExistsError("new distinct artifacts required")
    data, receipts = annotate(json.loads(args.dataset.read_text(encoding="utf-8")), args.sources,
                              args.reports, json.loads(args.manifest.read_text(encoding="utf-8")),
                              json.loads(args.interpretations.read_text(encoding="utf-8")))
    write_new(args.output, data)
    write_new(args.receipts, {"schema": "buglecat.verification.assertion-receipts.v1", "receipts": receipts})
    write_new(args.replay_manifest, replay_manifest(data, args.sources, args.reports))
    print(json.dumps({k: data[k] for k in ("review_counts", "label_counts", "candidate_kind_counts", "new_jev_calls")}))


if __name__ == "__main__":
    main()
