"""Genuine APPS holdout packets, shared one-use reservations and byte joins.

Holdout-only. Original once source and blind registration are authenticated.
Preparation/validation never execute an Oracle or transport. Reserve is explicit.
"""
import argparse
import json
import os
from collections import Counter
from pathlib import Path

from ..artifact_cohort import reservations
from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..collection_receipts import read_lines
from ..paid_budget import durable_new
from . import (
    approved_budget,
    development_intake,
    development_jev,
    holdout_intake,
    holdout_registration,
    intake,
    packet,
    resources,
)

ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / 'rust/crates/ncx-core/examples'
PACK = 'buglecat.verification.apps-holdout-jev-allocation.v3'
SCHEMA = 'buglecat.verification.apps-holdout-jev-prepared.v3'
OBSERVATION = 'buglecat.verification.apps-holdout-jev-observation.v3'
same = development_jev.same
parsed_status = development_jev.parsed_status


def owner_files():
    from . import formal_jev, holdout_collection
    files = dict(formal_jev.owner_files())
    files.update(intake_source_sha256=Path(__file__),
        holdout_registration_owner_source_sha256=Path(holdout_registration.__file__),
        holdout_collection_owner_source_sha256=Path(holdout_collection.__file__),
        holdout_intake_owner_source_sha256=Path(holdout_intake.__file__),
        holdout_evaluation_owner_source_sha256=Path(__file__).with_name('holdout_evaluation.py'),
        calibration_transport_owner_source_sha256=Path(formal_jev.__file__))
    return files


def runner_sources():
    files = dict(development_jev.runner_sources())
    files.update(runner_source_sha256=EXAMPLES / 'verification_apps_v3_jev_holdout.rs')
    files.update({k: p for k, p in owner_files().items() if ':' not in k})
    return files


def build_info(executable):
    import subprocess
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    actual = file_hash(executable)
    result = subprocess.run([str(Path(executable).resolve(strict=True)), 'build-info'], env=env,
                            capture_output=True, timeout=15, check=True)
    info = json.loads(result.stdout)
    require(actual == file_hash(executable) == info['runner_exe_sha256']
            and all(info.get(k) == file_hash(p) for k, p in runner_sources().items())
            and type(info['new_paid_calls']) is int and info['new_paid_calls'] == 0
            and type(info['network_calls']) is int and info['network_calls'] == 0
            and info['formal_certification'] is False and info['template_version'] == packet.TEMPLATE,
            'actual_compiled_formal_Jev_source_changed')
    return info


def _allocation(registration_file, registered, *, _verified_pack=None):
    reg, data, _ = registered
    cohort, tasks, _, _, _ = (_verified_pack if _verified_pack is not None
                             else holdout_registration.holdout_collection.verified_pack(data['holdout_source_file'])[1])
    cases = []
    for original in data['cases']:
        row = dict(original, request=None, request_hash=None)
        if original['call_eligible']:
            require(original['main_status'] == 'ok' and original['label'] in {'supported', 'unsupported'},
                    'genuine_original_binary_label_required')
            receipt = load(original['main_receipt_file'])
            extracted = intake.original_code(receipt['result']['candidate'])
            require(extracted is not None and extracted['code_sha256'] == original['candidate_code_sha256'],
                    'exact_original_candidate_required')
            body = resources.checked_request(tasks[original['task_id']], extracted['code'])
            packet.validate_request(body)
            row.update(request=body, request_hash=digest(body))
        else:
            require(original['label'] == 'unknown', 'terminal_nonbinary_must_remain_unknown')
        cases.append(row)
    dependencies = [{'role': role, 'file': str(path.resolve()), 'sha256': file_hash(path)}
                    for role, path in sorted(owner_files().items())]
    value = {'schema': PACK, 'split': 'holdout', 'cases': cases,
             'registration_file': str(Path(registration_file).resolve(strict=True)),
             'registration_sha256': file_hash(registration_file), 'registration_hash': reg['artifact_hash'],
             **{k: reg[k] for k in ('holdout_source_file','holdout_source_sha256','holdout_source_hash','fixed_threshold')},
             'artifact_cohort_file': data['cohort_file'], 'artifact_cohort_sha256': data['cohort_sha256'],
             'artifact_cohort_hash': cohort['artifact_hash'], 'authorization_file': reg['authorization_file'],
             'authorization_sha256': reg['authorization_sha256'], 'protocol_file': data['protocol_file'],
             'protocol_sha256': data['protocol_sha256'], 'original_dataset_hash': data['artifact_hash'],
             'allocated_groups': len(cases), 'eligible_groups': sum(c['call_eligible'] for c in cases),
             'label_counts': dict(Counter(c['label'] for c in cases)), 'source_bindings': dependencies,
             'scores_hidden': True, 'jev_probabilities_read': False, 'registered_for_formal_evaluation': True,
             'formal_certification': False, 'certified_threshold': None, 'holdout_opened': True,
             'production_authorized': False, 'new_paid_calls': 0}
    require(all(file_hash(b['file']) == b['sha256'] for b in dependencies), 'formal_packet_source_changed')
    return {**value, 'artifact_hash': digest(value)}


def prepare(registration_file):
    """Pure full original registration proof, whole allocation retained."""
    registered, pack = holdout_registration.proof_transaction(registration_file)
    return _allocation(registration_file, registered, _verified_pack=pack)


def validate_pack(path, *, _registered=None, _verified_pack=None):
    value = sealed(load(path))
    require(value['schema'] == PACK and value['split'] == 'holdout', 'formal_calibration_allocation_required')
    if _registered is None:
        registered, _verified_pack = holdout_registration.proof_transaction(value['registration_file'])
    else:
        registered = _registered
    require(same(value, _allocation(value['registration_file'], registered, _verified_pack=_verified_pack)), 'original_formal_allocation_changed')
    return value


def _prepared(allocation_file, allocation, identity):
    selected = [c for c in allocation['cases'] if c['id'] == identity]
    require(len(selected) == 1 and selected[0]['call_eligible'] is True, 'one_registered_binary_candidate_required')
    case = selected[0]
    body = {'schema': SCHEMA, 'id': identity, 'group_id': case['group_id'], 'split': 'holdout',
            'source_kind': case['source_kind'], 'template_version': packet.TEMPLATE, 'policy_version': packet.POLICY,
            'allocation_file': str(Path(allocation_file).resolve(strict=True)), 'allocation_sha256': file_hash(allocation_file),
            'allocation_hash': allocation['artifact_hash'], 'allocation_ids': [c['id'] for c in allocation['cases']],
            'artifact_cohort_hash': allocation['artifact_cohort_hash'], 'source_bindings': allocation['source_bindings'],
            'authorization_file': allocation['authorization_file'], 'authorization_sha256': allocation['authorization_sha256'],
            'protocol_sha256': allocation['protocol_sha256'],
            **{k: allocation[k] for k in ('registration_file', 'registration_sha256', 'registration_hash', 'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')},
            'request': case['request'], 'request_hash': case['request_hash'],
            'request_json': packet.canonical_bytes(case['request']).decode(),
            'registered_for_formal_evaluation': True, 'formal_certification': False, 'certified_threshold': None,
            'production_authorized': False, 'holdout_opened': True}
    return {**body, 'artifact_hash': digest(body)}


def prepared_value(allocation_file, identity):
    return _prepared(allocation_file, validate_pack(allocation_file), identity)


def prepared_batch(allocation_file):
    """One full original proof for all eligible exact requests, no persisted cache."""
    allocation = validate_pack(allocation_file)
    return [_prepared(allocation_file, allocation, c['id']) for c in allocation['cases'] if c['call_eligible']]


def validate(path, *, _allocation_pack=None):
    value = sealed(load(path))
    require(value['schema'] == SCHEMA and value['split'] == 'holdout'
            and value['registered_for_formal_evaluation'] is True and value['holdout_opened'] is True
            and all(value[k] is False for k in ('formal_certification', 'production_authorized'))
            and value['certified_threshold'] is None, 'genuine_registered_calibration_packet_required')
    allocation = _allocation_pack if _allocation_pack is not None else validate_pack(value['allocation_file'])
    require(file_hash(value['allocation_file']) == value['allocation_sha256']
            and same(value, _prepared(value['allocation_file'], allocation, value['id'])), 'exact_formal_request_changed')
    packet.validate_request(value['request'])
    return value


def validate_reservation(prepared_file, reservation_file, *, _value=None):
    value = _value if _value is not None else validate(prepared_file)
    chain = approved_budget.resolve(value['authorization_file'])
    require(chain['current_sha256'] == value['authorization_sha256'], 'approved_formal_authorization_changed')
    root = Path(chain['authorization']['jev']['ledger_dir']).resolve(strict=True)
    path = Path(reservation_file).resolve(strict=True)
    require(holdout_intake.development_intake.same_path(path, root / 'reservations' / (digest(value['group_id']) + '.json')),
            'original_shared_reservation_required')
    rows, _ = reservations(root, 'jev', chain)
    selected = [r for r in rows if r['group_id'] == value['group_id']]
    require(len(selected) == 1 and selected[0]['request_hash'] == value['request_hash']
            and selected[0]['authorization_sha256'] == value['authorization_sha256']
            and all(selected[0].get(k) == value[k] for k in ('registration_file', 'registration_sha256', 'registration_hash', 'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')),
            'registered_request_reservation_binding_changed')
    return {'status': 'apps_holdout_reservation_verified', 'request_hash': value['request_hash'],
            'reservation_file': str(path), 'reservation_sha256': file_hash(path),
            'marker_file': str(path.with_suffix('.started')), 'ledger_root': str(root), 'new_paid_calls': 0}


def reserve(prepared_file):
    """Explicit financial slot only, after original registration; no HTTP."""
    prepared_sha = file_hash(prepared_file)
    supplied = sealed(load(prepared_file))
    registered, pack = holdout_registration.proof_transaction(supplied['registration_file'])
    allocation = validate_pack(supplied['allocation_file'], _registered=registered, _verified_pack=pack)
    value = validate(prepared_file, _allocation_pack=allocation)
    reg = registered[0]
    require(reg['artifact_hash'] == value['registration_hash'] and file_hash(value['registration_file']) == value['registration_sha256'],
            'original_registration_changed_before_reserve')
    chain = approved_budget.resolve(value['authorization_file'])
    require(chain['current_sha256'] == value['authorization_sha256'], 'approved_reservation_authorization_changed')
    root = Path(chain['authorization']['jev']['ledger_dir']).resolve(strict=True)
    old, _ = reservations(root, 'jev', chain)
    cap = chain['authorization']['jev']['max_attempts']
    require(len(old) < cap and value['group_id'] not in {r['group_id'] for r in old}, 'once_group_or_budget_exhausted')
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.write(descriptor, str(os.getpid()).encode())
        os.fsync(descriptor)
        paths = list((root / 'reservations').glob('*.json'))
        expected = {digest(r['group_id']) + '.json': r for r in old}
        require(len(expected) == len(old) and {p.name for p in paths} == set(expected)
                and all(same(load(p), expected[p.name]) for p in paths)
                and load(root / 'authorization.json') == {'authorization_sha256': chain['ledger_authorization_sha256'], 'kind': 'jev'},
                'shared_reservation_namespace_changed_before_slot')
        require(file_hash(prepared_file) == prepared_sha and file_hash(value['registration_file']) == value['registration_sha256']
                and file_hash(value['authorization_file']) == value['authorization_sha256']
                and all(file_hash(b['file']) == b['sha256'] for b in reg['source_bindings'] + reg['original_evidence_bindings'] + value['source_bindings']),
                'registered_originals_changed_before_slot')
        row = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev', 'group_id': value['group_id'],
               'request_hash': value['request_hash'], 'authorization_sha256': chain['current_sha256'],
               'reservation_index': len(old) + 1, 'status': 'reserved_before_transport', 'automatic_retries': 0,
               **{k: value[k] for k in ('registration_file', 'registration_sha256', 'registration_hash', 'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')}}
        target = root / 'reservations' / (digest(value['group_id']) + '.json')
        durable_new(target, row)
        return {**row, 'path': str(target)}
    finally:
        os.close(descriptor)
        lock.unlink()



def _observe(value, prepared_file, attempt_directory, executable, *, _compiled=None):
    compiled = _compiled if _compiled is not None else build_info(executable)
    directory = Path(attempt_directory).resolve(strict=True)
    events = read_lines(directory / 'events.jsonl')
    require(len(events) in {2, 3} and events[0]['status'] == 'attempt_started' and events[-1]['status'] == 'attempt_finished',
            'original single attempt events required')
    start, finish = events[0], events[-1]
    proof = validate_reservation(prepared_file, start['reservation_file'], _value=value)
    root = Path(proof['ledger_root'])
    require(development_intake.within_root(directory, root / 'attempts'), 'attempt outside original shared ledger')
    require(same(load(proof['marker_file']), start) and same(load(directory / 'receipt.json'), finish),
            'original one-use start/finish bytes changed')
    expected = {'id': value['id'], 'group_id': value['group_id'], 'request_hash': value['request_hash'],
                'prepared_sha256': file_hash(prepared_file), 'template_version': packet.TEMPLATE,
                'allocation_hash': value['allocation_hash'], 'allocation_sha256': value['allocation_sha256'],
                'artifact_cohort_hash': value['artifact_cohort_hash'], 'protocol_sha256': value['protocol_sha256'],
                'source_kind': value['source_kind'],
                **{k: value[k] for k in ('registration_file', 'registration_sha256', 'registration_hash', 'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')}, 'automatic_retries': 0, 'formal_certification': False, 'offline': False}
    for event in events:
        require(all(same(event.get(k), v) for k, v in expected.items()), 'original APPS event binding changed')
        require(development_intake.same_path(event['attempt_directory'], directory)
                and development_intake.same_path(event['raw_response_file'], directory / 'raw-response.json')
                and development_intake.same_path(event['reservation_file'], proof['reservation_file'])
                and event['reservation_sha256'] == proof['reservation_sha256'], 'original attempt path/reservation changed')
        require(event['runner_exe_sha256'] == compiled['runner_exe_sha256']
                and all(event.get(k) == compiled[k] for k in runner_sources()), 'actual compiled APPS Jev source changed')
    submitted = len(events) == 3
    require(finish['submitted'] is submitted, 'original submission mismatch')
    if submitted:
        require(events[1]['status'] == 'request_submitted' and events[1]['actual_request_hash'] == value['request_hash'],
                'actual APPS submission hash required')
    else:
        require(finish['call_status'] in {'cancelled', 'missing_key'} and finish.get('response_sha256') is None,
                'unsubmitted attempt status mismatch')
    require(file_hash(directory / 'request.json') == value['request_hash']
            and same(load(directory / 'request.json'), value['request']), 'original actual request bytes changed')
    raw_file = directory / 'raw-response.json'
    require(finish.get('response_sha256') == file_hash(raw_file) if raw_file.exists() else finish.get('response_sha256') is None,
            'original raw response bytes changed')
    parsed_failures = {'invalid_json', 'actual_model_mismatch', 'answers', 'exact_answers_required', 'invalid_noul', 'invalid_usage'}
    nonraw_failures = {'timeout', 'cancelled', 'missing_key', 'response_budget', 'client_error', 'transport_error'}
    status = finish['call_status']
    require(status == 'ok' or status in parsed_failures or status in nonraw_failures
            or (isinstance(status, str) and status.startswith('http_') and status[5:].isdigit()
                and 100 <= int(status[5:]) <= 599 and not 200 <= int(status[5:]) <= 299), 'unknown transport outcome')
    require(raw_file.exists() is (status == 'ok' or status in parsed_failures), 'original outcome/raw response mismatch')
    if raw_file.exists():
        raw = raw_file.read_bytes()
        require(len(raw) <= 32000, 'original response budget exceeded')
        require(parsed_status(raw, value['request']) == status, 'original parsed outcome changed')
    body = {'schema': OBSERVATION, 'id': value['id'], 'group_id': value['group_id'], 'split': 'holdout',
            'prepared_file': str(Path(prepared_file).resolve(strict=True)), 'prepared_sha256': file_hash(prepared_file),
            'attempt_directory': str(directory), 'runner_executable_file': str(Path(executable).resolve(strict=True)),
            'runner_executable_sha256': compiled['runner_exe_sha256'],
            'allocation_file': value['allocation_file'], 'allocation_sha256': value['allocation_sha256'],
            'request_hash': value['request_hash'], 'actual_request_hash': events[1].get('actual_request_hash') if submitted else None,
            'allocation_hash': value['allocation_hash'],
            **{k: value[k] for k in ('registration_file', 'registration_sha256', 'registration_hash', 'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')}, 'call_status': finish['call_status'], 'acceptance_violation': None,
            'original_events_sha256': file_hash(directory / 'events.jsonl'), 'response_sha256': finish.get('response_sha256'),
            'transport_binding_status': 'original_transport_bytes_verified', 'verified_pass_signals': 0,
            'formal_certification': False, 'certified_threshold': None, 'production_authorized': False, 'holdout_opened': True}
    if finish['call_status'] == 'ok':
        require(submitted and raw_file.is_file(), 'original submitted 2xx response required')
        signal = packet.signal(value['request'], load(raw_file))
        body.update(acceptance_violation=signal['acceptance_violation'], model=signal['model'], usage=signal['usage'])
    return {**body, 'artifact_hash': digest(body)}


def observe(prepared_file, attempt_directory, executable):
    return _observe(validate(prepared_file), prepared_file, attempt_directory, executable)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['validate', 'validate-reservation'])
    parser.add_argument('--prepared', type=Path, required=True)
    parser.add_argument('--reservation', type=Path)
    args = parser.parse_args()
    if args.action == 'validate-reservation':
        result = validate_reservation(args.prepared, args.reservation)
    else:
        value = validate(args.prepared)
        result = {'status': 'apps_holdout_source_verified', 'request_hash': value['request_hash'], 'new_paid_calls': 0}
    print(json.dumps(result))


if __name__ == '__main__':
    main()
