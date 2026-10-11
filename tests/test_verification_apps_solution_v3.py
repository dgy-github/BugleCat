"""Only synthetic artifacts; never executes an APPS reference or candidate."""
import ast
import json
import subprocess

import pytest

from evals.verification.apps_solution_v3 import oracle, policy, source
from evals.verification.apps_v3 import oracle as frozen_oracle


def run(code, inputs=None, outputs=None, **kwargs):
    io = {"fn_name": "f", "inputs": [[]] if inputs is None else inputs,
          "outputs": [1] if outputs is None else outputs}
    return oracle.execute(code, io, **kwargs)


def test_actual_original_code_one_instance_order_and_runtime_continuation():
    code = "class Solution:\n def __init__(self): self.state=[]\n def f(self,x):\n  self.state.append(x)\n  if x==2: raise ValueError()\n  return len(self.state)"
    receipt = run(code, [[1], [2], [3]], [1, 2, 3])
    assert receipt['code_sha256'] == oracle.raw_sha(code.encode())
    assert receipt['pair_count'] == receipt['result']['pairs_executed'] == 3
    assert receipt['result']['status'] == 'failed'
    assert [item['passed'] for item in receipt['result']['pair_results']] == [True, False, True]
    assert receipt['paid_calls'] == 0 and receipt['credentials_removed']


def test_direct_self_public_augmented_assignment_and_internal_bound_helper():
    code = "class Solution:\n def __init__(self): self.n=0\n def helper(self):\n  self.n+=1\n  return self.n\n def f(self): return self.helper()"
    result = run(code, [[], []], [1, 2])['result']
    assert result['status'] == 'passed'


def test_typing_import_annotation_prelude_and_official_comparison_reused():
    code = "from typing import Optional, List\nclass Solution:\n def f(self,x: Optional[List[int]]) -> List[int]: return tuple(x.values())"
    result = run(code, [[{'1': 7}, 99]], [[[7]]])['result']
    assert result['status'] == 'passed'
    assert oracle.checked_result is frozen_oracle.checked_result
    assert oracle.CONTRACT == 'apps-call-based-builtins-ordered-v3'


@pytest.mark.parametrize('body', [
    'self=math\n  self.pi=0',
    'type(self).f(math)',
    'Solution.f(math)',
    'cls=type(self)\n  cls.f(math)',
    'cls=Solution\n  cls.f(math)',
    'import math as self\n  self.pi=0',
    'from math import sqrt as self',
    'del self',
    'for self in [math]: pass',
    '[self for self in [math]]',
    '(self:=math)',
    'try: raise ValueError()\n  except ValueError as self: pass',
    'def g(self): self.pi=0\n  g(math)',
    'g=lambda self: self',
    'def self(): return math',
    'self.__class__.f(math)',
    'self.x=math\n  self.x.pi=0',
    'x=self\n  x.value=0',
    'self.x=math\n  self.x.__dict__["pi"]=0',
    'self._private=0',
    'del self.x',
    'setattr(self,"x",0)',
    'self.x=globals()',
    'type("Other",(),{})',
    'match 1:\n   case self: pass',
    'match [1]:\n   case [*self]: pass',
    'match {}:\n   case {**self}: pass',
])
def test_receiver_and_class_handle_bypasses_fail_before_execution(body):
    code = 'import math\nclass Solution:\n def f(self):\n  ' + body + '\n  return 1'
    with pytest.raises(ValueError):
        policy.reviewed_code(code, 'f')
    assert run(code)['result']['status'] == 'unknown'


@pytest.mark.parametrize('code', [
    'class Solution(object):\n def f(self): return 1',
    'class Solution(metaclass=type):\n def f(self): return 1',
    '@decorator\nclass Solution:\n def f(self): return 1',
    'class Solution:\n @staticmethod\n def f(self): return 1',
    'class Other: pass\nclass Solution:\n def f(self): return 1',
    'class Solution:\n class Nested: pass\n def f(self): return 1',
    'class Solution:\n state=0\n def f(self): return 1',
    'class Solution:\n def __getattribute__(self,x): return 1\n def f(self): return 1',
    'class Solution:\n def f(receiver): return 1',
    'class Solution:\n def g(self): return 1\n f=g',
    'class  Solution:\n def f(self): return 1',
    'class Solution:\n def f(self): return 1\nSolution=1',
    'class Solution:\n def f(self): return 1\nimport math as Solution',
    'def g(self): return 1\nclass Solution:\n def f(self): return 1',
    'import typing\nclass Solution:\n def f(self): return 1',
    'from typing import get_type_hints\nclass Solution:\n def f(self): return 1',
    'import os\nclass Solution:\n def f(self): return 1',
])
def test_explicit_limited_class_and_dependency_scope(code):
    assert run(code)['result']['status'] == 'unknown'


def test_projection_does_not_rewrite_actual_tree_or_original_execution():
    code = 'class Solution:\n def __init__(self): self.n=41\n def f(self):\n  self.n+=1\n  return self.n'
    tree = policy.reviewed_code(code, 'f')
    assert ast.dump(tree) == ast.dump(ast.parse(code))
    assert any(isinstance(node, ast.Attribute) and isinstance(node.ctx, ast.Store) for node in ast.walk(tree))
    assert run(code, outputs=[42])['result']['status'] == 'passed'


@pytest.mark.parametrize('exception', ['ValueError', 'TypeError', 'KeyError'])
def test_actual_init_errors_are_failure_not_policy_unknown(exception):
    result = run(f'class Solution:\n def __init__(self): raise {exception}()\n def f(self): return 1')['result']
    assert result['status'] == 'failed'
    assert result['error_kind'] == exception
    assert result['failure_phase'] == 'compile_or_lookup'


def test_unknown_dependencies_and_non_builtin_output():
    assert run('class Solution:\n def f(self): return format(1,"d")')['result']['reason'] == 'unsupported_implicit_builtin'
    assert run('class Solution:\n def f(self): return np.array([1])')['result']['reason'] == 'unsupported_implicit_dependency'
    assert run('class Solution:\n def f(self): return self')['result']['reason'] == 'unsupported_comparison_value'
    assert run('len=0\nclass Solution:\n def f(self): return 1')['result']['reason'] == 'oracle_builtin_shadowed'


def test_timeout_syntax_and_malformed_output_do_not_fabricate_pair_results():
    receipt = run('class Solution:\n def f(self):\n  while True: pass', timeout=1)
    assert receipt['result'] == {'status': 'unknown', 'reason': 'worker_timeout', 'pairs_executed': None}
    syntax = run('class Solution:\n def f(self:')['result']
    assert syntax['status'] == 'failed' and syntax['pair_results'] == []
    assert run('class Solution:\n def f(self): return 1', inputs=[], outputs=[])['result']['status'] == 'unknown'
    assert oracle.checked_result({'status': 'passed'}, 1)['status'] == 'unknown'


def test_credentials_graph_runtime_and_parent_environment(monkeypatch, tmp_path):
    graph = {'solution_worker': tmp_path / 'worker', 'runtime': tmp_path / 'runtime'}
    for path in graph.values():
        path.write_bytes(b'original')
    monkeypatch.setattr(oracle, 'source_graph', lambda: graph)
    monkeypatch.setenv('DEEPSEEK_API_KEY', 'fixture-secret')
    monkeypatch.setenv('OPENAI_API_KEY', 'fixture-secret')
    monkeypatch.setenv('SOME_TOKEN', 'fixture-secret')
    monkeypatch.setenv('NCX_JEV_API_KEY', 'fixture-secret')

    def child(argv, **kwargs):
        assert argv[1:4] == ['-I', '-S', '-B']
        assert not any('KEY' in key or 'TOKEN' in key for key in kwargs['env'])
        assert kwargs['cwd'] != str(tmp_path)
        graph['runtime'].write_bytes(b'changed')
        return subprocess.CompletedProcess(argv, 0, json.dumps({'status': 'passed'}), '')
    monkeypatch.setattr(oracle.subprocess, 'run', child)
    with pytest.raises(ValueError, match='changed during execution'):
        run('class Solution:\n def f(self): return 1')
    import os
    assert os.environ['DEEPSEEK_API_KEY'] == 'fixture-secret'


def test_graph_covers_shared_execution_comparison_and_ast_typing_sources():
    graph = oracle.source_graph()
    assert set(graph) == {'solution_worker', 'solution_policy', 'solution_wrapper', 'pair_worker',
                          'comparison', 'pair_wrapper', 'typing_policy', 'ast_policy', 'runtime'}
    assert all(path.exists() for path in graph.values())


def test_importing_solution_owner_does_not_patch_frozen_free_worker():
    from evals.verification.apps_v3 import worker as frozen_worker
    original_review = frozen_worker.reviewed_code
    original_environment = frozen_worker.environment
    from evals.verification.apps_solution_v3 import worker as solution_worker
    assert solution_worker.free is not frozen_worker
    assert frozen_worker.reviewed_code is original_review
    assert frozen_worker.environment is original_environment
    with pytest.raises(ValueError, match='unsupported_construct'):
        frozen_worker.reviewed_code('class Solution:\n def f(self): return 1')


def test_init_declaration_does_not_allow_magic_access_or_self_alias_rebinding():
    for code in (
        'class Solution:\n def __init__(self): self=math\n def f(self): return 1',
        'class Solution:\n def __init__(self): self._private=1\n def f(self): return 1',
        'class Solution:\n def __init__(self): self.n=1\n def f(self): return self.__init__()',
        'class Solution:\n def method(self): self.pi=0\n def f(self): return type(self).method(math)',
        'class Solution:\n def method(self): self.pi=0\n def f(self): return Solution.method(math)',
    ):
        assert run(code)['result']['status'] == 'unknown'


def source_row(refs=None, io=None):
    return json.dumps({'id': 7, 'question': 'Synthetic original problem',
                       'url': 'https://example.invalid/problem', 'starter_code': '',
                       'input_output': json.dumps(io or {'fn_name': 'f', 'inputs': [[]], 'outputs': [1]}),
                       'solutions': json.dumps(refs or ['class Solution:\n def f(self): return 1'])}).encode()


def test_source_preserves_first0_raw_bytes_group_namespace_without_admission():
    code = 'class Solution:\n def f(self): return 1'
    raw = source_row([code, code.replace('1', '2')])
    task = source.parse_row(raw, 2)
    assert task['reference'] == code and task['reference_index'] == 0
    assert task['original_line_sha256'] == oracle.raw_sha(raw)
    assert task['reference_sha256'] == oracle.raw_sha(code.encode())
    assert task['group_id'] == f'apps-{source.REVISION}-train-7'
    assert task['source_admitted'] is False and task['independent_group_proven'] is False
    assert source.verified_rows.__module__ == 'evals.verification.apps_v3.source'


def test_source_cannot_fallback_from_invalid_first_reference():
    with pytest.raises(ValueError, match='class_form'):
        source.parse_row(source_row(['def f(): return 1', 'class Solution:\n def f(self): return 1']), 1)
    with pytest.raises(ValueError, match='entry_point'):
        source.parse_row(source_row(io={'fn_name': 'g', 'inputs': [[]], 'outputs': [1]}), 1)


def test_source_pair_count_validation_and_nonfinite_rejection():
    with pytest.raises(ValueError, match='invalid_or_empty_pairs'):
        source.parse_row(source_row(io={'fn_name': 'f', 'inputs': [[]], 'outputs': []}), 1)
    with pytest.raises(ValueError, match='nonfinite'):
        source.parse_row(source_row().replace(b'[1]', b'[NaN]'), 1)
