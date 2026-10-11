"""Original UTF8 code spans and independent actual Oracle receipt labels."""
from copy import deepcopy

import pytest

from evals.verification.apps_campaign_v3 import intake, packet
from evals.verification.apps_v3 import oracle


def task(kind=packet.FREE):
    io = {'fn_name': 'f', 'inputs': [[1], [2]], 'outputs': [1, 2]}
    return {'task_id': 101, 'source_kind': kind, 'question': 'Return supplied argument.',
            'starter_code': '', 'input_output': io, 'input_output_sha256': oracle.digest(io)}


def test_original_code_utf8_span_and_trailing_newline_preserved():
    answer = '中文说明\n```python\ndef f(x):\n    return x\n```\n完成。'
    code = intake.original_code(answer)
    assert code['code'] == 'def f(x):\n    return x\n'
    assert answer.encode()[code['start']:code['end']].decode() == code['code']
    assert code['answer_sha256'] == oracle.raw_sha(answer.encode())


@pytest.mark.parametrize('answer', ['def f(x): return x', '```js\nreturn x\n```',
                                   '```python\n\n```',
                                   '```python\ndef f(x): return x\n```\n```text\nextra\n```'])
def test_ambiguous_or_missing_fence_unknown_not_repaired(answer):
    assert intake.original_code(answer) is None
    assert intake.label(task(), answer)['label'] == 'unknown'


def test_read_only_label_never_executes_missing_oracle(monkeypatch):
    def refuse(*args, **kwargs):
        raise AssertionError('must not run a new child')
    monkeypatch.setattr(oracle, 'execute', refuse)
    value = intake.label(task(), '```python\ndef f(x): return x\n```')
    assert value['reason'] == 'independent_original_oracle_not_recorded'
    assert value['label'] == 'unknown' and value['call_eligible'] is False


@pytest.mark.parametrize('code, expected', [('def f(x): return x', 'supported'),
                                          ('def f(x): return x + 1', 'unsupported'),
                                          ('def f(x): return open("data")', 'unknown')])
def test_actual_original_pairs_label_only(code, expected):
    original_task = task()
    answer = f'```python\n{code}\n```'
    record = intake.execute_candidate(original_task, answer)
    result = intake.label(original_task, answer, record)
    assert result['label'] == expected
    assert record['execution_receipt']['pair_count'] == 2
    assert result['call_eligible'] is (expected != 'unknown')
    assert result['formal_certification'] is False


def test_solution_one_instance_original_order():
    original_task = task(packet.SOLUTION)
    original_task['input_output']['outputs'] = [1, 3]
    original_task['input_output_sha256'] = oracle.digest(original_task['input_output'])
    answer = '```python\nclass Solution:\n    def __init__(self): self.total = 0\n    def f(self, x):\n        self.total += x\n        return self.total\n```'
    record = intake.execute_candidate(original_task, answer)
    assert intake.label(original_task, answer, record)['label'] == 'supported'


@pytest.mark.parametrize('field', ['span', 'runtime', 'task', 'code', 'pair_count'])
def test_resealed_wrong_source_or_execution_rejected(field):
    original_task = task()
    answer = '```python\ndef f(x): return x\n```'
    record = deepcopy(intake.execute_candidate(original_task, answer))
    if field == 'span':
        record['original_code_span']['start'] += 1
    elif field == 'runtime':
        record['oracle_runtime_binding']['version'] = 'wrong-runtime'
    elif field == 'task':
        record['task_id'] += 1
    elif field == 'code':
        record['execution_receipt']['code_sha256'] = 'a' * 64
    else:
        record['execution_receipt']['pair_count'] = 1
    plain = {key: value for key, value in record.items() if key != 'artifact_hash'}
    record['artifact_hash'] = oracle.digest(plain)
    with pytest.raises(ValueError):
        intake.label(original_task, answer, record)


def test_resource_refusal_unknown_without_truncation_or_execution(monkeypatch):
    answer = '```python\n# ' + '\\' * 16000 + '\n```'
    def refuse(*args, **kwargs):
        raise AssertionError('oversize candidate cannot execute')
    monkeypatch.setattr(oracle, 'execute', refuse)
    assert intake.label(task(), answer)['reason'] == 'original_complete_packet_resource_refused'
    with pytest.raises(ValueError, match='candidate_json_byte_reserve'):
        intake.execute_candidate(task(), answer)


def test_original_local_receipt_no_overwrite_or_automatic_rerun(tmp_path, monkeypatch):
    path = tmp_path / 'oracle.json'
    answer = '```python\ndef f(x): return x\n```'
    intake.create_oracle(path, task(), answer)
    original = path.read_bytes()
    def refuse(*args, **kwargs):
        raise AssertionError('cannot repeat existing candidate')
    monkeypatch.setattr(oracle, 'execute', refuse)
    with pytest.raises(FileExistsError):
        intake.create_oracle(path, task(), answer)
    assert path.read_bytes() == original
