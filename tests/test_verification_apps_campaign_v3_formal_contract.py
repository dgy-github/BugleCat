"""Pure synthetic descriptors; no real source allocation, Oracle or transport."""
from copy import deepcopy

import pytest

from evals.verification.apps_campaign_v3 import formal_contract as owner
from evals.verification.apps_campaign_v3 import packet, sampling_proposal
from evals.verification.calibration import digest


def seal(value):
    body = {k: v for k, v in value.items() if k != 'artifact_hash'}
    return {**body, 'artifact_hash': digest(body)}


@pytest.fixture
def proposal():
    stages = {}
    for index, stage in enumerate(sampling_proposal.STAGES):
        stages[stage] = [dict(id=f'{stage}-{n}', group_id=f'{stage}-group-{n}', task_id=index * 1000 + n,
                             task_sha256=digest([stage, n]), source_kind=packet.FREE if n % 2 else packet.SOLUTION,
                             split=stage, original_split='train', original_difficulty='interview' if n < 175 else 'introductory',
                             pair_count=n % 4 + 1, difficulty_hash_rank=2 * (n if n < 175 else n - 175) + index)
                         for n in range(252)]
    value = dict(schema=sampling_proposal.SCHEMA, seed=sampling_proposal.SEED, count_per_stage=252,
                 interview_per_stage=175, introductory_per_stage=77, cases=stages,
                 excluded_whole_dev_groups=[f'dev-{n}' for n in range(8)],
                 excluded_started_reserved_or_historical_groups=[f'dev-{n}' for n in range(8)],
                 membership_changes_after_labels_or_probabilities_allowed=False,
                 natural_negative_sufficiency_proven=False, all_fixed_allocation_outcomes_retained=True,
                 source_only_proposal_not_source_admission_or_certification=True,
                 oracle_runtime_binding=dict(version_info=[3, 13, 0], implementation='cpython', platform='win32', executable_sha256='f' * 64),
                 certified_threshold=None, candidate_labels_read=False, jev_probabilities_read=False,
                 source_authority_file='fixture-authority', resource_inventory_file='fixture-resource',
                 development_cohort_file='fixture-dev8', protocol_file='fixture-protocol',
                 main_ledger_file='fixture-ledger', authorization_file='fixture-auth',
                 source_bindings=[{'file': 'fixture-owner', 'sha256': 'e' * 64, 'roles': ['fixture_owner']}],
                 authorization_chain_bindings=[{'file': 'fixture-auth', 'sha256': 'd' * 64}])
    value.update({name: 'c' * 64 for name in ('source_authority_sha256', 'resource_inventory_sha256',
                                             'development_cohort_sha256', 'protocol_sha256', 'development_cohort_hash',
                                             'current_ledger_snapshot_sha256', 'current_completion_snapshot_sha256')})
    return seal({**value, **{k: False for k in owner.FALSE_FIELDS}, **{k: 0 for k in owner.ZERO_FIELDS}})


def outcome(case, proposal, label='unknown', status='ok'):
    evidence = {key: digest([case['id'], key]) for key in owner.EVIDENCE}
    evidence['oracle_runtime_sha256'] = digest(proposal['oracle_runtime_binding'])
    row = {**{key: case[key] for key in owner.IDENTITY}, 'main_status': status, 'label': label,
           'unknown_reason': 'restricted_runtime_fixture' if label == 'unknown' else None,
           'oracle_status': {'supported': 'passed', 'unsupported': 'failed', 'unknown': 'unknown', 'unlabeled': 'not_run'}[label],
           'evidence': evidence}
    if status == 'main_not_started':
        row.update(label='unlabeled', oracle_status='not_run', unknown_reason=None, evidence={k: None for k in owner.EVIDENCE})
    elif status == 'main_not_finished':
        row.update(label='unlabeled', oracle_status='not_run', unknown_reason=None,
                   evidence={k: evidence[k] if k in {'main_start_sha256', 'main_request_sha256', 'collector_executable_sha256'}
                             else None for k in owner.EVIDENCE})
    elif status != 'ok':
        row.update(label='unknown', oracle_status='not_run', unknown_reason='original_main_failure_fixture')
        evidence['oracle_sha256'] = evidence['oracle_runtime_sha256'] = None
    return row


def outcomes(proposal, split='calibration', supported=150, unsupported=50):
    return [outcome(case, proposal, 'supported' if n < supported else 'unsupported' if n < supported + unsupported else 'unknown')
            for n, case in enumerate(proposal['cases'][split])]


def observations(rows):
    return [dict(id=row['id'], group_id=row['group_id'], split=row['split'],
                 candidate_sha256=row['evidence']['candidate_sha256'], oracle_sha256=row['evidence']['oracle_sha256'],
                 request_hash=digest([row['id'], 'request']), actual_request_hash=digest([row['id'], 'request']),
                 response_sha256=digest([row['id'], 'response']), model=packet.MODEL, call_status='ok',
                 acceptance_violation=0.0 if row['label'] == 'supported' else 1.0,
                 transport_binding_status='original_transport_bytes_verified',
                 attempt_started_sha256=digest([row['id'], 'attempt']), reservation_sha256=digest([row['id'], 'reservation']),
                 runner_executable_sha256='b' * 64)
            for row in rows if row['label'] in {'supported', 'unsupported'}]


def test_complete_counts_keep_unknown_distinct_from_unstarted_and_pending(proposal):
    rows = outcomes(proposal)
    rows[-1] = outcome(proposal['cases']['calibration'][-1], proposal, status='main_not_started')
    rows[-2] = outcome(proposal['cases']['calibration'][-2], proposal, status='main_not_finished')
    rows[-3] = outcome(proposal['cases']['calibration'][-3], proposal, status='transport_error')
    result = owner.stage_contract(proposal, 'calibration', rows)
    assert result['counts'] == {'supported': 150, 'unsupported': 50, 'unknown': 50, 'unlabeled': 2,
                                'allocated': 252, 'collected': 251, 'pending': 1, 'unstarted': 1, 'eligible': 200, 'failed_main': 1}
    assert result['complete_terminal_collection'] is False
    assert result['candidate_labels_read'] is True and result['jev_probabilities_read'] is False


@pytest.mark.parametrize('field', owner.FALSE_FIELDS)
@pytest.mark.parametrize('value', [True, 0, 'false', None])
def test_resealed_proposal_cannot_grant_any_permission_or_alias_bool(proposal, field, value):
    proposal[field] = value
    with pytest.raises(ValueError, match='cannot_grant_authority'):
        owner.stage_contract(seal(proposal), 'calibration', outcomes(proposal))


@pytest.mark.parametrize('field', owner.ZERO_FIELDS)
@pytest.mark.parametrize('value', [False, True, 0.0, 1])
def test_calls_and_reservations_are_strict_zero_int(proposal, field, value):
    proposal[field] = value
    with pytest.raises(ValueError):
        owner.stage_contract(seal(proposal), 'calibration', outcomes(proposal))


@pytest.mark.parametrize('mutation', ['drop', 'repeat', 'swap', 'split', 'group', 'task', 'kind', 'hash', 'extra_label'])
def test_original_order_source_identity_and_complete_allocation(proposal, mutation):
    rows = outcomes(proposal)
    if mutation == 'drop':
        rows.pop()
    elif mutation == 'repeat':
        rows[1] = deepcopy(rows[0])
    elif mutation == 'swap':
        rows[0], rows[1] = rows[1], rows[0]
    elif mutation == 'split':
        rows[0]['split'] = 'holdout'
    elif mutation == 'group':
        rows[0]['group_id'] = 'foreign'
    elif mutation == 'task':
        rows[0]['task_id'] = True
    elif mutation == 'kind':
        rows[0]['source_kind'] = 'stdin'
    elif mutation == 'hash':
        rows[0]['task_sha256'] = 'c' * 64
    else:
        rows[0]['oracle_passed'] = True
    with pytest.raises(ValueError):
        owner.stage_contract(proposal, 'calibration', rows)


@pytest.mark.parametrize('mutation', ['cross_stage', 'repeat_group', 'repeat_task', 'dev_exposed', 'bool_task', 'bool_pair', 'rank_split', 'source_kind'])
def test_proposal_whole_split_and_history_exclusion(proposal, mutation):
    row = proposal['cases']['holdout'][0]
    if mutation == 'cross_stage':
        row['split'] = 'calibration'
    elif mutation == 'repeat_group':
        row['group_id'] = proposal['cases']['calibration'][0]['group_id']
    elif mutation == 'repeat_task':
        row['task_id'] = proposal['cases']['calibration'][0]['task_id']
    elif mutation == 'dev_exposed':
        row['group_id'] = proposal['excluded_whole_dev_groups'][0]
    elif mutation == 'bool_task':
        row['task_id'] = True
    elif mutation == 'bool_pair':
        row['pair_count'] = True
    elif mutation == 'rank_split':
        row['difficulty_hash_rank'] = 0
    else:
        row['source_kind'] = 'legacy_artifact_acceptance'
    with pytest.raises(ValueError):
        owner.stage_contract(seal(proposal), 'calibration', outcomes(proposal))


@pytest.mark.parametrize('field', ['candidate_labels_read', 'jev_probabilities_read'])
def test_source_proposal_must_be_before_any_prediction_or_label(proposal, field):
    proposal[field] = True
    with pytest.raises(ValueError, match='cannot_have_seen'):
        owner.stage_contract(seal(proposal), 'calibration', outcomes(proposal))


@pytest.mark.parametrize('kwargs', [dict(scores_hidden=False), dict(scores_hidden=1),
                                  dict(jev_probabilities_read=True), dict(jev_probabilities_read=0)])
def test_registration_predicted_scores_cannot_be_redeclared_hidden(proposal, kwargs):
    with pytest.raises(ValueError, match='prediction_hidden'):
        owner.registration_candidate(proposal, 'calibration', outcomes(proposal), **kwargs)


@pytest.mark.parametrize('label_counts', [(149, 50), (150, 49), (200, 0)])
def test_registration_does_not_lower_quotas(proposal, label_counts):
    with pytest.raises(ValueError, match='150_supported_50_unsupported'):
        owner.registration_candidate(proposal, 'calibration', outcomes(proposal, supported=label_counts[0], unsupported=label_counts[1]))


@pytest.mark.parametrize('status', ['main_not_started', 'main_not_finished'])
def test_quota_ready_partial_collection_still_cannot_register(proposal, status):
    rows = outcomes(proposal)
    rows[-1] = outcome(proposal['cases']['calibration'][-1], proposal, status=status)
    with pytest.raises(ValueError, match='complete_terminal'):
        owner.registration_candidate(proposal, 'calibration', rows)


@pytest.mark.parametrize('mutation', ['binary_failure', 'unstarted_unknown', 'unknown_pass', 'no_oracle', 'wrong_runtime', 'invalid_sha'])
def test_original_outcome_descriptor_consistency(proposal, mutation):
    rows = outcomes(proposal)
    if mutation == 'binary_failure':
        rows[0]['main_status'] = 'timeout'
    elif mutation == 'unstarted_unknown':
        rows[-1] = outcome(proposal['cases']['calibration'][-1], proposal, status='main_not_started')
        rows[-1]['label'] = 'unknown'
    elif mutation == 'unknown_pass':
        rows[-1]['oracle_status'] = 'passed'
    elif mutation == 'no_oracle':
        rows[0]['evidence']['oracle_sha256'] = None
    elif mutation == 'wrong_runtime':
        rows[0]['evidence']['oracle_runtime_sha256'] = 'a' * 64
    else:
        rows[0]['evidence']['main_start_sha256'] = 'x' * 64
    with pytest.raises(ValueError):
        owner.stage_contract(proposal, 'calibration', rows)


def test_successful_numeric_calibration_and_holdout_never_grant_authority(proposal):
    rows = outcomes(proposal)
    result = owner.calibration_candidate(proposal, rows, observations(rows))
    assert result['numeric']['calibration_numeric_gate_met'] is True
    assert result['numeric']['qualified_numeric_threshold'] == 1.0
    assert len(result['numeric']['grid_results']) == 41
    hold_rows = outcomes(proposal, 'holdout')
    hold = owner.holdout_candidate(proposal, result, hold_rows, observations(hold_rows))
    assert hold['numeric']['gate_met'] is True and hold['threshold'] == 1.0
    for value in (result, hold):
        assert all(value[k] is False for k in owner.FALSE_FIELDS)
        assert all(value[k] == 0 for k in owner.ZERO_FIELDS)
        assert value['original_proof_verified'] is False and value['caller_evidence_claims_authenticated'] is False
        assert value['certified_threshold'] is None and value['native_claim_certification'] is False
        assert value['overall_agent_benefit_certification'] is False and value['jev_probabilities_read'] is True
        assert value['durable_holdout_consumption_performed'] is False
        assert value['missing_actual_interfaces'] == list(owner.MISSING_INTERFACES)
    assert hold['counts']['unknown'] == 52 and hold['counts']['eligible'] == 200


def test_failed_attempts_keep_binary_denominators_and_do_not_grant_holdout(proposal):
    rows = outcomes(proposal)
    seen = observations(rows)
    for row in seen[150:]:
        row.update(call_status='timeout', acceptance_violation=None, model=None, actual_request_hash=None, response_sha256=None)
    result = owner.calibration_candidate(proposal, rows, seen)
    assert result['numeric']['qualified_numeric_threshold'] is None
    assert result['numeric']['grid_results'][0]['unsupported_groups'] == 50
    assert len(result['numeric']['grid_results'][0]['failed_attempts']) == 50
    with pytest.raises(ValueError, match='qualified_numeric'):
        owner.holdout_candidate(proposal, result, outcomes(proposal, 'holdout'), [])


@pytest.mark.parametrize('mutation', ['drop', 'repeat', 'unknown', 'split', 'candidate', 'oracle', 'no_start', 'model', 'failed_score', 'old_template', 'nonfinite'])
def test_entire_binary_observation_join_shape_is_required(proposal, mutation):
    rows = outcomes(proposal)
    seen = observations(rows)
    if mutation == 'drop':
        seen.pop()
    elif mutation == 'repeat':
        seen[1] = deepcopy(seen[0])
    elif mutation == 'unknown':
        seen[-1]['id'] = rows[-1]['id']
    elif mutation == 'split':
        seen[0]['split'] = 'holdout'
    elif mutation == 'candidate':
        seen[0]['candidate_sha256'] = 'f' * 64
    elif mutation == 'oracle':
        seen[0]['oracle_sha256'] = 'f' * 64
    elif mutation == 'no_start':
        seen[0]['attempt_started_sha256'] = None
    elif mutation == 'model':
        seen[0]['model'] = 'jev-legacy'
    elif mutation == 'failed_score':
        seen[0]['call_status'] = 'cancelled'
    elif mutation == 'old_template':
        seen[0]['incomplete'] = .9
    else:
        seen[0]['acceptance_violation'] = float('inf')
    with pytest.raises(ValueError):
        owner.calibration_candidate(proposal, rows, seen)


@pytest.mark.parametrize('threshold', [.5, .95, True, '1', 1])
def test_holdout_cannot_replace_or_alias_locked_threshold(proposal, threshold):
    rows = outcomes(proposal)
    calibration = owner.calibration_candidate(proposal, rows, observations(rows))
    hold = outcomes(proposal, 'holdout')
    with pytest.raises(ValueError, match='threshold_replacement'):
        owner.holdout_candidate(proposal, calibration, hold, observations(hold), threshold=threshold)


def test_resealed_selected_threshold_or_parent_cannot_be_substituted(proposal):
    rows = outcomes(proposal)
    calibration = owner.calibration_candidate(proposal, rows, observations(rows))
    calibration['numeric']['qualified_numeric_threshold'] = .9
    with pytest.raises(ValueError, match='selected_threshold_changed'):
        owner.holdout_candidate(proposal, seal(calibration), outcomes(proposal, 'holdout'), [])
    calibration['proposal_hash'] = 'e' * 64
    with pytest.raises(ValueError, match='same_new_calibration'):
        owner.holdout_candidate(proposal, seal(calibration), [], [])


def test_readonly_source_review_exact_rebuild_and_no_write(proposal, monkeypatch, tmp_path):
    monkeypatch.setattr(owner, 'load', lambda path: deepcopy(proposal))
    calls = []
    def rebuild(*args):
        calls.append(args)
        return deepcopy(proposal)
    monkeypatch.setattr(owner.sampling_proposal, 'proposal', rebuild)
    before = list(tmp_path.iterdir())
    result = owner.review_proposal(tmp_path / 'existing-fixture.json')
    assert result['source_runtime_current_ledger_rebuilt'] is True and result['collection_authorized'] is False
    assert result['candidate_labels_read'] is False and result['jev_probabilities_read'] is False
    assert list(tmp_path.iterdir()) == before
    assert calls[0][-2:] == (252, 175)


@pytest.mark.parametrize('field', ['oracle_runtime_binding', 'current_ledger_snapshot_sha256', 'source_bindings', 'protocol_sha256'])
def test_proposal_source_runtime_ledger_protocol_drift_cannot_be_used(proposal, monkeypatch, field):
    monkeypatch.setattr(owner, 'load', lambda path: deepcopy(proposal))
    expected = deepcopy(proposal)
    expected[field] = {'changed': True}
    monkeypatch.setattr(owner.sampling_proposal, 'proposal', lambda *a: seal(expected))
    with pytest.raises(ValueError, match='source_history_runtime_or_ledger_changed'):
        owner.review_proposal('fixture-only')


def test_inputs_unchanged_and_missing_actual_once_owners_explicit(proposal):
    rows = outcomes(proposal)
    seen = observations(rows)
    before = deepcopy((proposal, rows, seen))
    owner.calibration_candidate(proposal, rows, seen)
    assert (proposal, rows, seen) == before
    facts = owner.capabilities()
    assert facts['original_main_oracle_jev_joins_implemented'] is False
    assert facts['durable_once_holdout_consumption_implemented'] is False
    assert not any(name in vars(owner) for name in ('register', 'reserve', 'open_source', 'live', 'consume'))


@pytest.mark.parametrize('field,value', [('source_bindings', []), ('authorization_chain_bindings', []),
                                       ('protocol_sha256', 'old'), ('development_cohort_hash', None),
                                       ('current_ledger_snapshot_sha256', True), ('main_ledger_file', ''),
                                       ('oracle_runtime_binding', {'version_info': [True, 13, 0]})])
def test_original_source_binding_and_runtime_descriptors_cannot_be_omitted(proposal, field, value):
    proposal[field] = value
    with pytest.raises(ValueError):
        owner.stage_contract(seal(proposal), 'calibration', outcomes(proposal))


def test_repeated_numeric_rehearsal_is_not_a_once_consumption(proposal):
    rows = outcomes(proposal)
    cal = owner.calibration_candidate(proposal, rows, observations(rows))
    hold = outcomes(proposal, 'holdout')
    first = owner.holdout_candidate(proposal, cal, hold, observations(hold))
    assert first == owner.holdout_candidate(proposal, cal, hold, observations(hold))
    assert first['holdout_opened'] is False and first['durable_holdout_consumption_performed'] is False
    assert 'durable_shared_once_holdout_source_marker_after_genuine_qualified_calibration' in first['missing_actual_interfaces']
