"""Pure original holdout packet policy; no actual registration authority."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, packet
from evals.verification.apps_stdin_v1 import holdout_registration as registration
from tests.test_verification_apps_stdin_v1_holdout_evaluation import fixture as evaluation_fixture


def fixture():
    campaign, data, _, _, _, _, threshold, _ = evaluation_fixture()
    code = 'print(input())\r\n'
    for row in data['cases'][408:]:
        if row['call_eligible']:
            row['candidate_code_span'] = {'code': code}
            row['candidate_code_sha256'] = registration.hashlib.sha256(code.encode()).hexdigest()
    tasks = {row['task_id']: {'question': 'Synthetic echo', 'starter_code': '',
        'input_output': {'inputs': ['7\n', '13\n'], 'outputs': ['7', '13']},
        'reference': 'HIDDEN_SYNTHETIC_REFERENCE'} for row in campaign['cases'][408:]}
    pack = {'cohort': {**campaign, 'oracle_runtime_binding': packet.runtime_info()},
        'source_pack': {'tasks': tasks}, 'opening': {'qualified_calibration_threshold': threshold},
        'chain': {'authorization': {'jev': {'max_attempts': 208}}},
        'current': {'jev_reservations': [{}] * 8}}
    return {'proof': {'pack': pack, 'dataset': data}, 'report': {'independent_quotas_met': True}}


def test_whole_original_binary_packets_are_blind_and_keep_utf8_code_and_ordered_io():
    joined = fixture()
    rows = registration._requests(joined)
    assert len(rows) == 200
    first = rows[0]
    assert first['request']['state']['candidate_code'] == 'print(input())\r\n'
    assert first['request']['state']['input_output'] == {'inputs': ['7\n', '13\n'], 'outputs': ['7', '13']}
    assert budget.digest(first['request']) == first['request_hash']
    assert packet.canonical_bytes(first['request']).decode() == first['request_json']
    assert 'HIDDEN_SYNTHETIC_REFERENCE' not in first['request_json']
    assert not any(key in first['request']['state'] for key in ('label', 'oracle_sha256', 'oracle_result', 'reference'))
    task = joined['proof']['pack']['source_pack']['tasks'][first['task_id']]
    assert first['request'] == packet.request(task['question'], task['starter_code'], 'print(input())\r\n',
        task['input_output'], joined['proof']['pack']['cohort']['oracle_runtime_binding'])


@pytest.mark.parametrize('mutation', ['quota', 'funds', 'threshold_replace', 'bool_threshold', 'code_changed'])
def test_pure_packet_requests_reject_incomplete_funding_or_replaced_frozen_inputs(mutation):
    joined = deepcopy(fixture())
    if mutation == 'quota':
        joined['report']['independent_quotas_met'] = False
    elif mutation == 'funds':
        joined['proof']['pack']['chain']['authorization']['jev']['max_attempts'] = 207
    elif mutation == 'threshold_replace':
        joined['proof']['dataset']['qualified_calibration_threshold'] = .9
    elif mutation == 'bool_threshold':
        joined['proof']['pack']['opening']['qualified_calibration_threshold'] = True
    else:
        joined['proof']['dataset']['cases'][408]['candidate_code_span']['code'] += 'print("changed")'
    with pytest.raises(ValueError):
        registration._requests(joined)
