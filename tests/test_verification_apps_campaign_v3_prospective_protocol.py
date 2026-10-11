import copy

import pytest

from evals.verification.apps_campaign_v3 import packet, protocol
from evals.verification.apps_campaign_v3 import prospective_protocol as owner


def fixture():
    cases = [{'id': f'p{i}', 'group_id': f'p{i}', 'label': 'supported'} for i in range(150)]
    cases += [{'id': f'n{i}', 'group_id': f'n{i}', 'label': 'unsupported'} for i in range(50)]
    rows = [{'id': c['id'], 'call_status': 'ok', 'model': packet.MODEL,
             'acceptance_violation': .29 if c['label'] == 'supported' else .47,
             'request_hash': 'a' * 64, 'actual_request_hash': 'a' * 64,
             'response_sha256': 'b' * 64, 'transport_binding_status': 'original_transport_bytes_verified'} for c in cases]
    return cases, rows


def test_full_grid_preserves_every_old_value_and_quality_gate():
    old, new = protocol.specification(), owner.specification()
    assert set(old['threshold_grid']) <= set(new['threshold_grid'])
    assert new['threshold_grid'] == sorted(set(new['threshold_grid']))
    assert new['threshold_grid'][0] == .025 and new['threshold_grid'][-1] == 1
    assert all(new[key] == old[key] for key in ('min_supported', 'min_unsupported', 'max_false_block_rate',
                                               'confidence', 'min_error_rejection_lower_bound'))
    assert new['holdout_threshold_tuning_allowed'] is False


def test_regular_grid_can_test_sub_half_without_certifying():
    cases, rows = fixture()
    before = copy.deepcopy((cases, rows))
    result = owner.calibrate_numbers(cases, rows, owner.specification())
    assert result['qualified_numeric_threshold'] == .45
    assert result['calibration_numeric_gate_met'] is True
    assert result['certified_threshold'] is None and result['formal_certification'] is False
    assert result['production_authorized'] is False and result['holdout_status'] == 'not_run'
    assert before == (cases, rows)
    assert protocol.calibrate_numbers(cases, rows, protocol.specification())['calibration_numeric_gate_met'] is False


@pytest.mark.parametrize('field,value', [('min_supported', 149), ('min_unsupported', 49),
    ('max_false_block_rate', .021), ('confidence', .90), ('min_error_rejection_lower_bound', .49),
    ('holdout_threshold_tuning_allowed', True), ('quality_gates_unchanged', 1)])
def test_resealed_quality_or_permission_drift_rejected(field, value):
    spec = owner.specification()
    spec[field] = value
    with pytest.raises(ValueError, match='prospective_protocol_changed'):
        owner.validate(spec)


@pytest.mark.parametrize('threshold', [True, False, .47, float('nan'), float('inf')])
def test_threshold_must_be_typed_and_predeclared(threshold):
    with pytest.raises(ValueError, match='threshold_not_predeclared'):
        owner.numbers(*fixture(), threshold, owner.specification())


def test_failure_retains_binary_denominator_without_probability():
    cases, rows = fixture()
    for row in rows[150:]:
        row.update(call_status='timeout', acceptance_violation=None)
    result = owner.numbers(cases, rows, .45, owner.specification())
    assert result['unsupported_groups'] == 50 and result['detected_unsupported'] == 0
    assert len(result['failed_attempts']) == 50 and result['failed_attempts_retained_in_denominators'] is True
    assert result['gate_met'] is False


@pytest.mark.parametrize('mutation', ['unknown', 'missing', 'duplicate', 'model', 'request', 'nonfinite', 'failure_score'])
def test_original_strict_transport_and_group_contract_reused(mutation):
    cases, rows = fixture()
    if mutation == 'unknown':
        cases[0]['label'] = 'unknown'
    elif mutation == 'missing':
        rows.pop()
    elif mutation == 'duplicate':
        rows[0] = dict(rows[1])
    elif mutation == 'model':
        rows[0]['model'] = 'foreign'
    elif mutation == 'request':
        rows[0]['actual_request_hash'] = 'c' * 64
    elif mutation == 'nonfinite':
        rows[0]['acceptance_violation'] = float('inf')
    else:
        rows[0]['call_status'] = 'timeout'
    with pytest.raises(ValueError):
        owner.calibrate_numbers(cases, rows, owner.specification())


def test_dev_sample_never_meets_formal_quotas():
    cases, rows = fixture()
    cases, rows = cases[:6] + cases[150:151], rows[:6] + rows[150:151]
    result = owner.calibrate_numbers(cases, rows, owner.specification())
    assert result['calibration_numeric_gate_met'] is False and result['qualified_numeric_threshold'] is None
    assert all('too_few_unsupported_groups' in trial['gate_reasons'] for trial in result['grid_results'])
