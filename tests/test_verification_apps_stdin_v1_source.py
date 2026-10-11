"""Synthetic row parsing only; never execute an APPS reference or candidate."""
import json

import pytest

from evals.verification.apps_stdin_v1 import contract, source


def row(**changes):
    value = {'id': 9, 'question': 'Echo the original line.', 'starter_code': '', 'url': 'https://example.test/task',
             'input_output': json.dumps({'inputs': [['x', 'y'], 'z\r\n'], 'outputs': ['x', 'z'], 'fn_name': None}),
             'solutions': json.dumps(['print(input())', 'print("unused later reference")']), 'difficulty': 'interview'}
    value.update(changes)
    return json.dumps(value, ensure_ascii=False).encode() + b'\n'


def test_complete_source_fields_original_zero_raw_io_and_identity():
    raw = row()
    task = source.parse_row(raw, 4)
    original = json.loads(raw)
    assert task['source_kind'] == 'apps_stdin_restricted_script'
    assert task['reference'] == 'print(input())' and task['reference_index'] == 0
    assert task['reference_policy'] == source.original.REFERENCE_POLICY
    assert task['input_output_original'] == original['input_output']
    assert task['input_output'] == json.loads(original['input_output'])
    assert task['input_output_sha256'] == source.original.digest(task['input_output'])
    assert task['references_original_sha256'] == source.original.raw_sha(original['solutions'].encode())
    assert task['original_line_sha256'] == source.original.raw_sha(raw)
    assert task['original_line_number'] == 4 and task['original_split'] == 'train'
    assert task['group_id'] == f'apps-{source.original.REVISION}-train-9'
    assert task['source_admitted'] is task['independent_group_proven'] is False


@pytest.mark.parametrize('changes', [
    {'id': True}, {'id': '9'}, {'question': ''}, {'starter_code': None}, {'url': []},
    {'input_output': {}}, {'solutions': []}, {'solutions': '[]'}, {'solutions': '[null,"print(1)"]'},
    {'input_output': '{"inputs":[],"outputs":[]}'},
    {'input_output': '{"inputs":["x"],"outputs":["x"],"fn_name":"f"}'},
    {'input_output': '{"inputs":["x"],"outputs":["x"],"label":"passed"}'},
    {'input_output': '{"inputs":[1],"outputs":["x"]}'},
    {'solutions': '[NaN,"print(1)"]'},
])
def test_missing_or_changed_original_shape_rejected(changes):
    with pytest.raises((ValueError, contract.ScopeError)):
        source.parse_row(row(**changes), 1)


@pytest.mark.parametrize('code', ['import sys\nprint(1)', 'x = input\nprint(x())',
                                  'print = lambda: 1', 'class Solution:\n pass'])
def test_first_original_reference_never_falls_back(code):
    with pytest.raises(contract.ScopeError):
        source.parse_row(row(solutions=json.dumps([code, 'print(input())'])), 1)


@pytest.mark.parametrize('line', [True, 0, -1, '1'])
def test_typed_original_line_required(line):
    with pytest.raises(ValueError):
        source.parse_row(row(), line)


def test_original_main_guard_and_full_list_io_are_preserved():
    code = "if __name__ == '__main__':\n    print(input())\n"
    task = source.parse_row(row(solutions=json.dumps([code])), 1)
    assert task['reference'] == code
    assert contract.ordered_pairs(task['input_output']) == [('x\ny', 'x'), ('z\r\n', 'z')]


def test_reader_is_original_whole_file_verifier():
    assert source.verified_rows is source.original.verified_rows
