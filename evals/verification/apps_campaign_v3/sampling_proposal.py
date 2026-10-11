"""Pure source-only allocation proposal, never collection or fee authority.

Difficulty/hash-ranked alternating groups, with complete development/history
exclusion. No candidate labels, probabilities, Oracle, transport or writes.
"""
import argparse
import json
from collections import Counter
from pathlib import Path

from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from . import development, packet, protocol

SCHEMA = 'buglecat.verification.apps-formal-sampling-proposal.v3'
SEED = 'apps-source-only-formal-alternating-difficulty-proposal-v3-20261011'
STAGES = ('calibration', 'holdout')


def _binding(role, path):
    return development.binding(role, path)


def _owners():
    # Original graph validators and their already declared dependencies are
    # reused. A new proposal never edits the graph of a paid owner.
    return [_binding('sampling_proposal_owner', __file__), *development.owners()]


def _bindings(cohort, authority, authority_file, resource_file, cohort_file):
    source_graph = [{**b, 'role': b.get('role', 'original_authority_data')} for b in authority['bindings']]
    source_graph += [{**b, 'role': b.get('role', 'original_authority_owner')}
                     for b in authority.get('owner_bindings', [])]
    values = [_binding('authority', authority_file), _binding('resources', resource_file),
              _binding('development_cohort', cohort_file), *_owners(),
              *cohort['bindings'], *source_graph]
    by_file = {}
    for value in values:
        path = str(Path(value['file']).resolve(strict=True))
        require(file_hash(path) == value['sha256'], 'source_binding_changed')
        if path in by_file:
            require(by_file[path]['sha256'] == value['sha256'], 'conflicting_original_source_bindings')
            by_file[path]['roles'].append(value['role'])
        else:
            by_file[path] = {'file': path, 'sha256': value['sha256'], 'roles': [value['role']]}
    return [{**value, 'roles': sorted(set(value['roles']))} for _, value in sorted(by_file.items())]


def _selected(authority, inventory, excluded, count, interview):
    original = {case['group_id']: case for case in authority['cases']}
    require(len(original) == len(authority['cases']), 'duplicate_original_source_group')
    resource = {case['group_id']: case for case in inventory['cases']}
    require(len(resource) == len(inventory['cases']) and original.keys() == resource.keys(),
            'complete_original_resource_groups_required')
    available = [case for group, case in original.items() if group not in excluded
                 and resource[group]['resource_status'] == 'qualified']
    require(all(case['original_difficulty'] in {'interview', 'introductory'}
                and case['source_kind'] in packet.KINDS for case in available), 'unsupported_source_stratum')
    selected = {stage: [] for stage in STAGES}
    for difficulty, target in (('interview', interview), ('introductory', count - interview)):
        ordered = sorted((case for case in available if case['original_difficulty'] == difficulty),
                         key=lambda case: (digest({'seed': SEED, 'difficulty': difficulty,
                                                   'group_id': case['group_id']}), case['group_id']))
        require(len(ordered) >= 2 * target, 'insufficient_fresh_difficulty_groups')
        # Fix the group representative first. No outcome or fallback reference
        # can replace it; every even/odd ranked member belongs to one stage.
        for rank, original_case in enumerate(ordered[:2 * target]):
            stage = STAGES[rank % 2]
            case = {key: original_case[key] for key in
                    ('group_id', 'task_id', 'task_sha256', 'source_kind', 'original_difficulty', 'pair_count')}
            case.update(id=f'apps-v3-proposed-{stage}-main-{case["task_id"]}', split=stage,
                        original_split='train', difficulty_hash_rank=rank)
            selected[stage].append(case)
    for stage in STAGES:
        # Interleave difficulty members by a second fixed public hash, with no
        # feedback from candidate/Oracle/Jev outcomes.
        selected[stage].sort(key=lambda case: (digest({'seed': SEED, 'stage': stage,
                                                       'group_id': case['group_id']}), case['group_id']))
    return available, selected


def _strata(cases):
    return {'groups': len(cases), 'difficulty': dict(sorted(Counter(c['original_difficulty'] for c in cases).items())),
            'source_kind': dict(sorted(Counter(c['source_kind'] for c in cases).items())),
            'difficulty_source_kind': dict(sorted(Counter(c['original_difficulty'] + ':' + c['source_kind']
                                                        for c in cases).items()))}


def _budget(baseline, count):
    authorization = baseline['authorization_chain']['authorization']
    main_cap, jev_cap = authorization['main_model']['max_attempts'], authorization['jev']['max_attempts']
    main_remaining = baseline['remaining_main_attempts']
    jev_used = len(baseline['jev_reservations'])
    require(all(type(n) is int and n >= 0 for n in (main_cap, jev_cap, main_remaining, jev_used))
            and main_remaining <= main_cap and jev_used <= jev_cap, 'invalid_current_shared_budget')
    total, jev_remaining = 2 * count, jev_cap - jev_used
    return {'current_main_cap': main_cap, 'current_jev_cap': jev_cap,
            'current_main_started': len(baseline['main_started']),
            'current_main_consumed_or_reserved': main_cap - main_remaining,
            'current_jev_reservations': jev_used, 'remaining_main_attempts': main_remaining,
            'remaining_jev_reservations': jev_remaining, 'planned_main_maximum': total, 'planned_jev_maximum': total,
            'holdout_main_earmark_only': count, 'holdout_jev_earmark_only': count,
            'additional_main_authority_required': max(0, total - main_remaining),
            'additional_jev_authority_required': max(0, total - jev_remaining),
            'budget_sufficient_for_maximum_allocation': main_remaining >= total and jev_remaining >= total,
            'worst_case_attempts_not_success_or_negative_guarantee': True,
            'additional_authorization_created': False, 'budget_earmark_written': False}


def proposal(authority_file, resource_inventory_file, development_cohort_file, count=316, interview_per_stage=175):
    """Return a sealed *proposal*; caller alone may persist it as evidence.

    Explicit316 and252 alternatives both preserve175 interview per stage.
    No real formal cohort/permission/holdout marker is created by this API.
    """
    require(type(count) is int and count in {252, 316} and type(interview_per_stage) is int
            and interview_per_stage == 175, 'explicit_fixed_316_or_252_with_175_interview_required')
    cohort = development.validate_cohort(development_cohort_file)
    require(Path(authority_file).samefile(cohort['source_authority_file'])
            and Path(resource_inventory_file).samefile(cohort['resource_inventory_file']),
            'original_development_source_identity_required')
    authority, inventory = development.inputs(authority_file, resource_inventory_file)
    bindings = _bindings(cohort, authority, authority_file, resource_inventory_file, development_cohort_file)
    role_map = {b['role']: b['file'] for b in cohort['bindings']}
    authorization_file = role_map['authorization']
    protocol_file = cohort['protocol_file']
    protocol.validate(load(protocol_file))
    require(file_hash(protocol_file) == cohort['protocol_sha256'], 'original_protocol_changed')
    runtime = packet.runtime_info()
    require(packet.canonical_bytes(runtime) == packet.canonical_bytes(cohort['oracle_runtime_binding']),
            'original_runtime_changed')
    baseline, finished = development.snapshot(authorization_file)
    dev_groups = {case['group_id'] for case in cohort['cases']}
    require(len(dev_groups) == len(cohort['cases']) == 8, 'complete_original_dev8_required')
    excluded = dev_groups | set(baseline['excluded_groups'])
    excluded |= {case['group_id'] for case in baseline['main_started']}
    for kind in ('main_reservations', 'jev_reservations'):
        excluded |= {case['group_id'] for case in baseline[kind]}
    available, stages = _selected(authority, inventory, excluded, count, interview_per_stage)
    selected_groups = {case['group_id'] for rows in stages.values() for case in rows}
    require(len(selected_groups) == 2 * count and not selected_groups & excluded,
            'proposal_group_leakage_or_repetition')
    body = {'schema': SCHEMA, 'seed': SEED, 'count_per_stage': count, 'interview_per_stage': interview_per_stage,
            'introductory_per_stage': count - interview_per_stage, 'cases': stages,
            'available_strata': _strata(available), 'stage_strata': {stage: _strata(rows) for stage, rows in stages.items()},
            'source_authority_file': str(Path(authority_file).resolve()), 'source_authority_sha256': file_hash(authority_file),
            'resource_inventory_file': str(Path(resource_inventory_file).resolve()), 'resource_inventory_sha256': file_hash(resource_inventory_file),
            'development_cohort_file': str(Path(development_cohort_file).resolve()),
            'development_cohort_sha256': file_hash(development_cohort_file), 'development_cohort_hash': cohort['artifact_hash'],
            'protocol_file': str(Path(protocol_file).resolve()), 'protocol_sha256': cohort['protocol_sha256'],
            'source_bindings': bindings, 'oracle_runtime_binding': runtime,
            'current_ledger_snapshot_sha256': digest(baseline), 'current_completion_snapshot_sha256': digest(finished),
            'main_ledger_file': baseline['main_ledger_file'], 'authorization_file': str(Path(authorization_file).resolve()),
            'authorization_chain_bindings': baseline['authorization_chain']['bindings'],
            'excluded_whole_dev_groups': sorted(dev_groups), 'excluded_started_reserved_or_historical_groups': sorted(excluded),
            'source_authority_history_excluded_component_count': authority.get('historical_excluded_groups', 0),
            'budget': _budget(baseline, count), 'all_fixed_allocation_outcomes_retained': True,
            'membership_changes_after_labels_or_probabilities_allowed': False, 'natural_negative_sufficiency_proven': False,
            'official_source_split': 'train', 'host_proposed_splits_are_not_official_apps_test': True,
            'source_only_proposal_not_source_admission_or_certification': True,
            'source_ledger_protocol_runtime_before_after_verified': True,
            'collection_authorized': False, 'formal_cohort_created': False, 'formal_registration': False,
            'holdout_opened': False, 'production_authorized': False, 'candidate_labels_read': False,
            'jev_probabilities_read': False, 'formal_certification': False, 'certified_threshold': None,
            'model_calls': 0, 'jev_calls': 0, 'oracle_executions': 0, 'reservations': 0}
    require(all(file_hash(b['file']) == b['sha256'] for b in bindings), 'source_changed_during_proposal')
    require(packet.canonical_bytes(development.snapshot(authorization_file)) == packet.canonical_bytes((baseline, finished)),
            'current_ledger_changed_during_proposal')
    require(packet.canonical_bytes(packet.runtime_info()) == packet.canonical_bytes(runtime), 'runtime_changed_during_proposal')
    require(all(file_hash(b['file']) == b['sha256'] for b in bindings), 'source_changed_during_proposal')
    return {**body, 'artifact_hash': digest(body)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--authority-file', required=True, type=Path)
    parser.add_argument('--resource-inventory-file', required=True, type=Path)
    parser.add_argument('--development-cohort-file', required=True, type=Path)
    parser.add_argument('--count', type=int, choices=(252, 316), default=316)
    args = parser.parse_args()
    value = proposal(args.authority_file, args.resource_inventory_file, args.development_cohort_file, args.count)
    print(json.dumps({key: value[key] for key in ('schema', 'artifact_hash', 'count_per_stage', 'available_strata',
                                                'stage_strata', 'budget', 'collection_authorized', 'formal_cohort_created',
                                                'candidate_labels_read', 'jev_probabilities_read', 'model_calls', 'jev_calls',
                                                'oracle_executions', 'reservations')}))


if __name__ == '__main__':
    main()
