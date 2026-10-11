"""Authenticate original failed receipts too; no silent fee/history reset."""
from copy import deepcopy

import pytest

from evals.verification.apps_campaign_v3 import development
from evals.verification.apps_campaign_v3 import development_intake as intake
from evals.verification.apps_v3 import oracle


def seal(value):
    return {**value, 'artifact_sha256': oracle.digest(value)}


def fixture(tmp_path):
    fields = (*development.BIND_FIELDS, 'id', 'group_id', 'task_id', 'source_kind', 'split', 'task_sha256',
              'manifest_sha256', 'artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
              'request_hash', 'settings', 'build_info', 'source_sha256', 'catalog_sha256',
              'protocol_sha256', 'authorization_sha256', 'case_directory', 'workspace', 'session_log')
    row = dict.fromkeys(fields, 'original')
    row.update(id='one', group_id='group-one', task_id=1, split='development',
               settings={'max_retries': 0}, build_info={'executable_sha256': 'a' * 64})
    start = {**row, 'event': 'attempt_started', 'attempt_ordinal': 1}
    submitted = {**start, 'event': 'request_submitted', 'actual_request_hash': row['request_hash']}
    receipt = seal({**row, 'schema': 'buglecat.verification.collected-main.v1', 'call_status': 'timeout',
                    'attempt_ordinal': 1, 'actual_request_hash': row['request_hash']})
    path = tmp_path / 'receipt.json'
    import json
    path.write_text(json.dumps(receipt), encoding='utf-8')
    finish = {**submitted, 'event': 'attempt_finished', 'call_status': 'timeout',
              'receipt_sha256': intake.source_authority.source.file_sha(path)}
    original = {'ordinal': 1, 'group_id': row['group_id'],
                'started': {key: value for key, value in start.items() if key != 'attempt_ordinal'}}
    return row, original, finish, [start, submitted, finish], receipt


def test_original_transport_failure_has_a_terminal_join(tmp_path):
    row, started, finished, events, _ = fixture(tmp_path)
    receipt, status = intake.terminal_join(row, started, finished, events, tmp_path)
    assert status == 'timeout' and receipt['attempt_ordinal'] == 1
    assert 'result' not in receipt


def test_unstarted_and_pending_are_distinct(tmp_path):
    row = {'id': 'one'}
    assert intake.terminal_join(row, None, None, [], tmp_path) == (None, 'main_not_started')
    event = {'id': 'one', 'event': 'attempt_started', 'attempt_ordinal': 1}
    started = {'ordinal': 1, 'started': {'id': 'one', 'event': 'attempt_started'}}
    assert intake.terminal_join(row, started, None, [event], tmp_path) == (None, 'main_not_finished')


@pytest.mark.parametrize('change', ['event_order', 'attempt_bool', 'source_kind', 'ordinal', 'body', 'receipt_bytes'])
def test_tampered_original_failure_not_accepted(tmp_path, change):
    row, started, finished, events, _ = deepcopy(fixture(tmp_path))
    if change == 'event_order':
        events[1]['event'] = 'attempt_started'
    elif change == 'attempt_bool':
        events[0]['attempt_ordinal'] = True
    elif change == 'source_kind':
        events[1]['source_kind'] = 'other-kind'
    elif change == 'ordinal':
        started['ordinal'] = 2
    elif change == 'body':
        events[1]['actual_request_hash'] = 'other-body'
    else:
        with (tmp_path / 'receipt.json').open('ab') as stream:
            stream.write(b'\n')
    with pytest.raises(ValueError):
        intake.terminal_join(row, started, finished, events, tmp_path)


def test_receipt_without_original_start_rejected(tmp_path):
    row, _, _, _, _ = fixture(tmp_path)
    with pytest.raises(ValueError, match='proof_without_original_main_start'):
        intake.terminal_join(row, None, None, [], tmp_path)
