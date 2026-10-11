import json

import pytest

from evals.verification.collection_manifest import file_hash
from evals.verification.extended_mbpp import SEED, derive, rebuild
from evals.verification.public_benchmark import catalog


def task(identity, factor):
    return {"task_id": identity, "prompt": f"Multiply by {factor}", "code": f"def f(n): return n * {factor}",
            "test_imports": [], "test_list": [f"assert f(2) == {factor*2}"]}


def full(task):
    return {"task_id": task["task_id"], "text": task["prompt"], "code": task["code"],
            "test_setup_code": "", "test_list": task["test_list"]}


def test_old_id_normalized_solution_and_prompt_are_not_new_independent_sources():
    original = task(1, 2)
    renamed = full(task(2, 2))
    renamed["code"] = "def twice(x): return x * 2"
    renamed["text"] = "Another wording"
    same_prompt = full(task(3, 3))
    same_prompt["text"] = original["prompt"]
    tasks, excluded = derive([original], [full(original), renamed, same_prompt, full(task(4, 4))])
    assert [item["task_id"] for item in tasks] == [1, 4]
    assert len(excluded) == 3


def test_expansion_preserves_every_original_split_and_group(tmp_path):
    original = [task(1, 2), task(2, 3)]
    base_file, base_index_file, full_file, source = [tmp_path / name for name in ("base.json", "index.json", "full.jsonl", "expanded.json")]
    base_file.write_text(json.dumps(original), encoding="utf-8")
    base_index = catalog(base_file, "a" * 40)
    base_index_file.write_text(json.dumps(base_index), encoding="utf-8")
    raw = [full(item) for item in [*original, task(3, 4), task(4, 5)]]
    full_file.write_text("".join(json.dumps(item) + "\n" for item in raw), encoding="utf-8")
    tasks, _ = derive(original, raw)
    source.write_text(json.dumps(tasks), encoding="utf-8")
    description = {"source_commit": "a" * 40, "split_seed": SEED}
    for name, path in (("base_source", base_file), ("base_catalog", base_index_file), ("full_source", full_file)):
        description[f"{name}_file"] = str(path)
        description[f"{name}_sha256"] = file_hash(path)
    result = rebuild(source, description)
    old = {item["task_id"]: item for item in base_index["tasks"]}
    for row in result["tasks"]:
        if row["task_id"] in old:
            assert row["group_id"] == old[row["task_id"]]["group_id"]
            assert row["reserved_split"] == old[row["task_id"]]["reserved_split"]
    tasks[-1]["code"] += "\n# mutation"
    source.write_text(json.dumps(tasks), encoding="utf-8")
    with pytest.raises(ValueError, match="differ"):
        rebuild(source, description)
