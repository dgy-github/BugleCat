"""External offline acceptance checks; never supplied to the repairing agent."""
import copy
import importlib.util
import sys
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('candidate', Path(sys.argv[1]) / 'bench/run.py')
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)

def evidence(passed=False, empty=False, missing=False, compile_bad=False, seconds=2):
    e = dict(passed=passed, elapsed_s=seconds, timed_out=False, returncode=0 if passed else 1,
             grader_note='' if passed else 'failure', stdout_tail=['empty response'] if empty else [],
             stderr_tail=[], missing_expected_files=['a.py'] if missing else [],
             python_compile=[dict(file='a.py', ok=False)] if compile_bad else [])
    e['failure_kind'] = b._failure_kind(e)
    return e

checks = []
def check(name, fn):
    try:
        fn()
        checks.append((name, 'PASS'))
    except Exception as exc:
        checks.append((name, 'FAIL: ' + repr(exc)))

def equal(actual, expected):
    assert actual == expected, (actual, expected)

check('empty response + missing file', lambda: equal(evidence(empty=True, missing=True)['failure_kind'], 'empty_response'))
check('compile failure precedence', lambda: equal(evidence(empty=True, missing=True, compile_bad=True)['failure_kind'], 'compile_failed'))
check('success mentioning empty response', lambda: equal(evidence(passed=True, empty=True)['failure_kind'], None))

def replay(limit, sequence, calls, passes, seconds):
    with patch.object(b, 'tasks', return_value=[Path('test_task')]), patch.object(
        b, '_run_once_detailed', side_effect=copy.deepcopy(sequence)
    ) as run:
        result = b.run_agent('nanocodex', 240, 1, empty_response_retries=limit)['test_task']
    equal(run.call_count, calls)
    equal(result['runs'], 1)
    equal(result['passes'], passes)
    equal(result['mean_s'], seconds)
    equal(len(result['attempts']), calls)
    equal(result['timeouts'], sum(e['timed_out'] for e in sequence[:calls]))

check('recovery keeps evidence and time', lambda: replay(2, [evidence(empty=True, missing=True), evidence(passed=True, seconds=3)], 2, 1, 5))
check('retry exhaustion bounded', lambda: replay(2, [evidence(empty=True)] * 3, 3, 0, 6))
check('zero disables retries', lambda: replay(0, [evidence(empty=True)], 1, 0, 2))
check('ordinary failure not retried', lambda: replay(2, [evidence(missing=True)], 1, 0, 2))
check('success never retried', lambda: replay(2, [evidence(passed=True, empty=True)], 1, 1, 2))

def wrapper():
    with patch.object(b, '_run_once_detailed', return_value=evidence(passed=True)):
        equal(b.run_once('nanocodex', Path('task'), 240), (True, 2, ''))
check('legacy tuple API', wrapper)
for name, status in checks:
    print(f'{status}: {name}')
sys.exit(any(status != 'PASS' for _, status in checks))
