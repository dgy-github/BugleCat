"""Read-only APPS source review and pure formal-contract rehearsals.

Caller-supplied labels/hashes/exposure flags are NOT authenticated original
evidence. Numerical gates never grant collection, registration, once-only
holdout consumption or certification. No writes, Oracle, transport or budget
operations exist here. Actual formal owners must implement the missing joins.
"""
from collections import Counter
from copy import deepcopy

from ..calibration import digest, load, require
from . import packet, sampling_proposal
from . import prospective_protocol as protocol

SCHEMA = 'buglecat.verification.apps-formal-contract-rehearsal.v3'
FALSE_FIELDS = ('collection_authorized', 'formal_cohort_created', 'formal_registration',
                'holdout_opened', 'production_authorized', 'formal_certification')
ZERO_FIELDS = ('model_calls', 'jev_calls', 'oracle_executions', 'reservations')
MISSING_INTERFACES = (
    'formal_cohort_with_source_history_runtime_budget_and_append_only_ledger_proof',
    'formal_collector_and_whole_allocation_main_sqlite_events_bytes_session_join',
    'independent_original_APPS_oracle_graph_runtime_all_ordered_pairs_join',
    'prediction_blind_create_new_registration_before_any_allocation_jev_reservation',
    'formal_sender_and_full_original_jev_reservation_bytes_usage_compiled_exe_join',
    'original_registration_and_all_observations_calibration_revalidation',
    'durable_shared_once_holdout_source_marker_after_genuine_qualified_calibration',
    'durable_shared_once_holdout_evaluation_marker_and_original_result_revalidation',
)
IDENTITY = ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')
EVIDENCE = ('main_start_sha256', 'main_request_sha256', 'main_response_sha256',
            'main_receipt_sha256', 'session_sha256', 'collector_executable_sha256',
            'candidate_sha256', 'oracle_sha256', 'oracle_runtime_sha256')
OUTCOME_FIELDS = set(IDENTITY) | {'main_status', 'label', 'unknown_reason', 'oracle_status', 'evidence'}
OBS_FIELDS = {'id', 'group_id', 'split', 'candidate_sha256', 'oracle_sha256', 'request_hash',
              'actual_request_hash', 'response_sha256', 'model', 'call_status', 'acceptance_violation',
              'transport_binding_status', 'attempt_started_sha256', 'reservation_sha256', 'runner_executable_sha256'}


def _same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def _hash(value):
    return type(value) is str and len(value) == 64 and all(c in '0123456789abcdef' for c in value)


def _text(value):
    return type(value) is str and bool(value.strip())


def _sealed(value):
    require(type(value) is dict and _hash(value.get('artifact_hash')), 'sealed_object_required')
    require(value['artifact_hash'] == digest({k: v for k, v in value.items() if k != 'artifact_hash'}),
            'local_artifact_seal_changed')
    return value


def _limits(value, *, source_only=False):
    require(all(value.get(key) is False for key in FALSE_FIELDS)
            and all(type(value.get(key)) is int and value[key] == 0 for key in ZERO_FIELDS)
            and value.get('certified_threshold') is None, 'proposal_or_rehearsal_cannot_grant_authority')
    if source_only:
        require(value.get('candidate_labels_read') is False and value.get('jev_probabilities_read') is False,
                'source_proposal_cannot_have_seen_labels_or_probabilities')


def capabilities():
    """Explicit boundary; this is neither a fee guard nor a certificate owner."""
    return {'read_only_source_proposal_rebuild': True, 'typed_full_allocation_contract': True,
            'quota_and_fixed_protocol_numeric_rehearsal': True,
            'original_main_oracle_jev_joins_implemented': False,
            'durable_once_holdout_consumption_implemented': False,
            'missing_actual_interfaces': list(MISSING_INTERFACES),
            'collection_authorized': False, 'formal_registration': False,
            'holdout_opened': False, 'formal_certification': False, 'production_authorized': False}


def _source_descriptors(value):
    for name in ('source_authority', 'resource_inventory', 'development_cohort', 'protocol'):
        require(_text(value.get(name + '_file')) and _hash(value.get(name + '_sha256')),
                'original_source_file_and_raw_hash_descriptors_required')
    require(_hash(value.get('development_cohort_hash'))
            and _hash(value.get('current_ledger_snapshot_sha256'))
            and _hash(value.get('current_completion_snapshot_sha256'))
            and _text(value.get('main_ledger_file')) and _text(value.get('authorization_file')),
            'original_dev_and_current_budget_snapshot_descriptors_required')
    for field in ('source_bindings', 'authorization_chain_bindings'):
        bindings = value.get(field)
        require(type(bindings) is list and bindings and all(type(b) is dict and _text(b.get('file'))
                and _hash(b.get('sha256')) for b in bindings), 'complete_original_binding_descriptors_required')
    runtime = value.get('oracle_runtime_binding')
    require(type(runtime) is dict and set(runtime) == {'executable_sha256', 'version_info', 'implementation', 'platform'}
            and _hash(runtime['executable_sha256']) and _text(runtime['implementation']) and _text(runtime['platform'])
            and type(runtime['version_info']) is list and len(runtime['version_info']) == 3
            and all(type(n) is int and n >= 0 for n in runtime['version_info']), 'original_runtime_descriptor_required')


def _proposal(value):
    _sealed(value)
    _limits(value, source_only=True)
    require(value.get('schema') == sampling_proposal.SCHEMA and value.get('seed') == sampling_proposal.SEED,
            'new_APPS_sampling_proposal_required')
    _source_descriptors(value)
    count = value.get('count_per_stage')
    require(type(count) is int and count in {252, 316}
            and type(value.get('interview_per_stage')) is int and value['interview_per_stage'] == 175
            and type(value.get('introductory_per_stage')) is int and value['introductory_per_stage'] == count - 175,
            'fixed_proposed_strata_required')
    require(value.get('membership_changes_after_labels_or_probabilities_allowed') is False
            and value.get('natural_negative_sufficiency_proven') is False
            and value.get('all_fixed_allocation_outcomes_retained') is True
            and value.get('source_only_proposal_not_source_admission_or_certification') is True,
            'fixed_source_only_membership_required')
    groups, identities, tasks = set(), set(), set()
    require(type(value.get('cases')) is dict and set(value['cases']) == set(sampling_proposal.STAGES),
            'both_fixed_stages_required')
    excluded = value['excluded_started_reserved_or_historical_groups']
    require(type(excluded) is list and all(_text(g) for g in excluded)
            and len(set(excluded)) == len(excluded), 'whole_history_exclusion_required')
    dev = value['excluded_whole_dev_groups']
    require(type(dev) is list and len(dev) == len(set(dev)) == 8 and set(dev) <= set(excluded),
            'whole_dev8_exclusion_required')
    for split, cases in value['cases'].items():
        require(type(cases) is list and len(cases) == count, 'whole_fixed_allocation_required')
        require(Counter(c['original_difficulty'] for c in cases) == {'interview': 175, 'introductory': count - 175},
                'fixed_proposed_strata_required')
        for case in cases:
            require(set(case) == set(IDENTITY) | {'original_difficulty', 'pair_count', 'original_split', 'difficulty_hash_rank'},
                    'source_case_fields_changed')
            require(_text(case['id']) and _text(case['group_id']) and _hash(case['task_sha256'])
                    and type(case['task_id']) is int and case['task_id'] >= 0
                    and type(case['pair_count']) is int and case['pair_count'] > 0
                    and type(case['difficulty_hash_rank']) is int and case['difficulty_hash_rank'] >= 0
                    and case['difficulty_hash_rank'] % 2 == sampling_proposal.STAGES.index(split)
                    and case['source_kind'] in packet.KINDS and case['split'] == split and case['original_split'] == 'train',
                    'source_identity_or_split_changed')
            require(case['group_id'] not in groups and case['id'] not in identities
                    and case['task_id'] not in tasks and case['group_id'] not in excluded, 'repeated_or_exposed_group')
            groups.add(case['group_id'])
            identities.add(case['id'])
            tasks.add(case['task_id'])
    return value


def review_proposal(proposal_file):
    """Precollection-only rebuild of original sources/runtime/current ledger.

    Exact rebuild rejects source changes and ledger movement. Later formal
    collection needs its own frozen prefix/append-only owner, not this API.
    """
    value = _proposal(load(proposal_file))
    expected = sampling_proposal.proposal(value['source_authority_file'], value['resource_inventory_file'],
                                         value['development_cohort_file'], value['count_per_stage'],
                                         value['interview_per_stage'])
    require(_same(expected, value), 'original_proposal_source_history_runtime_or_ledger_changed')
    return _result('source_review', proposal_hash=value['artifact_hash'],
                   source_runtime_current_ledger_rebuilt=True, source_review_valid_only_before_collection=True)


def _result(kind, **body):
    result = {'schema': SCHEMA, 'kind': kind, **body, 'local_contract_only': True,
              'caller_evidence_claims_authenticated': False, 'original_proof_verified': False,
              'durable_holdout_consumption_performed': False, 'certified_threshold': None,
              'native_claim_certification': False, 'overall_agent_benefit_certification': False,
              'candidate_labels_read': kind != 'source_review',
              'jev_probabilities_read': kind in {'calibration_candidate', 'holdout_candidate'},
              **{key: False for key in FALSE_FIELDS}, **{key: 0 for key in ZERO_FIELDS},
              'missing_actual_interfaces': list(MISSING_INTERFACES)}
    return {**result, 'artifact_hash': digest(result)}


def _outcome(row, case, runtime_hash):
    require(type(row) is dict and set(row) == OUTCOME_FIELDS, 'outcome_contract_fields_required')
    require(_same({k: row[k] for k in IDENTITY}, {k: case[k] for k in IDENTITY}), 'original_source_identity_changed')
    evidence = row['evidence']
    require(type(evidence) is dict and set(evidence) == set(EVIDENCE)
            and all(v is None or _hash(v) for v in evidence.values()), 'original_evidence_hash_descriptors_required')
    status, label, oracle = row['main_status'], row['label'], row['oracle_status']
    require(_text(status) and label in {'supported', 'unsupported', 'unknown', 'unlabeled'}
            and oracle in {'passed', 'failed', 'unknown', 'not_run'}, 'typed_outcome_required')
    if status == 'main_not_started':
        require(label == 'unlabeled' and oracle == 'not_run' and row['unknown_reason'] is None
                and all(v is None for v in evidence.values()), 'unstarted_is_not_unknown_or_evidence')
        return
    require(all(_hash(evidence[k]) for k in ('main_start_sha256', 'main_request_sha256', 'collector_executable_sha256')),
            'original_single_main_start_descriptor_required')
    if status == 'main_not_finished':
        require(label == 'unlabeled' and oracle == 'not_run' and row['unknown_reason'] is None
                and all(evidence[k] is None for k in EVIDENCE if k not in
                        {'main_start_sha256', 'main_request_sha256', 'collector_executable_sha256'}),
                'pending_cannot_be_terminal_or_labelled')
        return
    require(_hash(evidence['main_receipt_sha256']), 'terminal_main_receipt_descriptor_required')
    if status != 'ok':
        require(label == 'unknown' and oracle == 'not_run' and _text(row['unknown_reason'])
                and evidence['oracle_sha256'] is None and evidence['oracle_runtime_sha256'] is None,
                'main_failure_cannot_be_binary')
        return
    require(_hash(evidence['main_response_sha256']) and _hash(evidence['session_sha256']),
            'original_success_bytes_session_descriptors_required')
    if oracle != 'not_run':
        require(_hash(evidence['candidate_sha256']) and _hash(evidence['oracle_sha256'])
                and evidence['oracle_runtime_sha256'] == runtime_hash, 'original_oracle_runtime_descriptors_required')
    else:
        require(evidence['oracle_sha256'] is None and evidence['oracle_runtime_sha256'] is None,
                'unexecuted_oracle_cannot_have_receipt')
    if label in {'supported', 'unsupported'}:
        require(oracle == {'supported': 'passed', 'unsupported': 'failed'}[label]
                and row['unknown_reason'] is None, 'binary_label_requires_matching_independent_oracle_descriptor')
    else:
        require(label == 'unknown' and oracle in {'unknown', 'not_run'} and _text(row['unknown_reason']),
                'success_unknown_reason_required')


def stage_contract(proposal, split, outcomes, *, scores_hidden=True, jev_probabilities_read=False):
    """Check every fixed member; supplied hashes/labels remain unauthenticated."""
    _proposal(proposal)
    require(split in sampling_proposal.STAGES and scores_hidden is True and jev_probabilities_read is False,
            'fixed_split_and_prediction_hidden_labels_required')
    cases = proposal['cases'][split]
    require(type(outcomes) is list and len(outcomes) == len(cases), 'complete_allocation_including_unknown_and_unstarted_required')
    for row, case in zip(outcomes, cases, strict=True):
        _outcome(row, case, digest(proposal['oracle_runtime_binding']))
    labels, statuses = Counter(r['label'] for r in outcomes), Counter(r['main_status'] for r in outcomes)
    counts = {name: labels[name] for name in ('supported', 'unsupported', 'unknown', 'unlabeled')}
    counts.update(allocated=len(cases), collected=len(cases) - statuses['main_not_started'],
                  pending=statuses['main_not_finished'], unstarted=statuses['main_not_started'],
                  eligible=labels['supported'] + labels['unsupported'],
                  failed_main=sum(n for status, n in statuses.items() if status not in {'ok', 'main_not_started', 'main_not_finished'}))
    return _result('stage_contract', proposal_hash=proposal['artifact_hash'], split=split, counts=counts,
                   outcomes=deepcopy(outcomes), scores_hidden=True, probabilities_read_before_registration=False,
                   all_allocated_outcomes_retained=True, complete_terminal_collection=counts['pending'] == counts['unstarted'] == 0)


def registration_candidate(proposal, split, outcomes, *, scores_hidden=True, jev_probabilities_read=False):
    """Quota-ready local shape only; does not create genuine registration."""
    stage = stage_contract(proposal, split, outcomes, scores_hidden=scores_hidden,
                           jev_probabilities_read=jev_probabilities_read)
    spec = protocol.specification()
    require(stage['complete_terminal_collection'] is True, 'complete_terminal_allocation_required_before_registration')
    require(stage['counts']['supported'] >= spec['min_supported']
            and stage['counts']['unsupported'] >= spec['min_unsupported'], 'original_150_supported_50_unsupported_required')
    return _result('registration_candidate', proposal_hash=proposal['artifact_hash'], split=split,
                   stage=stage, protocol=spec, protocol_hash=digest(spec), labels_hidden_by_declared_contract=True)


def _observations(registration, observations):
    rows = registration['stage']['outcomes']
    eligible = [r for r in rows if r['label'] in {'supported', 'unsupported'}]
    require(type(observations) is list and len(observations) == len(eligible), 'every_binary_candidate_attempt_required')
    by_id = {r['id']: r for r in eligible}
    require(len({r['id'] for r in observations}) == len(observations)
            and {r['id'] for r in observations} == set(by_id), 'missing_repeated_or_foreign_jev_attempt')
    for row in observations:
        require(type(row) is dict and set(row) == OBS_FIELDS, 'original_attempt_descriptor_fields_required')
        original = by_id[row['id']]
        require(all(row[k] == original[k] for k in ('id', 'group_id', 'split'))
                and row['candidate_sha256'] == original['evidence']['candidate_sha256']
                and row['oracle_sha256'] == original['evidence']['oracle_sha256'], 'attempt_candidate_label_or_split_changed')
        require(all(_hash(row[k]) for k in ('request_hash', 'attempt_started_sha256', 'reservation_sha256', 'runner_executable_sha256'))
                and _text(row['call_status']), 'single_original_jev_attempt_descriptors_required')
        require(row['actual_request_hash'] is None or _hash(row['actual_request_hash']), 'actual_request_hash_descriptor_required')
        require(row['response_sha256'] is None or _hash(row['response_sha256']), 'response_hash_descriptor_required')
        if row['call_status'] != 'ok':
            require(row['acceptance_violation'] is None, 'failed_attempt_probability_forbidden')
    return eligible


def calibration_candidate(proposal, outcomes, observations):
    """All original-proof fields here are local claims, never certification."""
    registered = registration_candidate(proposal, 'calibration', outcomes)
    eligible = _observations(registered, observations)
    numeric = protocol.calibrate_numbers(eligible, observations, registered['protocol'])
    return _result('calibration_candidate', proposal_hash=proposal['artifact_hash'], registration_candidate=registered,
                   observations=deepcopy(observations), numeric=numeric, threshold_status='numeric_candidate_not_authenticated')


def holdout_candidate(proposal, calibration, outcomes, observations, *, threshold=None):
    """Locked-threshold numerical rehearsal, no opening or consumption.

    Real holdout requests still require every missing original-proof interface
    and the shared durable one-use marker; this artifact cannot be used as it.
    """
    _sealed(calibration)
    _limits(calibration)
    require(calibration.get('kind') == 'calibration_candidate'
            and calibration['proposal_hash'] == proposal['artifact_hash'], 'same_new_calibration_required')
    cal_outcomes = calibration['registration_candidate']['stage']['outcomes']
    expected = calibration_candidate(proposal, cal_outcomes, calibration['observations'])
    require(_same(expected, calibration), 'calibration_candidate_or_selected_threshold_changed')
    chosen = calibration['numeric']['qualified_numeric_threshold']
    require(calibration['numeric']['calibration_numeric_gate_met'] is True and chosen is not None,
            'qualified_numeric_calibration_required_for_holdout_rehearsal')
    require(threshold is None or (type(threshold) in (int, float) and _same(threshold, chosen)),
            'holdout_threshold_replacement_forbidden')
    registered = registration_candidate(proposal, 'holdout', outcomes)
    eligible = _observations(registered, observations)
    numeric = protocol.numbers(eligible, observations, chosen, registered['protocol'])
    return _result('holdout_candidate', proposal_hash=proposal['artifact_hash'], calibration_candidate_hash=calibration['artifact_hash'],
                   threshold=chosen, numeric=numeric, counts=registered['stage']['counts'],
                   holdout_status='numeric_rehearsal_only_not_opened_or_consumed', threshold_retuned=False,
                   authority_rejection_reason='numeric_results_are_not_original_proof_or_shared_once_markers')
