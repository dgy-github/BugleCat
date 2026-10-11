"""Pure packet tests; no reference execution or remote calls."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import packet as p


def body(**changes):
    values = {'question': 'Echo the line.', 'starter_code': '', 'candidate_code': 'print(input())',
              'input_output': {'inputs': ['secret original test', ['a', 'b']],
                               'outputs': ['secret original test', 'a']}}
    values.update(changes)
    return p.request(**values)


def test_complete_bytes_runtime_contract_and_no_aliasing():
    original = {'inputs': [['x', 'y']], 'outputs': ['x y'], 'fn_name': None}
    value = body(input_output=original)
    assert p.validate_request(value) is value
    assert value['model'] == 'jev-latest'
    assert value['questions']['acceptance_violation']['type'] == 'noul'
    assert value['state']['input_output'] == original
    assert value['state']['execution_contract']['runtime'] == p.runtime_info()
    assert value['state']['execution_contract']['parameters']['fresh_child_each_pair']
    assert not value['state']['execution_contract']['parameters']['official_evaluator_parity']
    assert 'packet' in value['state']['dependency_sha256']
    assert 'executable_file' not in str(value['state']['execution_contract']['runtime'])
    value['state']['input_output']['inputs'][0].append('changed')
    assert original['inputs'] == [['x', 'y']]


def test_main_prompt_has_public_task_and_policy_only():
    prompt = p.main_prompt('Echo public line.', '# public starter')
    assert 'Echo public line.' in prompt and '# public starter' in prompt
    assert 'secret original test' not in prompt
    assert 'fresh child' in prompt and 'input()' in prompt
    assert 'SystemExit' in prompt and 'Unknown' in prompt


@pytest.mark.parametrize('target,key', [
    ('body', 'label'), ('body', 'reference'), ('state', 'oracle_result'),
    ('state', 'reference_code'), ('state', 'probability'), ('interface', 'source_path'),
    ('io', 'reference'), ('contract', 'oracle_observed_status'), ('dependencies', 'fake_owner'),
])
def test_labels_results_references_and_extra_fields_rejected(target, key):
    value = body()
    destinations = {'body': value, 'state': value['state'], 'interface': value['state']['public_interface'],
                    'io': value['state']['input_output'], 'contract': value['state']['execution_contract'],
                    'dependencies': value['state']['dependency_sha256']}
    destinations[target][key] = 'forbidden'
    with pytest.raises(ValueError):
        p.validate_request(value)


@pytest.mark.parametrize('mutate', [
    lambda x: x.update(model='other'),
    lambda x: x['questions']['acceptance_violation'].update(type='other'),
    lambda x: x['questions']['acceptance_violation'].update(instructions='changed'),
    lambda x: x['state'].update(pair_count=True),
    lambda x: x['state'].update(pair_count=2.0),
    lambda x: x['state'].update(scope='general correctness'),
    lambda x: x['state']['public_interface'].update(mode='call_based'),
    lambda x: x['state']['execution_contract']['parameters'].update(per_pair_timeout_seconds=15),
    lambda x: x['state']['execution_contract'].update(comparison='float tolerance'),
])
def test_tampered_contracts_rejected(mutate):
    value = body()
    mutate(value)
    with pytest.raises(ValueError):
        p.validate_request(value)


@pytest.mark.parametrize('io', [
    {'inputs': [], 'outputs': []}, {'inputs': ['a'], 'outputs': []},
    {'inputs': [('a',)], 'outputs': ['a']}, {'inputs': ('a',), 'outputs': ['a']},
    {'inputs': [1], 'outputs': ['1']}, {'inputs': ['a'], 'outputs': [True]},
    {'inputs': ['a'], 'outputs': ['a'], 'fn_name': 'f'},
])
def test_invalid_or_projected_io_rejected(io):
    with pytest.raises(ValueError):
        body(input_output=io)


def test_resources_fail_without_cropping_originals():
    original = {'inputs': ['汉' * 16000], 'outputs': ['x']}
    copy = deepcopy(original)
    with pytest.raises(ValueError, match='byte budget'):
        body(input_output=original)
    assert original == copy
    with pytest.raises(ValueError, match='byte budget'):
        body(candidate_code='print(input())\n#' + 'x' * 48000)


def test_runtime_receipt_fields_checked_and_removed_from_request():
    runtime = {**p.runtime_info(), 'version': p.sys.version, 'executable_file': p.sys.executable}
    value = body(oracle_runtime_binding=runtime)
    assert value['state']['execution_contract']['runtime'] == p.runtime_info()
    with pytest.raises(ValueError):
        body(oracle_runtime_binding={**runtime, 'version_info': [0, 0, 0]})
    with pytest.raises(ValueError):
        body(oracle_runtime_binding={**runtime, 'label': 'supported'})


def response(value=0.4, usage=None):
    return {'model': 'jev-1.13.0', 'answers': {'acceptance_violation': {'type': 'noul', 'noul': value}},
            'usage': usage or {'input_tokens': 12, 'output_tokens': 1}}


def test_single_signal_is_not_a_verified_pass_or_certification():
    signal = p.signal(body(), response())
    assert signal['acceptance_violation'] == 0.4
    assert signal['verified_pass_signals'] == 0
    assert signal['certified_threshold'] is None
    assert not signal['production_authorized'] and not signal['formal_certification']


@pytest.mark.parametrize('value', [True, float('nan'), float('inf'), -0.1, 1.1])
def test_invalid_numeric_signal(value):
    with pytest.raises(ValueError):
        p.signal(body(), response(value))


@pytest.mark.parametrize('usage', [
    {'input_tokens': True, 'output_tokens': 1},
    {'input_tokens': 2**64, 'output_tokens': 1},
    {'input_tokens': 1, 'output_tokens': -1},
])
def test_invalid_usage(usage):
    with pytest.raises(ValueError):
        p.signal(body(), response(usage=usage))


def test_loaded_source_hash_drift_rejected(monkeypatch):
    monkeypatch.setattr(p, 'LOADED_SOURCE_SHA256', '0' * 64)
    with pytest.raises(ValueError, match='loaded STDIN policy'):
        body()


def test_loaded_builder_function_drift_rejected(monkeypatch):
    replacement = lambda *args: {}  # noqa: E731
    replacement.__module__ = p.__name__
    monkeypatch.setattr(p, 'execution_contract', replacement)
    with pytest.raises(ValueError, match='bytecode differs'):
        body()
