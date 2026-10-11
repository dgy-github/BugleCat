"""Actual new binary, isolated fixture roots; no HTTP/config/shared ledger."""
import json
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import cohort, intake

EXE = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_stdin_v1_collect.exe'


def run(*args):
    env = {key: value for key, value in os.environ.items() if not any(part in key.upper() for part in ('KEY', 'TOKEN', 'SECRET'))}
    return subprocess.run([str(EXE), *map(str, args)], capture_output=True, env=env, timeout=20)


def test_actual_build_all_explicit_dependency_inputs():
    child = run('build-info')
    assert child.returncode == 0, child.stderr
    value = json.loads(child.stdout)
    assert value['http_calls'] == value['paid_calls'] == 0
    info = value['build_info']
    assert info['executable_sha256'] == cohort.sha(EXE)
    assert all(info[role] == cohort.sha(path) for role, path in cohort.owners().items())
    assert len(cohort.owners()) >= 180


@pytest.mark.parametrize('mode,status', [('success', 'ok'), ('failure', 'transport_error'),
                                       ('http400', 'http_400'),
                                       ('malformed', 'response_content_invalid'), ('model_mismatch', 'response_model_or_usage_invalid'),
                                       ('invalid_usage', 'response_model_or_usage_invalid'), ('timeout', 'timeout'),
                                       ('cancel_during', 'cancelled'), ('duplicate', 'ok')])
def test_actual_fake_terminal_original_sql_and_events(tmp_path, mode, status):
    directory = tmp_path / mode
    child = run('offline-test', directory, mode)
    assert child.returncode == 0, child.stderr
    value = json.loads(child.stdout)
    assert value['http_calls'] == value['paid_calls'] == 0
    assert value['fake_transport_calls'] == 1
    assert intake.main_terminal_status(status)
    events = intake.lines(directory / 'attempt-events.jsonl')
    receipt = json.loads((directory / 'receipt.json').read_bytes())
    assert receipt['call_status'] == status
    original_receipt = (directory / 'receipt.json').read_bytes()
    intake.receipt_seal(receipt)
    assert events[-1]['receipt_sha256'] == cohort.sha(directory / 'receipt.json')
    assert [event['event'] for event in events] == ['attempt_started', 'request_submitted', 'attempt_finished']
    assert events[1]['actual_request_hash'] == receipt['actual_request_hash'] == cohort.sha(directory / 'request.json')
    assert receipt['build_info']['executable_sha256'] == cohort.sha(EXE)
    assert all(event['endpoint_sha256'] == receipt['endpoint_sha256'] for event in events)
    if status == 'http_400':
        assert receipt['http_status'] == events[-1]['http_status'] == 400
        assert receipt['response_sha256'] == cohort.sha(directory / 'raw-response.json')
        assert (directory / 'raw-response.json').read_bytes() == b'{"error":"fixture invalid public request"}'
    elif receipt['response_sha256']:
        assert receipt['http_status'] == events[-1]['http_status'] == 200
    else:
        assert receipt['http_status'] is None and not (directory / 'raw-response.json').exists()
    databases = list(directory.rglob('main-attempts.sqlite3'))
    assert len(databases) == 1
    with sqlite3.connect(databases[0]) as db:
        rows = db.execute('SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal').fetchall()
    assert len(rows) == 3 and rows[-1][0] == 3
    assert json.loads(rows[-1][2]) == {key: value for key, value in events[0].items() if key != 'attempt_ordinal'}
    assert json.loads(rows[-1][3]) == events[-1]
    if status == 'ok':
        assert (directory / 'candidate.txt').read_bytes() == receipt['result']['candidate'].encode()
        assert intake.original_code(receipt['result']['candidate'])['code'] == 'print(input())\n'
    second = run('offline-test', directory, mode)
    assert second.returncode != 0
    assert (directory / 'receipt.json').read_bytes() == original_receipt


@pytest.mark.parametrize('mode', ['cancel_before', 'tamper'])
def test_no_fake_call_or_ordinal_before_attempt(tmp_path, mode):
    child = run('offline-test', tmp_path / mode, mode)
    assert child.returncode == 0, child.stderr
    value = json.loads(child.stdout)
    assert value['fake_transport_calls'] == value['paid_calls'] == value['http_calls'] == 0
    assert value['global_main_attempts'] == 2


def test_actual_shared_prepare_is_not_genuine_live(tmp_path):
    directory = tmp_path / 'prepared-fixture'
    child = run('offline-test', directory, 'prepare')
    assert child.returncode == 0, child.stderr
    value = json.loads(child.stdout)
    assert value['status'] == 'prepared' and value['paid_attempts'] == value['http_calls'] == 0
    row = intake.lines(directory / 'prepared.jsonl')[0]
    assert row['request_hash'] == cohort.sha(directory / 'offline/request.json')
    assert row['body']['messages'][1]['content'] == 'Echo one line; fixture only'
    assert row['build_info']['executable_sha256'] == cohort.sha(EXE)
    # Only genuine Python funded-source validation can reach live; fixture schema cannot.
    child = run('live', directory, '--live')
    assert child.returncode != 0
    assert not (directory / 'attempt-events.jsonl').exists()
    assert not (directory / 'global-ledger').exists()
