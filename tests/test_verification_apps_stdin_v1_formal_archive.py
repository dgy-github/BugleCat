"""New local raw kernel parity/closure; only self-made script fixtures execute."""
import sys

import pytest

from evals.verification.apps_stdin_v1 import budget, formal_archive, intake, packet
from tests.stdin_collection_fixtures import write


@pytest.mark.parametrize('code', ['print(input())\n', 'print("wrong")\n', 'print(missing_dependency)\n'])
def test_original_full_oracle_read_kernel_matches_frozen_reader(tmp_path, code):
    task = {'question': 'Synthetic echo', 'starter_code': '', 'input_output': {'inputs': ['7\n'], 'outputs': ['7']}}
    answer = 'Ordinary assessment.\r\n```python\r\n' + code + '```\r\nUnknown certainty.'
    expected = intake._oracle(task, answer, tmp_path, True)
    context = formal_archive.packet_context(None)
    actual = formal_archive._oracle(task, answer, tmp_path, context)
    assert actual == expected
    formal_archive.close_context(context)
    original = budget.load(tmp_path / 'stdin-candidate-oracle.json')
    original['result']['pair_results'][0]['expected_sha256'] = 'a' * 64
    original.pop('artifact_hash')
    write(tmp_path / 'stdin-candidate-oracle.json', formal_archive.cohort.seal(original))
    with pytest.raises(ValueError, match='pair identity'):
        formal_archive._oracle(task, answer, tmp_path, context)


def test_original_stranded_oracle_never_executes_again(tmp_path):
    task = {'question': 'Synthetic echo', 'starter_code': '', 'input_output': {'inputs': ['7'], 'outputs': ['7']}}
    write(tmp_path / 'stdin-candidate-oracle.started', {'synthetic_crash': True})
    result = formal_archive._oracle(task, '```python\nprint(input())\n```', tmp_path, formal_archive.packet_context(None))
    assert result['reason'] == 'oracle_attempt_incomplete' and result['call_eligible'] is False
    assert not (tmp_path / 'stdin-candidate-oracle.json').exists()


def test_graph_proof_once_at_each_transaction_boundary_not_per_candidate():
    calls = []

    def trace(frame, event, _):
        if event == 'call' and frame.f_code is packet.dependencies.__code__:
            calls.append('original_graph')

    previous = sys.getprofile()
    try:
        sys.setprofile(trace)
        context = formal_archive.packet_context(None)
        for _ in range(40):
            formal_archive._packet('Synthetic task', '', 'print(input())\n', {'inputs': ['7'], 'outputs': ['7']}, context)
            formal_archive._prompt('Synthetic task', '', context)
        formal_archive.close_context(context)
    finally:
        sys.setprofile(previous)
    assert calls == ['original_graph', 'original_graph']


def test_context_policy_body_tamper_closes(tmp_path):
    context = formal_archive.packet_context(None)
    context['policy']['parameters']['max_original_pairs'] = 101
    with pytest.raises(ValueError, match='transaction-local'):
        formal_archive.close_context(context)
