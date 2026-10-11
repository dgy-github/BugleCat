"""Genuine original APPS calibration registration, with one shared marker.

Every validation replays the complete saved main/Oracle dataset read-only.
Registration locks the original Jev reservation namespace for its short
create-new transaction. No transport, reservations or Oracle execution.
"""
import os
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..paid_budget import durable_new
from . import formal_collection, formal_intake, packet, prospective_protocol

SCHEMA = 'buglecat.verification.apps-original-registration.formal.v3'
NAMESPACE = 'formal-apps-registrations-v3'


def _same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def _originals(dataset_file):
    data = sealed(load(dataset_file))
    require(data['schema'] == formal_intake.SCHEMA and data['split'] == 'calibration'
            and all(data[k] is False for k in ('formal_registration', 'holdout_opened',
                                               'formal_certification', 'production_authorized'))
            and data['certified_threshold'] is None and data['scores_hidden'] is True
            and data['jev_probabilities_read'] is False, 'prediction_blind_original_dataset_required')
    directories = [row['directory'] for row in data['collection_bindings']]
    actual = formal_intake.collect(data['cohort_file'], directories, data['collector_executable_file'])
    require(_same(actual, data), 'complete_original_main_and_oracle_dataset_changed')
    cohort = formal_collection.validate_cohort(data['cohort_file'])
    fixed = [c for c in cohort['cases'] if c['split'] == 'calibration']
    require(len(data['cases']) == len(fixed) == data['allocated_groups'] == data['collected_groups']
            and data['uncollected_groups'] == data['pending_groups'] == 0,
            'complete_terminal_allocation_required')
    require(all(row['main_status'] not in {'main_not_started', 'main_not_finished'} for row in data['cases']),
            'pending_or_unstarted_cannot_register')
    binary = [row for row in data['cases'] if row['label'] in {'supported', 'unsupported'}]
    require(all(row['call_eligible'] is (row['label'] in {'supported', 'unsupported'}) for row in data['cases']),
            'original_independent_binary_eligibility_required')
    spec = prospective_protocol.validate(load(data['protocol_file']))
    counts = {label: sum(row['label'] == label for row in data['cases'])
              for label in ('supported', 'unsupported', 'unknown', 'unlabeled')}
    require(counts['supported'] >= spec['min_supported'] and counts['unsupported'] >= spec['min_unsupported'],
            'original_150_supported_50_unsupported_required')
    authorization = next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization')
    baseline, finished = formal_collection.budget_snapshot(authorization)
    require(all(finished.get(c['group_id']) is not None for c in fixed), 'original_main_finish_required')
    return data, cohort, binary, counts, baseline


def _owner_bindings():
    return [formal_collection.development.binding(role, path) for role, path in (
        ('registration_owner', __file__), ('formal_collection_owner', formal_collection.__file__),
        ('formal_intake_owner', formal_intake.__file__), ('prospective_protocol_owner', prospective_protocol.__file__))]


def _root(baseline):
    return Path(baseline['authorization_chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)


def _evidence_bindings(data, cohort):
    paths = {Path(b['file']).resolve(strict=True) for b in cohort['bindings']}
    paths.add(Path(data['collector_executable_file']).resolve(strict=True))
    for row in data['collection_bindings']:
        directory = Path(row['directory']).resolve(strict=True)
        for path in directory.rglob('*'):
            require(not path.is_symlink(), 'original_collection_symlink')
            if path.is_file():
                require(path.resolve(strict=True).is_relative_to(directory), 'original_collection_path_escape')
                paths.add(path.resolve(strict=True))
    return [{'file': str(path), 'sha256': file_hash(path)} for path in sorted(paths)]


def registration_value(dataset_file):
    """Pure pre-Jev construction; returned data alone grants no authority."""
    data, cohort, binary, counts, baseline = _originals(dataset_file)
    groups = {row['group_id'] for row in data['cases']}
    require(not groups & {row['group_id'] for row in baseline['jev_reservations']},
            'allocation_jev_reservation_before_registration')
    root = _root(baseline)
    marker = root / NAMESPACE / (cohort['artifact_hash'] + '.json')
    require(not marker.exists(), 'formal_registration_already_consumed')
    body = {'schema': SCHEMA, 'split': 'calibration',
            'dataset_file': str(Path(dataset_file).resolve(strict=True)), 'dataset_sha256': file_hash(dataset_file),
            'dataset_hash': data['artifact_hash'], 'cohort_file': data['cohort_file'],
            'cohort_sha256': data['cohort_sha256'], 'cohort_hash': cohort['artifact_hash'],
            'protocol_file': data['protocol_file'], 'protocol_sha256': data['protocol_sha256'],
            'eligible_ids': [r['id'] for r in binary], 'all_allocation_groups': [r['group_id'] for r in data['cases']],
            'independent_label_counts': counts, 'original_jev_reservations': baseline['jev_reservations'],
            'original_jev_reservation_count': len(baseline['jev_reservations']),
            'authorization_file': next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization'),
            'authorization_sha256': baseline['authorization_chain']['current_sha256'],
            'shared_ledger_authorization_sha256': baseline['authorization_chain']['ledger_authorization_sha256'],
            'registration_file': str(marker), 'source_bindings': _owner_bindings(),
            'original_evidence_bindings': _evidence_bindings(data, cohort),
            'original_proof_verified': True, 'registered_before_allocation_jev': True,
            'whole_allocation_retained': True, 'scores_hidden': True, 'jev_probabilities_read': False,
            'formal_registration': True, 'formal_certification': False, 'certified_threshold': None,
            'holdout_opened': False, 'production_authorized': False, 'model_calls': 0, 'jev_calls': 0,
            'oracle_executions': 0, 'reservations': 0}
    require(all(file_hash(b['file']) == b['sha256'] for b in body['source_bindings']), 'registration_owner_changed')
    return {**body, 'artifact_hash': digest(body)}


def _under_lock(root, value):
    # The original reader refuses an active reservation.lock. Inspect the same
    # exact files here only while this process owns that lock; no second ledger.
    paths = list((root / 'reservations').glob('*.json'))
    expected = {digest(row['group_id']) + '.json': row for row in value['original_jev_reservations']}
    require({p.name for p in paths} == set(expected)
            and all(_same(load(path), expected[path.name]) for path in paths),
            'jev_namespace_changed_during_registration')
    require(load(root / 'authorization.json') == {
        'authorization_sha256': value['shared_ledger_authorization_sha256'], 'kind': 'jev'},
        'original_shared_ledger_metadata_changed')
    require(file_hash(value['dataset_file']) == value['dataset_sha256']
            and file_hash(value['cohort_file']) == value['cohort_sha256']
            and file_hash(value['protocol_file']) == value['protocol_sha256']
            and all(file_hash(b['file']) == b['sha256'] for b in value['source_bindings'] + value['original_evidence_bindings']),
            'original_registration_input_changed')


def register(dataset_file):
    """Write the one genuine shared registration marker; never a paid slot."""
    value = registration_value(dataset_file)
    root = Path(value['registration_file']).parent.parent
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.write(descriptor, str(os.getpid()).encode())
        os.fsync(descriptor)
        _under_lock(root, value)
        directory = root / NAMESPACE
        require(not directory.is_symlink(), 'registration_namespace_symlink')
        directory.mkdir(exist_ok=True)
        durable_new(Path(value['registration_file']), value)
    finally:
        os.close(descriptor)
        lock.unlink()
    return value


def revalidate(registration_file):
    """Rejoin actual original data and the durable before-Jev prefix."""
    value = sealed(load(registration_file))
    require(value['schema'] == SCHEMA and value['split'] == 'calibration'
            and value['formal_registration'] is True and value['original_proof_verified'] is True
            and value['registered_before_allocation_jev'] is True and value['scores_hidden'] is True
            and value['jev_probabilities_read'] is False and value['whole_allocation_retained'] is True
            and all(value[k] is False for k in ('formal_certification', 'holdout_opened', 'production_authorized'))
            and value['certified_threshold'] is None
            and all(type(value[k]) is int and value[k] == 0 for k in ('model_calls', 'jev_calls', 'oracle_executions', 'reservations')),
            'genuine_prediction_blind_registration_required')
    data, cohort, binary, counts, current = _originals(value['dataset_file'])
    require(file_hash(value['dataset_file']) == value['dataset_sha256'] and data['artifact_hash'] == value['dataset_hash']
            and data['cohort_file'] == value['cohort_file'] and data['cohort_sha256'] == value['cohort_sha256']
            and cohort['artifact_hash'] == value['cohort_hash']
            and data['protocol_file'] == value['protocol_file'] and data['protocol_sha256'] == value['protocol_sha256']
            and value['eligible_ids'] == [r['id'] for r in binary]
            and value['all_allocation_groups'] == [r['group_id'] for r in data['cases']]
            and counts == value['independent_label_counts'], 'original_registered_allocation_changed')
    require(_same(value['source_bindings'], _owner_bindings()), 'actual_registration_owner_changed')
    require(_same(value['original_evidence_bindings'], _evidence_bindings(data, cohort)),
            'actual_registered_original_evidence_changed')
    root = _root(current)
    expected = root / NAMESPACE / (cohort['artifact_hash'] + '.json')
    require(Path(registration_file).resolve(strict=True) == expected.resolve(strict=True)
            and value['registration_file'] == str(expected), 'genuine_shared_registration_marker_required')
    authorization_file = next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization')
    require(value['authorization_file'] == authorization_file
            and current['authorization_chain']['current_sha256'] == value['authorization_sha256']
            and current['authorization_chain']['ledger_authorization_sha256'] == value['shared_ledger_authorization_sha256'],
            'registration_authorization_changed')
    old = value['original_jev_reservations']
    require(type(value['original_jev_reservation_count']) is int
            and value['original_jev_reservation_count'] == len(old)
            and sorted(row['reservation_index'] for row in old) == list(range(1, len(old) + 1))
            and _same(sorted(old, key=lambda r: r['reservation_index']),
                      sorted((row for row in current['jev_reservations']
                              if row['reservation_index'] <= len(old)), key=lambda r: r['reservation_index']))
            and all(any(_same(row, before) for before in old)
                    for row in cohort['ledger_snapshot']['jev_reservations']),
            'original_before_jev_reservation_prefix_changed')
    prior_groups = {row['group_id'] for row in old}
    require(not prior_groups & set(value['all_allocation_groups']), 'allocation_was_not_prediction_blind')
    for row in current['jev_reservations']:
        if row['group_id'] in value['all_allocation_groups']:
            require(row['reservation_index'] > value['original_jev_reservation_count'], 'jev_started_before_registration')
    return value, data, binary
