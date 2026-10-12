"""Synthetic holdout statistics test frozen thresholds; no source authority."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, collection
from evals.verification.apps_stdin_v1 import holdout_evaluation as evaluation
from evals.verification.apps_stdin_v1 import holdout_terminal as terminal
from tests.test_verification_apps_stdin_v1_holdout_terminal import fixture as label_fixture


def fixture():
    fixed, rows, prior, spec = label_fixture()
    decision = terminal._decision(fixed, rows, prior, spec)
    data = {'cases': rows, 'qualified_calibration_threshold': .8,
        'allocated_groups': 808, 'holdout_allocated_groups': 400,
        'holdout_terminal_groups': 400, 'holdout_pending_groups': 0,
        'holdout_uncollected_groups': 0, 'holdout_collected_groups': 400,
        'unresolved_oracle_groups': 0, 'labels_ready': True,
        'label_counts': {key: value for key, value in decision['label_counts'].items() if value}}
    binary = [row for row in rows if row['split'] == 'holdout' and row['call_eligible']]
    requests = [{**{key: row[key] for key in (*collection.history_fields(), 'label',
                 'candidate_code_sha256', 'oracle_sha256')}, 'request_hash': budget.digest(row['id'])} for row in binary]
    registered = {'qualified_calibration_threshold': .8, 'eligible_ids': [row['id'] for row in binary],
                  'request_hashes': [row['request_hash'] for row in requests]}
    observations = [{**{key: row[key] for key in ('id', 'group_id', 'oracle_sha256')},
        'candidate_sha256': row['candidate_code_sha256'], 'call_status': 'ok',
        'acceptance_violation': .9 if row['label'] == 'unsupported' else .1,
        'model': 'jev-1.13.0', 'transport_binding_status': 'original_transport_bytes_verified',
        'request_hash': request['request_hash'], 'actual_request_hash': request['request_hash'],
        'response_sha256': 'e' * 64} for row, request in zip(binary, requests, strict=True)]
    return {'cases': fixed}, data, {'cases': prior}, registered, requests, observations, .8, spec


def test_whole_fixed_threshold_numbers_are_scoped_and_have_no_production_certificate():
    value = evaluation._decision(*fixture())
    assert value['status'] == 'holdout_passed' and value['all_registered_binary_terminal'] is True
    assert value['label_counts']['unknown'] == 200
    assert value['numeric_results']['threshold'] == .8
    assert value['threshold_tuning'] is False and value['certified_threshold'] is None
    assert all(value[key] is False for key in evaluation.FALSE_FLAGS)


def test_perfect_partial_holdout_cannot_be_consumed_or_certified():
    args = list(fixture())
    args[5] = args[5][:-1]
    value = evaluation._decision(*args)
    assert value['status'] == 'holdout_jev_incomplete'
    assert value['numeric_results'] is None and value['holdout_numeric_gate_met'] is False


def test_all_failed_negative_jev_slots_remain_in_denominator():
    args = list(fixture())
    for row in args[5][150:]:
        row.update(call_status='http_400', acceptance_violation=None)
    value = evaluation._decision(*args)
    assert value['status'] == 'holdout_failed' and value['failed_jev_attempts'] == 50
    assert value['numeric_results']['unsupported_groups'] == 50
    assert value['numeric_results']['detected_unsupported'] == 0


def test_failed_fixed_threshold_cannot_be_rescued_by_another_grid_point():
    args = list(fixture())
    args[6] = .99
    args[1]['qualified_calibration_threshold'] = .99
    args[3]['qualified_calibration_threshold'] = .99
    value = evaluation._decision(*args)
    assert value['status'] == 'holdout_failed' and value['fixed_threshold'] == .99
    assert value['numeric_results']['detected_unsupported'] == 0


@pytest.mark.parametrize('mutation', ['threshold_replace', 'bool_threshold', 'duplicate', 'foreign',
                                    'pending', 'candidate', 'model', 'bool_probability', 'request_order'])
def test_threshold_replacement_and_nonoriginal_numeric_transport_rejected(mutation):
    args = deepcopy(list(fixture()))
    if mutation == 'threshold_replace':
        args[6] = .9
    elif mutation == 'bool_threshold':
        args[6] = True
    elif mutation == 'duplicate':
        args[5][-1] = args[5][0]
    elif mutation == 'foreign':
        args[5][-1]['id'] = 'foreign'
    elif mutation == 'pending':
        args[5][-1]['call_status'] = 'reserved_before_transport'
    elif mutation == 'candidate':
        args[5][-1]['candidate_sha256'] = 'f' * 64
    elif mutation == 'model':
        args[5][-1]['model'] = 'wrong'
    elif mutation == 'bool_probability':
        args[5][-1]['acceptance_violation'] = True
    else:
        args[4][-1], args[4][-2] = args[4][-2], args[4][-1]
    with pytest.raises(ValueError):
        evaluation._decision(*args)
