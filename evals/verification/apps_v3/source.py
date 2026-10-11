"""Read the complete pinned APPS train, preserving original first references.

Structural eligibility alone is not execution admission or fresh-group proof.
No split is reassigned and no candidate or probability selects a reference.
"""

import ast
import hashlib
import json
from pathlib import Path

from .oracle import digest, raw_sha
from .worker import reviewed_code, validate_io

REVISION = "21e74ddf8de1a21436da12e3e653065c5213e9d1"
TRAIN_BYTES = 107101272
TRAIN_SHA256 = "45e82ef22ed8e7c0c04d881a21b923e9dd233157896b0b8d5b3493e887499cae"
REFERENCE_POLICY = "first-original-reference-index-0-no-fallback"


def file_sha(path):
    hasher = hashlib.sha256()
    with Path(path).open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            hasher.update(chunk)
    return hasher.hexdigest()


def reject_nonfinite(value):
    raise ValueError("nonfinite_source_json")


def parse_row(raw_line, line_number):
    """Preserve exact source fields; choose reference zero before any execution."""
    row = json.loads(raw_line, parse_constant=reject_nonfinite)
    if type(row) is not dict or type(row.get("id")) is not int:
        raise ValueError("invalid_source_id")
    if not isinstance(row.get("question"), str) or not row["question"].strip():
        raise ValueError("missing_original_question")
    io_raw, references_raw = row.get("input_output"), row.get("solutions")
    if type(io_raw) is not str or type(references_raw) is not str:
        raise ValueError("missing_original_json_fields")
    io = json.loads(io_raw, parse_constant=reject_nonfinite)
    references = json.loads(references_raw, parse_constant=reject_nonfinite)
    validate_io(io)
    if type(references) is not list or not references or type(references[0]) is not str or not references[0].strip():
        raise ValueError("missing_first_original_reference")
    code = references[0]
    tree = reviewed_code(code)
    if not any(isinstance(node, ast.FunctionDef) and node.name == io["fn_name"] for node in tree.body):
        raise ValueError("missing_top_level_function")
    return {"schema": "buglecat.verification.apps-original-task.v3",
            "source_kind": "apps_call_based_free_function", "original_split": "train",
            "revision": REVISION, "full_source_sha256": TRAIN_SHA256,
            "task_id": row["id"], "group_id": f"apps-{REVISION}-train-{row['id']}",
            "original_line_number": line_number, "original_line_sha256": raw_sha(raw_line),
            "question": row["question"], "starter_code": row.get("starter_code"),
            "url": row.get("url"), "input_output_original": io_raw, "input_output": io,
            "input_output_sha256": digest(io), "reference_policy": REFERENCE_POLICY,
            "reference_index": 0, "reference": code, "reference_sha256": raw_sha(code.encode()),
            "references_original_sha256": raw_sha(references_raw.encode()),
            "structurally_eligible": True, "source_admitted": False,
            "independent_group_proven": False}


def verified_rows(path):
    """Yield raw rows only after full source hash, then verify it again on exit."""
    path = Path(path)
    if path.stat().st_size != TRAIN_BYTES or file_sha(path) != TRAIN_SHA256:
        raise ValueError("pinned_train_bytes_mismatch")
    seen = set()
    try:
        with path.open("rb") as stream:
            for line_number, raw_line in enumerate(stream, 1):
                row = json.loads(raw_line, parse_constant=reject_nonfinite)
                task_id = row.get("id") if type(row) is dict else None
                if type(task_id) is not int or task_id in seen:
                    raise ValueError("duplicate_or_invalid_original_id")
                seen.add(task_id)
                yield line_number, raw_line
        if len(seen) != 5000:
            raise ValueError("original_train_row_count_mismatch")
    finally:
        if path.stat().st_size != TRAIN_BYTES or file_sha(path) != TRAIN_SHA256:
            raise ValueError("pinned_source_changed_during_read")
