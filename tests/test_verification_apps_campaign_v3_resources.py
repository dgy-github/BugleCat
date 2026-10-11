"""Resource refusal preserves complete pairs and original candidates."""
from copy import deepcopy

import pytest

from evals.verification.apps_campaign_v3 import packet, resources


def task():
    return {'question': 'Return the supplied argument.', 'starter_code': '',
            'input_output': {'fn_name': 'f', 'inputs': [[1], [2]], 'outputs': [1, 2]},
            'source_kind': packet.FREE, 'reference': 'DO NOT READ ME'}


def test_reserved_json_bytes_account_for_quotes_and_escaping():
    assert resources.candidate_size('"\\\n') == len(packet.canonical_bytes('"\\\n'))
    assert resources.candidate_size('"\\\n') > len('"\\\n'.encode())


def test_original_pairs_and_code_unchanged():
    original = task()
    before = deepcopy(original)
    code = 'def f(value):\n    return value\n'
    body = resources.checked_request(original, code)
    assert body['state']['candidate_code'] == code
    assert body['state']['input_output'] == original['input_output']
    assert body['state']['pair_count'] == 2 and original == before
    assert 'DO NOT READ ME' not in packet.canonical_bytes(body).decode()


def test_long_original_io_is_refused_without_pair_truncation():
    original = task()
    original['input_output']['inputs'] = [['x' * 31000]]
    original['input_output']['outputs'] = ['x' * 31000]
    before = deepcopy(original)
    with pytest.raises(ValueError, match='input byte budget'):
        resources.inspect(original)
    assert original == before


def test_reserved_space_refusal_before_candidate_exists():
    original = task()
    original['question'] = 'q' * 28000
    result = resources.inspect(original)
    assert result['maximum_reserved_packet_bytes'] > packet.MAX_INPUT_BYTES
    assert result['resource_qualified'] is False and result['candidate_label'] is None
    with pytest.raises(ValueError, match='source_complete_packet_reserve'):
        resources.checked_request(original, 'def f(value): return value')


def test_oversize_candidate_is_not_trimmed_or_labeled_negative():
    original = task()
    code = '# ' + '\\' * 16000
    with pytest.raises(ValueError, match='original_candidate_json_byte_reserve'):
        resources.checked_request(original, code)
    assert resources.candidate_size(code) > resources.CANDIDATE_JSON_BYTES
