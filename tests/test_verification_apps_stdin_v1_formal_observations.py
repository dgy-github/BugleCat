"""Strict numeric and actual new main binary receipts; network forbidden."""
import json
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    protocol,
)
from evals.verification.apps_stdin_v1 import (
    formal_jev as sender,
)
from evals.verification.apps_stdin_v1 import (
    formal_observations as observer,
)

EXE = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_stdin_v1_jev_formal.exe'


def response():
    return {'model': 'jev-1.13.0', 'answers': {'acceptance_violation': {'type': 'noul', 'noul': .1}},
            'usage': {'input_tokens': 12, 'output_tokens': 3}}


@pytest.mark.parametrize('raw', [b'{"model":NaN}', b'{"model":Infinity}', b'{"model":"jev-1.13.0","extra":1e400}',
                               b'{"model":"a","model":"jev-1.13.0"}', b'{"extra":{"n":1,"n":2}}'])
def test_ambiguous_nonfinite_json_never_numeric(raw):
    assert observer.parsed_status(raw) == ('invalid_json', None)


@pytest.mark.parametrize('value,status', [
    ({'model': 'other'}, 'actual_model_mismatch'),
    ({'model': 'jev-1.13.0', 'answers': {}}, 'exact_answers_required'),
    ({'model': 'jev-1.13.0', 'answers': {'acceptance_violation': {'type': 'noul', 'noul': True}}}, 'invalid_noul')])
def test_numeric_shape(value, status):
    assert observer.parsed_status(json.dumps(value).encode())[0] == status


@pytest.mark.parametrize('count', [True, -1, 1.0, '1', 2 ** 64])
def test_exact_unsigned_usage(count):
    value = response()
    value['usage']['input_tokens'] = count
    assert observer.parsed_status(json.dumps(value).encode())[0] == 'invalid_usage'


def offline(tmp_path, *, case='success', raw=None):
    root = tmp_path / 'synthetic'
    command = [str(EXE), 'offline', '--output', str(root), '--case', case]
    if raw is not None:
        file = tmp_path / 'synthetic-response.json'
        file.write_bytes(raw)
        command += ['--response-file', str(file)]
    done = subprocess.run(command, capture_output=True, timeout=25)
    assert done.returncode == 0, done.stderr.decode()
    assert json.loads(done.stdout)['new_paid_calls'] == 0
    row = budget.load(root / 'fixture-prepared.json')
    reservation = budget.load(root / 'fixture-reservation.json')
    return root, row, reservation, json.loads(done.stdout)


@pytest.mark.parametrize('case,status', [('success', 'ok'), ('http400', 'http_400'), ('timeout', 'timeout'),
                                       ('transport_error', 'transport_error'), ('response_budget', 'response_budget'),
                                       ('cancelled', 'cancelled'), ('cancel_after_submit', 'cancelled')])
def test_actual_binary_zero_http_original_join(tmp_path, case, status):
    root, row, reserved, summary = offline(tmp_path, case=case)
    original = observer.join(row, root / 'fixture-prepared.json', reserved, root / 'attempt', EXE, fixture=True)
    assert original['call_status'] == status == summary['call_status']
    assert summary['verified_pass_signals'] == 0
    assert (root / 'fixture-reservation.started').exists()
    if status == 'http_400':
        assert b'fixture-private-provider-fault' in (root / 'attempt/raw-response.json').read_bytes()
    if status == 'ok':
        cases = [{'id': row['id'], 'group_id': row['group_id'], 'source_kind': 'apps_stdin_restricted_script',
                  'label': 'supported', 'candidate_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256']}]
        protocol._verified(cases, [original], protocol.specification())
    again = subprocess.run([str(EXE), 'offline', '--output', str(root)], capture_output=True, timeout=25)
    assert again.returncode != 0  # exclusive original output/marker never overwritten


@pytest.mark.parametrize('raw,status', [
    (b'{"x":NaN}', 'invalid_json'), (b'{"x":Infinity}', 'invalid_json'), (b'{"x":1e400}', 'invalid_json'),
    (b'{"model":"x","model":"jev-1.13.0"}', 'invalid_json'),
    (json.dumps({**response(), 'model': 'jev-other'}).encode(), 'actual_model_mismatch'),
    (json.dumps({**response(), 'usage': {'input_tokens': 1.0, 'output_tokens': 2}}).encode(), 'invalid_usage'),
    (json.dumps({**response(), 'answers': {}}).encode(), 'exact_answers_required')])
def test_actual_binary_and_python_failure_status_agree(tmp_path, raw, status):
    root, row, reserved, _ = offline(tmp_path, raw=raw)
    actual = observer.join(row, root / 'fixture-prepared.json', reserved, root / 'attempt', EXE, fixture=True)
    assert actual['call_status'] == status and actual['acceptance_violation'] is None


@pytest.mark.parametrize('filename', ['request.json', 'raw-response.json', 'receipt.json', 'events.jsonl'])
def test_original_bytes_tamper_rejected(tmp_path, filename):
    root, row, reserved, _ = offline(tmp_path)
    file = root / 'attempt' / filename
    file.write_bytes(file.read_bytes() + b' ')
    with pytest.raises(ValueError):
        observer.join(row, root / 'fixture-prepared.json', reserved, root / 'attempt', EXE, fixture=True)


def test_actual_live_fixture_rejected_before_claim_or_client(tmp_path):
    root, _, _, _ = offline(tmp_path)
    before = sorted((str(path), path.read_bytes()) for path in root.rglob('*') if path.is_file())
    done = subprocess.run([str(EXE), 'send', '--live', '--prepared', str(root / 'fixture-prepared.json'),
                           '--reservation', str(root / 'fixture-reservation.json')], capture_output=True, timeout=25)
    assert done.returncode != 0 and b'original_calibration_request_contract' in done.stderr
    assert before == sorted((str(path), path.read_bytes()) for path in root.rglob('*') if path.is_file())


def test_exact_original_exponents_unicode_send_bytes(tmp_path):
    root, row, _, _ = offline(tmp_path)
    raw = (root / 'attempt/request.json').read_bytes()
    assert raw == row['request_json'].encode()
    assert b'1e-07,1.0,1' in raw and '你好'.encode() in raw
    assert sender.build_info(EXE)['runner_exe_sha256'] == sender.cohort.sha(EXE)
