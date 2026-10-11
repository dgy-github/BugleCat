"""Development receipt only: new process per ordered pair, no fee authority.

No reference admission, source split, independent label, registration or model
interface is provided. The language gate is not an operating-system sandbox.
"""
import hashlib
import inspect
import json
import math
import os
import subprocess
import sys
import tempfile
import time
import types
from pathlib import Path

contract_path = Path(__file__).with_name('contract.py')
contract = types.ModuleType('stdin_wrapper_contract')
contract.__file__ = str(contract_path)
exec(compile(contract_path.read_bytes(), str(contract_path), 'exec'), contract.__dict__)
LOADED_WRAPPER_SHA256 = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
LOADED_RUNTIME_SHA256 = hashlib.sha256(Path(sys.executable).resolve().read_bytes()).hexdigest()


def loaded_graph_check(before):
    if (before['wrapper'] != LOADED_WRAPPER_SHA256 or before['runtime'] != LOADED_RUNTIME_SHA256
            or before['contract'] != contract.LOADED_SOURCE_SHA256
            or before['frozen_policy_constants'] != contract.LOADED_ORIGINAL_SHA256
            or contract.PARAMETERS != contract.LOADED_PARAMETERS
            or (contract.MODULES, contract.BUILTINS, contract.PROTECTED, contract.FORBIDDEN,
                frozenset(contract.BAD_ATTRIBUTES)) != contract.LOADED_POLICY_VALUES
            or (contract.POLICY, contract.COMPARISON, contract.PAIR_TIMEOUT, contract.GROUP_TIMEOUT,
                contract.MAX_PAIRS, contract.MAX_OUTPUT_BYTES, contract.MAX_CODE_BYTES) !=
            tuple(contract.LOADED_PARAMETERS[k] for k in ('policy', 'comparison', 'per_pair_timeout_seconds',
                'whole_group_timeout_seconds', 'max_original_pairs', 'max_output_utf8_bytes', 'max_code_utf8_bytes'))):
        raise ValueError('loaded_owner_or_parameters_drift')
    # Reject stale wrapper bytecode: compare actual loaded functions to freshly
    # compiled source without executing a second wrapper or trusting a pyc.
    module_code = compile(Path(__file__).read_bytes(), execute.__code__.co_filename, 'exec')
    expected = {c.co_name: c for c in module_code.co_consts if isinstance(c, types.CodeType)}
    for name, value in globals().items():
        if inspect.isfunction(value) and value.__module__ == __name__ and value.__code__ != expected.get(name):
            raise ValueError('loaded_wrapper_bytecode_drift')


def raw_sha(value):
    return hashlib.sha256(value).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode()


def source_graph():
    folder = Path(__file__).resolve().parent
    return {'contract': folder / 'contract.py', 'worker': folder / 'worker.py', 'wrapper': folder / 'oracle.py',
            'frozen_policy_constants': contract.ORIGINAL_POLICY, 'runtime': Path(sys.executable).resolve()}


def checked_result(result, index):
    bad = {'index': index, 'status': 'unknown', 'passed': False, 'reason': 'invalid_worker_output'}
    if (type(result) is not dict or set(result) - {'index', 'status', 'passed', 'worker_pid', 'comparison',
            'output_truncated', 'output_bytes', 'output_sha256', 'reason', 'error_kind'}
            or type(result.get('index')) is not int or result['index'] != index
            or result.get('status') not in {'passed', 'failed', 'unknown'} or type(result.get('passed')) is not bool
            or result['passed'] is not (result['status'] == 'passed') or type(result.get('output_truncated')) is not bool
            or type(result.get('output_bytes')) is not int or not 0 <= result['output_bytes'] <= contract.MAX_OUTPUT_BYTES
            or type(result.get('worker_pid')) is not int or result['worker_pid'] <= 0
            or result.get('comparison') != contract.COMPARISON):
        return bad
    if result['status'] == 'unknown' and (type(result.get('reason')) is not str or not result['reason']):
        return bad
    if result['status'] != 'unknown' and result['output_truncated']:
        return bad
    digest = result.get('output_sha256')
    if digest is None:
        if (result['status'] == 'passed' or (result['status'] == 'failed' and result.get('error_kind') != 'SyntaxError')
                or result['output_bytes'] != 0):
            return bad
    elif type(digest) is not str or len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest):
        return bad
    return result


def strict_json(raw):
    def constant(_):
        raise ValueError('nonfinite_json')
    value = json.loads(raw, parse_constant=constant)
    def check(item):
        if type(item) is float and not math.isfinite(item):
            raise ValueError('nonfinite_json')
        if type(item) is dict:
            for child in item.values():
                check(child)
        elif type(item) is list:
            for child in item:
                check(child)
    check(value)
    return value


def _child(payload, work, environment, timeout):
    return subprocess.run([sys.executable, '-I', '-S', '-B', str(source_graph()['worker'])],
                          input=json.dumps(payload, allow_nan=False), text=True, encoding='utf-8',
                          capture_output=True, cwd=work, env=environment, timeout=timeout, check=False)


def execute(code, input_output):
    graph = source_graph()
    before = {role: raw_sha(path.read_bytes()) for role, path in graph.items()}
    loaded_graph_check(before)
    code_sha = raw_sha(code.encode()) if type(code) is str else None
    io_bytes = canonical(input_output)
    original = json.loads(io_bytes)
    pair_count = len(original.get('inputs', [])) if type(original) is dict and type(original.get('inputs')) is list else None
    start = time.monotonic()
    results = []
    invalid = None
    try:
        pairs = contract.ordered_pairs(original)
        contract.require(type(code) is str, 'original_code_required')
    except contract.ScopeError as error:
        pairs, invalid = [], str(error)
    environment = {k: v for k, v in os.environ.items() if k.upper() in {'SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP'}
                   and not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    with tempfile.TemporaryDirectory(prefix='ncx-stdin-fixture-') as work:
        for index, (input_text, expected_text) in enumerate(pairs):
            remaining = contract.GROUP_TIMEOUT - (time.monotonic() - start)
            row = {'index': index, 'input_sha256': raw_sha(input_text.encode()),
                   'expected_sha256': raw_sha(expected_text.encode()), 'child_started': False}
            if remaining <= 0:
                results.append({**row, 'status': 'unknown', 'passed': False, 'reason': 'whole_group_deadline'})
                continue
            row['child_started'] = True
            try:
                child = _child({'index': index, 'code': code, 'input_text': input_text, 'expected_text': expected_text},
                               work, environment, min(contract.PAIR_TIMEOUT, remaining))
                result = checked_result(strict_json(child.stdout), index) if child.returncode == 0 and len(child.stdout.encode()) <= 32768 else {
                    'index': index, 'status': 'unknown', 'passed': False, 'reason': 'worker_failed'}
            except subprocess.TimeoutExpired:
                result = {'index': index, 'status': 'unknown', 'passed': False, 'reason': 'pair_timeout'}
            except (ValueError, UnicodeError):
                result = {'index': index, 'status': 'unknown', 'passed': False, 'reason': 'invalid_worker_output'}
            except OSError:
                result = {'index': index, 'status': 'unknown', 'passed': False, 'reason': 'worker_start_failed'}
                row['child_started'] = False
            results.append({**result, **row})
    after = {role: raw_sha(path.read_bytes()) for role, path in graph.items()}
    if before != after or io_bytes != canonical(input_output):
        raise ValueError('original_owner_runtime_or_io_changed')
    loaded_graph_check(after)
    status = ('unknown' if invalid or any(r['status'] == 'unknown' for r in results)
              else 'failed' if any(r['status'] == 'failed' for r in results) else 'passed')
    body = {'schema': 'buglecat.verification.stdin-development-execution.v1',
            'parameters': dict(contract.PARAMETERS), 'code_sha256': code_sha, 'input_output_sha256': raw_sha(io_bytes),
            'source_graph': {role: {'file': str(graph[role]), 'before_sha256': before[role], 'after_sha256': after[role]}
                             for role in graph},
            'runtime': {'executable_file': str(Path(sys.executable).resolve()), 'executable_sha256': before['runtime'],
                        'version': sys.version, 'version_info': list(sys.version_info[:3]), 'platform': sys.platform},
            'result': {'status': status, 'reason': invalid, 'pair_count': pair_count,
                       'pairs_executed': sum(r['child_started'] for r in results),
                       'pairs_executed_definition': 'child start attempts, including timeout', 'pair_results': results},
            'elapsed_ms': round((time.monotonic() - start) * 1000), 'credentials_removed': True,
            'original_script_unchanged': True, 'full_ordered_pairs_retained': len(results) == pair_count,
            'scope': 'restricted original script / fresh child each pair / ordered whitespace tokens only',
            'official_evaluator_parity': False, 'os_sandbox': False, 'memory_isolation_proven': False,
            'source_admitted': False, 'formal_registration': False, 'production_authorized': False,
            'paid_calls': 0, 'reservations': 0}
    return {**body, 'artifact_hash': raw_sha(canonical(body))}
