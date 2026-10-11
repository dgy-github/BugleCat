"""Versioned APPS fixed statistics; pure numbers never grant certification.

Reuse the original immutable artifact gates and exact binomial computation.
Unknown candidate labels are excluded explicitly. Failed Jev attempts for a
registered binary candidate remain in the denominator and are never retried.
"""
from copy import deepcopy

from ..artifact_gate import metrics
from ..artifact_v2_formal import GATES, GRID
from ..calibration import probability, require
from . import packet, resources

SCHEMA = "buglecat.verification.apps-ordered-pairs-protocol.v3"


def specification():
    return {'schema': SCHEMA, 'template_version': packet.TEMPLATE, 'policy_version': packet.POLICY,
            'model': packet.MODEL, 'source_kinds': sorted(packet.KINDS),
            'categories': ['apps_ordered_pair_acceptance'], 'score': 'single_acceptance_violation_noul',
            'threshold_grid': list(GRID), **deepcopy(GATES),
            'reference_admission_timeout_s': 5, 'candidate_oracle_timeout_s': 5,
            'maximum_request_bytes': packet.MAX_INPUT_BYTES,
            'candidate_json_byte_reserve': resources.CANDIDATE_JSON_BYTES,
            'all_original_pairs_required': True, 'reference_index': 0,
            'automatic_retries': 0, 'per_group_max_main_attempts': 1, 'per_group_max_jev_attempts': 1,
            'calibration_and_holdout_each_require_original_group_quotas': True,
            'binary_labels_registered_before_jev': True,
            'unknown_not_binary': True, 'failed_jev_attempts_retained_in_denominators': True,
            'holdout_requires_qualified_new_calibration': True, 'holdout_max_consumptions': 1,
            'old_template_probabilities_or_thresholds_reused': False,
            'native_claim_certification': False, 'overall_agent_benefit_certification': False,
            'scope': 'new APPS original restricted ordered call-based pairs only; no general correctness'}


def validate(spec):
    require(packet.canonical_bytes(spec) == packet.canonical_bytes(specification()),
            'frozen_APPS_protocol_changed')
    return spec


def numbers(cases, observations, threshold, spec):
    validate(spec)
    require(type(threshold) in (int, float) and threshold in GRID and not isinstance(threshold, bool),
            'threshold_not_predeclared')
    by_id = {case['id']: case for case in cases}
    require(len(by_id) == len(cases) and len({case['group_id'] for case in cases}) == len(cases),
            'one_candidate_per_independent_group_required')
    require(all(case['label'] in {'supported', 'unsupported'} for case in cases),
            'unknown_cannot_be_binary')
    rows = {row['id']: row for row in observations}
    require(len(rows) == len(observations) and rows.keys() == by_id.keys(),
            'missing_repeated_or_foreign_observation')
    rejected, failures = set(), {}
    for identity, row in rows.items():
        if row['call_status'] != 'ok':
            require(row.get('acceptance_violation') is None, 'failed_probability_not_permitted')
            failures[identity] = row['call_status']
            continue
        require(row.get('model') == packet.MODEL
                and row.get('transport_binding_status') == 'original_transport_bytes_verified'
                and row.get('actual_request_hash') == row.get('request_hash'),
                'original_versioned_transport_required')
        for name in ('request_hash', 'actual_request_hash', 'response_sha256'):
            value = row.get(name)
            require(type(value) is str and len(value) == 64 and all(char in '0123456789abcdef' for char in value),
                    'original_transport_hash_required')
        value = row.get('acceptance_violation')
        require(probability(value), 'invalid_exact_question_probability')
        if value >= threshold:
            rejected.add(identity)
    supported = {identity for identity, case in by_id.items() if case['label'] == 'supported'}
    unsupported = set(by_id) - supported
    result = metrics(len(supported), len(unsupported), len(supported & rejected),
                     len(unsupported & rejected), threshold, spec)
    return {**result, 'failed_attempts': failures, 'failed_attempts_retained_in_denominators': True,
            'reject_signals': len(rejected), 'unverified_signals': len(cases) - len(rejected),
            'verified_pass_signals': 0, 'formal_certification': False, 'production_authorized': False}


def calibrate_numbers(cases, observations, spec):
    trials = [numbers(cases, observations, threshold, spec) for threshold in validate(spec)['threshold_grid']]
    eligible = [trial for trial in trials if trial['gate_met']]
    chosen = max(eligible, key=lambda trial: (trial['detected_unsupported'], trial['threshold'])) if eligible else None
    return {'grid_results': trials, 'calibration_numeric_gate_met': chosen is not None,
            'qualified_numeric_threshold': chosen['threshold'] if chosen else None,
            'certified_threshold': None, 'formal_certification': False, 'production_authorized': False,
            'holdout_status': 'not_run', 'native_claim_certification': False,
            'overall_agent_benefit_certification': False,
            'scope': 'numbers only; original registered source/labels/transport and a fresh holdout remain required'}
