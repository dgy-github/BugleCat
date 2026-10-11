"""Original full holdout label registration, no numeric/source privilege shortcut."""
import os
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..paid_budget import durable_new
from . import formal_registration, holdout_collection, holdout_intake, packet

SCHEMA = 'buglecat.verification.apps-original-registration.holdout.v3'
NAMESPACE = 'formal-apps-holdout-registrations-v3'


def originals(dataset_file):
    dataset_sha = file_hash(dataset_file)
    data = sealed(load(dataset_file))
    require(data['schema'] == holdout_intake.SCHEMA and data['split'] == 'holdout'
            and data['holdout_opened'] is True and data['scores_hidden'] is True and data['jev_probabilities_read'] is False
            and all(data[k] is False for k in ('formal_registration', 'formal_certification', 'production_authorized'))
            and data['certified_threshold'] is None, 'blind_original_holdout_dataset_required')
    marker, pack = holdout_collection.verified_pack(data['holdout_source_file'])
    captured = {'dataset_sha256': dataset_sha,
                'evidence': formal_registration._evidence_bindings(data, pack[0]) + marker['source_bindings']}
    actual = holdout_intake.collect(data['holdout_source_file'], [b['directory'] for b in data['collection_bindings']],
                                    data['collector_executable_file'], _source_pack=(marker, pack))
    require(packet.canonical_bytes(actual) == packet.canonical_bytes(data), 'actual_complete_holdout_dataset_changed')
    authorization = next(b['file'] for b in pack[0]['bindings'] if b['role'] == 'authorization')
    require(file_hash(dataset_file) == dataset_sha
            and all(file_hash(b['file']) == b['sha256'] for b in marker['source_bindings'])
            and packet.canonical_bytes(formal_registration._evidence_bindings(data, pack[0]) + marker['source_bindings']) == packet.canonical_bytes(captured['evidence'])
            and packet.canonical_bytes(holdout_collection.budget_snapshot(authorization)) == packet.canonical_bytes((pack[3], pack[4])),
            'original_holdout_inputs_or_budget_changed_during_proof_transaction')
    require(data['allocated_groups'] == data['collected_groups'] == len(marker['cases']) == len(data['cases'])
            and data['uncollected_groups'] == data['pending_groups'] == 0
            and all(c['main_status'] not in {'main_not_started', 'main_not_finished'} for c in data['cases']),
            'complete_terminal_holdout_allocation_required')
    require(all(c['call_eligible'] is (c['label'] in {'supported', 'unsupported'}) for c in data['cases']), 'actual_holdout_eligibility_required')
    spec = holdout_collection.formal_collection.protocol.validate(load(data['protocol_file']))
    counts = {label: sum(c['label'] == label for c in data['cases']) for label in ('supported', 'unsupported', 'unknown', 'unlabeled')}
    require(counts['supported'] >= spec['min_supported'] and counts['unsupported'] >= spec['min_unsupported'], 'original_holdout_150_50_required')
    return data, marker, pack, [c for c in data['cases'] if c['call_eligible']], counts, captured


def _base(dataset_file, original, prior):
    data, marker, pack, binary, counts, captured = original
    cohort, _, _, current, _ = pack
    root = Path(current['authorization_chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    path = root / NAMESPACE / (cohort['artifact_hash'] + '.json')
    bindings = [holdout_collection.development.binding(role, path) for role, path in (
        ('holdout_registration_owner', __file__), ('holdout_intake_owner', holdout_intake.__file__),
        ('holdout_collection_owner', holdout_collection.__file__))]
    require(file_hash(dataset_file) == captured['dataset_sha256']
            and all(file_hash(b['file']) == b['sha256'] for b in captured['evidence'])
            and packet.canonical_bytes(formal_registration._evidence_bindings(data, cohort) + marker['source_bindings']) == packet.canonical_bytes(captured['evidence']),
            'original_holdout_evidence_changed_before_registration_projection')
    body = {'schema': SCHEMA, 'split': 'holdout', 'dataset_file': str(Path(dataset_file).resolve(strict=True)),
            'dataset_sha256': captured['dataset_sha256'], 'dataset_hash': data['artifact_hash'],
            'cohort_file': data['cohort_file'], 'cohort_sha256': data['cohort_sha256'], 'cohort_hash': data['cohort_hash'],
            'protocol_file': data['protocol_file'], 'protocol_sha256': data['protocol_sha256'],
            **{k: data[k] for k in ('holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')},
            'eligible_ids': [c['id'] for c in binary], 'all_allocation_groups': [c['group_id'] for c in data['cases']],
            'independent_label_counts': counts, 'original_jev_reservations': prior, 'original_jev_reservation_count': len(prior),
            'authorization_file': next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization'),
            'authorization_sha256': current['authorization_chain']['current_sha256'],
            'shared_ledger_authorization_sha256': current['authorization_chain']['ledger_authorization_sha256'],
            'registration_file': str(path), 'source_bindings': bindings,
            'original_evidence_bindings': captured['evidence'],
            'original_proof_verified': True, 'registered_before_allocation_jev': True, 'whole_allocation_retained': True,
            'scores_hidden': True, 'jev_probabilities_read': False, 'formal_registration': True,
            'formal_certification': False, 'certified_threshold': None, 'holdout_opened': True,
            'production_authorized': False, 'model_calls': 0, 'jev_calls': 0, 'oracle_executions': 0, 'reservations': 0}
    return {**body, 'artifact_hash': digest(body)}


def registration_value(dataset_file):
    original = originals(dataset_file)
    prior = original[2][3]['jev_reservations']
    require(not {c['group_id'] for c in original[0]['cases']} & {r['group_id'] for r in prior}, 'holdout_probability_seen_before_labels')
    value = _base(dataset_file, original, prior)
    require(not Path(value['registration_file']).exists(), 'holdout_registration_already_consumed')
    return value


def register(dataset_file):
    value = registration_value(dataset_file)
    root = Path(value['registration_file']).parent.parent
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.write(descriptor, str(os.getpid()).encode())
        os.fsync(descriptor)
        formal_registration._under_lock(root, value)
        namespace = root / NAMESPACE
        require(not namespace.is_symlink(), 'holdout_registration_namespace_symlink')
        namespace.mkdir(exist_ok=True)
        durable_new(Path(value['registration_file']), value)
    finally:
        os.close(descriptor)
        lock.unlink()
    return value


def proof_transaction(registration_file):
    value = sealed(load(registration_file))
    require(value['schema'] == SCHEMA and value['split'] == 'holdout', 'genuine_holdout_registration_required')
    original = originals(value['dataset_file'])
    current = original[2][3]
    prior = value['original_jev_reservations']
    require(packet.canonical_bytes(prior) == packet.canonical_bytes(original[1]['before_holdout_snapshot']['jev_reservations'])
            and type(value['original_jev_reservation_count']) is int and value['original_jev_reservation_count'] == len(prior)
            and sorted(r['reservation_index'] for r in prior) == list(range(1, len(prior) + 1))
            and packet.canonical_bytes(sorted(prior, key=lambda r: r['reservation_index'])) == packet.canonical_bytes(
                sorted((r for r in current['jev_reservations'] if r['reservation_index'] <= len(prior)), key=lambda r: r['reservation_index']))
            and not set(value['all_allocation_groups']) & {r['group_id'] for r in prior}, 'original_blind_holdout_reservation_prefix_changed')
    actual = _base(value['dataset_file'], original, prior)
    require(packet.canonical_bytes(actual) == packet.canonical_bytes(value)
            and Path(registration_file).resolve(strict=True) == Path(actual['registration_file']).resolve(strict=True), 'actual_shared_holdout_registration_changed')
    return (value, original[0], original[3]), original[2]


def revalidate(path):
    return proof_transaction(path)[0]
