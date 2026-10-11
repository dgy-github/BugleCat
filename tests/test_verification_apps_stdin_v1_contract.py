import ast
import sys

import pytest

from evals.verification.apps_stdin_v1 import contract as c


@pytest.mark.parametrize('code', [
    'print(input())', "if __name__ == '__main__':\n    print(input())",
    'import math\nprint(math.sqrt(4))',
    'def f(x):\n    return x+1\nprint(f(2))',
    'try:\n    print(input())\nexcept EOFError:\n    print(0)',
])
def test_supported_tree(code):
    assert isinstance(c.reviewed_tree(code), ast.Module)


@pytest.mark.parametrize('code', [
    '__name__="x"', 'print(__name__)', "print('__main__')", 'input=0', 'print=0',
    'alias=input', 'alias=print', 'map(print,[])', 'def f(print):\n    pass',
    'def input():\n    pass', 'try:\n    pass\nexcept Exception as print:\n    pass',
    'match 0:\n    case print:\n        pass', '[input for input in []]', '(print:=0)',
    'import math as print', 'from math import sqrt as input', 'del print',
    '@print\ndef f():\n    pass', 'def f(x=input):\n    pass', 'def f[print]():\n    pass',
    'type X = int', 'class X:\n    pass', 'type("X",(),{})', 'import sys', 'import os',
    'open("x")', 'print.__call__(1)', 'import math\nmath.x=0', 'import math\ndel math.pi',
    'print("{x}".format(x=1))', 'from string import Formatter', 'globals()',
    'import operator\noperator.attrgetter("x")', 'print(input)',
    'def f(__name__):\n    pass', 'global x', 'nonlocal x',
])
def test_scope_rejects_binding_and_escape(code):
    if ('def f[' in code or code.startswith('type X')) and sys.version_info < (3, 12):
        with pytest.raises(SyntaxError):
            c.reviewed_tree(code)
    else:
        with pytest.raises(c.ScopeError):
            c.reviewed_tree(code)


def test_io_original_order_no_appended_lf():
    assert c.ordered_pairs({'inputs': [['x', 'y'], 'a'], 'outputs': ['1', ['2', '3']]}) == [
        ('x\ny', '1'), ('a', '2\n3')]


@pytest.mark.parametrize('io', [
    {'inputs': [], 'outputs': []}, {'inputs': ['x'], 'outputs': []},
    {'inputs': [1], 'outputs': ['1']}, {'inputs': ['x'], 'outputs': [False]},
    {'inputs': ['x'], 'outputs': ['1'], 'fn_name': 'f'},
    {'inputs': ['x'] * 101, 'outputs': ['1'] * 101},
])
def test_no_io_projection(io):
    with pytest.raises(c.ScopeError):
        c.ordered_pairs(io)


@pytest.mark.parametrize('actual,expected,equal', [
    ('a  b\n', 'a\tb', True), ('1.0', '1', False), ('a b', 'b a', False),
    ('A', 'a', False), ('0.30000000000000004', '0.3', False), ('', '  ', True),
])
def test_ordered_tokens_only(actual, expected, equal):
    assert c.compare(actual, expected) is equal
