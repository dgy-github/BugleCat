"""Isolated registered packets/reservations and actual no-network sender tests."""
import copy
import json
import os
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import formal_jev as owner
from evals.verification.apps_campaign_v3 import packet
from evals.verification.calibration import create, digest
from evals.verification.collection_manifest import file_hash

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'rust/target/debug/examples/verification_apps_v3_jev_formal.exe'


def write(path, value):
    path.write_bytes(packet.canonical_bytes(value))


@pytest.fixture
def registered(tmp_path, monkeypatch):
    main = tmp_path / 'main'
    main.mkdir()
    regfile = tmp_path / 'registration.json'
    write(regfile, {'original': True})
    authfile = tmp_path / 'authorization.json'
    write(authfile, {'fixture': 'approved'})
    tasks, cases = {}, []
    for i, label in enumerate(['supported', 'unsupported', 'unknown']):
        receipt = main / f'receipt-{i}.json'
        write(receipt, {'result': {'candidate': '```python\ndef f(x):\n    return x\n```'}})
        task = {'question': f'Original question {i}', 'starter_code': '', 'source_kind': packet.FREE,
                'input_output': {'fn_name': 'f', 'inputs': [[1e-7], [1.0], [1]], 'outputs': [1e-7, 1.0, 1]}}
        tasks[i] = task
        cases.append({'id': f'case-{i}', 'group_id': f'group-{i}', 'task_id': i,
                      'source_kind': packet.FREE, 'main_status': 'ok', 'call_eligible': label != 'unknown',
                      'label': label, 'oracle_file': 'original', 'oracle_sha256': 'a' * 64,
                      'main_receipt_file': str(receipt),
                      'candidate_code_sha256': owner.intake.original_code(load(receipt)['result']['candidate'])['code_sha256']})
    data = {'cases': cases, 'artifact_hash': 'd' * 64, 'cohort_file': 'cohort', 'cohort_sha256': 'c' * 64,
            'protocol_file': 'protocol', 'protocol_sha256': 'p' * 64}
    reg = {'artifact_hash': 'r' * 64, 'authorization_file': str(authfile), 'authorization_sha256': file_hash(authfile),
           'source_bindings': [], 'original_evidence_bindings': [], 'eligible_ids': ['case-0', 'case-1']}
    state = {'reg': reg, 'data': data, 'tasks': tasks, 'reviews': []}
    def review(path):
        state['reviews'].append(str(path))
        return copy.deepcopy((reg, data, cases[:2]))
    monkeypatch.setattr(owner.formal_registration, 'revalidate', review)
    def transaction(path):
        registered_value = owner.formal_registration.revalidate(path)
        return registered_value, ({'artifact_hash': 'c' * 64}, tasks, None, None, None)
    monkeypatch.setattr(owner.formal_registration, 'proof_transaction', transaction)
    monkeypatch.setattr(owner.formal_registration.formal_collection, 'verified_pack',
                        lambda p: ({'artifact_hash': 'c' * 64}, tasks, None, None, None))
    monkeypatch.setattr(owner, 'owner_files', lambda: {'fixture': regfile})
    allocation_file = tmp_path / 'allocation.json'
    create(allocation_file, owner.prepare(regfile))
    prepared_file = tmp_path / 'prepared.json'
    create(prepared_file, owner.prepared_value(allocation_file, 'case-0'))
    return dict(state=state, regfile=regfile, allocation=allocation_file, prepared=prepared_file, tmp=tmp_path)


def load(path):
    return json.loads(Path(path).read_bytes())


def test_true_registration_retains_unknown_and_exact_json_no_label_in_request(registered):
    value = owner.validate(registered['prepared'])
    allocation = load(registered['allocation'])
    assert allocation['allocated_groups'] == 3 and allocation['eligible_groups'] == 2
    assert allocation['cases'][2]['request'] is None
    assert '1e-07' in value['request_json'] and '1.0' in value['request_json']
    assert value['registration_sha256'] == file_hash(registered['regfile'])
    assert value['registered_for_formal_evaluation'] is True
    assert value['certified_threshold'] is None and value['formal_certification'] is False
    assert not any(k in value['request']['state'] for k in ('label', 'oracle', 'reference'))
    with pytest.raises(ValueError):
        owner.prepared_value(registered['allocation'], 'case-2')


@pytest.mark.parametrize('field', ['registration_sha256', 'registration_hash', 'request_json', 'request_hash', 'split',
                                 'registered_for_formal_evaluation', 'formal_certification', 'holdout_opened'])
def test_resealed_request_registration_or_permission_change_refused(registered, field):
    value = load(registered['prepared'])
    value[field] = 'changed'
    value['artifact_hash'] = digest({k: v for k, v in value.items() if k != 'artifact_hash'})
    write(registered['prepared'], value)
    with pytest.raises(ValueError):
        owner.validate(registered['prepared'])


def test_missing_genuine_registration_not_caller_boolean(registered, monkeypatch):
    monkeypatch.setattr(owner.formal_registration, 'revalidate', lambda p: (_ for _ in ()).throw(ValueError('genuine proof missing')))
    with pytest.raises(ValueError, match='genuine proof'):
        owner.validate(registered['prepared'])


@pytest.fixture
def slots(registered, monkeypatch):
    root = registered['tmp'] / 'jev'
    root.mkdir()
    (root / 'reservations').mkdir()
    write(root / 'authorization.json', {'authorization_sha256': 'old-auth', 'kind': 'jev'})
    row = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev', 'group_id': 'older',
           'request_hash': 'a' * 64, 'authorization_sha256': 'old-auth', 'reservation_index': 1,
           'status': 'reserved_before_transport', 'automatic_retries': 0}
    write(root / 'reservations' / (digest('older') + '.json'), row)
    marker = root / 'reservations' / (digest('older') + '.started')
    marker.write_text('original cancelled marker')
    chain = {'authorization': {'jev': {'ledger_dir': str(root), 'max_attempts': 861}},
             'current_sha256': registered['state']['reg']['authorization_sha256'], 'ledger_authorization_sha256': 'old-auth'}
    monkeypatch.setattr(owner.approved_budget, 'resolve', lambda p: copy.deepcopy(chain))
    monkeypatch.setattr(owner, 'reservations', lambda *a: ([load(p) for p in (root / 'reservations').glob('*.json')], []))
    return dict(registered=registered, root=root, chain=chain, original_marker=marker)


def test_shared_financial_slot_preserves_old_failed_once_prefix_no_send(slots):
    old = slots['original_marker'].read_bytes()
    result = owner.reserve(slots['registered']['prepared'])
    assert result['reservation_index'] == 2 and result['registration_sha256'] == file_hash(slots['registered']['regfile'])
    assert load(slots['root'] / 'authorization.json')['authorization_sha256'] == 'old-auth'
    assert slots['original_marker'].read_bytes() == old
    with pytest.raises(ValueError, match='once_group'):
        owner.reserve(slots['registered']['prepared'])
    proof = owner.validate_reservation(slots['registered']['prepared'], result['path'])
    assert proof['reservation_sha256'] == file_hash(result['path'])


def test_entire_prepare_batch_and_reserve_each_use_one_genuine_transaction(slots, monkeypatch):
    fixture = slots['registered']
    before = len(fixture['state']['reviews'])
    def forbidden_second_source_proof(*args):
        raise AssertionError('unexpected duplicate full source proof')
    monkeypatch.setattr(owner.formal_registration.formal_collection, 'verified_pack', forbidden_second_source_proof)
    rows = owner.prepared_batch(fixture['allocation'])
    assert len(rows) == 2 and len(fixture['state']['reviews']) - before == 1
    before = len(fixture['state']['reviews'])
    owner.reserve(fixture['prepared'])
    assert len(fixture['state']['reviews']) - before == 1


def test_stale_lock_or_namespace_drift_or_cap_refuses_before_slot(slots, monkeypatch):
    lock = slots['root'] / 'reservation.lock'
    lock.write_text('original interrupted owner')
    with pytest.raises(FileExistsError):
        owner.reserve(slots['registered']['prepared'])
    assert lock.read_text() == 'original interrupted owner'
    lock.unlink()  # Isolated fixture cleanup, never an operational retry.
    slots['chain']['authorization']['jev']['max_attempts'] = 1
    with pytest.raises(ValueError, match='exhausted'):
        owner.reserve(slots['registered']['prepared'])
    assert len(list((slots['root'] / 'reservations').glob('*.json'))) == 1


@pytest.mark.parametrize('field', ['registration_sha256', 'registration_hash', 'request_hash', 'authorization_sha256'])
def test_original_reservation_binding_tamper_rejected(slots, field):
    result = owner.reserve(slots['registered']['prepared'])
    result[field] = 'changed'
    write(Path(result['path']), {k: v for k, v in result.items() if k != 'path'})
    with pytest.raises(ValueError):
        owner.validate_reservation(slots['registered']['prepared'], result['path'])


def binary(args):
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    return subprocess.run([str(BINARY), *args], capture_output=True, timeout=30, env=env)


def test_actual_new_binary_compiled_graph_and_no_implicit_live(tmp_path):
    result = owner.build_info(BINARY)
    assert result['runner_exe_sha256'] == file_hash(BINARY)
    assert result['network_calls'] == result['new_paid_calls'] == 0
    assert binary(['live', 'anything']).returncode != 0


@pytest.mark.parametrize('scenario,status', [
    ('success', 'ok'), ('http401', 'http_401'), ('http403', 'http_403'), ('http429', 'http_429'),
    ('timeout', 'timeout'), ('cancel_before', 'cancelled'), ('cancel_during', 'cancelled'),
    ('missing_key', 'missing_key'), ('malformed', 'invalid_json'), ('nan_extra', 'invalid_json'),
    ('infinity_extra', 'invalid_json'), ('overflow_extra', 'invalid_json'), ('deep_extra', 'invalid_json'),
    ('model_mismatch', 'actual_model_mismatch'), ('invalid_usage', 'invalid_usage'), ('missing_usage', 'invalid_usage'),
    ('missing_answer', 'exact_answers_required'), ('extra_answer', 'exact_answers_required'),
    ('boolean_probability', 'invalid_noul'), ('oversize', 'response_budget'),
])
def test_actual_original_byte_transport_once_failures_no_paid_http(tmp_path, scenario, status):
    result = binary(['offline-transport-test', str(tmp_path / scenario), scenario])
    assert result.returncode == 0, result.stderr
    summary = json.loads(result.stdout)
    assert summary['call_status'] == status and summary['duplicate_reservation_blocked'] is True
    assert summary['exact_request_bytes_preserved'] is True and summary['new_paid_calls'] == summary['network_calls'] == 0
    receipt = load(tmp_path / scenario / 'attempt/receipt.json')
    assert receipt['call_status'] == status
    raw = tmp_path / scenario / 'attempt/raw-response.json'
    if raw.exists():
        assert owner.parsed_status(raw.read_bytes(), load(tmp_path / scenario / 'attempt/request.json')) == status


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
             'allocation_file': str(tmp_path / 'allocation.json'),
             'registration_file': 'original-registration', 'registration_sha256': 'reg-sha', 'registration_hash': 'reg-hash',
             'allocation_hash': 'allocation', 'allocation_sha256': 'allocation-sha',
             'artifact_cohort_hash': 'cohort', 'protocol_sha256': 'protocol', 'source_kind': packet.FREE}
    monkeypatch.setattr(owner, 'validate', lambda p: value)
    compiled = {'runner_exe_sha256': file_hash(exe), **{k: file_hash(p) for k, p in owner.runner_sources().items()}}
    monkeypatch.setattr(owner, 'build_info', lambda p: compiled)
    reservation = root / 'reservations' / (digest('group') + '.json')
    write(reservation, {})
    proof = {'ledger_root': str(root), 'reservation_file': str(reservation),
             'reservation_sha256': file_hash(reservation), 'marker_file': str(reservation.with_suffix('.started'))}
    monkeypatch.setattr(owner, 'validate_reservation', lambda *a, **kw: proof)
    common = {'id': 'case', 'group_id': 'group', 'request_hash': digest(body),
              'prepared_sha256': file_hash(prepared), 'template_version': packet.TEMPLATE,
              **{k: value[k] for k in ('allocation_hash', 'allocation_sha256', 'artifact_cohort_hash', 'protocol_sha256', 'source_kind', 'registration_file', 'registration_sha256', 'registration_hash')},
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


def test_slot_lock_checks_each_original_file_once_not_quadratic(slots, monkeypatch):
    for n in range(2, 21):
        row = {'group_id': f'older-{n}', 'reservation_index': n, 'request_hash': 'b' * 64}
        write(slots['root'] / 'reservations' / (digest(row['group_id']) + '.json'), row)
    observed = []
    original = owner.load
    def counted(path):
        if Path(path).parent == slots['root'] / 'reservations':
            observed.append(str(path))
        return original(path)
    monkeypatch.setattr(owner, 'load', counted)
    value = owner.reserve(slots['registered']['prepared'])
    assert value['reservation_index'] == 21
    assert len(observed) == len(set(observed)) == 20
