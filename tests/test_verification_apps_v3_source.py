"""Source-only deterministic policy and original-byte retention."""

import json

import pytest

from evals.verification.apps_v3.source import parse_row, verified_rows


def row(**changes):
    value = {"id": 12, "question": "Original question", "starter_code": "def f(x):", "url": "source-url",
             "input_output": json.dumps({"fn_name": "f", "inputs": [[1]], "outputs": [2]}),
             "solutions": json.dumps(["def f(x): return x + 1", "def f(x): return 2"])}
    value.update(changes)
    return json.dumps(value).encode() + b"\n"


def test_exact_source_provenance_without_claiming_admission():
    raw = row()
    task = parse_row(raw, 13)
    assert task["reference"] == "def f(x): return x + 1"
    assert task["reference_index"] == 0
    assert task["original_split"] == "train" and task["original_line_number"] == 13
    assert task["input_output_original"] == json.loads(raw)["input_output"]
    assert task["source_admitted"] is False and task["independent_group_proven"] is False


def test_no_fallback_to_later_compatible_reference():
    with pytest.raises(ValueError, match="restricted_import"):
        parse_row(row(solutions=json.dumps(["import os\ndef f(x): return x", "def f(x): return 2"])), 1)


@pytest.mark.parametrize("changes", [
    {"solutions": "[]"}, {"solutions": '["", "def f(x): return 2"]'},
    {"input_output": '{"fn_name": null, "inputs": [[1]], "outputs": [2]}'},
    {"input_output": '{"fn_name": "f", "inputs": [], "outputs": []}'},
    {"input_output": '{"fn_name": "f", "inputs": [[1]], "outputs": [NaN]}'},
])
def test_invalid_source_is_not_an_artificial_negative(changes):
    with pytest.raises(ValueError):
        parse_row(row(**changes), 1)


def test_only_full_original_pinned_file_is_allowed(tmp_path):
    path = tmp_path / "partial.jsonl"
    path.write_bytes(row())
    with pytest.raises(ValueError, match="pinned_train_bytes_mismatch"):
        list(verified_rows(path))
