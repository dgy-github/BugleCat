"""Source-only fixed STDIN allocation proposal, no paid or holdout authority.

Select before any new candidate/probability. Freeze the entire development
allocation and both later stages; unused sources cannot repair outcome quotas.
"""
import hashlib
import sys
from collections import Counter
from copy import deepcopy
from pathlib import Path

from ..apps_campaign_v3 import approved_budget
from ..artifact_profile_cohort import SETTINGS, validate_profile
from ..artifact_scoring import sealed
from ..artifact_v2_development_cohort import system_prompt
from ..calibration import digest, load
from ..collection_manifest import file_hash
from . import packet, protocol

ROOT = Path(__file__).resolve().parents[3]
SCHEMA = 'buglecat.verification.stdin-source-only-sampling-proposal.v1'
SEED = 'stdin-natural-400-v1'
STAGES = ('development', 'calibration', 'holdout')
STRATA = {'development': {'interview': 4, 'competition': 4},
          'calibration': {'interview': 340, 'competition': 60},
          'holdout': {'interview': 340, 'competition': 60}}
LEGACY = (
    ('reports/.nanocodex/artifact-v3-apps-development-20261011/cohort.json',
     '339f18fc9d2c02e02df08e452f2ee4d9cbe7b384f2ecf12ee939ee1350f62974', 8),
    ('reports/.nanocodex/artifact-v3-apps-formal316-20261011/cohort.json',
     '9743e41ae1fdadfbad950dfcbc430ecdc7c0f259696c02fbdf29651a816e55e8', 632),
)


def rank(group_id):
    return hashlib.sha256((SEED + ':' + group_id).encode()).hexdigest()


def choose(cases, excluded):
    """Pure stable selection; never infer freshness from source count alone."""
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'SEED', 'STAGES', 'STRATA', 'LEGACY'))
    packet.require(type(cases) is list and type(excluded) in (list, set, tuple)
                   and all(type(g) is str and bool(g) for g in excluded), 'complete source/exclusion lists required')
    group_ids, task_ids, members = set(), set(), set()
    for case in cases:
        packet.require(type(case) is dict and type(case.get('group_id')) is str and bool(case['group_id'])
                       and type(case.get('task_id')) is int and case['task_id'] >= 0
                       and case['group_id'] not in group_ids and case['task_id'] not in task_ids,
                       'unique original source representatives required')
        original = case.get('original_members')
        packet.require(type(original) is list and bool(original)
                       and all(type(m) is str and bool(m) for m in original)
                       and len(set(original)) == len(original) and not members.intersection(original),
                       'whole original groups cannot overlap')
        group_ids.add(case['group_id'])
        task_ids.add(case['task_id'])
        members.update(original)
    excluded = set(excluded)
    pools = {difficulty: sorted((case for case in cases if case['group_id'] not in excluded
                               and case.get('original_difficulty') == difficulty),
                               key=lambda case: (rank(case['group_id']), case['group_id']))
             for difficulty in ('interview', 'competition')}
    packet.require(all(len(pools[d]) >= sum(STRATA[s][d] for s in STAGES) for d in pools),
                   'insufficient fresh complex groups for complete fixed allocation')
    offsets, result = Counter(), {}
    ordinal = 0
    for split in STAGES:
        selected = []
        for difficulty in ('interview', 'competition'):
            start = offsets[difficulty]
            count = STRATA[split][difficulty]
            for original in pools[difficulty][start:start + count]:
                ordinal += 1
                selected.append({**deepcopy(original), 'split': split,
                                 'id': f'stdin-v1-{split}-main-{original["task_id"]}',
                                 'difficulty_hash_rank': rank(original['group_id']),
                                 'allocation_ordinal': ordinal})
            offsets[difficulty] += count
        result[split] = selected
    return result


def legacy_exclusions():
    excluded, bindings = set(), []
    for relative, expected, count in LEGACY:
        path = ROOT / relative
        packet.require(file_hash(path) == expected, 'complete old allocation bytes changed')
        value = sealed(load(path))
        packet.require(type(value['cases']) is list and len(value['cases']) == count,
                       'complete old development/calibration/holdout allocation required')
        excluded.update(value.get('excluded_groups', []))
        excluded.update(case['group_id'] for case in value['cases'])
        bindings.append({'file': str(path.resolve()), 'sha256': expected})
    return excluded, bindings


def bindings(paths, original):
    result = {}
    for item in original:
        path = str(Path(item['file']).resolve(strict=True))
        packet.require(file_hash(path) == item['sha256'], 'original source/proposal binding changed')
        if path in result:
            packet.require(result[path]['sha256'] == item['sha256'], 'conflicting source binding')
        result[path] = {'file': path, 'sha256': item['sha256']}
    for path in paths:
        path = str(Path(path).resolve(strict=True))
        actual = {'file': path, 'sha256': file_hash(path)}
        packet.require(path not in result or result[path] == actual, 'conflicting current owner binding')
        result[path] = actual
    return [result[path] for path in sorted(result)]


def build(authority_file, authorization_file, profile_file):
    """Read-only concrete proposal, never collection approval or registration."""
    from . import source_authority
    locked = [{'file': str(Path(path).resolve(strict=True)), 'sha256': file_hash(path)}
              for path in (authority_file, authorization_file, profile_file)]
    authority = source_authority.validate(authority_file)
    source_authority.check(locked)
    profile, profile_bindings = validate_profile(profile_file)
    source_authority.check(locked)
    baseline, finished = approved_budget.snapshot(authorization_file)
    source_authority.check(locked)
    input_sha = {item['file']: item['sha256'] for item in locked}
    old, old_bindings = legacy_exclusions()
    excluded = old | set(baseline['excluded_groups'])
    allocation = choose(authority['cases'], excluded)
    owners = [Path(__file__), Path(protocol.__file__), Path(approved_budget.__file__),
              Path(source_authority.__file__), authority_file, authorization_file, profile_file]
    owners += list(packet.dependency_files().values())
    owners += [ROOT / 'evals/verification/artifact_gate.py',
               ROOT / 'evals/verification/artifact_v2_formal.py',
               ROOT / 'evals/verification/artifact_profile_cohort.py',
               ROOT / 'evals/verification/artifact_v2_development_cohort.py',
               ROOT / 'rust/crates/ncx-core/examples/verification_collect/support.rs']
    all_bindings = bindings(owners, [*locked, *authority['bindings'], *old_bindings, *profile_bindings,
                                     *baseline['authorization_chain']['bindings']])
    main_used = len(baseline['main_started'])
    jev_used = len(baseline['jev_reservations'])
    main_remaining = baseline['remaining_main_attempts']
    jev_cap = baseline['authorization_chain']['authorization']['jev']['max_attempts']
    body = {'schema': SCHEMA, 'seed': SEED, 'cases': allocation, 'strata': deepcopy(STRATA),
            'source_authority_file': str(Path(authority_file).resolve()),
            'source_authority_sha256': input_sha[str(Path(authority_file).resolve())],
            'source_authority_hash': authority['artifact_hash'],
            'authorization_file': str(Path(authorization_file).resolve()),
            'authorization_sha256': input_sha[str(Path(authorization_file).resolve())],
            'current_ledger_snapshot_sha256': digest(baseline),
            'current_completion_snapshot_sha256': digest(finished),
            'main_ledger_file': baseline['main_ledger_file'],
            'excluded_started_reserved_or_complete_old_allocation_groups': sorted(excluded),
            'main_resource_profile_file': str(Path(profile_file).resolve()),
            'main_resource_profile_sha256': input_sha[str(Path(profile_file).resolve())],
            'main_resource_profile_hash': profile['artifact_hash'], 'main_collection_settings': deepcopy(SETTINGS),
            'public_system_prompt_sha256': hashlib.sha256(system_prompt().encode()).hexdigest(),
            'packet_dependency_sha256': packet.dependencies(), 'oracle_runtime_binding': packet.runtime_info(),
            'protocol': protocol.specification(), 'bindings': all_bindings,
            'whole_development_allocation_excluded_from_formal': True,
            'membership_changes_after_candidates_or_probabilities_allowed': False,
            'original_main_started': main_used, 'original_phase_jev_reservations': jev_used,
            'maximum_new_main_requests': 808, 'maximum_new_jev_requests': 808,
            'additional_main_requests_needed': max(0, 808 - main_remaining),
            'additional_jev_requests_needed': max(0, 808 - (jev_cap - jev_used)),
            'model_calls': 0, 'jev_calls': 0, 'oracle_executions': 0, 'reservations': 0,
            'candidate_labels_read': False, 'jev_probabilities_read': False,
            'collection_authorized': False, 'formal_cohort_created': False, 'formal_registration': False,
            'holdout_opened': False, 'production_authorized': False, 'certified_threshold': None,
            'natural_negative_sufficiency_proven': False,
            'scope': 'Source-only fixed proposal; actual collector/cohort/budget admission still required'}
    packet.require(approved_budget.snapshot(authorization_file) == (baseline, finished),
                   'source-only proposal shared accounting changed')
    packet.require(all(file_hash(item['file']) == item['sha256'] for item in all_bindings),
                   'source-only proposal owner graph changed')
    source_authority.check(locked)
    return {**body, 'artifact_hash': digest(body)}


def validate(path):
    """Pre-collection check against the current ledger; not a paid runtime gate.

    A future frozen cohort must authenticate its original baseline and permitted
    append-only prefix through the budget owner after new starts are recorded.
    Rebuilding this proposal from a changed current ledger is deliberately denied.
    """
    value = sealed(load(path))
    expected = build(value['source_authority_file'], value['authorization_file'], value['main_resource_profile_file'])
    packet.require(packet.canonical_bytes(value) == packet.canonical_bytes(expected),
                   'source-only sampling proposal changed')
    return value
