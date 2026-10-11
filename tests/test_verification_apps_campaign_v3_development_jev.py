"""Original APPS joins and actual new binary fixtures; never use a live key."""
import copy
import json
import os
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import development_jev as owner
from evals.verification.apps_campaign_v3 import packet
from evals.verification.calibration import create, digest
from evals.verification.collection_manifest import file_hash

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'rust/target/debug/examples/verification_apps_v3_jev_development.exe'


def write(path, value):
    path.write_bytes(packet.canonical_bytes(value))


@pytest.fixture
def collection(tmp_path, monkeypatch):
    directory = tmp_path / 'main'
    directory.mkdir()
    exe = tmp_path / 'main.exe'
    exe.write_bytes(b'fixture')
    (directory / 'attempt-events.jsonl').write_bytes(b'original-events')
    cohort = {'cases': [{'task_id': i} for i in range(3)]}
    manifest = {'artifact_cohort_file': 'cohort', 'artifact_cohort_sha256': 'cohort-sha',
                'artifact_cohort_hash': 'cohort-hash', 'authorization_file': 'auth',
                'authorization_sha256': 'auth-sha', 'protocol_file': 'protocol', 'protocol_sha256': 'protocol-sha',
                'source_authority_file': 'authority', 'resource_inventory_file': 'resources'}
    tasks = {i: {'question': f'Original question {i}', 'starter_code': '', 'source_kind': packet.FREE,
                 'input_output': {'fn_name': 'f', 'inputs': [[1e-7], [1.0], [1]], 'outputs': [1e-7, 1.0, 1]}}
             for i in range(3)}
    cases = []
    for i, label in enumerate(['supported', 'unsupported', 'unknown']):
        receipt = tmp_path / f'receipt-{i}.json'
        write(receipt, {'result': {'candidate': '```python\ndef f(x):\n    return x\n```'}})
        cases.append({'id': f'case-{i}', 'group_id': f'group-{i}', 'task_id': i,
                      'source_kind': packet.FREE, 'main_status': 'ok', 'call_eligible': label != 'unknown',
                      'label': label, 'oracle_file': 'oracle', 'oracle_sha256': 'oracle-sha',
                      'main_receipt_file': str(receipt),
                      'candidate_code_sha256': owner.intake.original_code(json.loads(receipt.read_bytes())['result']['candidate'])['code_sha256']})
    data = {'split': 'development', 'formal_registration': False, 'cases': cases,
            'artifact_hash': 'data-hash', 'main_manifest_sha256': 'manifest-sha', 'prepared_sha256': 'prepared-sha'}
    monkeypatch.setattr(owner.development_intake, 'collect', lambda *a, **kw: copy.deepcopy(data))
    monkeypatch.setattr(owner.development, 'validate_manifest', lambda p: manifest)
    monkeypatch.setattr(owner.development, 'validate_cohort', lambda p: cohort)
    monkeypatch.setattr(owner.development, 'inputs', lambda *a: ({}, {}))
    monkeypatch.setattr(owner.development, 'original_tasks', lambda *a: (tasks, None))
    monkeypatch.setattr(owner, 'owner_files', lambda: {'fixture': exe})
    return directory, exe, data


def test_whole_allocation_retains_unknown_and_only_original_binary_request(collection):
    directory, exe, _ = collection
    result = owner.prepare_collection(directory, exe)
    assert result['allocated_groups'] == 3 and result['eligible_groups'] == 2
    assert result['label_counts'] == {'supported': 1, 'unsupported': 1, 'unknown': 1}
    assert result['cases'][2]['request'] is None
    body = result['cases'][0]['request']
    assert body['state']['input_output']['inputs'] == [[1e-7], [1.0], [1]]
    assert not any(k in body['state'] for k in ('label', 'oracle', 'reference', 'main_receipt_file'))
    assert result['new_paid_calls'] == 0 and result['formal_certification'] is False


@pytest.mark.parametrize('statuses', [['ok', 'main_not_finished', 'main_not_started'],
                                     ['ok', 'main_not_started', 'main_not_started'],
                                     ['main_not_started'] * 3,
                                     ['main_not_started', 'ok', 'ok']])
def test_inflight_unclosed_or_out_of_order_prefix_refused(collection, statuses):
    directory, exe, data = collection
    for row, status in zip(data['cases'], statuses, strict=True):
        row.update(main_status=status, call_eligible=status == 'ok', label='supported' if status == 'ok' else 'unlabeled')
    with pytest.raises(ValueError):
        owner.prepare_collection(directory, exe)


def test_first_fault_terminal_retains_unstarted_not_unknown(collection):
    directory, exe, data = collection
    data['cases'][1].update(main_status='transport_error', call_eligible=False, label='unknown')
    data['cases'][2].update(main_status='main_not_started', call_eligible=False, label='unlabeled')
    value = owner.prepare_collection(directory, exe)
    assert value['label_counts'] == {'supported': 1, 'unknown': 1, 'unlabeled': 1}
    assert value['eligible_groups'] == 1 and value['allocated_groups'] == 3


def test_original_exact_request_json_and_resealed_change_rejected(collection, tmp_path):
    directory, exe, _ = collection
    allocation = tmp_path / 'allocation.json'
    create(allocation, owner.prepare_collection(directory, exe))
    value = owner.prepared_value(allocation, 'case-0')
    assert '1e-07' in value['request_json'] and '1.0' in value['request_json']
    prepared = tmp_path / 'prepared.json'
    create(prepared, value)
    assert owner.validate(prepared) == value
    value['request_json'] = value['request_json'].replace('1e-07', '1e-7')
    write(prepared, {**{k: v for k, v in value.items() if k != 'artifact_hash'},
                     'artifact_hash': digest({k: v for k, v in value.items() if k != 'artifact_hash'})})
    with pytest.raises(ValueError, match='original request differs'):
        owner.validate(prepared)
    with pytest.raises(ValueError, match='eligible'):
        owner.prepared_value(allocation, 'case-2')


@pytest.fixture
def attempt(tmp_path, monkeypatch):
    root = tmp_path / 'ledger'
    directory = root / 'attempts/one'
    directory.mkdir(parents=True)
    (root / 'reservations').mkdir()
    exe = tmp_path / 'jev.exe'
    exe.write_bytes(b'actual-fixture')
    prepared = tmp_path / 'prepared.json'
    prepared.write_bytes(b'prepared')
    body = packet.request('Return x', '', 'f', 'def f(x):\n    return x\n',
                          {'fn_name': 'f', 'inputs': [[1e-7]], 'outputs': [1e-7]}, packet.FREE)
    value = {'id': 'case', 'group_id': 'group', 'request_hash': digest(body), 'request': body,
             'request_json': packet.canonical_bytes(body).decode(), 'authorization_file': 'auth',
             'allocation_hash': 'allocation', 'allocation_sha256': 'allocation-sha',
             'artifact_cohort_hash': 'cohort', 'protocol_sha256': 'protocol', 'source_kind': packet.FREE}
    monkeypatch.setattr(owner, 'validate', lambda p: value)
    compiled = {'runner_exe_sha256': file_hash(exe), **{k: file_hash(p) for k, p in owner.runner_sources().items()}}
    monkeypatch.setattr(owner, 'build_info', lambda p: compiled)
    reservation = root / 'reservations' / (digest('group') + '.json')
    write(reservation, {})
    proof = {'ledger_root': str(root), 'reservation_file': str(reservation),
             'reservation_sha256': file_hash(reservation), 'marker_file': str(reservation.with_suffix('.started'))}
    monkeypatch.setattr(owner, 'validate_reservation', lambda *a: proof)
    common = {'id': 'case', 'group_id': 'group', 'request_hash': digest(body),
              'prepared_sha256': file_hash(prepared), 'template_version': packet.TEMPLATE,
              **{k: value[k] for k in ('allocation_hash', 'allocation_sha256', 'artifact_cohort_hash', 'protocol_sha256', 'source_kind')},
              'automatic_retries': 0, 'formal_certification': False, 'offline': False,
              'attempt_directory': str(directory), 'raw_response_file': str(directory / 'raw-response.json'),
              'reservation_file': str(reservation), 'reservation_sha256': file_hash(reservation), **compiled}
    write(directory / 'request.json', body)
    raw = {'model': packet.MODEL, 'answers': {'acceptance_violation': {'type': 'noul', 'noul': .12}},
           'usage': {'input_tokens': 12, 'output_tokens': 2}}
    write(directory / 'raw-response.json', raw)
    events = [{**common, 'status': 'attempt_started'},
              {**common, 'status': 'request_submitted', 'actual_request_hash': digest(body)},
              {**common, 'status': 'attempt_finished', 'submitted': True, 'call_status': 'ok',
               'response_sha256': file_hash(directory / 'raw-response.json')}]

    def save():
        (directory / 'events.jsonl').write_bytes(b''.join(packet.canonical_bytes(e) + b'\n' for e in events))
        write(directory / 'receipt.json', events[-1])
        write(reservation.with_suffix('.started'), events[0])
    save()
    return prepared, directory, exe, events, save


def test_original_bytes_signal_never_pass_or_certification(attempt):
    p, d, exe, _, _ = attempt
    result = owner.observe(p, d, exe)
    assert result['acceptance_violation'] == .12
    assert result['verified_pass_signals'] == 0 and result['formal_certification'] is False
    assert result['certified_threshold'] is None and result['production_authorized'] is False


@pytest.mark.parametrize('field', ['actual_request_hash', 'runner_source_sha256', 'runner_exe_sha256',
                                 'reservation_sha256', 'allocation_hash', 'protocol_sha256', 'source_kind'])
def test_event_provenance_tamper_rejected(attempt, field):
    p, d, exe, events, save = attempt
    events[1][field] = 'changed'
    save()
    with pytest.raises(ValueError):
        owner.observe(p, d, exe)


@pytest.mark.parametrize('filename', ['request.json', 'raw-response.json', 'receipt.json'])
def test_raw_original_bytes_tamper_rejected(attempt, filename):
    p, d, exe, _, _ = attempt
    (d / filename).write_bytes(b'changed')
    with pytest.raises((ValueError, json.JSONDecodeError)):
        owner.observe(p, d, exe)


@pytest.mark.parametrize('status,allowed', [('missing_key', True), ('cancelled', True), ('timeout', False), ('ok', False)])
def test_unsubmitted_permanent_claim_only_cancel_missing_key(attempt, status, allowed):
    p, d, exe, events, save = attempt
    events.pop(1)
    events[-1].update(submitted=False, call_status=status, response_sha256=None)
    (d / 'raw-response.json').unlink()
    save()
    if allowed:
        assert owner.observe(p, d, exe)['acceptance_violation'] is None
    else:
        with pytest.raises(ValueError):
            owner.observe(p, d, exe)


def test_failure_cannot_hide_valid_original_response(attempt):
    p, d, exe, events, save = attempt
    events[-1]['call_status'] = 'invalid_usage'
    save()
    with pytest.raises(ValueError, match='parsed outcome'):
        owner.observe(p, d, exe)


def test_existing_raw_parse_failure_authenticates_bytes_and_status(attempt):
    p, d, exe, events, save = attempt
    raw = json.loads((d / 'raw-response.json').read_bytes())
    raw['usage']['output_tokens'] = -1
    write(d / 'raw-response.json', raw)
    events[-1].update(call_status='invalid_usage', response_sha256=file_hash(d / 'raw-response.json'))
    save()
    assert owner.observe(p, d, exe)['acceptance_violation'] is None
    events[-1]['call_status'] = 'invalid_noul'
    save()
    with pytest.raises(ValueError, match='parsed outcome'):
        owner.observe(p, d, exe)


@pytest.mark.parametrize('change', ['path', 'group', 'request', 'authorization'])
def test_shared_original_reservation_identity_not_resettable(tmp_path, monkeypatch, change):
    root = tmp_path / 'ledger'
    (root / 'reservations').mkdir(parents=True)
    value = {'group_id': 'original-group', 'request_hash': 'original-request',
             'authorization_file': 'auth', 'authorization_sha256': 'original-chain'}
    path = root / 'reservations' / (digest('original-group') + '.json')
    path.write_text('{}')
    record = {'group_id': 'original-group', 'request_hash': 'original-request'}
    chain = {'current_sha256': 'original-chain', 'authorization': {'jev': {'ledger_dir': str(root)}}}
    monkeypatch.setattr(owner, 'validate', lambda p: value)
    monkeypatch.setattr(owner, 'resolve', lambda p: chain)
    monkeypatch.setattr(owner, 'reservations', lambda *a: ([record], None))
    assert owner.validate_reservation('prepared', path)['marker_file'] == str(path.with_suffix('.started'))
    if change == 'path':
        path = root / 'reservations/other.json'
        path.write_text('{}')
    elif change == 'authorization':
        chain['current_sha256'] = 'different-authorization'
    else:
        record['group_id' if change == 'group' else 'request_hash'] = 'changed'
    with pytest.raises(ValueError):
        owner.validate_reservation('prepared', path)


def test_build_info_child_env_strips_credentials_without_mutating_parent(monkeypatch, tmp_path):
    exe = tmp_path / 'fixture.exe'
    exe.write_bytes(b'fixture')
    monkeypatch.setenv('DEEPSEEK_API_KEY', 'synthetic-fixture')
    monkeypatch.setenv('OTHER_SECRET', 'synthetic-fixture')
    proof = {'runner_exe_sha256': file_hash(exe), **{k: file_hash(p) for k, p in owner.runner_sources().items()},
             'new_paid_calls': 0, 'network_calls': 0, 'formal_certification': False, 'template_version': packet.TEMPLATE}

    def run(argv, **kw):
        assert argv == [str(exe.resolve()), 'build-info']
        assert all(not any(t in k.upper() for t in ('KEY', 'TOKEN', 'SECRET')) for k in kw['env'])
        assert {k.upper(): v for k, v in kw['env'].items()}['SYSTEMROOT'] == os.environ['SystemRoot']
        return subprocess.CompletedProcess(argv, 0, json.dumps(proof).encode(), b'')
    monkeypatch.setattr(owner.subprocess, 'run', run)
    assert owner.build_info(exe) == proof
    assert os.environ['DEEPSEEK_API_KEY'] == 'synthetic-fixture'


@pytest.mark.parametrize('changed', ['1e-07', '1.0'])
def test_equal_numeric_meaning_still_distinct_original_byte_hash(attempt, changed):
    p, d, exe, _, _ = attempt
    raw = (d / 'request.json').read_bytes()
    if changed == '1.0':
        # A synthetic added IO cannot survive the original canonical binding.
        raw = raw.replace(b'1e-07', b'1.0')
    else:
        raw = raw.replace(b'1e-07', b'1e-7')
    (d / 'request.json').write_bytes(raw)
    with pytest.raises(ValueError, match='actual request bytes'):
        owner.observe(p, d, exe)


@pytest.mark.parametrize('scenario,status,calls', [
    ('success', 'ok', 1), ('missing_answer', 'exact_answers_required', 1), ('extra_answer', 'exact_answers_required', 1),
    ('model_mismatch', 'actual_model_mismatch', 1), ('invalid_usage', 'invalid_usage', 1),
    ('missing_usage', 'invalid_usage', 1), ('boolean_probability', 'invalid_noul', 1),
    ('malformed', 'invalid_json', 1), ('oversize', 'response_budget', 1),
    ('nan_extra', 'invalid_json', 1), ('infinity_extra', 'invalid_json', 1),
    ('overflow_extra', 'invalid_json', 1), ('deep_extra', 'invalid_json', 1),
    ('http401', 'http_401', 1), ('http403', 'http_403', 1), ('http429', 'http_429', 1),
    ('timeout', 'timeout', 1), ('cancel_before', 'cancelled', 0), ('cancel_during', 'cancelled', 1),
    ('missing_key', 'missing_key', 0)])
def test_actual_new_binary_fake_receipts_exact_python_bytes(tmp_path, scenario, status, calls):
    assert BINARY.exists(), 'build only the new example before this explicit binary gate'
    env = {k: v for k, v in os.environ.items() if not any(t in k.upper() for t in ('KEY', 'TOKEN', 'SECRET'))}
    directory = tmp_path / scenario
    run = subprocess.run([str(BINARY), 'offline-transport-test', str(directory), scenario],
                         env=env, capture_output=True, timeout=15, check=False)
    assert run.returncode == 0, run.stderr.decode()
    result = json.loads(run.stdout)
    assert result['call_status'] == status and result['fake_transport_calls'] == calls
    assert result['new_paid_calls'] == result['network_calls'] == 0
    assert result['duplicate_reservation_blocked'] and result['exact_request_bytes_preserved']
    raw = (directory / 'attempt/request.json').read_bytes()
    assert b'1e-07,1.0,1' in raw and '汉字'.encode() in raw and b'line\\n\\"quoted\\"\\\\end' in raw
    # serde's parse/serialize would preserve numeric meaning but change bytes.
    assert file_hash(directory / 'attempt/request.json') == result['request_hash']
    assert json.loads((directory / 'fixture.started').read_bytes())['status'] == 'attempt_started'
    events = [json.loads(line) for line in (directory / 'attempt/events.jsonl').read_bytes().splitlines()]
    assert events[-1] == json.loads((directory / 'attempt/receipt.json').read_bytes())
    assert len(events) == (3 if calls else 2)
    raw_response = directory / 'attempt/raw-response.json'
    if raw_response.exists():
        assert owner.parsed_status(raw_response.read_bytes(), json.loads(raw)) == status


def test_actual_build_proof_pure_and_no_implicit_live(tmp_path):
    proof = owner.build_info(BINARY)
    assert proof['new_paid_calls'] == proof['network_calls'] == 0
    run = subprocess.run([str(BINARY), 'live', 'missing', 'missing', str(tmp_path / 'attempt'), 'cancel'],
                         capture_output=True, check=False)
    assert run.returncode != 0 and not (tmp_path / 'attempt').exists()
