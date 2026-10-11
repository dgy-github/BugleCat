"""Temporary packet joins; synthetic labels cannot certify or permit spending."""
from copy import deepcopy
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import development_packets as p
from tests.test_verification_apps_stdin_v1_development_review import data_value, fixture_rows, save


@pytest.fixture
def originals(tmp_path, monkeypatch):
    fixed, rows = fixture_rows()
    preparation = tmp_path / 'fixture-preparation'
    preparation.mkdir()
    executable = tmp_path / 'fixture-executable.exe'
    executable.write_bytes(b'fixture-not-executable')
    tasks, original_cases = {}, []
    for case in fixed:
        tasks[case['task_id']] = {'question': 'Echo the input.', 'starter_code': '',
            'input_output': {'inputs': ['x\n', 'y\n'], 'outputs': ['x', 'y']},
            'reference': 'DO_NOT_SEND_REFERENCE'}
        case['task_sha256'] = p.budget.digest(tasks[case['task_id']])
        rows[case['task_id']]['task_sha256'] = case['task_sha256']
        original_cases.append({key: case[key] for key in ('group_id', 'task_id', 'task_sha256', 'source_kind')})
    train = tmp_path / 'fixture-train.jsonl'
    train.write_bytes(b'fixture-not-APPS')
    authority_file = save(tmp_path / 'fixture-authority.json', p.cohort.seal({'cases': original_cases, 'train_file': str(train)}))
    for row in rows[:8]:
        answer = '```python\r\nprint(input())\r\n```\r\nI have not run tests.'
        extracted = p.intake.original_code(answer)
        row.update(candidate_code_span=extracted, candidate_code_sha256=extracted['code_sha256'])
        receipt = save(preparation / (row['id'] + '.json'), {'result': {'candidate': answer}})
        row.update(main_receipt_file=str(receipt), main_receipt_sha256=p.cohort.sha(receipt))
    value = p.cohort.seal({'cases': fixed, 'source_authority_file': str(authority_file),
        'source_authority_sha256': p.cohort.sha(authority_file), 'bindings': [],
        'protocol_sha256': '5' * 64, 'oracle_runtime_binding': p.packet.runtime_info(),
        'ledger_snapshot': {'authorization_chain': {'fixture_only': True}}})
    cohort_file = save(tmp_path / 'fixture-cohort.json', value)
    data = p.cohort.seal({**data_value(rows), 'cohort_file': str(cohort_file),
                         'cohort_sha256': p.cohort.sha(cohort_file)})
    dataset_file = save(tmp_path / 'fixture-dataset.json', data)
    snapshot = ({'fixture_only': True}, {})

    def review(*args):
        return p.cohort.seal({'development_main_oracle_evidence_complete': True,
            'dataset_sha256': p.cohort.sha(dataset_file), 'dataset_hash': data['artifact_hash'],
            'cohort_sha256': p.cohort.sha(cohort_file), 'cohort_hash': value['artifact_hash'],
            'budget_snapshot_hash': p.budget.digest(snapshot[0]),
            'completion_snapshot_hash': p.budget.digest(snapshot[1]), 'review_bindings': []})

    def task_read(authority, selected):
        assert selected == original_cases  # Actual source shape, no phase/id extras.
        return deepcopy(tasks), train

    monkeypatch.setattr(p, 'loaded_check', lambda: None)
    monkeypatch.setattr(p.development_review, 'review', review)
    monkeypatch.setattr(p.budget, '_snapshot_chain', lambda chain: deepcopy(snapshot))
    monkeypatch.setattr(p.source_authority, '_read_original_tasks', task_read)
    monkeypatch.setattr(p.intake.oracle, 'execute', lambda *args: pytest.fail('packet preparation executed Oracle'))
    return {'dataset': dataset_file, 'preparation': preparation, 'exe': executable, 'rows': rows,
            'data': data, 'tasks': tasks, 'authority': authority_file, 'snapshot': snapshot}


def run(f):
    return p.prepare(f['dataset'], [f['preparation']], f['exe'])


def test_original_code_crlf_io_complete_no_reference_and_no_authority(originals):
    f = originals
    before = {path: path.read_bytes() for path in f['dataset'].parent.rglob('*') if path.is_file()}
    result = run(f)
    assert len(result['cases']) == result['development_groups'] == result['eligible_groups'] == 8
    assert result['whole_allocation_groups_retained_in_original_dataset'] == 808
    assert 'DO_NOT_SEND_REFERENCE' not in str(result)
    request = result['cases'][0]['request']
    assert 'print(input())\\r\\n' in p.packet.canonical_bytes(request).decode()
    assert 'I have not run tests.' not in str(request)
    assert result['new_model_calls'] == result['new_jev_calls'] == result['oracle_executions'] == 0
    for key in ('collection_authorized', 'reservation_authorized', 'formal_registration',
                'calibration_opened', 'holdout_opened', 'formal_certification', 'production_authorized'):
        assert result[key] is False
    assert result['certified_threshold'] is None
    assert before == {path: path.read_bytes() for path in f['dataset'].parent.rglob('*') if path.is_file()}


def test_unknown_outcome_remains_without_packet(originals):
    f = originals
    f['data']['cases'][0].update(label='unknown', reason='unknown', call_eligible=False)
    changed = p.cohort.seal({key: value for key, value in f['data'].items() if key != 'artifact_hash'})
    f['data'].clear()
    f['data'].update(changed)
    save(f['dataset'], f['data'])
    result = run(f)
    assert len(result['cases']) == 8 and result['eligible_groups'] == 7
    assert result['cases'][0]['label'] == 'unknown' and result['cases'][0]['request'] is None


def test_incomplete_dev_scope_cannot_prepare_jev(originals, monkeypatch):
    original = p.development_review.review
    def incomplete(*args):
        result = original(*args)
        result['development_main_oracle_evidence_complete'] = False
        return result
    monkeypatch.setattr(p.development_review, 'review', incomplete)
    with pytest.raises(ValueError, match='complete development'):
        run(originals)


@pytest.mark.parametrize('change', ['candidate', 'span', 'hash', 'authority', 'evidence', 'accounting', 'io_resource'])
def test_changed_original_bytes_or_resources_rejected(originals, monkeypatch, change):
    f = originals
    original_read = p.source_authority._read_original_tasks
    def changed_read(*args):
        tasks, train = original_read(*args)
        row = f['rows'][0]
        if change == 'candidate':
            save(Path(row['main_receipt_file']), {'result': {'candidate': '```python\nprint(999)\n```'}})
        elif change in ('span', 'hash'):
            row['candidate_code_span' if change == 'span' else 'candidate_code_sha256'] = {} if change == 'span' else '0' * 64
            f['data']['cases'][0] = row
            save(f['dataset'], p.cohort.seal({key: value for key, value in f['data'].items() if key != 'artifact_hash'}))
        elif change == 'authority':
            f['authority'].write_bytes(b'changed source bytes')
        elif change == 'evidence':
            (f['preparation'] / 'injected.txt').write_bytes(b'new evidence')
        elif change == 'accounting':
            f['snapshot'][0]['new_attempt'] = True
        else:
            tasks[0]['input_output']['outputs'] = ['x' * 48_001, 'y']
        return tasks, train
    monkeypatch.setattr(p.source_authority, '_read_original_tasks', changed_read)
    with pytest.raises(ValueError):
        run(f)


def test_production_loaded_checks_are_not_fixture_substitutes():
    p.loaded_check()


def test_replaced_original_source_reader_cannot_invent_packet(monkeypatch):
    monkeypatch.setattr(p.source_authority, '_read_original_tasks', lambda *args: ({}, None))
    with pytest.raises(ValueError):
        p.loaded_check()
