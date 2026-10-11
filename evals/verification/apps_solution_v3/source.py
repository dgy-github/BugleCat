"""Original first-reference Solution source adapter; structural review only."""
import json

from ..apps_v3.oracle import digest, raw_sha
from ..apps_v3.source import (
    REFERENCE_POLICY,
    REVISION,
    TRAIN_SHA256,
    reject_nonfinite,
)
from ..apps_v3.source import (
    TRAIN_BYTES as TRAIN_BYTES,
)
from ..apps_v3.source import (
    file_sha as file_sha,
)
from ..apps_v3.source import (
    verified_rows as verified_rows,
)
from ..apps_v3.worker import validate_io
from .policy import reviewed_code


def parse_row(raw_line, line_number):
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
    reviewed_code(code, io["fn_name"])
    return {"schema": "buglecat.verification.apps-original-solution-task.v3",
            "source_kind": "apps_call_based_solution_method", "original_split": "train",
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
