"""Only mocked source/ledger transactions; never create actual allocations."""
import copy
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import packet
from evals.verification.apps_campaign_v3 import sampling_proposal as owner
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash


class UnreadLabels(dict):
    def __getitem__(self, key):
        if key in {'candidate_label', 'oracle', 'label', 'probability'}:
            raise AssertionError('candidate outcome must never be read')
        return super().__getitem__(key)


@pytest.fixture
def fixture(tmp_path, monkeypatch):
    authority_file, resource_file, cohort_file, auth_file, protocol_file, bound = [
        tmp_path / name for name in ('authority.json', 'resources.json', 'cohort.json',
                                    'authorization.json', 'protocol.json', 'owner.py')]
    for path in (authority_file, resource_file, cohort_file, auth_file, protocol_file, bound):
        path.write_text('{}')
    def bind(role, path):
        return {'role': role, 'file': str(path), 'sha256': file_hash(path)}
    cases = []
    for difficulty, count in [('interview', 370), ('introductory', 400)]:
        for n in range(count):
            cases.append(UnreadLabels(group_id=f'{difficulty}-{n}', task_id=len(cases),
                                      task_sha256=digest([difficulty, n]), pair_count=n % 4 + 1,
                                      source_kind=packet.FREE if n % 2 else packet.SOLUTION,
                                      original_difficulty=difficulty, candidate_label='NEVER_READ', probability='NEVER_READ'))
    # Real source_authority bindings deliberately have no role key; owners are
    # a separate graph. Exercise that schema rather than a simplified mock.
    authority = {'cases': cases, 'bindings': [{'file': str(bound), 'sha256': file_hash(bound)}],
                 'owner_bindings': [{'file': str(auth_file), 'sha256': file_hash(auth_file)}],
                 'historical_excluded_groups': 27}
    inventory = {'cases': [{'group_id': c['group_id'], 'resource_status': 'qualified'} for c in cases]}
    runtime = {'implementation': 'fixture', 'version_info': [3, 13, 0], 'platform': 'win32', 'executable_sha256': 'runtime'}
    cohort = {'artifact_hash': 'dev8-hash', 'cases': [{'group_id': f'{d}-{n}'}
                for d in ('interview', 'introductory') for n in range(4)],
              'source_authority_file': str(authority_file), 'resource_inventory_file': str(resource_file),
              'protocol_file': str(protocol_file), 'protocol_sha256': file_hash(protocol_file),
              'oracle_runtime_binding': runtime,
              'bindings': [bind('authorization', auth_file), bind('protocol', protocol_file)]}
    baseline = {'authorization_chain': {'authorization': {'main_model': {'max_attempts': 750},
                                                         'jev': {'max_attempts': 750}},
                                       'bindings': [bind('authorization', auth_file)]},
                'main_started': [{'group_id': 'interview-4'}, *[{'group_id': f'old-{i}'} for i in range(517)]],
                'main_reservations': [{'group_id': 'interview-6'}],
                'jev_reservations': [{'group_id': 'interview-7'}, *[{'group_id': f'old-jev-{i}'} for i in range(228)]],
                'excluded_groups': ['interview-5'], 'main_ledger_file': str(tmp_path / 'ledger.sqlite'),
                'remaining_main_attempts': 232}
    finished = {'old-pending': None}
    monkeypatch.setattr(owner.development, 'inputs', lambda *a: (authority, inventory))
    monkeypatch.setattr(owner.development, 'validate_cohort', lambda p: cohort)
    monkeypatch.setattr(owner.development, 'snapshot', lambda p: copy.deepcopy((baseline, finished)))
    monkeypatch.setattr(owner.protocol, 'validate', lambda value: value)
    monkeypatch.setattr(owner.packet, 'runtime_info', lambda: copy.deepcopy(runtime))
    monkeypatch.setattr(owner, '_owners', lambda: [bind('proposal_owner_fixture', bound)])
    monkeypatch.setattr(owner.development, 'original_tasks', lambda *a: pytest.fail('no task execution/read required'))
    return {'args': (authority_file, resource_file, cohort_file), 'baseline': baseline,
            'cohort': cohort, 'authority': authority, 'inventory': inventory, 'bound': bound,
            'runtime': runtime, 'finished': finished}


def test_fixed_alternating_disjoint_source_only_stages(fixture):
    value = owner.proposal(*fixture['args'])
    assert value['artifact_hash'] == digest({k: v for k, v in value.items() if k != 'artifact_hash'})
    expected = {'interview': 175, 'introductory': 141}
    for stage in owner.STAGES:
        assert value['stage_strata'][stage]['difficulty'] == expected
        assert value['stage_strata'][stage]['groups'] == 316
    left, right = ({c['group_id'] for c in value['cases'][s]} for s in owner.STAGES)
    excluded = set(value['excluded_started_reserved_or_historical_groups'])
    assert not left & right and not (left | right) & excluded
    # Both dev strata including the never-started/Unknown cases are excluded.
    assert {f'{d}-{n}' for d in ('interview', 'introductory') for n in range(4)} <= excluded
    assert {f'interview-{n}' for n in range(4, 8)} <= excluded
    ranks = {stage: {c['group_id']: c['difficulty_hash_rank'] for c in value['cases'][stage]} for stage in owner.STAGES}
    assert all(n % 2 == 0 for n in ranks['calibration'].values())
    assert all(n % 2 == 1 for n in ranks['holdout'].values())
    assert value['natural_negative_sufficiency_proven'] is False
    assert value['official_source_split'] == 'train'
    assert value['source_authority_history_excluded_component_count'] == 27
    assert any('original_authority_owner' in b['roles'] for b in value['source_bindings'])
    assert any('original_authority_data' in b['roles'] for b in value['source_bindings'])


def test_input_order_and_unread_labels_cannot_change_selection(fixture):
    first = owner.proposal(*fixture['args'])
    fixture['authority']['cases'].reverse()
    fixture['inventory']['cases'].reverse()
    for case in fixture['authority']['cases']:
        case['candidate_label'] = 'changed-unread'
        case['probability'] = .999
    second = owner.proposal(*fixture['args'])
    assert first == second
    assert all(set(c) == {'group_id', 'task_id', 'task_sha256', 'source_kind', 'original_difficulty',
                          'pair_count', 'id', 'split', 'original_split', 'difficulty_hash_rank'}
               for rows in second['cases'].values() for c in rows)


def test_explicit_alternative_and_exact_current_budget(fixture):
    value = owner.proposal(*fixture['args'], count=252)
    assert value['stage_strata']['holdout']['difficulty'] == {'interview': 175, 'introductory': 77}
    budget = value['budget']
    assert budget['planned_main_maximum'] == budget['planned_jev_maximum'] == 504
    assert budget['additional_main_authority_required'] == 272
    assert budget['additional_jev_authority_required'] == 0
    assert budget['remaining_jev_reservations'] == 521
    assert not budget['budget_sufficient_for_maximum_allocation']
    default = owner.proposal(*fixture['args'])['budget']
    assert default['additional_main_authority_required'] == 400
    assert default['additional_jev_authority_required'] == 111
    fixture['baseline']['jev_reservations'] = fixture['baseline']['jev_reservations'][:222]
    assert owner.proposal(*fixture['args'])['budget']['additional_jev_authority_required'] == 104


def test_authorized_caps_not_hardcoded_or_new_authorization(fixture):
    fixture['baseline']['authorization_chain']['authorization']['main_model']['max_attempts'] = 1150
    fixture['baseline']['authorization_chain']['authorization']['jev']['max_attempts'] = 861
    fixture['baseline']['remaining_main_attempts'] = 632
    value = owner.proposal(*fixture['args'])
    assert value['budget']['budget_sufficient_for_maximum_allocation']
    assert value['budget']['additional_main_authority_required'] == value['budget']['additional_jev_authority_required'] == 0
    assert not value['budget']['additional_authorization_created']
    assert not value['collection_authorized']


def test_resource_unqualified_is_excluded_without_substitution_by_label(fixture):
    fixture['inventory']['cases'][8]['resource_status'] = 'oversize'
    value = owner.proposal(*fixture['args'])
    assert 'interview-8' not in {c['group_id'] for rows in value['cases'].values() for c in rows}
    assert value['available_strata']['difficulty']['interview'] == 361


def test_insufficient_source_does_not_shrink_stage_or_move_holdout(fixture):
    for row in fixture['inventory']['cases']:
        if row['group_id'].startswith('interview') and int(row['group_id'].split('-')[1]) >= 350:
            row['resource_status'] = 'unqualified'
    with pytest.raises(ValueError, match='insufficient_fresh'):
        owner.proposal(*fixture['args'])


@pytest.mark.parametrize('count,interview', [(True, 175), (316.0, 175), (200, 175), (316, True), (316, 174), (252, 175.0)])
def test_only_explicit_typed_plan_variants(fixture, count, interview):
    with pytest.raises(ValueError, match='explicit_fixed'):
        owner.proposal(*fixture['args'], count=count, interview_per_stage=interview)


@pytest.mark.parametrize('mutation', ['binding', 'runtime', 'start', 'completion', 'reservation', 'authorization'])
def test_transaction_drift_refuses_proposal(fixture, monkeypatch, mutation):
    calls = 0

    def snapshot(path):
        nonlocal calls
        calls += 1
        if calls == 2:
            if mutation == 'binding':
                fixture['bound'].write_text('mutated')
            elif mutation == 'runtime':
                fixture['runtime']['version_info'][1] = 14
            elif mutation == 'completion':
                fixture['finished']['old-pending'] = {'status': 'changed'}
            elif mutation == 'reservation':
                fixture['baseline']['jev_reservations'].append({'group_id': 'new-reservation'})
            elif mutation == 'authorization':
                fixture['baseline']['authorization_chain']['authorization']['main_model']['max_attempts'] += 1
            else:
                fixture['baseline']['main_started'].append({'group_id': 'new-start'})
        return copy.deepcopy((fixture['baseline'], fixture['finished']))
    monkeypatch.setattr(owner.development, 'snapshot', snapshot)
    # Binding mutation must occur before the last binding rehash, not just
    # after it; use the runtime hook at the construction boundary for this case.
    if mutation == 'binding':
        original = owner._selected

        def selected(*args):
            result = original(*args)
            fixture['bound'].write_text('mutated')
            return result
        monkeypatch.setattr(owner, '_selected', selected)
    with pytest.raises(ValueError, match='changed_during_proposal'):
        owner.proposal(*fixture['args'])


def test_source_alias_or_incomplete_dev_allocation_refused(fixture, tmp_path):
    alias = tmp_path / 'different-authority.json'
    alias.write_bytes(fixture['args'][0].read_bytes())
    with pytest.raises(ValueError, match='source_identity'):
        owner.proposal(alias, *fixture['args'][1:])
    fixture['cohort']['cases'].pop()
    with pytest.raises(ValueError, match='complete_original_dev8'):
        owner.proposal(*fixture['args'])


def test_no_new_files_permissions_labels_or_fee_authority(fixture):
    before = {str(p): p.read_bytes() for p in Path(fixture['args'][0]).parent.iterdir() if p.is_file()}
    value = owner.proposal(*fixture['args'])
    after = {str(p): p.read_bytes() for p in Path(fixture['args'][0]).parent.iterdir() if p.is_file()}
    assert before == after
    for field in ('collection_authorized', 'formal_cohort_created', 'formal_registration', 'holdout_opened',
                  'production_authorized', 'candidate_labels_read', 'jev_probabilities_read', 'formal_certification'):
        assert value[field] is False
    assert all(value[k] == 0 for k in ('model_calls', 'jev_calls', 'oracle_executions', 'reservations'))
    assert value['certified_threshold'] is None
