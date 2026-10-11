import hashlib
import importlib.util
import json
import shutil
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from evals.verification.apps_stdin_v1 import oracle as o


def io(inputs, outputs):
    return {'inputs': inputs, 'outputs': outputs}


def good(index=0):
    return {'index': index, 'status': 'passed', 'passed': True, 'worker_pid': 42,
            'output_truncated': False, 'output_bytes': 0, 'output_sha256': hashlib.sha256(b'').hexdigest(),
            'comparison': o.contract.COMPARISON}


def test_actual_fresh_children_order_and_raw_bindings():
    code = "if __name__ == '__main__':\n    seen=[]\n    seen.append(input())\n    print(len(seen), seen[0])"
    data = io([['a'], ['b']], ['1 a', '1 b'])
    receipt = o.execute(code, data)
    assert receipt['result']['status'] == 'passed'
    pairs = receipt['result']['pair_results']
    assert [p['index'] for p in pairs] == [0, 1]
    assert len({p['worker_pid'] for p in pairs}) == 2
    assert receipt['code_sha256'] == hashlib.sha256(code.encode()).hexdigest()
    assert receipt['input_output_sha256'] == hashlib.sha256(o.canonical(data)).hexdigest()
    assert receipt['full_ordered_pairs_retained'] is True
    assert receipt['source_admitted'] is receipt['production_authorized'] is False
    assert receipt['paid_calls'] == receipt['reservations'] == 0


def test_actual_all_pairs_after_failure_unknown_dominates():
    code = 'x=input()\nif x=="bad":\n    raise ValueError()\nif x=="unknown":\n    raise SystemExit()\nprint(x)'
    receipt = o.execute(code, io(['bad', 'unknown', 'ok'], ['', '', 'ok']))
    assert [r['status'] for r in receipt['result']['pair_results']] == ['failed', 'unknown', 'passed']
    assert receipt['result']['status'] == 'unknown'
    assert receipt['result']['pairs_executed'] == 3


def test_actual_list_no_trailing_newline_eof():
    assert o.execute('print(input())\ntry:\n    input()\nexcept EOFError:\n    print("EOF")',
                     io([['x']], ['x EOF']))['result']['status'] == 'passed'


def test_actual_pair_timeout_then_next_pair_runs():
    receipt = o.execute('x=input()\nwhile x=="spin":\n    pass\nprint(x)', io(['spin', 'ok'], ['', 'ok']))
    assert [r['status'] for r in receipt['result']['pair_results']] == ['unknown', 'passed']
    assert receipt['result']['pair_results'][0]['reason'] == 'pair_timeout'
    assert receipt['result']['pairs_executed'] == 2


def test_frozen_core_owner_bytes_preserved():
    base = Path(o.__file__).resolve().parents[1]
    paths = [base/'benchmark_oracle_worker.py'] + [base/folder/name for folder, names in (
        ('apps_v3', ('semantics.py', 'worker.py', 'oracle.py', 'source.py')),
        ('apps_solution_v3', ('policy.py', 'worker.py', 'oracle.py', 'source.py')))
        for name in names]
    before = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
    assert o.execute('print(input())', io(['fixture'], ['fixture']))['result']['status'] == 'passed'
    assert {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths} == before


def test_no_credentials_or_parent_environment_change(monkeypatch):
    monkeypatch.setenv('NCX_JEV_API_KEY', 'fixture-only')
    monkeypatch.setenv('OPENAI_TOKEN', 'fixture-only')
    monkeypatch.setenv('OTHER_SECRET', 'fixture-only')
    monkeypatch.setenv('SystemRoot', 'fixture-root')
    seen = []
    def child(payload, work, environment, timeout):
        seen.append(environment)
        return SimpleNamespace(returncode=0, stdout=json.dumps(good(payload['index'])))
    monkeypatch.setattr(o, '_child', child)
    assert o.execute('pass', io([''], ['']))['result']['status'] == 'passed'
    assert {k.upper(): v for k, v in seen[0].items()} == {k.upper(): v for k, v in o.os.environ.items()
                     if k.upper() in {'WINDIR', 'TEMP', 'TMP', 'SYSTEMROOT'}}
    assert not any(any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET')) for k in seen[0])
    assert o.os.environ['NCX_JEV_API_KEY'] == 'fixture-only'


def test_timeouts_retained_and_deadline_unstarted(monkeypatch):
    times = iter([0, 0, 31, 31, 31])
    monkeypatch.setattr(o.time, 'monotonic', lambda: next(times))
    def child(*args):
        raise subprocess.TimeoutExpired('fixture', 5)
    monkeypatch.setattr(o, '_child', child)
    receipt = o.execute('pass', io(['a', 'b', 'c'], ['', '', '']))
    pairs = receipt['result']['pair_results']
    assert [p['child_started'] for p in pairs] == [True, False, False]
    assert [p['reason'] for p in pairs] == ['pair_timeout', 'whole_group_deadline', 'whole_group_deadline']
    assert receipt['full_ordered_pairs_retained'] is True


def test_limit_no_projection(monkeypatch):
    monkeypatch.setattr(o, '_child', lambda *a: pytest.fail('child must not start'))
    result = o.execute('pass', io(['']*101, ['']*101))
    assert result['result']['status'] == 'unknown'
    assert result['result']['pair_count'] == 101 and result['result']['pairs_executed'] == 0


@pytest.mark.parametrize('patch', [
    {'index': True}, {'passed': 1}, {'worker_pid': False}, {'output_bytes': True},
    {'status': 'failed'}, {'comparison': 'tolerant'}, {'output_truncated': True},
    {'output_sha256': None}, {'output_sha256': 'f'*63}, {'output_sha256': 'F'*64},
    {'raw_candidate': 'must not propagate'},
])
def test_malformed_worker_receipt(patch):
    assert o.checked_result({**good(), **patch}, 0)['status'] == 'unknown'


@pytest.mark.parametrize('raw', ['{"x":NaN}', '{"x":Infinity}', '{"x":1e400}'])
def test_nonfinite_raw_is_unknown(monkeypatch, raw):
    monkeypatch.setattr(o, '_child', lambda *a: SimpleNamespace(returncode=0, stdout=raw))
    assert o.execute('pass', io([''], ['']))['result']['status'] == 'unknown'


def test_outputless_syntax_receipt_is_valid():
    result = {**good(), 'status': 'failed', 'passed': False, 'error_kind': 'SyntaxError', 'output_sha256': None}
    assert o.checked_result(result, 0) is result


def test_input_mutation_during_child_rejected(monkeypatch):
    data = io([''], [''])
    def child(*args):
        data['outputs'][0] = 'mutated'
        return SimpleNamespace(returncode=0, stdout=json.dumps(good()))
    monkeypatch.setattr(o, '_child', child)
    with pytest.raises(ValueError, match='changed'):
        o.execute('pass', data)


def copied_wrapper(tmp_path):
    folder = tmp_path/'verification'/'apps_stdin_v1'
    folder.mkdir(parents=True)
    for name in ('contract.py', 'worker.py', 'oracle.py'):
        shutil.copyfile(Path(o.__file__).with_name(name), folder/name)
    shutil.copyfile(o.contract.ORIGINAL_POLICY, folder.parent/'benchmark_oracle_worker.py')
    spec = importlib.util.spec_from_file_location('fixture_stdin_wrapper', folder/'oracle.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize('role', ['contract', 'wrapper', 'frozen_policy_constants'])
def test_import_then_disk_drift_rejected_before_child(tmp_path, monkeypatch, role):
    module = copied_wrapper(tmp_path)
    path = module.source_graph()[role]
    path.write_bytes(path.read_bytes()+b'\n# fixture drift\n')
    monkeypatch.setattr(module, '_child', lambda *a: pytest.fail('must not start'))
    with pytest.raises(ValueError, match='loaded_owner'):
        module.execute('pass', io([''], ['']))


@pytest.mark.parametrize('attribute,value', [('PAIR_TIMEOUT', 1), ('MAX_OUTPUT_BYTES', 100)])
def test_loaded_parameters_drift(tmp_path, monkeypatch, attribute, value):
    module = copied_wrapper(tmp_path)
    monkeypatch.setattr(module.contract, attribute, value)
    with pytest.raises(ValueError, match='loaded_owner'):
        module.execute('pass', io([''], ['']))


def test_loaded_parameters_dictionary_drift(tmp_path):
    module = copied_wrapper(tmp_path)
    module.contract.PARAMETERS['whole_group_timeout_seconds'] = 300
    with pytest.raises(ValueError, match='loaded_owner'):
        module.execute('pass', io([''], ['']))


def test_loaded_policy_values_drift(tmp_path):
    module = copied_wrapper(tmp_path)
    module.contract.MODULES = module.contract.MODULES | {'os'}
    with pytest.raises(ValueError, match='loaded_owner'):
        module.execute('pass', io([''], ['']))


def test_loaded_runtime_hash_drift(tmp_path, monkeypatch):
    module = copied_wrapper(tmp_path)
    monkeypatch.setattr(module, 'LOADED_RUNTIME_SHA256', '0'*64)
    with pytest.raises(ValueError, match='loaded_owner'):
        module.execute('pass', io([''], ['']))


def test_stale_wrapper_bytecode_rejected(tmp_path, monkeypatch):
    module = copied_wrapper(tmp_path)
    original = module.checked_result
    module.checked_result = lambda *a: None
    module.checked_result.__module__ = module.__name__
    monkeypatch.setattr(module, '_child', lambda *a: pytest.fail('must not start'))
    with pytest.raises(ValueError, match='bytecode_drift'):
        module.execute('pass', io([''], ['']))
    module.checked_result = original


def test_source_drift_during_child_rejected(tmp_path, monkeypatch):
    module = copied_wrapper(tmp_path)
    def child(*args):
        path = module.source_graph()['worker']
        path.write_bytes(path.read_bytes()+b'\n# fixture drift\n')
        return SimpleNamespace(returncode=0, stdout=json.dumps(good()))
    monkeypatch.setattr(module, '_child', child)
    with pytest.raises(ValueError, match='changed'):
        module.execute('pass', io([''], ['']))
