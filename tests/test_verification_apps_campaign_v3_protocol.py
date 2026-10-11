"""Fixed gates, original denominator treatment and no numerical certification."""
from copy import deepcopy

import pytest

from evals.verification.apps_campaign_v3 import packet, protocol


def fixtures():
    cases = [{'id': str(i), 'group_id': f'group-{i}', 'label': 'supported' if i < 150 else 'unsupported'}
             for i in range(200)]
    rows = [{'id': str(i), 'call_status': 'ok', 'acceptance_violation': 0.0 if i < 150 else 1.0,
             'model': packet.MODEL, 'transport_binding_status': 'original_transport_bytes_verified',
             'actual_request_hash': 'a' * 64, 'request_hash': 'a' * 64, 'response_sha256': 'b' * 64}
            for i in range(200)]
    return cases, rows


def test_numerical_success_is_never_formal_certification_or_pass():
    cases, rows = fixtures()
    result = protocol.calibrate_numbers(cases, rows, protocol.specification())
    assert result['calibration_numeric_gate_met'] is True
    assert result['certified_threshold'] is None and result['formal_certification'] is False
    assert result['holdout_status'] == 'not_run' and result['production_authorized'] is False
    assert all(trial['verified_pass_signals'] == 0 for trial in result['grid_results'])


def test_failed_jev_attempts_retain_original_binary_denominators():
    cases, rows = fixtures()
    for row in rows[150:]:
        row.update(call_status='timeout', acceptance_violation=None)
    result = protocol.numbers(cases, rows, .5, protocol.specification())
    assert result['supported_groups'] == 150 and result['unsupported_groups'] == 50
    assert len(result['failed_attempts']) == 50 and result['detected_unsupported'] == 0
    assert result['gate_met'] is False
    assert 'error_rejection_lower_bound_below_limit' in result['gate_reasons']


@pytest.mark.parametrize('mutation', ['quota', 'false_rate', 'grid', 'retry', 'boolean_count'])
def test_protocol_edits_and_boolean_integer_aliases_rejected(mutation):
    spec = protocol.specification()
    if mutation == 'quota':
        spec['min_supported'] = 100
    elif mutation == 'false_rate':
        spec['max_false_block_rate'] = .05
    elif mutation == 'grid':
        spec['threshold_grid'].append(.3)
    elif mutation == 'retry':
        spec['automatic_retries'] = 1
    else:
        spec['per_group_max_main_attempts'] = True
    with pytest.raises(ValueError, match='frozen_APPS_protocol_changed'):
        protocol.validate(spec)


@pytest.mark.parametrize('mutation', ['unknown', 'duplicate_group', 'missing_row', 'failed_score', 'transport', 'hash', 'model'])
def test_unknown_repeated_missing_or_unverified_evidence_rejected(mutation):
    cases, rows = deepcopy(fixtures())
    if mutation == 'unknown':
        cases[0]['label'] = 'unknown'
    elif mutation == 'duplicate_group':
        cases[1]['group_id'] = cases[0]['group_id']
    elif mutation == 'missing_row':
        rows.pop()
    elif mutation == 'failed_score':
        rows[0]['call_status'] = 'timeout'
    elif mutation == 'transport':
        rows[0]['actual_request_hash'] = 'c' * 64
    elif mutation == 'hash':
        rows[0]['response_sha256'] = 'x' * 64
    else:
        rows[0]['model'] = 'different-model'
    with pytest.raises(ValueError):
        protocol.numbers(cases, rows, .5, protocol.specification())
