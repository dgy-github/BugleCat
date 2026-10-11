"""Add pinned official MBPP tasks without reshuffling the original source split.

Only previously absent task IDs and normalized solutions are admitted. Static
Oracle/interface review happens before any new model output, not by its label.
"""
import argparse
import json
from collections import Counter
from pathlib import Path

from .benchmark_oracle_worker import reviewed_tree
from .calibration import create, digest, load, require
from .collection_manifest import file_hash, interfaces
from .public_benchmark import catalog, solution_fingerprint

SEED = "jev-artifact-official-mbpp-expansion-20261011-v1"


def derive(base, full):
    identifiers = {task["task_id"] for task in base}
    fingerprints = {solution_fingerprint(task["code"]) for task in base}
    prompts = {" ".join(task["prompt"].lower().split()) for task in base}
    additions, excluded = [], []
    for raw in sorted(full, key=lambda task: task["task_id"]):
        identity = raw["task_id"]
        if identity in identifiers:
            excluded.append({"task_id": identity, "reason": "existing_task_id"})
            continue
        setup = raw.get("test_setup_code", "")
        task = {"task_id": identity, "prompt": raw["text"], "code": raw["code"],
                "test_imports": [setup] if setup.strip() else [], "test_list": raw["test_list"]}
        try:
            fingerprint = solution_fingerprint(task["code"])
            normalized_prompt = " ".join(task["prompt"].lower().split())
            if fingerprint in fingerprints or normalized_prompt in prompts:
                excluded.append({"task_id": identity, "reason": "existing_normalized_solution_or_prompt"})
                continue
            interfaces(task["code"])
            for code in [task["code"], *task["test_imports"], *task["test_list"]]:
                reviewed_tree(code)
        except (ValueError, SyntaxError):
            excluded.append({"task_id": identity, "reason": "static_oracle_or_interface_unsupported"})
            continue
        additions.append(task)
        prompts.add(normalized_prompt)
    require(len({task["task_id"] for task in additions}) == len(additions), "duplicate supplemental task ID")
    return [*base, *additions], excluded


def rebuild(source, description):
    base_file, base_index_file, full_file = [Path(description[f"{name}_file"])
                                           for name in ("base_source", "base_catalog", "full_source")]
    for name, path in zip(("base_source", "base_catalog", "full_source"), (base_file, base_index_file, full_file)):
        require(file_hash(path) == description[f"{name}_sha256"], "expanded source provenance changed")
    base, base_index = load(base_file), load(base_index_file)
    require(catalog(base_file, base_index["source_commit"], base_index["split_seed"]) == base_index,
            "original source catalog changed")
    full = [json.loads(line) for line in full_file.read_text(encoding="utf-8").splitlines() if line.strip()]
    tasks, exclusions = derive(base, full)
    require(load(source) == tasks, "expanded tasks differ from original official sources")
    require(description["source_commit"] == base_index["source_commit"]
            and description["split_seed"] == SEED, "expanded source commit/seed changed")
    result = catalog(source, description["source_commit"], SEED)
    original = {row["task_id"]: row for row in base_index["tasks"]}
    fresh_groups = sorted({row["group_id"] for row in result["tasks"] if row["task_id"] not in original},
                          key=lambda group: digest({"seed": SEED, "group": group}))
    new_calibration = set(fresh_groups[:len(fresh_groups) // 2])
    for row in result["tasks"]:
        if row["task_id"] in original:
            require(row["group_id"] == original[row["task_id"]]["group_id"], "old source group changed")
            row["reserved_split"] = original[row["task_id"]]["reserved_split"]
        else:
            row["reserved_split"] = "calibration" if row["group_id"] in new_calibration else "holdout"
    groups = {row["group_id"]: row["reserved_split"] for row in result["tasks"]}
    result.update(source="Google Research sanitized plus additional official MBPP tasks",
                  source_url=f"https://github.com/google-research/google-research/blob/{description['source_commit']}/mbpp/mbpp.jsonl",
                  split_group_counts=dict(Counter(groups.values())),
                  supplemental_task_n=len(tasks) - len(base), supplemental_exclusions=exclusions,
                  catalog_extension="official-mbpp-preserve-original-split-v1")
    for name in ("base_source", "base_catalog", "full_source"):
        result[f"{name}_file"] = str(Path(description[f"{name}_file"]).resolve())
        result[f"{name}_sha256"] = description[f"{name}_sha256"]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("base-source", "base-catalog", "full-source", "output-source", "output-catalog"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    require(not args.output_source.exists() and not args.output_catalog.exists(), "new source outputs required")
    index = load(args.base_catalog)
    full = [json.loads(line) for line in args.full_source.read_text(encoding="utf-8").splitlines() if line.strip()]
    tasks, _ = derive(load(args.base_source), full)
    create(args.output_source, tasks)
    description = {"source_commit": index["source_commit"], "split_seed": SEED}
    for name, path in (("base_source", args.base_source), ("base_catalog", args.base_catalog), ("full_source", args.full_source)):
        description[f"{name}_file"] = str(path.resolve())
        description[f"{name}_sha256"] = file_hash(path)
    result = rebuild(args.output_source, description)
    create(args.output_catalog, result)
    print(json.dumps({key: result[key] for key in ("task_n", "source_group_n", "supplemental_task_n", "split_group_counts")}))


if __name__ == "__main__":
    main()
