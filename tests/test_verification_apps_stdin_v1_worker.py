import pytest

from evals.verification.apps_stdin_v1 import worker as w


def run(code, input_text='', expected=''):
    return w.run({'index': 0, 'code': code, 'input_text': input_text, 'expected_text': expected})


@pytest.mark.parametrize('text,expected', [('x\r', "'x\\r'"), ('x\r\n', "'x'"), ('x\n', "'x'"), ('x', "'x'")])
def test_input_lf_adjacent_cr_only(text, expected):
    assert run('print(repr(input()))', text, expected)['status'] == 'passed'


def test_eof_is_supported_normal_exception():
    assert run('input()')['error_kind'] == 'EOFError'
    assert run('try:\n    input()\nexcept EOFError:\n    print("done")', expected='done')['status'] == 'passed'
    assert run('input(prompt="x")', 'x')['error_kind'] == 'TypeError'


@pytest.mark.parametrize('code', ['raise ValueError()', '1/0', 'x=[0]\nprint(x[2])', 'def broken('])
def test_supported_failures(code):
    assert run(code)['status'] == 'failed'


@pytest.mark.parametrize('code', ['raise SystemExit()', 'raise MemoryError()', 'raise RecursionError()',
                                 'print(missing)', 'import sys', 'class X:\n    pass', 'alias=input'])
def test_unsupported_is_unknown(code):
    assert run(code)['status'] == 'unknown'


def test_print_semantics_prompt_and_sticky_limit():
    assert run('print(input("prompt:"), 2, sep="|", end="!")', 'x', 'prompt:x|2!')['status'] == 'passed'
    output = run('try:\n    print("汉"*22000)\nexcept:\n    pass')
    assert output['status'] == 'unknown' and output['output_truncated'] is True
    assert output['output_bytes'] <= 65536
    assert run('print("x"*65536,end="")', expected='x'*65536)['status'] == 'passed'


def test_catching_unsupported_output_does_not_promote_pass():
    output = run('try:\n    print("x",file=1)\nexcept:\n    pass')
    assert output['status'] == 'unknown' and output['reason'] == 'unsupported_output_interface'


def test_original_main_guard_and_no_input_rebinding():
    assert run("if __name__ == '__main__':\n    print(42)", expected='42')['status'] == 'passed'
    assert run('__name__="other"')['status'] == 'unknown'
