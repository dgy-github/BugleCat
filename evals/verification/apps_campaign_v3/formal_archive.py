"""Replay original calibration after an authorized append-only holdout starts.

No cached decisions: original main/Oracle and every original Jev byte/event
join are recomputed. A fixed source marker pins the complete pre-holdout ledger
prefix. Only correctly bound, ordered holdout additions are accepted.
"""
from collections import Counter
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import load, require
from ..collection_manifest import file_hash
from . import (
    formal_collection,
    formal_evaluation,
    formal_holdout,
    formal_intake,
    formal_jev,
    formal_registration,
    packet,
)


def _archive_originals(dataset_file, *, pack):
    data = replay_dataset(dataset_file, pack=pack)
    cohort = pack[0]
    require(data['schema'] == formal_intake.SCHEMA and data['split'] == 'calibration'
            and all(data[k] is False for k in ('formal_registration', 'holdout_opened', 'formal_certification', 'production_authorized'))
            and data['certified_threshold'] is None and data['scores_hidden'] is True and data['jev_probabilities_read'] is False,
            'prediction_blind_original_dataset_required')
    fixed = [c for c in cohort['cases'] if c['split'] == 'calibration']
    require(len(data['cases']) == len(fixed) == data['allocated_groups'] == data['collected_groups']
            and data['uncollected_groups'] == data['pending_groups'] == 0, 'complete_original_terminal_allocation_required')
    require(all(row['main_status'] not in {'main_not_started', 'main_not_finished'}
                and row['call_eligible'] is (row['label'] in {'supported', 'unsupported'}) for row in data['cases']),
            'original_independent_terminal_eligibility_required')
    spec = formal_collection.protocol.validate(load(data['protocol_file']))
    counts = {label: sum(row['label'] == label for row in data['cases']) for label in ('supported', 'unsupported', 'unknown', 'unlabeled')}
    require(counts['supported'] >= spec['min_supported'] and counts['unsupported'] >= spec['min_unsupported'], 'original_150_50_labels_required')
    return data, cohort, [r for r in data['cases'] if r['call_eligible']], counts, pack[3]


def _prefix(value, current, finished, cohort):
    before = value['before_holdout_snapshot']
    require(formal_jev.same(current['main_ledger_file'], before['main_ledger_file'])
            and formal_jev.same(current['main_metadata'], before['main_metadata'])
            and formal_jev.same(current['authorization_chain'], before['authorization_chain'])
            and formal_jev.same(current['main_started'][:len(before['main_started'])], before['main_started'])
            and all(formal_jev.same(finished.get(g), f) for g, f in value['before_holdout_finished'].items()),
            'original_pre_holdout_main_prefix_changed')
    for field in ('main_reservations', 'jev_reservations'):
        old = sorted(before[field], key=lambda r: r['reservation_index'])
        actual = sorted(current[field], key=lambda r: r['reservation_index'])
        require(formal_jev.same(actual[:len(old)], old), 'original_pre_holdout_reservation_prefix_changed')
        require(all(r['group_id'] in value['allocation_groups']
                    and r.get('holdout_source_hash') == value['artifact_hash'] for r in actual[len(old):]),
                'unbound_or_other_scope_reservation_after_holdout')
    added = current['main_started'][len(before['main_started']):]
    cases = value['cases']
    require(len(added) <= len(cases), 'holdout_main_allocation_exceeded')
    for row, case in zip(added, cases):
        start = row['started']
        require(row['group_id'] == case['group_id']
                and all(formal_jev.same(start.get(k), case[k]) for k in ('id', 'group_id', 'task_id', 'source_kind', 'task_sha256', 'split'))
                and start.get('artifact_cohort_hash') == cohort['artifact_hash']
                and start.get('holdout_source_hash') == value['artifact_hash']
                and start.get('holdout_source_file') == value['marker_file']
                and start.get('holdout_source_sha256') == file_hash(value['marker_file']),
                'actual_holdout_start_requires_exact_once_marker_and_fixed_prefix')


def _pack(value):
    cohort = sealed(load(value['cohort_file']))
    require(file_hash(value['cohort_file']) == value['cohort_sha256'] and cohort['artifact_hash'] == value['cohort_hash']
            and all(file_hash(b['file']) == b['sha256'] for b in cohort['bindings']), 'archived_formal_source_changed')
    require(all(any(formal_jev.same(b, original) for b in cohort['bindings']) for original in formal_collection.owners()),
            'archived_formal_owner_graph_changed')
    formal_collection.protocol.validate(load(cohort['protocol_file']))
    require(file_hash(cohort['protocol_file']) == cohort['protocol_sha256']
            and formal_jev.same(cohort['oracle_runtime_binding'], packet.runtime_info())
            and formal_jev.same(cohort['main_collection_settings'], formal_collection.SETTINGS)
            and cohort['packet_source_sha256'] == formal_collection.development.PACKET_SHA,
            'archived_protocol_runtime_or_main_settings_changed')
    profile, _ = formal_collection.validate_profile(cohort['main_resource_profile_file'])
    require(profile['artifact_hash'] == cohort['main_resource_profile_hash']
            and file_hash(cohort['main_resource_profile_file']) == cohort['main_resource_profile_sha256'], 'archived_profile_changed')
    _, tasks, train = formal_collection._source_pack(cohort)
    require(formal_jev.same(formal_collection._funded(cohort['ledger_snapshot'], len(cohort['cases']) // 2), cohort['funded_allocation'])
            and formal_jev.same(formal_collection.approved_budget.bridge(
                cohort['source_proposal_baseline'], cohort['source_proposal_completed'], cohort['ledger_snapshot'],
                cohort['source_proposal_completed']), cohort['approved_budget_bridge']), 'archived_funding_bridge_changed')
    authorization = next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization')
    current, finished = formal_collection.budget_snapshot(authorization)
    _prefix(value, current, finished, cohort)
    formal_collection._history(cohort, value['before_holdout_snapshot'], value['before_holdout_finished'])
    before = value['before_holdout_snapshot']
    spent = {r['group_id'] for r in before['main_started']} | {r['group_id'] for r in before['main_reservations']}
    require(before['remaining_main_attempts'] == before['authorization_chain']['authorization']['main_model']['max_attempts'] - len(spent),
            'pre_holdout_remaining_budget_changed')
    return cohort, tasks, train, current, finished


def replay_dataset(dataset_file, *, pack):
    """Reuse original strict row joins, substituting no decisions or labels."""
    data = sealed(load(dataset_file))
    cohort, tasks, _, snapshot, finished = pack
    compiled = formal_intake.build(data['collector_executable_file'])
    starts = {r['group_id']: r for r in snapshot['main_started']}
    fixed = [c for c in cohort['cases'] if c['split'] == 'calibration']
    own = {c['group_id']: starts[c['group_id']] for c in fixed if c['group_id'] in starts}
    require(len(own) == len(fixed) and all(finished.get(g) is not None for g in own), 'complete_original_terminal_calibration_required')
    actual, bindings = {}, []
    directories = [Path(b['directory']).resolve(strict=True) for b in data['collection_bindings']]
    require(len(directories) == len(set(directories)), 'duplicate_archived_collection')
    for directory in directories:
        rows, binding = formal_intake._prepared(directory, data['collector_executable_file'], compiled, pack, starts, finished)
        bindings.append(binding)
        for row in rows:
            if row[5]:
                group = row[0]['group_id']
                require(group in own and group not in actual, 'duplicate_or_other_scope_archived_candidate')
                actual[group] = row
    require(set(actual) == set(own), 'original_calibration_start_or_receipt_omitted')
    outcomes = [formal_intake._outcome(c, actual[c['group_id']], tasks[c['task_id']], False) for c in fixed]
    body = {k: v for k, v in data.items() if k != 'artifact_hash'}
    body.update(cases=outcomes, collection_bindings=bindings, build_info=compiled,
                collector_executable_sha256=compiled['executable_sha256'], allocated_groups=len(fixed),
                collected_groups=len(own), uncollected_groups=0, pending_groups=0,
                label_counts=dict(Counter(c['label'] for c in outcomes)))
    require(formal_jev.same(body, {k: v for k, v in data.items() if k != 'artifact_hash'}), 'archived_original_main_or_oracle_changed')
    return data


def _source_proof(marker_file, value=None):
    """Rebuild original qualifying result, authenticate the shared fixed marker."""
    original = sealed(load(marker_file))
    value = original if value is None else value
    require(formal_jev.same(value, original), 'actual_fixed_source_marker_changed')
    require(value['schema'] == formal_holdout.SCHEMA and value['split'] == 'holdout'
            and value['holdout_collection_authorized'] is True and value['qualified_original_calibration_verified'] is True
            and all(value[k] is False for k in ('threshold_retuning_allowed', 'holdout_labels_read', 'holdout_probabilities_read',
                                               'production_authorized', 'formal_certification', 'native_claim_certification',
                                               'overall_agent_benefit_certification'))
            and value['certified_threshold'] is None
            and all(type(value[k]) is int and value[k] == 0 for k in ('new_paid_calls', 'reservations', 'oracle_executions')),
            'actual_shared_once_source_contract_required')
    require(all(file_hash(b['file']) == b['sha256'] for b in value['source_bindings'])
            and file_hash(value['calibration_file']) == value['calibration_sha256'], 'original_once_proof_changed')
    before = value['before_holdout_snapshot']
    root = Path(before['authorization_chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    expected = root / formal_holdout.NAMESPACE / (value['cohort_hash'] + '.json')
    require(Path(marker_file).resolve(strict=True) == expected.resolve(strict=True)
            and value['marker_file'] == str(expected) and value['ledger_root'] == str(root), 'shared_fixed_once_marker_required')
    pack = _pack(value)
    cohort = pack[0]
    holdout = [c for c in cohort['cases'] if c['split'] == 'holdout']
    require(formal_jev.same(value['cases'], holdout) and value['allocation_groups'] == [c['group_id'] for c in holdout]
            and not set(value['allocation_groups']) & {r['group_id'] for r in before['main_started']}
            and not set(value['allocation_groups']) & {r['group_id'] for field in ('main_reservations', 'jev_reservations') for r in before[field]},
            'fixed_prediction_blind_holdout_changed')
    reg = revalidate_registration(value['registration_file'], pack=pack)
    cal = sealed(load(value['calibration_file']))
    actual = formal_evaluation.calibration_value(cal['registration_file'], cal['allocation_file'], cal['observations_file'],
                                                cal['runner_executable_file'], _original_registration=reg, _verified_pack=pack)
    require(formal_jev.same(actual, cal) and cal['artifact_hash'] == value['calibration_hash']
            and value['registration_sha256'] == file_hash(value['registration_file'])
            and cal['qualified_for_once_holdout'] is True and type(value['fixed_threshold']) in {int, float}
            and value['fixed_threshold'] == cal['qualified_for_holdout_threshold'], 'original_qualified_threshold_or_evidence_changed')
    require(formal_jev.same(value['source_bindings'], formal_holdout._bindings(cal)), 'complete_original_archive_bindings_required')
    require(formal_jev.same(sealed(load(marker_file)), original)
            and all(file_hash(b['file']) == b['sha256'] for b in value['source_bindings'])
            and formal_jev.same(formal_collection.budget_snapshot(next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization')),
                                (pack[3], pack[4])), 'source_or_shared_history_changed_during_archive_replay')
    return value, pack, reg, cal


def proof_transaction(marker_file):
    """Actual full original proof once; typed pack only lives in this call."""
    value, pack, _, _ = _source_proof(marker_file)
    return value, pack


def validate_source(marker_file, value=None):
    return _source_proof(marker_file, value)[0]


# Registration structural/prefix checks retained from the separately bound owner.
_same = formal_registration._same
_root = formal_registration._root
_owner_bindings = formal_registration._owner_bindings
_evidence_bindings = formal_registration._evidence_bindings
SCHEMA = formal_registration.SCHEMA
NAMESPACE = formal_registration.NAMESPACE
def revalidate_registration(registration_file, *, pack):
    """Rejoin actual original data and the durable before-Jev prefix."""
    value = sealed(load(registration_file))
    require(value['schema'] == SCHEMA and value['split'] == 'calibration' and (value['formal_registration'] is True) and (value['original_proof_verified'] is True) and (value['registered_before_allocation_jev'] is True) and (value['scores_hidden'] is True) and (value['jev_probabilities_read'] is False) and (value['whole_allocation_retained'] is True) and all((value[k] is False for k in ('formal_certification', 'holdout_opened', 'production_authorized'))) and (value['certified_threshold'] is None) and all((type(value[k]) is int and value[k] == 0 for k in ('model_calls', 'jev_calls', 'oracle_executions', 'reservations'))), 'genuine_prediction_blind_registration_required')
    data, cohort, binary, counts, current = _archive_originals(value['dataset_file'], pack=pack)
    require(file_hash(value['dataset_file']) == value['dataset_sha256'] and data['artifact_hash'] == value['dataset_hash'] and (data['cohort_file'] == value['cohort_file']) and (data['cohort_sha256'] == value['cohort_sha256']) and (cohort['artifact_hash'] == value['cohort_hash']) and (data['protocol_file'] == value['protocol_file']) and (data['protocol_sha256'] == value['protocol_sha256']) and (value['eligible_ids'] == [r['id'] for r in binary]) and (value['all_allocation_groups'] == [r['group_id'] for r in data['cases']]) and (counts == value['independent_label_counts']), 'original_registered_allocation_changed')
    require(_same(value['source_bindings'], _owner_bindings()), 'actual_registration_owner_changed')
    require(_same(value['original_evidence_bindings'], _evidence_bindings(data, cohort)), 'actual_registered_original_evidence_changed')
    root = _root(current)
    expected = root / NAMESPACE / (cohort['artifact_hash'] + '.json')
    require(Path(registration_file).resolve(strict=True) == expected.resolve(strict=True) and value['registration_file'] == str(expected), 'genuine_shared_registration_marker_required')
    authorization_file = next((b['file'] for b in cohort['bindings'] if b['role'] == 'authorization'))
    require(value['authorization_file'] == authorization_file and current['authorization_chain']['current_sha256'] == value['authorization_sha256'] and (current['authorization_chain']['ledger_authorization_sha256'] == value['shared_ledger_authorization_sha256']), 'registration_authorization_changed')
    old = value['original_jev_reservations']
    require(type(value['original_jev_reservation_count']) is int and value['original_jev_reservation_count'] == len(old) and (sorted((row['reservation_index'] for row in old)) == list(range(1, len(old) + 1))) and _same(sorted(old, key=lambda r: r['reservation_index']), sorted((row for row in current['jev_reservations'] if row['reservation_index'] <= len(old)), key=lambda r: r['reservation_index'])) and all((any((_same(row, before) for before in old)) for row in cohort['ledger_snapshot']['jev_reservations'])), 'original_before_jev_reservation_prefix_changed')
    prior_groups = {row['group_id'] for row in old}
    require(not prior_groups & set(value['all_allocation_groups']), 'allocation_was_not_prediction_blind')
    for row in current['jev_reservations']:
        if row['group_id'] in value['all_allocation_groups']:
            require(row['reservation_index'] > value['original_jev_reservation_count'], 'jev_started_before_registration')
    return (value, data, binary)
