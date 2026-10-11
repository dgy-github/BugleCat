"""Isolated source-proof fixtures, no real authority creation or execution."""
import json
import sys
from copy import deepcopy
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import source
from evals.verification.apps_stdin_v1 import source_authority as a


@pytest.fixture(scope='module')
def synthetic():
    raw = json.dumps({'id': 12, 'question': 'Echo the public line.', 'starter_code': '', 'url': None,
        'input_output': json.dumps({'inputs': ['x', ['a', 'b']], 'outputs': ['x', 'a']}),
        'solutions': json.dumps(['print(input())']), 'difficulty': 'interview'}).encode()
    task = source.parse_row(raw, 13)
    graph = {role: a.binding(path) for role, path in a.oracle.source_graph().items()}
    runtime = {'executable_file': str(Path(sys.executable).resolve()), 'executable_sha256': graph['runtime']['sha256'],
               'version': sys.version, 'version_info': list(sys.version_info[:3]), 'platform': sys.platform}
    pairs = a.contract.ordered_pairs(task['input_output'])
    results = [{'index': i, 'child_started': True, 'status': 'passed', 'passed': True,
        'input_sha256': a.oracle.raw_sha(text.encode()), 'expected_sha256': a.oracle.raw_sha(expected.encode()),
        'worker_pid': 123, 'comparison': a.contract.COMPARISON, 'output_truncated': False,
        'output_bytes': len(expected.encode()), 'output_sha256': a.oracle.raw_sha(expected.encode()),
        'reason': None, 'error_kind': None} for i, (text, expected) in enumerate(pairs)]
    receipt = {'schema': 'buglecat.verification.stdin-development-execution.v1',
        'code_sha256': task['reference_sha256'], 'input_output_sha256': task['input_output_sha256'],
        'parameters': dict(a.contract.PARAMETERS), 'runtime': runtime,
        'source_graph': {role: {'file': b['file'], 'before_sha256': b['sha256'], 'after_sha256': b['sha256']}
                         for role, b in graph.items()}, 'credentials_removed': True, 'original_script_unchanged': True,
        'full_ordered_pairs_retained': True, 'official_evaluator_parity': False, 'source_admitted': False,
        'formal_registration': False, 'production_authorized': False, 'memory_isolation_proven': False,
        'os_sandbox': False, 'paid_calls': 0, 'reservations': 0,
        'result': {'pair_count': 2, 'pair_results': results, 'pairs_executed': 2, 'status': 'passed'}}
    return task, receipt, graph, runtime


def checked(receipt, fixture):
    task, _, graph, runtime = fixture
    return a._receipt_join(a.seal(receipt), task, a.contract.PARAMETERS, graph, runtime)


def test_original_all_pairs_graph_runtime_join(synthetic):
    status, graph = checked(deepcopy(synthetic[1]), synthetic)
    assert status == 'passed' and len(graph) == 5


@pytest.mark.parametrize('change', ['code', 'io', 'parameter', 'runtime', 'graph', 'count', 'delete',
                                  'order', 'input', 'expected', 'worker_shape', 'status', 'started',
                                  'paid_bool', 'paid', 'authority', 'credentials'])
def test_changed_original_receipt_rejected_even_resealed(synthetic, change):
    receipt = deepcopy(synthetic[1])
    result, pair = receipt['result'], receipt['result']['pair_results'][0]
    if change in ('code', 'io'):
        receipt['code_sha256' if change == 'code' else 'input_output_sha256'] = '0' * 64
    elif change == 'parameter':
        receipt['parameters']['per_pair_timeout_seconds'] = 10
    elif change == 'runtime':
        receipt['runtime']['version_info'] = [3, 14, 0]
    elif change == 'graph':
        receipt['source_graph']['worker']['after_sha256'] = '0' * 64
    elif change == 'count':
        result['pair_count'] = True
    elif change == 'delete':
        result['pair_results'].pop()
    elif change == 'order':
        result['pair_results'].reverse()
    elif change in ('input', 'expected'):
        pair[change + '_sha256'] = '0' * 64
    elif change == 'worker_shape':
        pair['output_sha256'] = 'garbage'
    elif change == 'status':
        result['status'] = 'failed'
    elif change == 'started':
        pair['child_started'] = False
    elif change == 'paid_bool':
        receipt['paid_calls'] = False
    elif change == 'paid':
        receipt['paid_calls'] = 1
    elif change == 'authority':
        receipt['source_admitted'] = True
    elif change == 'credentials':
        receipt['credentials_removed'] = False
    with pytest.raises(ValueError):
        checked(receipt, synthetic)


def test_unknown_dominates_failed_and_preserves_every_pair(synthetic):
    receipt = deepcopy(synthetic[1])
    receipt['result']['pair_results'][0].update(status='failed', passed=False, error_kind='OutputMismatch')
    receipt['result']['pair_results'][1].update(status='unknown', passed=False, reason='unsupported_dependency')
    receipt['result']['status'] = 'unknown'
    assert checked(receipt, synthetic)[0] == 'unknown'


def test_deadline_unstarted_unknown_count_is_not_a_negative(synthetic):
    receipt = deepcopy(synthetic[1])
    pair = receipt['result']['pair_results'][1]
    receipt['result']['pair_results'][1] = {k: pair[k] for k in ('index', 'input_sha256', 'expected_sha256')}
    receipt['result']['pair_results'][1].update(child_started=False, status='unknown', passed=False,
                                             reason='whole_group_deadline')
    receipt['result'].update(status='unknown', pairs_executed=1)
    assert checked(receipt, synthetic)[0] == 'unknown'


@pytest.fixture(scope='module')
def complete_groups():
    rows = {f'apps-train-{i}': (i + 1, b'', {}) for i in range(5000)}
    groups = []
    for members in ([f'apps-train-{i}'] for i in range(5000)):
        groups.append({'group_id': 'apps-source-v3-' + a.digest(sorted(members))[:24], 'members': members,
                       'representative': members[0], 'excluded_by_history': False,
                       'historical_source_matches': [], 'source_admitted': False, 'evaluation_split': None})
    return rows, groups


def test_complete_group_partition_and_whole_history_exclusion(complete_groups):
    rows, groups = complete_groups
    changed = deepcopy(groups)
    changed[0]['members'].append(changed[1]['members'][0])
    changed[0]['group_id'] = 'apps-source-v3-' + a.digest(sorted(changed[0]['members']))[:24]
    changed[0].update(excluded_by_history=True, historical_source_matches=['original-mbpp-whole-source'])
    changed.pop(1)
    assert len(a._groups({'groups': changed}, rows)) == 4999


@pytest.mark.parametrize('change', ['missing', 'overlap', 'earliest', 'id', 'history', 'authority', 'split'])
def test_group_leakage_or_source_replacement_rejected(complete_groups, change):
    rows, original = complete_groups
    groups = deepcopy(original)
    if change == 'missing':
        groups.pop()
    elif change == 'overlap':
        groups.append(groups[0])
    elif change == 'earliest':
        groups[0]['representative'] = 'apps-train-1'
    elif change == 'id':
        groups[0]['group_id'] = 'replacement'
    elif change == 'history':
        groups[0]['historical_source_matches'] = ['original-human-source']
    elif change == 'authority':
        groups[0]['source_admitted'] = True
    elif change == 'split':
        groups[0]['evaluation_split'] = 'holdout'
    with pytest.raises(ValueError):
        a._groups({'groups': groups}, rows)


def test_fresh_template_resource_body_matches_actual_pinned_builder(synthetic):
    task = synthetic[0]
    case = {'group_id': 'synthetic-test-only', 'difficulty': 'interview', 'original_line': 13}
    record = {'receipt_sha256': 'f' * 64}
    template = a.packet.request('template', '', 'pass #' + 'x' * (15998 - 6), {'inputs': [''], 'outputs': ['']})
    assert a._resource_record(case, record, task, template) == a._resource_record(case, record, task)
    long_task = deepcopy(task)
    long_task['question'] = 'x' * 48000
    assert a._resource_record(case, record, long_task, template) == a._resource_record(case, record, long_task)
    assert a._resource_record(case, record, long_task, template)['status'] == 'resource_rejected'


def test_bindings_reject_mutation_duplicate_and_missing(tmp_path):
    file = tmp_path / 'fixture.txt'
    file.write_text('before')
    bound = a.binding(file)
    a.check([bound])
    with pytest.raises(ValueError, match='duplicate'):
        a.check([bound, bound])
    file.write_text('after')
    with pytest.raises(ValueError, match='drifted'):
        a.check([bound])


def test_synthetic_proof_cannot_pass_production_pins(tmp_path):
    for name in a.PINS:
        (tmp_path / name).write_text(json.dumps(a.seal({'fixture': True})))
    with pytest.raises(ValueError, match='pinned pool'):
        a.rebuild(tmp_path)


def test_validate_requires_real_rebuild_not_caller_seal(tmp_path, monkeypatch):
    path = tmp_path / 'fixture-authority.json'
    fixture = {'schema': a.SCHEMA, 'pool_directory': str(tmp_path), 'source_admitted': True,
               'threshold': None, **dict.fromkeys(a.FALSE_FLAGS, False), **dict.fromkeys(a.ZERO_COUNTS, 0)}
    path.write_text(json.dumps(a.seal(fixture)))
    calls = []
    def rebuild(directory):
        calls.append(directory)
        return a.seal({**fixture, 'collection_authorized': True})
    monkeypatch.setattr(a, 'rebuild', rebuild)
    with pytest.raises(ValueError, match='differs from original'):
        a.validate(path)
    assert calls == [str(tmp_path)]


@pytest.mark.parametrize('field,value', [('collection_authorized', True), ('formal_registration', True),
    ('holdout_opened', True), ('threshold', 0.5), ('model_calls', False), ('jev_calls', 1)])
def test_source_only_typed_capability_flags(field, value, tmp_path, monkeypatch):
    fixture = {'schema': a.SCHEMA, 'source_admitted': True, 'threshold': None,
               **dict.fromkeys(a.FALSE_FLAGS, False), **dict.fromkeys(a.ZERO_COUNTS, 0)}
    fixture[field] = value
    path = tmp_path / 'fixture.json'
    path.write_text(json.dumps(a.seal(fixture)))
    monkeypatch.setattr(a, 'rebuild', lambda _: pytest.fail('invalid flags reached proof'))
    with pytest.raises(ValueError, match='source-only'):
        a.validate(path)


def pool_case(task, directory, prefix='stdin-source-pool-v1:'):
    members = [f"apps-train-{task['task_id']}"]
    group_id = 'apps-source-v3-' + a.digest(members)[:24]
    return {'group_id': group_id, 'representative': members[0], 'all_original_members': members,
        'original_line': task['original_line_number'], 'raw_original_row_sha256': task['original_line_sha256'],
        'difficulty': 'interview', 'reference_index': 0, 'code_sha256': task['reference_sha256'],
        'original_input_output_raw_sha256': a.oracle.raw_sha(task['input_output_original'].encode()),
        'canonical_input_output_sha256': task['input_output_sha256'],
        'complete_question_sha256': a.oracle.raw_sha(task['question'].encode()),
        'complete_original_pair_count': len(task['input_output']['inputs']),
        'complete_question_original_io_envelope_utf8_bytes': len(a.packet.canonical_bytes({
            'question': task['question'], 'original_input_output': task['input_output_original']})),
        'rank_sha256': a.oracle.raw_sha((prefix + group_id).encode()),
        'pool_index': 1, 'existing_pilot_reference': False,
        'start_file': str(directory / 'started.json'), 'receipt_file': str(directory / 'receipt.json')}


@pytest.mark.parametrize('pilot', [False, True])
def test_terminal_original_plan_start_receipt_join_no_reexecution(synthetic, tmp_path, monkeypatch, pilot):
    task, receipt, graph, runtime = synthetic
    case = pool_case(task, tmp_path)
    started_case = deepcopy(case)
    if pilot:
        started_case['rank_sha256'] = a.oracle.raw_sha(('stdin-feasibility-v1:' + case['group_id']).encode())
        started_case.pop('pool_index')
        started_case.pop('existing_pilot_reference')
    original_plan = a.seal({'cases': [started_case], 'parameters': dict(a.contract.PARAMETERS)})
    plan_path = tmp_path / 'plan.json'
    plan_path.write_text(json.dumps(original_plan))
    raw_sha = a.pinned.file_sha(plan_path)
    monkeypatch.setattr(a, 'PILOT_PLAN_SHA', raw_sha)
    monkeypatch.setattr(a, 'PINS', {**a.PINS, 'full-plan.json': raw_sha})
    case['existing_pilot_reference'] = pilot
    if not pilot:
        original_plan = a.seal({'cases': [deepcopy(case)], 'parameters': dict(a.contract.PARAMETERS)})
        plan_path.write_text(json.dumps(original_plan))
        raw_sha = a.pinned.file_sha(plan_path)
        monkeypatch.setattr(a, 'PINS', {**a.PINS, 'full-plan.json': raw_sha})
        started_case = deepcopy(case)
    start = a.seal({'case': started_case, 'plan_file': str(plan_path), 'plan_hash': original_plan['artifact_hash'],
                   'plan_raw_sha256': raw_sha, 'reference_execution_attempted': True,
                   'retry_authorized': False, 'paid_calls': 0})
    Path(case['start_file']).write_text(json.dumps(start))
    Path(case['receipt_file']).write_text(json.dumps(a.seal(receipt)))
    record = {**{k: case[k] for k in ('group_id', 'difficulty', 'pool_index', 'existing_pilot_reference',
                                    'start_file', 'receipt_file')}, 'original_pair_count': 2, 'status': 'passed',
              'start_sha256': a.pinned.file_sha(case['start_file']),
              'receipt_sha256': a.pinned.file_sha(case['receipt_file'])}
    assert a._terminal_join(case, record, task, original_plan, {}, graph_proof=graph, runtime=runtime)[0] == 'passed'
    Path(case['receipt_file']).write_text(json.dumps(a.seal({**receipt, 'paid_calls': 1})))
    with pytest.raises(ValueError, match='evidence bytes changed'):
        a._terminal_join(case, record, task, original_plan, {}, graph_proof=graph, runtime=runtime)


@pytest.mark.parametrize('field', ['all_original_members', 'raw_original_row_sha256', 'reference_index',
                                 'complete_original_pair_count', 'complete_question_sha256'])
def test_pool_case_never_replaces_member_reference_or_original_pair_count(synthetic, tmp_path, field):
    task = synthetic[0]
    case = pool_case(task, tmp_path)
    group = {'group_id': case['group_id'], 'members': case['all_original_members'],
             'representative': case['representative']}
    item = (group, task, case['complete_question_original_io_envelope_utf8_bytes'], case['difficulty'])
    a._case_join(case, item)
    case = deepcopy(case)
    case[field] = ['apps-train-99'] if field == 'all_original_members' else 1
    with pytest.raises(ValueError, match='source association'):
        a._case_join(case, item)


def test_authenticated_batch_reader_closes_bindings_and_exhausts_reader(synthetic, tmp_path, monkeypatch):
    task = synthetic[0]
    row = {'id': task['task_id'], 'question': task['question'], 'starter_code': task['starter_code'], 'url': task['url'],
           'input_output': task['input_output_original'], 'solutions': json.dumps([task['reference']]),
           'difficulty': 'interview'}
    # Rebuild hash for the synthetic raw row, not a real pinned source identity.
    raw = json.dumps(row).encode()
    task = source.parse_row(raw, 13)
    file = tmp_path / 'fixture-source.jsonl'
    file.write_bytes(raw)
    case = {'group_id': 'fixture-whole-group', 'task_id': task['task_id'], 'task_sha256': a.digest(task),
        'source_kind': source.SOURCE_KIND, 'original_difficulty': 'interview', 'pair_count': 2, 'original_split': 'train',
        'original_line_sha256': task['original_line_sha256'], 'reference_sha256': task['reference_sha256'],
        'input_output_sha256': task['input_output_sha256'], 'source_reference_receipt_file': 'fixture-only',
        'source_reference_receipt_sha256': 'f' * 64, 'original_members': ['apps-train-12']}
    value = a.seal({'schema': a.SCHEMA, 'source_admitted': True, 'threshold': None, 'cases': [case],
        'bindings': [a.binding(file)], 'train_file': str(file), **dict.fromkeys(a.FALSE_FLAGS, False),
        **dict.fromkeys(a.ZERO_COUNTS, 0)})
    exhausted = []
    def rows(_):
        yield 13, raw
        exhausted.append(True)
    monkeypatch.setattr(source, 'verified_rows', rows)
    actual, path = a._read_original_tasks(value, [case])
    assert actual == {task['task_id']: task} and path == file and exhausted == [True]
    changed = {**case, 'pair_count': 1}
    with pytest.raises(ValueError, match='selected case differs'):
        a._read_original_tasks(value, [changed])
    def drifting(_):
        yield 13, raw
        file.write_bytes(b'changed after original source read')
    monkeypatch.setattr(source, 'verified_rows', drifting)
    with pytest.raises(ValueError, match='bytes drifted'):
        a._read_original_tasks(value, [case])


@pytest.mark.parametrize('module,name,value', [
    ('authority', 'SCHEMA', 'caller-schema'), ('authority', 'PINS', {}),
    ('authority', 'GROUP_SHA', '0' * 64), ('authority', 'HISTORY_SHA', '0' * 64),
    ('authority', 'PACKET_SHA', '0' * 64), ('authority', 'FRESH_SHA', '0' * 64),
    ('authority', 'STATIC_SHA', '0' * 64), ('authority', 'PILOT_PLAN_SHA', '0' * 64),
    ('authority', 'FALSE_FLAGS', ()), ('authority', 'ZERO_COUNTS', ()),
    ('source', 'SCHEMA', 'caller-task'), ('source', 'SOURCE_KIND', 'caller-kind'),
    ('pinned', 'TRAIN_SHA256', '0' * 64), ('pinned', 'TRAIN_BYTES', True),
    ('pinned', 'REVISION', 'caller-revision'), ('pinned', 'REFERENCE_POLICY', 'later-reference'),
])
def test_literal_constants_cannot_drift_without_bytecode_change(module, name, value, monkeypatch):
    target = {'authority': a, 'source': source, 'pinned': a.pinned}[module]
    monkeypatch.setattr(target, name, value)
    with pytest.raises(ValueError, match='loaded numeric constants differ'):
        a.loaded_check()


@pytest.mark.parametrize('fields', [frozenset(), set(a.CASE_FIELDS), a.CASE_FIELDS | {'candidate_label'}])
def test_case_fields_frozenset_recipe_and_exact_type_checked(fields, monkeypatch):
    monkeypatch.setattr(a, 'CASE_FIELDS', fields)
    with pytest.raises(ValueError, match='loaded case-field set differs'):
        a.loaded_check()


def test_owner_copy_changed_after_import_cannot_pin_old_memory_to_new_bytes(tmp_path, monkeypatch):
    copied = tmp_path / 'authority.py'
    copied.write_bytes(Path(a.__file__).read_bytes() + b'\n# changed after original import\n')
    monkeypatch.setattr(a, '__file__', str(copied))
    with pytest.raises(ValueError, match='differs from disk'):
        a.loaded_check()


def test_reader_alias_cannot_replace_pinned_whole_train_verifier(monkeypatch):
    monkeypatch.setattr(source, 'verified_rows', lambda _: iter([]))
    with pytest.raises(ValueError, match='reader alias differs'):
        a.loaded_check()


def test_only_caller_seal_cannot_expose_tasks_as_admitted(tmp_path):
    for name in a.PINS:
        (tmp_path / name).write_text(json.dumps(a.seal({'fixture': True})))
    caller = a.seal({'schema': a.SCHEMA, 'pool_directory': str(tmp_path), 'source_admitted': True,
        'threshold': None, 'cases': [], 'bindings': [], **dict.fromkeys(a.FALSE_FLAGS, False),
        **dict.fromkeys(a.ZERO_COUNTS, 0)})
    with pytest.raises(ValueError, match='pinned pool proof mismatch'):
        a.original_tasks(caller, [])


def test_loaded_check_keeps_packet_contract_and_original_pins():
    graph = a.loaded_check()
    assert graph['packet'] == a.PACKET_SHA
    assert a.CASE_FIELDS == frozenset(('group_id', 'task_id', 'task_sha256', 'source_kind', 'original_difficulty',
        'pair_count', 'original_split', 'original_line_sha256', 'reference_sha256', 'input_output_sha256',
        'source_reference_receipt_file', 'source_reference_receipt_sha256', 'original_members'))
