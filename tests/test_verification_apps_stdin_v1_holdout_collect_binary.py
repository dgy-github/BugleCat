"""New actual EXE, synthetic temporary raw joins only; never live authority."""
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    cohort,
    holdout_collection,
    holdout_intake,
    intake,
)

EXE = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_stdin_v1_holdout_collect.exe'


def run(*args):
    env = {key: value for key, value in os.environ.items() if not any(part in key.upper() for part in ('KEY', 'TOKEN', 'SECRET'))}
    return subprocess.run([str(EXE), *map(str, args)], capture_output=True, env=env, timeout=25)


def test_actual_calibration_compiled_image_and_pure_build_info():
    done = run('build-info')
    assert done.returncode == 0, done.stderr
    value = budget.parse(done.stdout)
    assert value['paid_calls'] == value['http_calls'] == 0
    info = value['build_info']
    assert info['runner_schema'] == 'stdin-holdout-only-v1'
    assert info['executable_sha256'] == cohort.sha(EXE)
    assert all(info[role] == cohort.sha(path) for role, path in holdout_collection.runner_sources().items())
    assert len(holdout_collection.runner_sources()) == 237


@pytest.mark.parametrize('mode,status', [('success', 'ok'), ('http400', 'http_400'), ('failure', 'transport_error'),
    ('malformed', 'response_content_invalid'), ('model_mismatch', 'response_model_or_usage_invalid'),
    ('invalid_usage', 'response_model_or_usage_invalid'), ('timeout', 'timeout'), ('cancel_during', 'cancelled'), ('duplicate', 'ok')])
def test_actual_fake_original_raw_and_sql_are_bound_once(tmp_path, mode, status):
    directory = tmp_path / mode
    done = run('offline-test', directory, mode)
    assert done.returncode == 0, done.stderr
    outcome = budget.parse(done.stdout)
    assert outcome['http_calls'] == outcome['paid_calls'] == 0 and outcome['fake_transport_calls'] == 1
    events = intake.lines(directory / 'attempt-events.jsonl')
    receipt = budget.load(directory / 'receipt.json')
    assert receipt['call_status'] == status and holdout_intake.main_terminal_status(status)
    assert [event['event'] for event in events] == ['attempt_started', 'request_submitted', 'attempt_finished']
    assert receipt['request_hash'] == receipt['actual_request_hash'] == cohort.sha(directory / 'request.json')
    assert all(event['endpoint_sha256'] == receipt['endpoint_sha256'] for event in events)
    assert all(event[key] == receipt[key] for event in events for key in holdout_collection.originals.OPENING_FIELDS)
    assert receipt['build_info']['executable_sha256'] == cohort.sha(EXE)
    assert events[-1]['receipt_sha256'] == cohort.sha(directory / 'receipt.json')
    with sqlite3.connect(next(directory.rglob('main-attempts.sqlite3'))) as db:
        rows = db.execute('SELECT ordinal,started_json,finished_json FROM attempts ORDER BY ordinal').fetchall()
    assert len(rows) == 3 and rows[-1][0] == 3
    assert budget.parse(rows[-1][1]) == {key: value for key, value in events[0].items() if key != 'attempt_ordinal'}
    assert budget.parse(rows[-1][2]) == events[-1]
    if receipt['response_sha256']:
        assert receipt['response_sha256'] == cohort.sha(directory / 'raw-response.json')
    if status == 'http_400':
        assert receipt['http_status'] == 400 and (directory / 'raw-response.json').read_bytes() == b'{"error":"fixture invalid public request"}'
    before = cohort.sha(directory / 'receipt.json')
    assert run('offline-test', directory, mode).returncode != 0
    assert cohort.sha(directory / 'receipt.json') == before


@pytest.mark.parametrize('mode', ['tamper', 'cancel_before'])
def test_no_ordinal_or_transport_before_attempt(tmp_path, mode):
    done = run('offline-test', tmp_path / mode, mode)
    assert done.returncode == 0, done.stderr
    result = budget.parse(done.stdout)
    assert result['fake_transport_calls'] == 0 and result['global_main_attempts'] == 2


def test_actual_fixture_prepare_cannot_grant_calibration_live(tmp_path):
    directory = tmp_path / 'prepared'
    assert run('offline-test', directory, 'prepare').returncode == 0
    rows = intake.lines(directory / 'prepared.jsonl')
    assert len(rows) == 1 and rows[0]['split'] == 'holdout'
    rejected = run('live', directory, '--live')
    assert rejected.returncode != 0
    assert not list(directory.rglob('main-attempts.sqlite3'))
    assert not (directory / 'attempt-events.jsonl').exists()


def test_offline_root_must_be_a_fresh_temporary_child(tmp_path):
    outside = Path(__file__).resolve().parents[1] / 'reports/not-a-holdout-fixture'
    assert run('offline-test', outside, 'success').returncode != 0
    assert not outside.exists()
