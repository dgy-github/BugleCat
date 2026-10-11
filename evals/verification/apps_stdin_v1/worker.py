"""One original fixture script / one pair; credential-free parent owns deadlines."""
import builtins
import hashlib
import importlib
import io
import json
import os
import types
from pathlib import Path

contract_path = Path(__file__).with_name('contract.py')
contract = types.ModuleType('stdin_contract')
contract.__file__ = str(contract_path)
exec(compile(contract_path.read_bytes(), str(contract_path), 'exec'), contract.__dict__)


class OutputLimit(BaseException):
    pass


class Capture:
    def __init__(self, input_text):
        self.input = io.StringIO(input_text)
        self.parts = []
        self.bytes = 0
        self.limit_hit = False
        self.scope_hit = False

    def write(self, value):
        count = len(value.encode('utf-8'))
        if self.bytes + count > contract.MAX_OUTPUT_BYTES:
            self.limit_hit = True
            raise OutputLimit()
        self.bytes += count
        self.parts.append(value)

    def read(self, prompt='', /):
        self.write(str(prompt))
        line = self.input.readline()
        if line == '':
            raise EOFError()
        if line.endswith('\n'):
            line = line[:-1]
            if line.endswith('\r'):
                line = line[:-1]
        return line

    def print(self, *objects, sep=' ', end='\n', file=None, flush=False):
        if file is not None:
            self.scope_hit = True
            raise contract.ScopeError('print_file_not_supported')
        if sep is None:
            sep = ' '
        if end is None:
            end = '\n'
        if type(sep) is not str or type(end) is not str:
            raise TypeError()
        self.write(sep.join(str(value) for value in objects) + end)


def importer(name, globals=None, locals=None, fromlist=(), level=0):
    contract.require(name in contract.MODULES and not level, 'restricted_import')
    return importlib.import_module(name)


def run(payload):
    index = payload.get('index')
    common = {'index': index, 'worker_pid': os.getpid(), 'comparison': contract.COMPARISON,
              'output_truncated': False, 'output_bytes': 0, 'output_sha256': None}
    try:
        contract.require(type(index) is int and index >= 0 and type(payload.get('input_text')) is str
                         and type(payload.get('expected_text')) is str, 'invalid_pair_payload')
        contract.reviewed_tree(payload.get('code'))
    except SyntaxError:
        return {**common, 'status': 'failed', 'passed': False, 'error_kind': 'SyntaxError'}
    except (contract.ScopeError, ValueError, TypeError, RecursionError, MemoryError):
        return {**common, 'status': 'unknown', 'passed': False, 'reason': 'unsupported_language_or_payload'}
    capture = Capture(payload['input_text'])
    safe = {name: getattr(builtins, name) for name in contract.BUILTINS}
    safe.update(input=capture.read, print=capture.print, __import__=importer)
    scope = {'__builtins__': safe, '__name__': '__main__'}
    status, reason, error_kind = 'passed', None, None
    try:
        # Compile original source, not a projected or rewritten AST.
        exec(compile(payload['code'], 'original-script.py', 'exec'), scope)
    except (OutputLimit, MemoryError, RecursionError, SystemExit, NameError, ImportError, UnicodeError, contract.ScopeError):
        status, reason = 'unknown', 'unsupported_dependency_exit_or_resource'
    except Exception as error:
        status, error_kind = 'failed', type(error).__name__
    except BaseException:
        status, reason = 'unknown', 'unclassified_base_exception'
    if capture.limit_hit or capture.scope_hit:
        status, reason = 'unknown', 'output_limit' if capture.limit_hit else 'unsupported_output_interface'
    output = ''.join(capture.parts)
    if status == 'passed' and not contract.compare(output, payload['expected_text']):
        status, error_kind = 'failed', 'OutputMismatch'
    return {**common, 'status': status, 'passed': status == 'passed',
            'reason': reason, 'error_kind': error_kind, 'output_truncated': capture.limit_hit,
            'output_bytes': capture.bytes, 'output_sha256': hashlib.sha256(output.encode()).hexdigest()}


if __name__ == '__main__':
    import sys
    try:
        result = run(json.load(sys.stdin))
    except Exception:
        result = {'status': 'unknown', 'passed': False, 'reason': 'worker_protocol_failure', 'index': None}
    sys.stdout.write(json.dumps(result, sort_keys=True, allow_nan=False))
