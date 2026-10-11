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
    with pytest.raises(ValueError, match='16000 byte reserve'):
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


@pytest.mark.parametrize('code', ['x'*15998, '汉'*5332, '\\'*7999, 'x'+'\n'*7998, '"'*7999],
                         ids=['ascii', 'unicode', 'backslash', 'newline', 'quote'])
def test_exact_literal_reserve_is_general_utf8_escaped_bound(code):
    assert len(p.canonical_bytes(code)) <= 16000
    probe = body(candidate_code='x'*15998)
    actual = body(candidate_code=code)
    assert len(p.canonical_bytes(actual)) <= len(p.canonical_bytes(probe))
    assert actual['state']['candidate_code'] == code


@pytest.mark.parametrize('code', ['x'*15999, '汉'*5333, '\\'*8000, 'x'+'\n'*7999, '"'*8000],
                         ids=['ascii', 'unicode', 'backslash', 'newline', 'quote'])
def test_no_clipping_to_fit_reserve(code):
    with pytest.raises(ValueError, match='16000 byte reserve'):
        body(candidate_code=code)


@pytest.mark.parametrize('name,value', [('PAIR_TIMEOUT', 15), ('GROUP_TIMEOUT', 60), ('MAX_PAIRS', 1000),
                                      ('MAX_OUTPUT_BYTES', 1_000_000), ('MAX_CODE_BYTES', 100_000), ('MAX_PAIRS', 100.0)])
def test_actual_loaded_policy_parameters_checked(monkeypatch, name, value):
    monkeypatch.setattr(p.contract, name, value)
    with pytest.raises(ValueError, match='parameters differ'):
        body()


def test_more_than_100_pairs_cannot_hide_behind_loaded_policy(monkeypatch):
    monkeypatch.setattr(p.contract, 'MAX_PAIRS', 1000)
    with pytest.raises(ValueError, match='parameters differ'):
        body(input_output={'inputs': ['']*101, 'outputs': ['']*101})


@pytest.mark.parametrize('name,value', [('MODEL', 'jev-other'), ('MAX_INPUT_BYTES', 500_000),
                                      ('MAX_INPUT_BYTES', 48000.0), ('PREFIX', 'changed')])
def test_numeric_loaded_constants_checked(monkeypatch, name, value):
    monkeypatch.setattr(p.numeric, name, value)
    with pytest.raises(ValueError, match='numeric constants differ'):
        body()


@pytest.mark.parametrize('module,name', [('numeric', 'answer_signal'), ('numeric_checks', 'probability'),
                                       ('numeric_checks', 'digest'), ('numeric_checks', 'require')])
def test_loaded_numeric_functions_checked_regardless_module_tag(monkeypatch, module, name):
    replacement = lambda *args: {}  # noqa: E731
    monkeypatch.setattr(getattr(p, module), name, replacement)
    with pytest.raises(ValueError, match='bytecode differs'):
        body()


def test_stale_numeric_function_alias_checked(monkeypatch):
    monkeypatch.setattr(p, 'answer_signal', lambda *args: {})
    with pytest.raises(ValueError, match='import aliases differ'):
        body()


def test_stale_numeric_probability_alias_checked(monkeypatch):
    monkeypatch.setattr(p.numeric, 'probability', lambda *args: True)
    with pytest.raises(ValueError, match='import aliases differ'):
        body()


@pytest.mark.parametrize('module,name', [('numeric', 'request'), ('numeric_checks', 'digest')])
def test_loaded_numeric_declaration_not_function(monkeypatch, module, name):
    monkeypatch.setattr(getattr(p, module), name, 0)
    with pytest.raises(ValueError, match='bytecode differs'):
        body()


def test_numeric_bound_shape_sets_checked(monkeypatch):
    monkeypatch.setattr(p.numeric_checks, 'VERDICTS', {'unknown'})
    with pytest.raises(ValueError, match='numeric constants differ'):
        body()


def test_local_reserve_cannot_be_relaxed_in_memory(monkeypatch):
    monkeypatch.setattr(p, 'MAX_CANDIDATE_JSON_LITERAL_BYTES', 100_000)
    with pytest.raises(ValueError, match='numeric constants differ'):
        body(candidate_code='x'*20_000)


@pytest.mark.parametrize('name', ['LOADED_NUMERIC_SHA256', 'LOADED_NUMERIC_CHECKS_SHA256'])
def test_loaded_numeric_source_hash_drift(monkeypatch, name):
    monkeypatch.setattr(p, name, '0'*64)
    with pytest.raises(ValueError, match='loaded STDIN policy'):
        body()


def test_loaded_runtime_file_drift(monkeypatch):
    monkeypatch.setattr(p, 'LOADED_RUNTIME_SHA256', '0'*64)
    with pytest.raises(ValueError, match='runtime source changed'):
        body()


def test_builder_function_foreign_module_tag_also_rejected(monkeypatch):
    monkeypatch.setattr(p, 'execution_contract', lambda *args: {})
    with pytest.raises(ValueError, match='bytecode differs'):
        body()


@pytest.mark.parametrize('response_patch', [
    {'model': 'jev-other'}, {'answers': {}},
    {'answers': {'foreign': {'type': 'noul', 'noul': .4}}},
    {'answers': {'acceptance_violation': {'type': 'text', 'noul': .4}}},
    {'usage': {'input_tokens': 1}},
])
def test_model_question_shape_and_usage_exact(response_patch):
    with pytest.raises(ValueError):
        p.signal(body(), {**response(), **response_patch})


def test_unbounded_noul_integer_is_invalid_not_an_overflow_interface():
    with pytest.raises(ValueError, match='numeric magnitude'):
        p.signal(body(), response(10**1000))
