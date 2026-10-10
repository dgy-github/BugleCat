"""Freeze actual source tasks and public API headers before any paid call."""

import argparse
import ast
import hashlib
from pathlib import Path

from .calibration import create, digest, load, require


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def interfaces(code):
    result = []
    for function in ast.parse(code).body:
        if not isinstance(function, ast.FunctionDef):
            continue
        args = function.args
        for arg in [*args.posonlyargs, *args.args, *args.kwonlyargs]:
            arg.annotation = None
        if args.vararg:
            args.vararg.annotation = None
        if args.kwarg:
            args.kwarg.annotation = None
        # Defaults are public interface information, not reference algorithm.
        # Do not leak executable expressions in default values.
        for default in [*args.defaults, *[d for d in args.kw_defaults if d]]:
            require(isinstance(default, (ast.Constant, ast.List, ast.Tuple, ast.Dict, ast.Set, ast.UnaryOp)),
                    "public interface has nonliteral default; needs review")
        result.append(f"def {function.name}({ast.unparse(args)}):")
    require(bool(result), "no public function interface")
    return result


def prepare(source, catalog, authorization, protocol, split, offset, count, max_tokens=4096):
    source, catalog, authorization, protocol = map(lambda p: Path(p).resolve(), (source, catalog, authorization, protocol))
    tasks = {t["task_id"]: t for t in load(source)}
    index = load(catalog)
    require(file_hash(source) == index["source_sha256"], "official source changed")
    require(split in {"calibration", "holdout"}, "unknown split")
    require(type(offset) is int and offset >= 0 and type(count) is int and 1 <= count <= 200, "bounded batch required")
    require(max_tokens in {4096, 8192}, "frozen resource limits only")
    groups = {}
    for row in sorted(index["tasks"], key=lambda r: r["task_id"]):
        require(digest(tasks[row["task_id"]]) == row["task_sha256"], "task changed")
        if row["reserved_split"] == split:
            groups.setdefault(row["group_id"], row)
    order = sorted(groups, key=lambda group: digest({"seed": index["split_seed"], "group": group}))
    selected = order[offset:offset + count]
    require(len(selected) == count, "not enough source groups")
    cases = []
    for group in selected:
        row = groups[group]
        cases.append({"id": f"mbpp-main-{row['task_id']}", "task_id": row["task_id"],
                      "group_id": group, "split": split,
                      "public_interface": interfaces(tasks[row["task_id"]]["code"])})
    return {"schema": "buglecat.verification.mbpp-collection.v1",
            "source_file": str(source), "source_sha256": file_hash(source),
            "catalog_file": str(catalog), "catalog_sha256": file_hash(catalog),
            "authorization_file": str(authorization), "authorization_sha256": file_hash(authorization),
            "protocol_file": str(protocol), "protocol_sha256": file_hash(protocol),
            "model": "deepseek-flash", "reasoning_effort": "ultra",
            "max_tokens": max_tokens, "timeout_s": 60, "cases": cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "catalog", "authorization", "protocol", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--split", choices=["calibration", "holdout"], default="calibration")
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--count", type=int, default=8)
    parser.add_argument("--max-tokens", type=int, choices=[4096, 8192], default=4096)
    parser.add_argument("--resource-adjustment", type=Path)
    args = parser.parse_args()
    value = prepare(args.source, args.catalog, args.authorization, args.protocol, args.split, args.offset, args.count, args.max_tokens)
    if args.resource_adjustment:
        value["resource_adjustment_file"] = str(args.resource_adjustment.resolve())
        value["resource_adjustment_sha256"] = file_hash(args.resource_adjustment)
    create(args.output, value)
    print(f"prepared {len(value['cases'])} source groups; paid calls 0")


if __name__ == "__main__":
    main()
