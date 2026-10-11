"""Genuine source/ledger-bound APPS formal main collection, no transport.

Preregister is pure; only Root may persist its returned cohort. Unlike a source
proposal, a cohort fixes actual collection authority and funded whole stages.
Holdout is fail-closed until an APPS original-calibration/once-marker owner exists.
"""
import argparse
import json
from pathlib import Path

from ..artifact_profile_cohort import SETTINGS, validate_profile
from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from . import approved_budget, development, formal_contract, packet, sampling_proposal
from . import prospective_protocol as protocol

SCHEMA = 'buglecat.verification.apps-formal-main-cohort.v3'
MANIFEST = 'buglecat.verification.apps-formal-main-collection.v3'
BIND_FIELDS = (*development.BIND_FIELDS, 'sampling_proposal_file', 'sampling_proposal_sha256', 'sampling_proposal_hash')
ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / 'rust/crates/ncx-core/examples'


def budget_snapshot(authorization):
    """Original roots, approved chain and same logical SQLite schema."""
    return approved_budget.snapshot(authorization)


def owners():
    values = development.owners()
    files = {'formal_collection_owner': Path(__file__), 'formal_intake_owner': Path(__file__).with_name('formal_intake.py'),
             'approved_budget_owner': Path(approved_budget.__file__),
             'prospective_protocol_owner': Path(protocol.__file__),
             'sampling_proposal_owner': Path(sampling_proposal.__file__), 'formal_contract_owner': Path(formal_contract.__file__),
             'formal_collector_source': EXAMPLES / 'verification_apps_v3_formal_collect.rs',
             'formal_collector_engine': EXAMPLES / 'verification_apps_v3_formal_collect/engine.rs',
             'formal_collector_support': EXAMPLES / 'verification_apps_v3_formal_collect/support.rs'}
    files['formal_ledger_source'] = EXAMPLES / 'verification_apps_v3_formal_collect/ledger.rs'
    return values + [development.binding(role, p) for role, p in sorted(files.items())]


def _same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def _funded(baseline, count):
    authorization = baseline['authorization_chain']['authorization']
    jev_remaining = authorization['jev']['max_attempts'] - len(baseline['jev_reservations'])
    require(type(baseline['remaining_main_attempts']) is int and type(jev_remaining) is int
            and min(baseline['remaining_main_attempts'], jev_remaining) >= 2 * count,
            'approved_shared_budget_cannot_fund_complete_calibration_and_holdout')
    return {'main_at_freeze': baseline['remaining_main_attempts'], 'jev_at_freeze': jev_remaining,
            'calibration_main_max': count, 'holdout_main_earmark': count,
            'calibration_jev_max': count, 'holdout_jev_earmark': count,
            'maximum_total_main': 2 * count, 'maximum_total_jev': 2 * count,
            'budget_earmark_is_not_shared_reservation': True}


def _source_pack(value):
    authority, inventory = development.inputs(value['source_authority_file'], value['resource_inventory_file'])
    proposal = sealed(load(value['sampling_proposal_file']))
    require(file_hash(value['sampling_proposal_file']) == value['sampling_proposal_sha256']
            and proposal['artifact_hash'] == value['sampling_proposal_hash'], 'original_sampling_proposal_changed')
    formal_contract._proposal(proposal)
    require(value['development_protocol_file'] == proposal['protocol_file']
            and value['development_protocol_sha256'] == proposal['protocol_sha256'] == development.PROTOCOL_SHA
            and file_hash(value['development_protocol_file']) == development.PROTOCOL_SHA, 'original_development_protocol_history_changed')
    baseline = value['source_proposal_baseline']
    completed = value['source_proposal_completed']
    require(digest(baseline) == proposal['current_ledger_snapshot_sha256']
            and digest(completed) == proposal['current_completion_snapshot_sha256'], 'original_proposal_baseline_changed')
    excluded = set(baseline['excluded_groups']) | set(proposal['excluded_whole_dev_groups'])
    _, fixed = sampling_proposal._selected(authority, inventory, excluded, proposal['count_per_stage'], proposal['interview_per_stage'])
    require(_same(fixed, proposal['cases']) and set(proposal['excluded_started_reserved_or_historical_groups']) == excluded,
            'original_frozen_source_allocation_or_whole_history_changed')
    cases = [dict(case) for split in sampling_proposal.STAGES for case in proposal['cases'][split]]
    tasks, train = development.original_tasks(authority, cases)
    for case in cases:
        task = tasks[case['task_id']]
        case['public_interface'] = {'starter_code': task['starter_code'], 'fn_name': task['input_output']['fn_name'],
                                    'source_kind': case['source_kind']}
    require(_same(cases, value['cases']), 'whole_formal_cases_changed')
    return authority, tasks, train


def _history(value, current, finished):
    baseline = value['ledger_snapshot']
    starts = {r['group_id']: r for r in current['main_started']}
    require(all(_same(starts.get(r['group_id']), r) for r in baseline['main_started'])
            and all(_same(finished.get(group), original) for group, original in value['original_completed'].items())
            and all(dict(current['main_metadata']).get(key) == old for key, old in baseline['main_metadata']),
            'original_shared_history_changed_or_reset')
    for field in ('main_reservations', 'jev_reservations'):
        require(all(packet.canonical_bytes(row) in {packet.canonical_bytes(c) for c in current[field]}
                    for row in baseline[field]), 'original_reservation_deleted_or_changed')
    for case in value['cases']:
        if case['group_id'] in starts:
            started = starts[case['group_id']]['started']
            require(started.get('artifact_cohort_hash') == value['artifact_hash']
                    and all(_same(started.get(k), case[k]) for k in ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')),
                    'allocated_group_started_in_other_scope_or_identity_changed')
            require(case['split'] == 'calibration', 'holdout_start_without_genuine_APPS_once_owner')
    own = [r['group_id'] for r in current['main_started'] if r['started'].get('artifact_cohort_hash') == value['artifact_hash']]
    require(own == [c['group_id'] for c in value['cases'] if c['split'] == 'calibration'][:len(own)],
            'actual_main_starts_must_follow_whole_frozen_prefix')


def preregister(proposal_file, authorization, profile_file, output_directory, *, protocol_file):
    """Root-only pure construction; approval/source snapshots stay distinct."""
    formal_contract.review_proposal(proposal_file)
    proposal = load(proposal_file)
    protocol.validate(load(protocol_file))
    baseline, finished = budget_snapshot(authorization)
    auth = baseline['authorization_chain']['authorization']
    if auth.get('schema') == approved_budget.SCHEMA:
        require(Path(auth['source_proposal_file']).resolve() == Path(proposal_file).resolve(),
                'approved_budget_bound_to_another_source_proposal')
        _, source_baseline, source_finished = approved_budget.source_snapshot(auth['source_snapshot_file'], proposal_file)
    else:
        source_baseline, source_finished = baseline, finished
    require(digest(source_baseline) == proposal['current_ledger_snapshot_sha256']
            and digest(source_finished) == proposal['current_completion_snapshot_sha256'], 'proposal_budget_snapshot_changed')
    budget_bridge = approved_budget.bridge(source_baseline, source_finished, baseline, finished)
    budget = _funded(baseline, proposal['count_per_stage'])
    profile, original_profile = validate_profile(profile_file)
    cases = [dict(c) for split in sampling_proposal.STAGES for c in proposal['cases'][split]]
    authority, _ = development.inputs(proposal['source_authority_file'], proposal['resource_inventory_file'])
    tasks, train = development.original_tasks(authority, cases)
    for case in cases:
        task = tasks[case['task_id']]
        case['public_interface'] = {'starter_code': task['starter_code'], 'fn_name': task['input_output']['fn_name'], 'source_kind': case['source_kind']}
    dependencies = owners() + [development.binding(role, path) for role, path in (
        ('sampling_proposal', proposal_file), ('authorization', authorization), ('profile', profile_file), ('source', train),
        ('source_authority', proposal['source_authority_file']), ('resource_inventory', proposal['resource_inventory_file']),
        ('protocol', protocol_file))]
    dependencies += [{**b, 'role': 'original_proposal_graph'} for b in proposal['source_bindings']]
    dependencies += [development.binding('authorization_chain', b['file']) for b in baseline['authorization_chain']['bindings']]
    if auth.get('schema') == approved_budget.SCHEMA:
        dependencies.append(development.binding('source_proposal_snapshot', auth['source_snapshot_file']))
    dependencies += [development.binding('profile_original', b['file']) for b in original_profile]
    body = {'schema': SCHEMA, 'cases': cases, 'bindings': dependencies, 'ledger_snapshot': baseline,
            'source_proposal_baseline': source_baseline, 'source_proposal_completed': source_finished,
            'approved_budget_bridge': budget_bridge,
            'original_completed': {g: f for g, f in finished.items() if f is not None},
            'sampling_proposal_file': str(Path(proposal_file).resolve()), 'sampling_proposal_sha256': file_hash(proposal_file),
            'sampling_proposal_hash': proposal['artifact_hash'], 'excluded_groups': proposal['excluded_started_reserved_or_historical_groups'],
            'output_directory': str(Path(output_directory).resolve()), 'funded_allocation': budget,
            'main_collection_settings': SETTINGS, 'main_system_prompt_sha256': development.source.raw_sha(development.system_prompt().encode()),
            'main_resource_profile_file': str(Path(profile_file).resolve()), 'main_resource_profile_sha256': file_hash(profile_file),
            'main_resource_profile_hash': profile['artifact_hash'], 'source_authority_file': proposal['source_authority_file'],
            'source_authority_sha256': proposal['source_authority_sha256'], 'resource_inventory_file': proposal['resource_inventory_file'],
            'resource_inventory_sha256': proposal['resource_inventory_sha256'], 'packet_source_sha256': development.PACKET_SHA,
            'oracle_runtime_binding': packet.runtime_info(), 'protocol_file': str(Path(protocol_file).resolve()), 'protocol_sha256': file_hash(protocol_file),
            'development_protocol_file': proposal['protocol_file'], 'development_protocol_sha256': proposal['protocol_sha256'],
            'collection_authorized': True, 'collection_authorized_split': 'calibration', 'before_main_requests': True,
            'candidate_labels_assigned': False, 'formal_registration': False, 'holdout_opened': False,
            'production_authorized': False, 'formal_certification': False, 'certified_threshold': None, 'new_paid_calls': 0,
            'actual_APPS_holdout_owner_available': False, 'source_only_proposal_is_not_fee_authority': True}
    require(all(file_hash(b['file']) == b['sha256'] for b in dependencies)
            and _same(budget_snapshot(authorization), (baseline, finished)), 'source_or_shared_budget_changed_during_cohort')
    return {**body, 'artifact_hash': digest(body)}


def verified_pack(cohort_file):
    """Batch transaction: complete real source/history/runtime proof once."""
    value = sealed(load(cohort_file))
    require(value['schema'] == SCHEMA and value['collection_authorized'] is True
            and value['collection_authorized_split'] == 'calibration' and value['before_main_requests'] is True
            and all(value[k] is False for k in ('candidate_labels_assigned', 'formal_registration', 'holdout_opened',
                                                'production_authorized', 'formal_certification', 'actual_APPS_holdout_owner_available'))
            and value['certified_threshold'] is None and type(value['new_paid_calls']) is int and value['new_paid_calls'] == 0,
            'genuine_calibration_collection_only_contract_required')
    require(all(file_hash(b['file']) == b['sha256'] for b in value['bindings']), 'formal_source_graph_changed')
    require(all(any(_same(bound, expected) for bound in value['bindings']) for expected in owners()),
            'complete_actual_formal_owner_graph_required')
    protocol.validate(load(value['protocol_file']))
    require(file_hash(value['protocol_file']) == value['protocol_sha256']
            and value['packet_source_sha256'] == development.PACKET_SHA
            and _same(value['oracle_runtime_binding'], packet.runtime_info())
            and _same(value['main_collection_settings'], SETTINGS)
            and value['main_system_prompt_sha256'] == development.source.raw_sha(development.system_prompt().encode()), 'formal_protocol_runtime_settings_changed')
    profile, _ = validate_profile(value['main_resource_profile_file'])
    require(profile['artifact_hash'] == value['main_resource_profile_hash']
            and file_hash(value['main_resource_profile_file']) == value['main_resource_profile_sha256'], 'formal_profile_changed')
    _, tasks, train = _source_pack(value)
    require(_same(approved_budget.bridge(value['source_proposal_baseline'], value['source_proposal_completed'],
                                        value['ledger_snapshot'], value['source_proposal_completed']),
                  value['approved_budget_bridge']), 'original_approved_budget_bridge_changed')
    count = len(value['cases']) // 2
    require(_same(_funded(value['ledger_snapshot'], count), value['funded_allocation']), 'original_funding_or_allocation_changed')
    authorization = next(b['file'] for b in value['bindings'] if b['role'] == 'authorization')
    current, finished = budget_snapshot(authorization)
    _history(value, current, finished)
    started = {r['group_id'] for r in current['main_started']}
    reserved = {r['group_id'] for r in current['jev_reservations']}
    all_groups = {c['group_id'] for c in value['cases']}
    remaining_jev = current['authorization_chain']['authorization']['jev']['max_attempts'] - len(current['jev_reservations'])
    require(current['remaining_main_attempts'] >= len(all_groups - started)
            and remaining_jev >= len(all_groups - reserved), 'remaining_shared_budget_no_longer_funds_complete_allocation')
    require(all(file_hash(b['file']) == b['sha256'] for b in value['bindings']), 'source_changed_during_batch_proof')
    return value, tasks, train, current, finished


def validate_cohort(path):
    return verified_pack(path)[0]


def _manifest(cohort_file, cohort, tasks, train, cases):
    authorization = next(b for b in cohort['bindings'] if b['role'] == 'authorization')
    body = {'schema': MANIFEST, 'split': 'calibration', 'formal_registration': False, 'cases': cases, **SETTINGS,
            'artifact_cohort_file': str(Path(cohort_file).resolve()), 'artifact_cohort_sha256': file_hash(cohort_file),
            'artifact_cohort_hash': cohort['artifact_hash'], 'source_file': str(Path(train).resolve()), 'source_sha256': development.source.TRAIN_SHA256,
            'catalog_file': cohort['source_authority_file'], 'catalog_sha256': cohort['source_authority_sha256'],
            'protocol_file': cohort['protocol_file'], 'protocol_sha256': cohort['protocol_sha256'],
            'authorization_file': authorization['file'], 'authorization_sha256': authorization['sha256'],
            'public_tasks': [development.public_task(tasks[c['task_id']]) for c in cases],
            **{key: cohort[key] for key in BIND_FIELDS},
            **{key: cohort[key] for key in ('main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')}}
    return {**body, 'artifact_hash': digest(body)}


def manifest_value(cohort_file, count=8, split='calibration'):
    require(split == 'calibration', 'genuine_APPS_qualified_calibration_and_once_source_owner_unavailable')
    require(type(count) is int and 1 <= count <= 8, 'batch_upper_bound_eight')
    cohort, tasks, train, current, _ = verified_pack(cohort_file)
    attempted = {r['group_id'] for r in current['main_started']}
    cases = [c for c in cohort['cases'] if c['split'] == split and c['group_id'] not in attempted][:count]
    require(cases, 'calibration_allocation_exhausted')
    require(current['remaining_main_attempts'] >= len(cases)
            + cohort['funded_allocation']['holdout_main_earmark'], 'remaining_main_holdout_earmark_insufficient')
    reserved = {r['group_id'] for key in ('main_reservations', 'jev_reservations') for r in current[key]}
    require(not reserved & {c['group_id'] for c in cases}, 'unresolved_group_reservation_no_skip_or_retry')
    return _manifest(cohort_file, cohort, tasks, train, cases)


def manifest_from_pack(value, pack):
    cohort, tasks, train, _, _ = pack
    sealed(value)
    require(value['schema'] == MANIFEST and value['split'] == 'calibration'
            and value['artifact_cohort_hash'] == cohort['artifact_hash'] and type(value['cases']) is list
            and 1 <= len(value['cases']) <= 8, 'formal_calibration_manifest_required')
    positions = {c['id']: n for n, c in enumerate(cohort['cases']) if c['split'] == 'calibration'}
    indexes = [positions.get(c['id'], -1) for c in value['cases']]
    require(indexes == list(range(indexes[0], indexes[0] + len(indexes))) and indexes[0] >= 0,
            'original_contiguous_prefix_preparation_required')
    require(_same(value, _manifest(value['artifact_cohort_file'], cohort, tasks, train, value['cases'])), 'original_public_manifest_changed')
    require(all(_same(c, cohort['cases'][positions[c['id']]]) for c in value['cases']), 'original_case_metadata_changed')
    return value


def validate_manifest(path):
    value = load(path)
    return manifest_from_pack(value, verified_pack(value['artifact_cohort_file']))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['validate-manifest', 'validate-cohort'])
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--cohort', type=Path)
    args = parser.parse_args()
    value = validate_manifest(args.manifest) if args.action == 'validate-manifest' else validate_cohort(args.cohort)
    print(json.dumps({'status': 'apps_formal_calibration_source_verified', 'hash': value['artifact_hash'],
                      'formal_registration': False, 'holdout_opened': False, 'new_paid_calls': 0}))


if __name__ == '__main__':
    main()
