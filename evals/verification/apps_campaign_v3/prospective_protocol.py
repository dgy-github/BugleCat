"""Prospective full-scale score grid; old paid protocol remains immutable.

Development outcomes motivate testing the whole score scale, never selecting a
production threshold. New original calibration plus one fresh locked holdout
are still required. Quality gates and exact confidence limits are unchanged.
"""
from ..artifact_gate import metrics
from ..calibration import digest, require
from . import packet, protocol

SCHEMA = 'buglecat.verification.apps-ordered-pairs-protocol.v4'
GRID = tuple(sorted({n / 40 for n in range(1, 41)} | set(protocol.specification()['threshold_grid'])))


def specification():
    base = protocol.specification()
    return {**base, 'schema': SCHEMA, 'threshold_grid': list(GRID),
            'development_protocol_canonical_sha256': digest(base),
            'development_allocation_excluded_from_formal': True,
            'candidate_thresholds_fixed_before_new_formal_requests': True,
            'quality_gates_unchanged': True,
            'development_numeric_results_cannot_qualify_threshold': True,
            'holdout_threshold_tuning_allowed': False}


def validate(spec):
    require(packet.canonical_bytes(spec) == packet.canonical_bytes(specification()),
            'prospective_protocol_changed')
    return spec


def _verified(cases, observations):
    # Reuse the original strict numeric input contract once per transaction.
    # This validates shapes/model/request/response hashes; the genuine formal
    # owner must additionally authenticate actual original file/ledger joins.
    checked = protocol.numbers(cases, observations, 1.0, protocol.specification())
    return checked, {case['id'] for case in cases if case['label'] == 'supported'}


def _numbers(cases, observations, threshold, spec, checked, supported):
    rejected = {row['id'] for row in observations if row['call_status'] == 'ok'
                and row['acceptance_violation'] >= threshold}
    unsupported = {case['id'] for case in cases} - supported
    return {**checked, **metrics(len(supported), len(unsupported), len(supported & rejected),
                               len(unsupported & rejected), threshold, spec),
            'reject_signals': len(rejected), 'unverified_signals': len(cases) - len(rejected),
            'formal_certification': False, 'production_authorized': False,
            'certified_threshold': None, 'verified_pass_signals': 0}


def numbers(cases, observations, threshold, spec):
    validate(spec)
    require(type(threshold) in {int, float} and threshold in GRID, 'threshold_not_predeclared')
    checked, supported = _verified(cases, observations)
    return _numbers(cases, observations, threshold, spec, checked, supported)


def calibrate_numbers(cases, observations, spec):
    validate(spec)
    checked, supported = _verified(cases, observations)
    trials = [_numbers(cases, observations, threshold, spec, checked, supported) for threshold in GRID]
    eligible = [trial for trial in trials if trial['gate_met']]
    chosen = max(eligible, key=lambda trial: (trial['detected_unsupported'], trial['threshold'])) if eligible else None
    return {'grid_results': trials, 'calibration_numeric_gate_met': chosen is not None,
            'qualified_numeric_threshold': chosen['threshold'] if chosen else None,
            'certified_threshold': None, 'formal_certification': False,
            'production_authorized': False, 'holdout_status': 'not_run',
            'native_claim_certification': False, 'overall_agent_benefit_certification': False,
            'quality_gates_unchanged': True,
            'scope': 'numbers only; genuine registered original evidence and one fresh locked holdout required'}
