"""APPS development packets and immutable single-attempt observation joins.

Rebuild the entire allocation before choosing a binary candidate. No Oracle,
transport, reservations, thresholds, formal registration or holdout opening.
"""
import argparse
import json
import math
import os
import subprocess
from collections import Counter
from pathlib import Path

from ..artifact_cohort import reservations
from ..artifact_scoring import sealed
from ..budget_authorization_v2 import resolve
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..collection_receipts import read_lines
from . import development, development_intake, intake, packet, protocol, resources

ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / 'rust/crates/ncx-core/examples'
PACK = 'buglecat.verification.apps-development-jev-allocation.v3'
SCHEMA = 'buglecat.verification.apps-development-jev-prepared.v3'
OBSERVATION = 'buglecat.verification.apps-development-jev-observation.v3'


def same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def owner_files():
    files = {'intake_source_sha256': Path(__file__).resolve(), 'builder_source_sha256': Path(packet.__file__),
             'development_cohort_source_sha256': Path(development.__file__),
             'development_intake_source_sha256': Path(development_intake.__file__),
             'candidate_intake_source_sha256': Path(intake.__file__), 'resource_owner_source_sha256': Path(resources.__file__),
             'protocol_owner_source_sha256': Path(protocol.__file__)}
    for kind in sorted(packet.KINDS):
        files.update({kind + ':' + role: p for role, p in packet.dependency_files(kind).items()})
    return files


def runner_sources():
    return {'runner_source_sha256': EXAMPLES / 'verification_apps_v3_jev_development.rs',
            'provider_source_sha256': ROOT / 'rust/crates/ncx-provider/src/jev.rs',
            'frozen_support_sha256': EXAMPLES / 'verification_artifact_profile_formal/support.rs',
            'frozen_formal_sha256': EXAMPLES / 'verification_artifact_profile_formal/formal.rs',
            **{k: p for k, p in owner_files().items() if ':' not in k}}


def build_info(executable):
    """Read the actual compiled binary's proof, with credentials removed."""
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    result = subprocess.run([str(Path(executable).resolve(strict=True)), 'build-info'], env=env,
                            capture_output=True, timeout=15, check=False)
    require(result.returncode == 0, 'actual APPS Jev build proof unavailable')
    info = json.loads(result.stdout)
    require(info['runner_exe_sha256'] == file_hash(executable)
            and all(info.get(k) == file_hash(p) for k, p in runner_sources().items())
            and info['new_paid_calls'] == 0 and info['network_calls'] == 0
            and info['formal_certification'] is False and info['template_version'] == packet.TEMPLATE,
            'actual compiled APPS Jev source changed')
    return info


def prepare_collection(main_directory, collector_executable):
    """Pure full-allocation replay; never execute an absent Oracle receipt."""
    directory, executable = Path(main_directory).resolve(strict=True), Path(collector_executable).resolve(strict=True)
    data = development_intake.collect(directory, executable, execute_new_oracles=False)
    require(data['split'] == 'development' and data['formal_registration'] is False
            and 1 <= len(data['cases']) <= 8, 'complete development allocation required')
    statuses = [c['main_status'] for c in data['cases']]
    require('main_not_finished' not in statuses, 'in_flight_main_scope_rejected')
    unstarted = statuses.index('main_not_started') if 'main_not_started' in statuses else len(statuses)
    require(all(status == 'main_not_started' for status in statuses[unstarted:]), 'original fixed main prefix required')
    # A successful prefix while later groups are not started can still be a
    # live main batch. Only a recorded first fault or complete terminal scope
    # supplies closure; a free no-start state cannot authorize Jev.
    require(unstarted == len(statuses) or (unstarted > 0 and statuses[unstarted - 1] != 'ok'),
            'whole_main_scope_not_terminal')
    manifest = development.validate_manifest(directory / 'collection-manifest.json')
    cohort = development.validate_cohort(manifest['artifact_cohort_file'])
    authority, _ = development.inputs(manifest['source_authority_file'], manifest['resource_inventory_file'])
    tasks, _ = development.original_tasks(authority, cohort['cases'])
    cases = []
    for original in data['cases']:
        row = dict(original)
        row['request'] = None
        row['request_hash'] = None
        if original['call_eligible']:
            require(original['main_status'] == 'ok' and original['label'] in {'supported', 'unsupported'}
                    and original['oracle_file'] and original['oracle_sha256'], 'independent binary label required')
            receipt = load(original['main_receipt_file'])
            task = tasks[original['task_id']]
            extracted = intake.original_code(receipt['result']['candidate'])
            require(extracted is not None and extracted['code_sha256'] == original['candidate_code_sha256'], 'exact original code required')
            body = resources.checked_request(task, extracted['code'])
            packet.validate_request(body)
            row.update(request=body, request_hash=digest(body))
        else:
            require(original['label'] in {'unknown', 'unlabeled'}, 'nonbinary cannot be eligible')
        cases.append(row)
    files = owner_files()
    bindings = [{'role': role, 'file': str(path.resolve()), 'sha256': file_hash(path)} for role, path in sorted(files.items())]
    body = {'schema': PACK, 'split': 'development', 'original_collection_directory': str(directory),
            'main_executable': str(executable), 'main_executable_sha256': file_hash(executable),
            'original_dataset_hash': data['artifact_hash'], 'main_manifest_sha256': data['main_manifest_sha256'],
            'prepared_main_sha256': data['prepared_sha256'], 'original_events_sha256': file_hash(directory / 'attempt-events.jsonl'),
            'artifact_cohort_file': manifest['artifact_cohort_file'], 'artifact_cohort_sha256': manifest['artifact_cohort_sha256'],
            'artifact_cohort_hash': manifest['artifact_cohort_hash'], 'authorization_file': manifest['authorization_file'],
            'authorization_sha256': manifest['authorization_sha256'], 'protocol_file': manifest['protocol_file'],
            'protocol_sha256': manifest['protocol_sha256'], 'template_version': packet.TEMPLATE, 'policy_version': packet.POLICY,
            'cases': cases, 'allocated_groups': len(cases), 'eligible_groups': sum(c['call_eligible'] for c in cases),
            'label_counts': dict(Counter(c['label'] for c in cases)), 'source_bindings': bindings,
            'whole_development_allocation_excluded_from_formal': True, 'scores_hidden': True, 'jev_probabilities_read': False,
            'registered_for_formal_evaluation': False, 'holdout_opened': False, 'production_authorized': False,
            'formal_certification': False, 'certified_threshold': None, 'new_paid_calls': 0}
    require(all(file_hash(b['file']) == b['sha256'] for b in bindings), 'APPS preparation owner changed')
    require(same(development_intake.collect(directory, executable, execute_new_oracles=False), data),
            'original main scope changed during preparation')
    return {**body, 'artifact_hash': digest(body)}


def validate_pack(path):
    value = sealed(load(path))
    require(value['schema'] == PACK, 'APPS whole allocation required')
    require(same(value, prepare_collection(value['original_collection_directory'], value['main_executable'])),
            'whole allocation original proof changed')
    return value


def prepared_value(allocation_file, identity):
    allocation = validate_pack(allocation_file)
    selected = [c for c in allocation['cases'] if c['id'] == identity]
    require(len(selected) == 1 and selected[0]['call_eligible'] is True, 'one eligible original APPS candidate required')
    case = selected[0]
    body = {'schema': SCHEMA, 'id': identity, 'group_id': case['group_id'], 'split': 'development',
            'source_kind': case['source_kind'], 'template_version': packet.TEMPLATE, 'policy_version': packet.POLICY,
            'allocation_file': str(Path(allocation_file).resolve()), 'allocation_sha256': file_hash(allocation_file),
            'allocation_hash': allocation['artifact_hash'], 'allocation_ids': [c['id'] for c in allocation['cases']],
            'artifact_cohort_hash': allocation['artifact_cohort_hash'], 'source_bindings': allocation['source_bindings'],
            'authorization_file': allocation['authorization_file'], 'authorization_sha256': allocation['authorization_sha256'],
            'protocol_sha256': allocation['protocol_sha256'], 'request': case['request'], 'request_hash': case['request_hash'],
            'request_json': packet.canonical_bytes(case['request']).decode(),
            'registered_for_formal_evaluation': False, 'formal_certification': False, 'certified_threshold': None,
            'production_authorized': False, 'holdout_opened': False}
    return {**body, 'artifact_hash': digest(body)}


def validate(path):
    value = sealed(load(path))
    require(value['schema'] == SCHEMA and value['split'] == 'development'
            and all(value[k] is False for k in ('registered_for_formal_evaluation', 'formal_certification', 'production_authorized', 'holdout_opened'))
            and value['certified_threshold'] is None, 'APPS development-only packet required')
    require(file_hash(value['allocation_file']) == value['allocation_sha256'], 'original allocation bytes changed')
    require(same(value, prepared_value(value['allocation_file'], value['id'])), 'APPS original request differs')
    packet.validate_request(value['request'])
    return value


def validate_reservation(prepared_file, reservation_file):
    value = validate(prepared_file)
    chain = resolve(value['authorization_file'])
    require(chain['current_sha256'] == value['authorization_sha256'], 'original approved chain changed')
    root = Path(chain['authorization']['jev']['ledger_dir']).resolve(strict=True)
    path = Path(reservation_file).resolve(strict=True)
    require(development_intake.same_path(path, root / 'reservations' / (digest(value['group_id']) + '.json')),
            'original shared reservation required')
    rows, _ = reservations(root, 'jev', chain)
    selected = [r for r in rows if r['group_id'] == value['group_id']]
    require(len(selected) == 1 and selected[0]['request_hash'] == value['request_hash'], 'original reservation body mismatch')
    return {'status': 'apps_development_reservation_verified', 'request_hash': value['request_hash'],
            'reservation_file': str(path), 'reservation_sha256': file_hash(path),
            'marker_file': str(path.with_suffix('.started')), 'ledger_root': str(root), 'new_paid_calls': 0}


def parsed_status(raw, request):
    """Recompute the bounded sender's response outcome from original bytes."""
    def nonfinite(_):
        raise ValueError('nonfinite JSON')
    try:
        response = json.loads(raw, parse_constant=nonfinite)
        pending = [(response, 0)]
        while pending:
            item, depth = pending.pop()
            if isinstance(item, float) and not math.isfinite(item):
                raise ValueError('nonfinite JSON number')
            if isinstance(item, (dict, list)):
                if depth >= 127:
                    raise ValueError('serde JSON recursion limit')
                pending.extend((v, depth + 1) for v in (item.values() if isinstance(item, dict) else item))
    except (ValueError, UnicodeError, RecursionError):
        return 'invalid_json'
    if not isinstance(response, dict) or response.get('model') != packet.MODEL:
        return 'actual_model_mismatch'
    answers = response.get('answers')
    if not isinstance(answers, dict):
        return 'answers'
    if set(answers) != set(request['questions']):
        return 'exact_answers_required'
    for answer in answers.values():
        probability = answer.get('noul') if isinstance(answer, dict) else None
        if not isinstance(answer, dict) or answer.get('type') != 'noul' or type(probability) not in {int, float} or not math.isfinite(probability) or not 0 <= probability <= 1:
            return 'invalid_noul'
    usage = response.get('usage')
    if not isinstance(usage, dict) or any(type(usage.get(k)) is not int or not 0 <= usage[k] <= 2**64 - 1 for k in ('input_tokens', 'output_tokens')):
        return 'invalid_usage'
    return 'ok'


def observe(prepared_file, attempt_directory, executable):
    value = validate(prepared_file)
    compiled = build_info(executable)
    directory = Path(attempt_directory).resolve(strict=True)
    events = read_lines(directory / 'events.jsonl')
    require(len(events) in {2, 3} and events[0]['status'] == 'attempt_started' and events[-1]['status'] == 'attempt_finished',
            'original single attempt events required')
    start, finish = events[0], events[-1]
    proof = validate_reservation(prepared_file, start['reservation_file'])
    root = Path(proof['ledger_root'])
    require(development_intake.within_root(directory, root / 'attempts'), 'attempt outside original shared ledger')
    require(same(load(proof['marker_file']), start) and same(load(directory / 'receipt.json'), finish),
            'original one-use start/finish bytes changed')
    expected = {'id': value['id'], 'group_id': value['group_id'], 'request_hash': value['request_hash'],
                'prepared_sha256': file_hash(prepared_file), 'template_version': packet.TEMPLATE,
                'allocation_hash': value['allocation_hash'], 'allocation_sha256': value['allocation_sha256'],
                'artifact_cohort_hash': value['artifact_cohort_hash'], 'protocol_sha256': value['protocol_sha256'],
                'source_kind': value['source_kind'], 'automatic_retries': 0, 'formal_certification': False, 'offline': False}
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
    body = {'schema': OBSERVATION, 'id': value['id'], 'group_id': value['group_id'], 'split': 'development',
            'request_hash': value['request_hash'], 'actual_request_hash': events[1].get('actual_request_hash') if submitted else None,
            'allocation_hash': value['allocation_hash'], 'call_status': finish['call_status'], 'acceptance_violation': None,
            'original_events_sha256': file_hash(directory / 'events.jsonl'), 'response_sha256': finish.get('response_sha256'),
            'transport_binding_status': 'original_transport_bytes_verified', 'verified_pass_signals': 0,
            'formal_certification': False, 'certified_threshold': None, 'production_authorized': False, 'holdout_opened': False}
    if finish['call_status'] == 'ok':
        require(submitted and raw_file.is_file(), 'original submitted 2xx response required')
        signal = packet.signal(value['request'], load(raw_file))
        body.update(acceptance_violation=signal['acceptance_violation'], model=signal['model'], usage=signal['usage'])
    return {**body, 'artifact_hash': digest(body)}


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
        result = {'status': 'apps_development_source_verified', 'request_hash': value['request_hash'], 'new_paid_calls': 0}
    print(json.dumps(result))


if __name__ == '__main__':
    main()
