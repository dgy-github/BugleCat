"""Development-only STDIN contract; no source admission or paid authority.

Original script is compiled unchanged, with __name__='__main__'. Each ordered
pair gets a fresh process/namespace. Comparison is exact ordered split() tokens,
not official APPS evaluator parity. The separate AST reviewer intentionally
restricts aliases, decorators and formatting reflection beyond the old policy.
This is a language gate, not OS isolation or a memory sandbox.
input/print are direct calls only, never aliases or arguments. __name__ and the
__main__ literal occur only in the exact standard equality guard. No type(),
generic type parameters, decorators, class declarations or format reflection.
"""
import ast
import hashlib
import types
from pathlib import Path

POLICY = 'restricted-stdin-script-fresh-pair-v1'
COMPARISON = 'ordered-whitespace-tokens-v1'
PAIR_TIMEOUT = 5
GROUP_TIMEOUT = 30
MAX_PAIRS = 100
MAX_OUTPUT_BYTES = 65536
MAX_CODE_BYTES = 65536
ORIGINAL_POLICY = Path(__file__).resolve().parents[1] / 'benchmark_oracle_worker.py'
LOADED_SOURCE_SHA256 = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
original_bytes = ORIGINAL_POLICY.read_bytes()
LOADED_ORIGINAL_SHA256 = hashlib.sha256(original_bytes).hexdigest()
original = types.ModuleType('stdin_frozen_constants')
exec(compile(original_bytes, str(ORIGINAL_POLICY), 'exec'), original.__dict__)
MODULES = frozenset(original.MODULES)
BUILTINS = tuple(n for n in original.BUILTINS if n not in {'type', 'issubclass'}) + (
    'EOFError', 'SystemExit', 'MemoryError', 'RecursionError')
PROTECTED = frozenset(BUILTINS) | {'input', 'print', '__name__'}
FORBIDDEN = (frozenset(original.FORBIDDEN) - {'input', 'print'}) | {'type', 'issubclass', 'sys'}
BAD_ATTRIBUTES = {'attrgetter', 'methodcaller', 'get_field', 'get_value', 'format', 'format_map', 'Formatter', 'mro'}
PARAMETERS = {'policy': POLICY, 'comparison': COMPARISON, 'per_pair_timeout_seconds': PAIR_TIMEOUT,
              'whole_group_timeout_seconds': GROUP_TIMEOUT, 'max_original_pairs': MAX_PAIRS,
              'max_output_utf8_bytes': MAX_OUTPUT_BYTES, 'max_code_utf8_bytes': MAX_CODE_BYTES,
              'original_script_unchanged': True, 'fresh_child_each_pair': True, 'official_evaluator_parity': False}
LOADED_PARAMETERS = dict(PARAMETERS)
LOADED_POLICY_VALUES = (MODULES, BUILTINS, PROTECTED, FORBIDDEN, frozenset(BAD_ATTRIBUTES))


class ScopeError(ValueError):
    """Unsupported input or language scope, never a negative benchmark label."""


def require(condition, reason):
    if not condition:
        raise ScopeError(reason)


def text(value):
    if type(value) is str:
        return value
    if type(value) is list and all(type(item) is str for item in value):
        return '\n'.join(value)
    raise ScopeError('input_output_must_be_text_or_list_of_text')


def ordered_pairs(io):
    require(type(io) is dict and io.get('fn_name') is None, 'stdin_original_io_required')
    inputs, outputs = io.get('inputs'), io.get('outputs')
    require(type(inputs) is list and type(outputs) is list and inputs and len(inputs) == len(outputs), 'full_nonempty_ordered_pairs_required')
    require(len(inputs) <= MAX_PAIRS, 'original_pair_limit')
    return [(text(a), text(b)) for a, b in zip(inputs, outputs, strict=True)]


def compare(actual, expected):
    require(type(actual) is str and type(expected) is str, 'text_comparison_required')
    return actual.split() == expected.split()


def reviewed_tree(code):
    require(type(code) is str and len(code.encode('utf-8')) <= MAX_CODE_BYTES, 'code_size_or_type')
    tree = ast.parse(code)
    guards, literals, direct_io = set(), set(), set()
    for node in ast.walk(tree):
        if isinstance(node, ast.If):
            test = node.test
            if (isinstance(test, ast.Compare) and isinstance(test.left, ast.Name) and test.left.id == '__name__'
                    and len(test.ops) == len(test.comparators) == 1 and isinstance(test.ops[0], ast.Eq)
                    and isinstance(test.comparators[0], ast.Constant) and test.comparators[0].value == '__main__'):
                guards.add(test.left)
                literals.add(test.comparators[0])
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in {'input', 'print'}:
            direct_io.add(node.func)
    for node in ast.walk(tree):
        require(not isinstance(node, (ast.ClassDef, ast.AsyncFunctionDef, ast.AsyncFor, ast.AsyncWith, ast.Await,
                                     ast.Global, ast.Nonlocal)), 'unsupported_construct')
        require(not getattr(node, 'type_params', []) and type(node).__name__ not in
                {'TypeAlias', 'TypeVar', 'ParamSpec', 'TypeVarTuple'}, 'unsupported_generic_binding')
        if isinstance(node, ast.Name):
            require(node.id not in FORBIDDEN, 'restricted_name')
            require(not node.id.startswith('__') or node in guards, 'restricted_dunder_name')
            require(not isinstance(node.ctx, (ast.Store, ast.Del)) or node.id not in PROTECTED, 'protected_binding')
            require(node.id not in {'input', 'print'} or node in direct_io, 'io_interface_alias_or_escape')
        elif isinstance(node, ast.Attribute):
            require(not node.attr.startswith('_') and node.attr not in BAD_ATTRIBUTES, 'restricted_attribute')
            require(not isinstance(node.ctx, (ast.Store, ast.Del)), 'dependency_attribute_mutation')
        elif isinstance(node, ast.Constant) and type(node.value) is str:
            require('__' not in node.value or node in literals, 'restricted_reflection_string')
        elif isinstance(node, ast.arg):
            require(node.arg not in PROTECTED | FORBIDDEN and not node.arg.startswith('__'), 'protected_argument')
        elif isinstance(node, (ast.FunctionDef, ast.Lambda)):
            require(not isinstance(node, ast.FunctionDef) or (node.name not in PROTECTED | FORBIDDEN and not node.name.startswith('__')), 'protected_function')
            require(not getattr(node, 'decorator_list', []), 'unsupported_decorator')
        elif isinstance(node, ast.ExceptHandler) and node.name:
            require(node.name not in PROTECTED | FORBIDDEN and not node.name.startswith('__'), 'protected_exception_binding')
        elif isinstance(node, (ast.MatchAs, ast.MatchStar)) and node.name:
            require(node.name not in PROTECTED | FORBIDDEN and not node.name.startswith('__'), 'protected_pattern_binding')
        elif isinstance(node, ast.MatchMapping) and node.rest:
            require(node.rest not in PROTECTED | FORBIDDEN and not node.rest.startswith('__'), 'protected_pattern_binding')
        elif isinstance(node, ast.Import):
            require(all(a.name in MODULES for a in node.names), 'restricted_import')
        elif isinstance(node, ast.ImportFrom):
            require(not node.level and node.module in MODULES and all(not a.name.startswith('_') and a.name not in BAD_ATTRIBUTES | {'*'} for a in node.names), 'restricted_import')
        elif isinstance(node, ast.alias):
            name = node.asname or node.name.split('.')[0]
            require(name not in PROTECTED | FORBIDDEN and not name.startswith('__'), 'protected_import_binding')
    return tree
