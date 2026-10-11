"""Actual copied holdout collector fixtures, never source opening or fees."""
import json
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import holdout_collection as owner
from evals.verification.apps_campaign_v3 import holdout_intake as reader
from evals.verification.calibration import create
from evals.verification.collection_manifest import file_hash
from evals.verification.collection_receipts import verify

BINARY = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_v3_holdout_collect.exe'

def binary(args):
    if not BINARY.exists():
        pytest.fail('build the new example actual entry first')
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    return subprocess.run([str(BINARY), *args], capture_output=True, text=True, env=env, timeout=20)


def test_actual_binary_build_info_proves_all_new_and_legacy_sources():
    result = reader.build(BINARY)
    assert result['executable_sha256'] == file_hash(BINARY)
    assert all(result[role] == file_hash(path) for role, path in reader.runner_sources().items())


@pytest.mark.parametrize('scenario,status,count', [('success', 'ok', 511), ('duplicate', 'ok', 511),
                                                 ('failure', 'offline_transport_failure', 511), ('timeout', 'timeout', 511),
                                                 ('malformed', 'response_content_invalid', 511),
                                                 ('model_mismatch', 'response_model_or_usage_invalid', 511),
                                                 ('invalid_usage', 'response_model_or_usage_invalid', 511),
                                                 ('cancel_during', 'cancelled', 511), ('cancel_before', None, 510), ('tamper', None, 510)])
def test_actual_fake_binary_receipt_sqlite_raw_session_sources_cancel_no_real_budget(tmp_path, scenario, status, count):
    directory = tmp_path / scenario
    child = binary(['offline-test', str(directory), scenario])
    assert child.returncode == 0, child.stderr
    result = json.loads(child.stdout)
    assert result['paid_calls'] == result['http_calls'] == 0 and result['global_main_attempts'] == count
    with sqlite3.connect(directory / 'offline-ledger/main-attempts.sqlite3') as db:
        assert db.execute('select count(*) from attempts').fetchone()[0] == count
    if status:
        receipt = json.loads((directory / 'receipt.json').read_bytes())
        events = [json.loads(line) for line in (directory / 'attempt-events.jsonl').read_text().splitlines()]
        assert receipt['call_status'] == status and events[-1]['receipt_sha256'] == file_hash(directory / 'receipt.json')
        assert events[1]['actual_request_hash'] == receipt['actual_request_hash'] == file_hash(directory / 'request.json')
        if status == 'ok':
            row = json.loads((directory / 'offline-prepared.json').read_bytes())
            assert verify(row, directory / 'receipt.json', events, BINARY) == receipt
    else:
        assert result['fake_transport_calls'] == 0


def test_actual_binary_no_implicit_live_holdout_rejected_before_config_budget(tmp_path):
    assert binary(['live', str(tmp_path)]).returncode != 0
    path = tmp_path / 'manifest.json'
    create(path, {'schema': owner.MANIFEST, 'split': 'holdout', 'artifact_cohort_file': str(path)})
    child = binary(['prepare', str(path), str(tmp_path / 'must-not-create')])
    assert child.returncode != 0 and not (tmp_path / 'must-not-create').exists()


@pytest.mark.parametrize('scenario', ['manifest', 'cohort', 'source', 'holdout', 'marker', 'proof'])
def test_actual_binary_rehash_original_marker_and_calibration_proof_before_any_attempt(tmp_path, scenario):
    result = binary(['offline-local-source-test', str(tmp_path / scenario), scenario])
    assert result.returncode == 0, result.stderr
    value = json.loads(result.stdout)
    assert value['scenario'] == scenario
    assert value['paid_calls'] == value['http_calls'] == value['budget_writes'] == 0
    assert not (tmp_path / scenario / 'main-attempts.sqlite3').exists()
