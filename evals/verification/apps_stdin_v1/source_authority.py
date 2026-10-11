"""Rebuild source-only authority from immutable first-reference and resource proof.

No Oracle is executed. This authority proves this restricted source scope only;
fresh fee-ledger prefixes and collection rights belong to later owners. Its
historic accounting snapshot is evidence, never a perpetual current-ledger cap.
"""
import ast
import hashlib
import json
import sys
from collections import Counter
from copy import deepcopy
from pathlib import Path

from ..apps_v3 import source as pinned
from . import contract, oracle, packet, source

SCHEMA = 'buglecat.verification.stdin-source-authority.v1'
ROOT = Path(__file__).resolve().parents[3]
PINS = {
    'full-plan.json': '962e443beaba3435e5beda519322e8eefaf95b2a9688c3d798a00839982ac996',
    'aggregate.json': 'b1ec05dbea9207f3bb6a9a635889f9e7b731f34dfc8fdeaceb2b52fbfbc1ab30',
    'root-source-resource-preflight-revision2.json': 'ab5d64c9b79c40193a0ac4c0c9f025995d3216c708a335d2f72e557f476c328c'}
GROUP_SHA = '5ef40c673407d09e52da819e43713e29289d2b284ca7ad57297adea290687e5a'
HISTORY_SHA = '6fab7f355a9de9c5f83102b3a99938d54216cca33ccccda3d6e5c4789a20c625'
PACKET_SHA = '8e854531467011072044eef3168bd92ffe7f38e4132da72fce8cf105b1c67445'
FRESH_SHA = '3cbca8d264df6eb8d3aa0e3d8768357a88136e685dde7275cab5d3578e70ace3'
STATIC_SHA = '3613e54894bb5ccc71b5d31bd61f34952f35a22d9a800c46373e333fc727c172'
PILOT_PLAN_SHA = '96e99081bdb8492d487edf7174bb3192e5aa3bdbde375325b757edad7e45705b'
CASE_FIELDS = frozenset(('group_id', 'task_id', 'task_sha256', 'source_kind', 'original_difficulty',
    'pair_count', 'original_split', 'original_line_sha256', 'reference_sha256', 'input_output_sha256',
    'source_reference_receipt_file', 'source_reference_receipt_sha256', 'original_members'))
FALSE_FLAGS = ('collection_authorized', 'formal_cohort_created', 'formal_registration', 'holdout_opened',
               'production_authorized', 'candidate_labels_read', 'jev_probabilities_read')
ZERO_COUNTS = ('model_calls', 'jev_calls', 'oracle_executions', 'reservations', 'new_paid_calls')
LOADED_SOURCE_SHA = pinned.file_sha(source.__file__)
LOADED_AUTHORITY_SHA = pinned.file_sha(__file__)


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def digest(value):
    return oracle.raw_sha(packet.canonical_bytes(value))


def load(path):
    return oracle.strict_json(Path(path).read_bytes())


def sealed(value):
    require(type(value) is dict and value.get('artifact_hash') ==
            digest({k: v for k, v in value.items() if k != 'artifact_hash'}), 'original seal mismatch')
    return value


def seal(value):
    return {**value, 'artifact_hash': digest(value)}


def binding(path):
    path = Path(path).resolve(strict=True)
    return {'file': str(path), 'sha256': pinned.file_sha(path)}


def check(bindings):
    require(type(bindings) is list, 'complete bindings required')
    seen = set()
    for row in bindings:
        require(type(row) is dict and set(row) == {'file', 'sha256'}, 'binding shape invalid')
        path = str(Path(row['file']).resolve(strict=True))
        require(path not in seen and type(row['sha256']) is str and len(row['sha256']) == 64,
                'duplicate or invalid binding')
        seen.add(path)
        require(pinned.file_sha(path) == row['sha256'], 'original bound bytes drifted')


def _merge_bindings(*groups):
    result = {}
    for group in groups:
        for row in group:
            path = str(Path(row['file']).resolve(strict=True))
            require(path not in result or result[path] == row['sha256'], 'conflicting original bindings')
            result[path] = row['sha256']
    return [{'file': path, 'sha256': value} for path, value in sorted(result.items())]


def _known_file(plan, suffix, expected=None):
    matches = [row for row in plan['bindings'] if Path(row['file']).as_posix().endswith(suffix)]
    require(len(matches) == 1, 'missing or ambiguous original evidence role: ' + suffix)
    if expected:
        require(matches[0]['sha256'] == expected, 'pinned evidence role mismatch: ' + suffix)
    return Path(matches[0]['file'])


def loaded_check():
    """Check loaded declaration values as well as code and current source bytes."""
    require(pinned.file_sha(source.__file__) == LOADED_SOURCE_SHA and pinned.file_sha(__file__) == LOADED_AUTHORITY_SHA,
            'loaded source authority differs from disk')
    for module in (source, sys.modules[__name__], pinned):
        packet.loaded_functions(module, Path(module.__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__),
        ('SCHEMA', 'PINS', 'GROUP_SHA', 'HISTORY_SHA', 'PACKET_SHA', 'FRESH_SHA', 'STATIC_SHA',
         'PILOT_PLAN_SHA', 'FALSE_FLAGS', 'ZERO_COUNTS'))
    packet.loaded_constants(source, Path(source.__file__), ('SCHEMA', 'SOURCE_KIND'))
    packet.loaded_constants(pinned, Path(pinned.__file__), ('TRAIN_SHA256', 'TRAIN_BYTES', 'REVISION', 'REFERENCE_POLICY'))
    declarations = [node.value for node in ast.parse(Path(__file__).read_bytes()).body
                    if isinstance(node, ast.Assign) and any(isinstance(target, ast.Name) and target.id == 'CASE_FIELDS'
                                                           for target in node.targets)]
    require(len(declarations) == 1, 'case-field declaration absent')
    recipe = declarations[0]
    require(isinstance(recipe, ast.Call) and isinstance(recipe.func, ast.Name) and recipe.func.id == 'frozenset'
            and len(recipe.args) == 1 and not recipe.keywords, 'case-field recipe changed')
    fields = ast.literal_eval(recipe.args[0])
    require(type(fields) is tuple and all(type(field) is str for field in fields) and len(set(fields)) == len(fields)
            and type(CASE_FIELDS) is frozenset and CASE_FIELDS == frozenset(fields), 'loaded case-field set differs')
    require(source.verified_rows is pinned.verified_rows, 'loaded pinned reader alias differs')
    graph = packet.dependencies()
    require(graph['packet'] == PACKET_SHA, 'frozen packet revision changed')
    oracle.loaded_graph_check({role: pinned.file_sha(path) for role, path in oracle.source_graph().items()})
    return graph


def _groups(grouping, rows):
    """Verify all original components, including excluded or non-STDIN rows."""
    groups, covered, identities = {}, set(), set()
    for group in grouping['groups']:
        members = group['members']
        require(type(members) is list and bool(members) and all(type(x) is str for x in members),
                'complete original group members required')
        require(len(set(members)) == len(members) and not covered.intersection(members), 'overlapping groups')
        require(set(members) <= rows.keys(), 'group references absent original source')
        require(members == sorted(members, key=lambda x: rows[x][0]), 'original member order differs')
        require(group['representative'] == members[0], 'earliest original representative changed')
        expected = 'apps-source-v3-' + digest(sorted(members))[:24]
        require(group['group_id'] == expected and expected not in identities, 'whole group identity mismatch')
        require(type(group.get('excluded_by_history')) is bool and
                group['excluded_by_history'] is bool(group.get('historical_source_matches')),
                'whole-history exclusion mismatch')
        require(group.get('source_admitted') is False and group.get('evaluation_split') is None,
                'original grouping grants authority')
        covered.update(members)
        identities.add(expected)
        groups[expected] = group
    require(covered == rows.keys() and len(rows) == 5000, 'complete source partition required')
    return groups


def _pool_members(plan, groups, rows, excluded):
    fresh, static, eligible, rejected = [], [], [], []
    for group in groups.values():
        line, raw, row = rows[group['representative']]
        try:
            io = json.loads(row['input_output'], parse_constant=pinned.reject_nonfinite)
        except (KeyError, TypeError, ValueError):
            continue
        if group['excluded_by_history'] or group['group_id'] in excluded or type(io) is not dict or io.get('fn_name') is not None:
            continue
        identity = {'group_id': group['group_id'], 'representative': group['representative'],
                    'all_original_members': group['members']}
        fresh.append(identity)
        try:
            task = source.parse_row(raw, line)
        except (SyntaxError, ValueError, TypeError, contract.ScopeError, MemoryError, RecursionError):
            continue
        static.append(group['group_id'])
        envelope = len(packet.canonical_bytes({'question': task['question'],
                                              'original_input_output': task['input_output_original']}))
        item = (group, task, envelope, row['difficulty'])
        (eligible if envelope <= 32000 else rejected).append(item)
    require(fresh == plan['fresh_earliest_original_sources'] and len(fresh) == 1734 and
            digest([x['group_id'] for x in fresh]) == plan['fresh_group_ids_sha256'] == FRESH_SHA,
            'fresh whole allocation/history proof changed')
    require(len(static) == plan['static_pass_count'] == 1131 and digest(static) ==
            plan['static_pass_group_ids_sha256'] == STATIC_SHA, 'static original index-zero pool changed')
    eligible.sort(key=lambda x: hashlib.sha256(('stdin-source-pool-v1:' + x[0]['group_id']).encode()).hexdigest())
    require(len(eligible) == len(plan['cases']) == 1130 and len(rejected) == len(plan['resource_excluded']) == 1,
            'complete resource pool changed')
    require(plan['resource_excluded'][0]['group_id'] == rejected[0][0]['group_id'], 'resource exclusion replaced')
    return eligible


def _case_join(case, item, rank_prefix='stdin-source-pool-v1:'):
    group, task, envelope, difficulty = item
    expected = {'group_id': group['group_id'], 'representative': group['representative'],
        'all_original_members': group['members'], 'original_line': task['original_line_number'],
        'raw_original_row_sha256': task['original_line_sha256'], 'difficulty': difficulty, 'reference_index': 0,
        'code_sha256': task['reference_sha256'], 'original_input_output_raw_sha256':
        oracle.raw_sha(task['input_output_original'].encode()), 'canonical_input_output_sha256': task['input_output_sha256'],
        'complete_question_sha256': oracle.raw_sha(task['question'].encode()),
        'complete_original_pair_count': len(task['input_output']['inputs']),
        'complete_question_original_io_envelope_utf8_bytes': envelope,
        'rank_sha256': oracle.raw_sha((rank_prefix + group['group_id']).encode())}
    require(all(type(case.get(k)) is type(v) and case[k] == v for k, v in expected.items()),
            'original pool case/source association changed')


def _receipt_join(receipt, task, parameters, graph_proof=None, runtime=None):
    sealed(receipt)
    require(receipt.get('schema') == 'buglecat.verification.stdin-development-execution.v1', 'original execution receipt required')
    require(receipt.get('code_sha256') == task['reference_sha256'] and
            receipt.get('input_output_sha256') == task['input_output_sha256'], 'original code or IO receipt mismatch')
    require(packet.canonical_bytes(receipt.get('parameters')) == packet.canonical_bytes(parameters), 'execution parameter drift')
    runtime = runtime or {'executable_file': str(Path(sys.executable).resolve()),
        'executable_sha256': pinned.file_sha(sys.executable), 'version': sys.version,
        'version_info': list(sys.version_info[:3]), 'platform': sys.platform}
    require(receipt.get('runtime') == runtime, 'execution runtime drift')
    for name in ('credentials_removed', 'original_script_unchanged', 'full_ordered_pairs_retained'):
        require(receipt.get(name) is True, 'original execution scope not retained')
    for name in ('source_admitted', 'formal_registration', 'production_authorized', 'official_evaluator_parity',
                 'memory_isolation_proven', 'os_sandbox'):
        require(receipt.get(name) is False, 'receipt grants unexpected authority')
    require(type(receipt.get('paid_calls')) is int and receipt['paid_calls'] == 0 and
            type(receipt.get('reservations')) is int and receipt['reservations'] == 0, 'reference receipt contains fee')
    expected_graph = oracle.source_graph()
    graph_proof = graph_proof or {role: binding(path) for role, path in expected_graph.items()}
    require(set(receipt['source_graph']) == set(expected_graph), 'original execution graph incomplete')
    evidence = []
    for role, path in expected_graph.items():
        bound = receipt['source_graph'][role]
        require(Path(bound['file']).resolve() == path.resolve() and bound['before_sha256'] ==
                bound['after_sha256'] == graph_proof[role]['sha256'], 'original execution owner bytes drifted')
        evidence.append(graph_proof[role])
    pairs, result = contract.ordered_pairs(task['input_output']), receipt['result']
    require(type(result['pair_count']) is int and result['pair_count'] == len(pairs) and
            type(result['pair_results']) is list and len(result['pair_results']) == len(pairs), 'original pairs deleted')
    for index, (row, pair) in enumerate(zip(result['pair_results'], pairs, strict=True)):
        require(type(row['index']) is int and row['index'] == index and type(row['child_started']) is bool and
                row['input_sha256'] == oracle.raw_sha(pair[0].encode()) and
                row['expected_sha256'] == oracle.raw_sha(pair[1].encode()), 'original pair index or input changed')
        require(row['status'] in {'passed', 'failed', 'unknown'} and type(row['passed']) is bool and
                row['passed'] is (row['status'] == 'passed'), 'invalid pair result')
        worker = {k: v for k, v in row.items() if k not in ('input_sha256', 'expected_sha256', 'child_started')}
        if 'worker_pid' in worker:
            require(oracle.checked_result(worker, index) == worker, 'malformed worker result')
        else:
            require(row['status'] == 'unknown' and type(row.get('reason')) is str and bool(row['reason']),
                    'unstarted or resource pair cannot be binary')
        require(row['child_started'] or row['status'] == 'unknown', 'unstarted pair labelled binary')
    expected = ('unknown' if any(x['status'] == 'unknown' for x in result['pair_results']) else
                'failed' if any(x['status'] == 'failed' for x in result['pair_results']) else 'passed')
    require(result['status'] == expected and type(result['pairs_executed']) is int and
            result['pairs_executed'] == sum(x['child_started'] for x in result['pair_results']), 'group status/count differs')
    return expected, evidence


def _terminal_join(case, record, task, plan, aggregate, plans=None, graph_proof=None, runtime=None):
    require(type(case['pool_index']) is int and case['pool_index'] == record['pool_index'] and
            case['group_id'] == record['group_id'] and case['difficulty'] == record['difficulty'] and
            type(case['existing_pilot_reference']) is bool and
            case['existing_pilot_reference'] is record['existing_pilot_reference'], 'terminal pool row changed')
    require(Path(case['start_file']).resolve() == Path(record['start_file']).resolve() and
            Path(case['receipt_file']).resolve() == Path(record['receipt_file']).resolve(), 'terminal evidence path mismatch')
    evidence = [binding(record[k]) for k in ('start_file', 'receipt_file')]
    require(evidence[0]['sha256'] == record['start_sha256'] and evidence[1]['sha256'] == record['receipt_sha256'],
            'original terminal evidence bytes changed')
    start = sealed(load(record['start_file']))
    plans = plans if plans is not None else {}
    plan_path = str(Path(start['plan_file']).resolve(strict=True))
    if plan_path not in plans:
        plans[plan_path] = (binding(plan_path), sealed(load(plan_path)))
    original_binding, original_plan = plans[plan_path]
    evidence.append(original_binding)
    require(evidence[-1]['sha256'] == start['plan_raw_sha256'] and original_plan['artifact_hash'] == start['plan_hash'],
            'original start plan binding differs')
    require(start['reference_execution_attempted'] is True and start['retry_authorized'] is False and
            type(start['paid_calls']) is int and start['paid_calls'] == 0, 'start grants retry/fee')
    if case['existing_pilot_reference']:
        require(start['plan_raw_sha256'] == PILOT_PLAN_SHA and start['case'] in original_plan['cases'], 'pilot plan differs')
        _case_join(start['case'], (dict(group_id=case['group_id'], representative=case['representative'],
            members=case['all_original_members']), task,
            case['complete_question_original_io_envelope_utf8_bytes'], case['difficulty']), 'stdin-feasibility-v1:')
    else:
        require(start['plan_raw_sha256'] == PINS['full-plan.json'] and start['case'] == case and
                original_plan == plan, 'full-pool start differs')
    status, graph = _receipt_join(load(record['receipt_file']), task, plan['parameters'], graph_proof, runtime)
    require(status == record['status'] and type(record['original_pair_count']) is int and
            record['original_pair_count'] == case['complete_original_pair_count'], 'terminal status or pairs changed')
    return status, evidence + graph


def _resource_record(case, record, task, template=None):
    result = {'group_id': case['group_id'], 'difficulty': case['difficulty'], 'original_line': case['original_line'],
              'reference_receipt_sha256': record['receipt_sha256']}
    probe = 'pass #' + 'x' * (15998 - 6)
    require(len(packet.canonical_bytes(probe)) == 16000, 'candidate reserve changed')
    try:
        if template is None:
            body = packet.request(task['question'], task['starter_code'], probe, task['input_output'])
        else:
            # Only variable original public fields change. Template is freshly
            # produced by the pinned builder within this transaction, never a
            # caller authority or persistent cache. Full bytes/IO remain intact.
            body = deepcopy(template)
            body['state'].update(task_question=task['question'],
                public_interface={'starter_code': task['starter_code'], 'mode': 'stdin_script'},
                input_output=deepcopy(task['input_output']), pair_count=len(task['input_output']['inputs']))
            require(len(packet.canonical_bytes(body)) <= packet.MAX_INPUT_BYTES,
                    'complete STDIN request exceeds input byte budget')
        return {**result, 'status': 'resource_pass', 'full_packet_utf8_bytes': len(packet.canonical_bytes(body)),
                'resource_probe_packet_sha256': digest(body)}
    except ValueError as error:
        return {**result, 'status': 'resource_rejected', 'reason': str(error)}


def rebuild(pool_directory):
    """Read every frozen receipt and full pinned source; return no collection rights."""
    graph = loaded_check()
    folder = Path(pool_directory).resolve(strict=True)
    evidence = [binding(folder / name) for name in PINS]
    require(all(x['sha256'] == PINS[Path(x['file']).name] for x in evidence), 'pinned pool proof mismatch')
    plan, aggregate, resource = [sealed(load(x['file'])) for x in evidence]
    check(plan['bindings'])
    require(plan['schema'] == 'buglecat.verification.stdin-reference-pool-plan.v1' and
            aggregate['plan_raw_sha256'] == PINS['full-plan.json'] and aggregate['plan_hash'] == plan['artifact_hash'],
            'original pool plan association differs')
    require(aggregate['terminal_groups'] == aggregate['fixed_allocation_groups'] == 1130 and
            type(aggregate['remaining_groups']) is int and aggregate['remaining_groups'] == 0 and
            aggregate['source_fee_closure_unchanged'] is True, 'original pool incomplete')
    require(packet.canonical_bytes(plan['parameters']) == packet.canonical_bytes(contract.PARAMETERS), 'loaded parameter drift')
    require(resource['reference_pool_plan_sha256'] == PINS['full-plan.json'] and
            resource['reference_pool_aggregate_sha256'] == PINS['aggregate.json'] and
            resource['dependency_sha256'] == graph and resource['runtime'] == packet.runtime_info(), 'resource graph differs')
    train = _known_file(plan, 'artifact-v3-apps-census-20261011/train.jsonl', pinned.TRAIN_SHA256)
    grouping_path = _known_file(plan, 'artifact-v3-apps-groups-20261011/groups.json', GROUP_SHA)
    history_path = _known_file(plan, 'artifact-v3-apps-source-authority-20261011/source-authority.json', HISTORY_SHA)
    history = sealed(load(history_path))
    check(history['bindings'])
    rows = {}
    for number, raw in source.verified_rows(train):
        row = json.loads(raw, parse_constant=pinned.reject_nonfinite)
        rows[f"apps-train-{row['id']}"] = (number, raw, row)
    groups = _groups(load(grouping_path), rows)
    formal = sealed(load(_known_file(plan, 'artifact-v3-apps-formal316-20261011/cohort.json')))
    development = sealed(load(_known_file(plan, 'artifact-v3-apps-development-20261011/cohort.json')))
    excluded = set(formal['excluded_groups']) | {x['group_id'] for x in formal['cases']} | {
        x['group_id'] for x in development['cases']}
    items = _pool_members(plan, groups, rows, excluded)
    records = aggregate['all_original_terminal_results']
    require(type(records) is list and len(records) == len(items), 'original terminal rows omitted')
    graph_proof = {role: binding(path) for role, path in oracle.source_graph().items()}
    runtime = {'executable_file': str(Path(sys.executable).resolve()), 'executable_sha256': graph_proof['runtime']['sha256'],
               'version': sys.version, 'version_info': list(sys.version_info[:3]), 'platform': sys.platform}
    plans = {evidence[0]['file']: (evidence[0], plan)}
    template = packet.request('resource template', '', 'pass #' + 'x' * (15998 - 6), {'inputs': [''], 'outputs': ['']})
    cases, probes, counts = [], [], Counter()
    for index, (case, record, item) in enumerate(zip(plan['cases'], records, items, strict=True), 1):
        require(case['pool_index'] == index, 'original pool order differs')
        _case_join(case, item)
        task = item[1]
        status, joined = _terminal_join(case, record, task, plan, aggregate, plans, graph_proof, runtime)
        evidence.extend(joined)
        counts[status] += 1
        if status != 'passed':
            continue
        probe = _resource_record(case, record, task, template)
        probes.append(probe)
        if probe['status'] == 'resource_pass':
            cases.append({'group_id': case['group_id'], 'task_id': task['task_id'], 'task_sha256': digest(task),
                'source_kind': source.SOURCE_KIND, 'original_difficulty': case['difficulty'],
                'pair_count': case['complete_original_pair_count'], 'original_split': 'train',
                'original_line_sha256': task['original_line_sha256'], 'reference_sha256': task['reference_sha256'],
                'input_output_sha256': task['input_output_sha256'], 'source_reference_receipt_file': record['receipt_file'],
                'source_reference_receipt_sha256': record['receipt_sha256'], 'original_members': case['all_original_members']})
    require(dict(counts) == aggregate['status_counts'] == resource['source_reference_status'] and
            probes == resource['records'] and len(cases) == resource['packet_resource_pass'] == 988,
            'complete resource/source outcome association differs')
    dependency_paths = [Path(__file__), Path(source.__file__), Path(pinned.__file__), *packet.dependency_files().values(),
                        Path(sys.executable)]
    bindings = _merge_bindings(plan['bindings'], history['bindings'], evidence, [binding(p) for p in dependency_paths])
    check(bindings)
    body = {'schema': SCHEMA, 'pool_directory': str(folder), 'train_file': str(train),
        'source_kind': source.SOURCE_KIND, 'original_split': 'train', 'revision': pinned.REVISION,
        'full_source_sha256': pinned.TRAIN_SHA256, 'cases': cases, 'bindings': bindings,
        'source_reference_terminal_counts': dict(counts), 'qualified_groups': len(cases),
        'qualified_difficulty': dict(Counter(x['original_difficulty'] for x in cases)),
        'reference_pool_plan_sha256': PINS['full-plan.json'], 'reference_pool_aggregate_sha256': PINS['aggregate.json'],
        'resource_proof_sha256': PINS['root-source-resource-preflight-revision2.json'],
        'dependency_sha256': graph, 'runtime': packet.runtime_info(), 'source_admitted': True,
        'source_admission_scope': 'first-original-reference-under-restricted-stdin-v1-and-complete-packet-resource-only',
        'official_evaluator_parity': False, 'semantic_near_duplicate_absence_proven': False,
        'natural_negative_sufficiency_proven': False, 'threshold': None,
        'historical_accounting_snapshot': {'accounting': plan['accounting'],
            'ledger_ordinal_group_sha256': plan['ledger_ordinal_group_sha256'],
            'reservation_raw_bytes_sha256': plan['reservation_raw_bytes_sha256']},
        **dict.fromkeys(FALSE_FLAGS, False), **dict.fromkeys(ZERO_COUNTS, 0)}
    # All bytes are closed again; no shared ledger is read or mutated here.
    check(bindings)
    require(loaded_check() == graph, 'loaded source graph changed during authority rebuild')
    return seal(body)


def validate(path):
    """Self-seal is insufficient: reproduce the complete pinned proof each call."""
    before = binding(path)
    value = sealed(load(path))
    require(value.get('schema') == SCHEMA and value.get('source_admitted') is True and
            all(value.get(k) is False for k in FALSE_FLAGS) and value.get('threshold') is None and
            all(type(value.get(k)) is int and value[k] == 0 for k in ZERO_COUNTS), 'source-only authority required')
    rebuilt = rebuild(value['pool_directory'])
    require(packet.canonical_bytes(value) == packet.canonical_bytes(rebuilt), 'authority differs from original proof')
    check([before])
    return value


def _read_original_tasks(authority, cases):
    """Read batch rows after original_tasks has authenticated the authority."""
    sealed(authority)
    require(authority.get('schema') == SCHEMA and authority.get('source_admitted') is True and
            all(authority.get(k) is False for k in FALSE_FLAGS) and authority.get('threshold') is None and
            all(type(authority.get(k)) is int and authority[k] == 0 for k in ZERO_COUNTS), 'source-only authority required')
    check(authority['bindings'])
    allocated = {x['group_id']: x for x in authority['cases']}
    require(len(allocated) == len(authority['cases']) and type(cases) is list and bool(cases), 'unique selected groups required')
    chosen = {}
    for case in cases:
        require(type(case) is dict and set(case) == CASE_FIELDS and type(case['task_id']) is int and
                case == allocated.get(case['group_id']) and case['task_id'] not in chosen, 'selected case differs')
        chosen[case['task_id']] = case
    tasks = {}
    for number, raw in source.verified_rows(authority['train_file']):
        row = json.loads(raw, parse_constant=pinned.reject_nonfinite)
        if row['id'] not in chosen:
            continue
        task, case = source.parse_row(raw, number), chosen[row['id']]
        require(digest(task) == case['task_sha256'] and task['reference_sha256'] == case['reference_sha256'] and
                task['original_line_sha256'] == case['original_line_sha256'] and
                task['input_output_sha256'] == case['input_output_sha256'], 'original task changed')
        tasks[row['id']] = task
    require(tasks.keys() == chosen.keys(), 'selected original source absent')
    check(authority['bindings'])
    return tasks, Path(authority['train_file'])


def original_tasks(authority, cases):
    """Authenticate a path or complete authority before exposing batch tasks.

    A caller's self-sealed dict is never admission evidence. Rebuild the pinned
    source proof once for this entire batch, then read all selected original rows.
    This grants no collection, fee or registration capability.
    """
    before = loaded_check()
    if isinstance(authority, (str, Path)):
        authority = validate(authority)
    else:
        sealed(authority)
        rebuilt = rebuild(authority['pool_directory'])
        require(packet.canonical_bytes(authority) == packet.canonical_bytes(rebuilt),
                'task authority differs from original proof')
    result = _read_original_tasks(authority, cases)
    require(loaded_check() == before, 'loaded source graph changed during original task read')
    return result
