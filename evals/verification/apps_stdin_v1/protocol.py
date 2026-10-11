"""STDIN numeric gates; original provenance and one new holdout are separate.

Reuse frozen exact-binomial computation. Failed Jev attempts keep their binary
group denominator. Pure numbers cannot certify or authorize paid operations.
"""
import hashlib
import sys
from copy import deepcopy
from pathlib import Path

from .. import artifact_gate, artifact_v2_formal, calibration
from ..artifact_gate import metrics
from ..artifact_v2_formal import GATES
from ..calibration import probability
from . import contract, packet

SCHEMA = 'buglecat.verification.stdin-ordered-pairs-protocol.v1'
GRID = tuple(sorted({n / 40 for n in range(1, 41)} | {.99}))
MODEL = 'jev-1.13.0'
SOURCE_KIND = 'apps_stdin_restricted_script'
HASH_FIELDS = ('candidate_sha256', 'oracle_sha256', 'request_hash', 'actual_request_hash', 'response_sha256')
TERMINAL_STATUSES = ('ok', 'invalid_json', 'actual_model_mismatch', 'answers', 'exact_answers_required',
                     'invalid_noul', 'invalid_usage', 'timeout', 'cancelled', 'missing_key',
                     'response_budget', 'client_error', 'transport_error')


def dependencies():
    files = {'protocol': Path(__file__), 'numeric_gates': Path(artifact_gate.__file__),
             'original_quality_gates': Path(artifact_v2_formal.__file__)}
    hashes = {role: hashlib.sha256(path.read_bytes()).hexdigest() for role, path in files.items()}
    for module in (sys.modules[__name__], artifact_gate):
        packet.loaded_functions(module, Path(module.__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__),
                            ('SCHEMA', 'MODEL', 'SOURCE_KIND', 'HASH_FIELDS', 'TERMINAL_STATUSES'))
    packet.require(type(GRID) is tuple and GRID == tuple(sorted({n / 40 for n in range(1, 41)} | {.99})),
                   'loaded STDIN threshold grid changed')
    packet.loaded_constants(artifact_v2_formal, Path(artifact_v2_formal.__file__), ('GATES',))
    packet.require(packet.canonical_bytes(GATES) == packet.canonical_bytes({
        'min_supported': 150, 'min_unsupported': 50, 'max_false_block_rate': .02,
        'confidence': .95, 'min_error_rejection_lower_bound': .5}),
        'typed original STDIN quality gates changed')
    packet.require(metrics is artifact_gate.metrics and artifact_gate.upper_bound is calibration.upper_bound
                   and probability is calibration.probability and GATES is artifact_v2_formal.GATES,
                   'loaded STDIN numeric gate aliases changed')
    return hashes


def specification():
    return {'schema': SCHEMA, 'template_version': packet.TEMPLATE,
            'packet_dependency_sha256': packet.dependencies(),
            'numeric_gate_dependency_sha256': dependencies(),
            'execution_parameters': deepcopy(contract.LOADED_PARAMETERS),
            'model': MODEL, 'source_kinds': [SOURCE_KIND],
            'categories': ['stdin_ordered_pair_acceptance'],
            'score': 'single_acceptance_violation_noul', 'threshold_grid': list(GRID), **deepcopy(GATES),
            'candidate_json_literal_byte_limit': packet.MAX_CANDIDATE_JSON_LITERAL_BYTES,
            'maximum_request_bytes': packet.MAX_INPUT_BYTES,
            'automatic_retries': 0, 'per_group_max_main_attempts': 1, 'per_group_max_jev_attempts': 1,
            'all_original_pairs_required': True, 'reference_index': 0,
            'comparison': 'ordered-whitespace-tokens-v1', 'original_script_unchanged': True,
            'fresh_child_per_pair': True, 'official_evaluator_parity': False,
            'unknown_not_binary': True, 'binary_labels_registered_before_jev': True,
            'failed_jev_attempts_retained_in_denominators': True,
            'terminal_statuses': list(TERMINAL_STATUSES),
            'terminal_http_status_rule': 'http_NNN ASCII non-2xx status 100..599 only',
            'pending_or_not_started_in_numeric_denominators_allowed': False,
            'quality_gates_unchanged': True,
            'threshold_tie_break': 'max_detected_unsupported_then_largest_threshold',
            'holdout_requires_qualified_new_calibration': True, 'holdout_max_consumptions': 1,
            'old_template_probabilities_or_thresholds_reused': False,
            'native_claim_certification': False, 'overall_agent_benefit_certification': False,
            'scope': 'Restricted STDIN original ordered pairs only; no general correctness or pass verdict'}


def validate(spec):
    packet.require(packet.canonical_bytes(spec) == packet.canonical_bytes(specification()),
                   'frozen STDIN protocol changed')
    return spec


def _hash(value):
    return type(value) is str and len(value) == 64 and all(c in '0123456789abcdef' for c in value)


def terminal_status(value):
    return type(value) is str and (value in TERMINAL_STATUSES or (
        len(value) == 8 and value.startswith('http_') and value[5:].isascii() and value[5:].isdigit()
        and 100 <= int(value[5:]) <= 599 and not 200 <= int(value[5:]) <= 299))


def _verified(cases, observations, spec):
    validate(spec)
    packet.require(type(cases) is list and type(observations) is list, 'complete original lists required')
    ids, groups, supported = set(), set(), set()
    for case in cases:
        packet.require(type(case) is dict and type(case.get('id')) is str and bool(case['id'])
                       and type(case.get('group_id')) is str and bool(case['group_id'])
                       and case['id'] not in ids and case['group_id'] not in groups,
                       'one original candidate per independent group required')
        packet.require(case.get('source_kind') == SOURCE_KIND
                       and case.get('label') in ('supported', 'unsupported')
                       and _hash(case.get('candidate_sha256')) and _hash(case.get('oracle_sha256')),
                       'new STDIN binary label and candidate/Oracle hashes required')
        ids.add(case['id'])
        groups.add(case['group_id'])
        if case['label'] == 'supported':
            supported.add(case['id'])
    by_id = {case['id']: case for case in cases}
    rows, failures, scores = set(), {}, {}
    for row in observations:
        packet.require(type(row) is dict and type(row.get('id')) is str
                       and row['id'] in ids and row['id'] not in rows,
                       'missing repeated or foreign observation')
        case = by_id[row['id']]
        packet.require(row.get('group_id') == case['group_id']
                       and row.get('candidate_sha256') == case['candidate_sha256']
                       and row.get('oracle_sha256') == case['oracle_sha256'],
                       'observation candidate or Oracle identity changed')
        rows.add(row['id'])
        status = row.get('call_status')
        packet.require(terminal_status(status), 'terminal Jev status required; pending and unstarted are not outcomes')
        if status != 'ok':
            packet.require(row.get('acceptance_violation') is None, 'failed attempt cannot contain probability')
            failures[row['id']] = status
            continue
        packet.require(row.get('model') == MODEL
                       and row.get('transport_binding_status') == 'original_transport_bytes_verified'
                       and row.get('request_hash') == row.get('actual_request_hash')
                       and all(_hash(row.get(key)) for key in HASH_FIELDS),
                       'original versioned numeric transport shape required')
        value = row.get('acceptance_violation')
        try:
            valid = probability(value)
        except OverflowError:
            valid = False
        packet.require(valid, 'invalid exact-question NOUL probability')
        scores[row['id']] = value
    packet.require(rows == ids, 'complete original observations required')
    return supported, ids - supported, failures, scores


def _numbers(supported, unsupported, failures, scores, threshold, spec):
    rejected = {identity for identity, score in scores.items() if score >= threshold}
    result = metrics(len(supported), len(unsupported), len(supported & rejected),
                     len(unsupported & rejected), threshold, spec)
    return {**result, 'failed_attempts': failures, 'failed_attempts_retained_in_denominators': True,
            'reject_signals': len(rejected),
            'unverified_signals': len(supported) + len(unsupported) - len(rejected),
            'verified_pass_signals': 0, 'formal_certification': False, 'production_authorized': False,
            'certified_threshold': None}


def numbers(cases, observations, threshold, spec):
    packet.require(type(threshold) in (int, float) and threshold in GRID, 'threshold not predeclared')
    verified = _verified(cases, observations, spec)
    return _numbers(*verified, threshold, spec)


def calibrate_numbers(cases, observations, spec):
    verified = _verified(cases, observations, spec)
    trials = [_numbers(*verified, threshold, spec) for threshold in GRID]
    eligible = [trial for trial in trials if trial['gate_met']]
    chosen = max(eligible, key=lambda trial: (trial['detected_unsupported'], trial['threshold'])) if eligible else None
    return {'grid_results': trials, 'calibration_numeric_gate_met': chosen is not None,
            'qualified_numeric_threshold': chosen['threshold'] if chosen else None,
            'certified_threshold': None, 'formal_certification': False, 'production_authorized': False,
            'holdout_status': 'not_run', 'native_claim_certification': False,
            'overall_agent_benefit_certification': False,
            'scope': 'Numbers only; original independent provenance and once-only new holdout remain required'}
