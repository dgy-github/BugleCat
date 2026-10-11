"""Original development main/SQLite/bytes joins; no fee or phase authority.

Read-only by default. Root may explicitly execute a previously unrecorded
candidate once, using its complete original IO; a stranded marker is permanent.
"""
import os
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

from .. import collection_receipts, paid_budget
from ..collection_receipts import verify
from ..paid_budget import durable_new
from . import budget, cohort, collection, contract, oracle, packet

SCHEMA = 'buglecat.verification.stdin-original-development-dataset.v1'
MAIN_TERMINAL_STATUSES = ('ok', 'client_error', 'transport_error', 'response_budget', 'timeout', 'cancelled',
    'response_invalid_json', 'response_choices', 'response_content_invalid', 'candidate_literal_resource_budget',
    'candidate_string', 'candidate_json', 'response_model_or_usage_invalid', 'session_persistence_failed',
    'packet_json', 'host_json', 'session_read', 'sources_read', 'packet_read',
    'output_exists_or_unwritable', 'output_write', 'output_sync')


def main_terminal_status(status):
    return type(status) is str and (status in MAIN_TERMINAL_STATUSES or (
        len(status) == 8 and status.startswith('http_') and status[5:].isascii() and status[5:].isdigit()
        and 100 <= int(status[5:]) <= 599 and not 200 <= int(status[5:]) <= 299))


def loaded_check():
    packet.dependencies()
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'MAIN_TERMINAL_STATUSES'))
    packet.loaded_functions(collection_receipts, Path(collection_receipts.__file__))
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))
    packet.require(verify is collection_receipts.verify and durable_new is paid_budget.durable_new,
                   'loaded receipt/durability aliases changed')
    return packet.runtime_info()


def directory_bindings(directory):
    """Reject symlink/junction escape and bind all original files bidirectionally."""
    directory = Path(directory)
    packet.require(not directory.is_symlink() and not directory.is_junction(), 'collection directory link forbidden')
    root = directory.resolve(strict=True)
    paths = []
    for path in directory.rglob('*'):
        packet.require(not path.is_symlink() and not path.is_junction()
                       and path.resolve(strict=True).is_relative_to(root), 'collection path link or escape forbidden')
        if path.is_file():
            paths.append(path)
    return budget.raw_bindings(paths)


def runner_sources():
    return cohort.owners()


def build(executable):
    executable = Path(executable).resolve(strict=True)
    before = cohort.sha(executable)
    env = {key: value for key, value in os.environ.items() if not any(token in key.upper() for token in ('KEY', 'TOKEN', 'SECRET'))}
    child = subprocess.run([str(executable), 'build-info'], capture_output=True, check=True, timeout=10, env=env)
    value = budget.parse(child.stdout)
    packet.require(type(value.get('paid_calls')) is int and value['paid_calls'] == 0
                   and type(value.get('http_calls')) is int and value['http_calls'] == 0, 'pure build-info required')
    info = value['build_info']
    packet.require(info['executable_sha256'] == before == cohort.sha(executable)
                   and all(info.get(role) == cohort.sha(path) for role, path in runner_sources().items()),
                   'genuine current compiled collector owner graph required')
    return info


def lines(path):
    return [budget.parse(line) for line in Path(path).read_bytes().splitlines() if line.strip()]


def receipt_seal(row):
    packet.require(row.get('artifact_sha256') == budget.digest({key: child for key, child in row.items() if key != 'artifact_sha256'}),
                   'original transport seal changed')


def same_path(left, right):
    def identity(path):
        value = str(Path(path).resolve())
        if value.startswith('\\\\?\\UNC\\'):
            value = '\\\\' + value[8:]
        elif value.startswith('\\\\?\\'):
            value = value[4:]
        return os.path.normcase(value)
    return identity(left) == identity(right)


def original_code(answer):
    """One Python artifact amid assessment; preserve its exact UTF8 inner span."""
    if type(answer) is not str:
        return None
    if answer.count('```') != 2:
        return None
    match = re.search(r'(?m)^```python\r?\n([\s\S]*?)^```(?=\r?$|\r?\n)', answer)
    if not match:
        return None
    code = match.group(1)
    return {'code': code, 'code_sha256': oracle.raw_sha(code.encode()),
            'start': len(answer[:match.start(1)].encode()), 'end': len(answer[:match.end(1)].encode())}


def _prepared(row, case, task, manifest, directory, compiled):
    receipt_seal(row)
    case_dir = directory / case['id']
    fields = (*collection.BIND_FIELDS, 'artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
              'authorization_sha256', 'source_sha256', 'catalog_sha256', 'protocol_sha256',
              'main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')
    packet.require(row['schema'] == 'buglecat.verification.prepared-main.v1'
                   and type(row.get('endpoint_sha256')) is str and re.fullmatch('[0-9a-f]{64}', row['endpoint_sha256'])
                   and all(budget.same(row.get(key), case[key]) for key in (*collection.history_fields(), 'public_interface'))
                   and all(budget.same(row.get(key), manifest[key]) for key in fields)
                   and budget.same(row['settings'], cohort.frozen.EXPECTED_SETTINGS)
                   and budget.same(row['build_info'], compiled)
                   and row['manifest_sha256'] == cohort.sha(directory / 'collection-manifest.json')
                   and same_path(row['case_directory'], case_dir)
                   and same_path(row['workspace'], case_dir / 'workspace')
                   and same_path(row['session_log'], case_dir / 'workspace/.nanocodex/sessions' / (case['id'] + '.jsonl'))
                   and same_path(row['manifest_file'], directory / 'collection-manifest.json'), 'prepared scope/source/path/build changed')
    prompt = collection.public_task(task)['main_prompt']
    body = budget.load(case_dir / 'request.json')
    packet.require(row['prompt'] == prompt and row['prompt_sha256'] == oracle.raw_sha(prompt.encode())
                   and body['messages'] == [{'role': 'system', 'content': cohort.artifact_v2_development_cohort.system_prompt()},
                                            {'role': 'user', 'content': prompt}]
                   and budget.same(row['body'], body) and row['request_hash'] == cohort.sha(case_dir / 'request.json'),
                   'public-only actual request body changed')


def terminal(row, start, finish, events, directory):
    selected = [event for event in events if event['id'] == row['id']]
    path = directory / 'receipt.json'
    if start is None:
        packet.require(not selected and not path.exists() and finish is None, 'transport proof without original start')
        return None, 'main_not_started'
    packet.require(selected and budget.same(start['started'], {key: child for key, child in selected[0].items() if key != 'attempt_ordinal'})
                   and type(selected[0]['attempt_ordinal']) is int and selected[0]['attempt_ordinal'] == start['ordinal'],
                   'original SQLite/start event mismatch')
    if not path.exists():
        packet.require(finish is None, 'finish without original receipt')
        return None, 'main_not_finished'
    value = budget.load(path)
    receipt_seal(value)
    fields = (*collection.BIND_FIELDS, *collection.history_fields(), 'artifact_cohort_file', 'artifact_cohort_sha256',
              'artifact_cohort_hash', 'manifest_sha256', 'request_hash', 'settings', 'build_info', 'case_directory',
              'workspace', 'session_log', 'source_sha256', 'catalog_sha256', 'protocol_sha256', 'authorization_sha256')
    fields = (*fields, 'endpoint_sha256')
    packet.require([event['event'] for event in selected] == ['attempt_started', 'request_submitted', 'attempt_finished']
                   and budget.same(selected[-1], finish) and selected[-1]['receipt_sha256'] == cohort.sha(path)
                   and all(budget.same(event.get(key), row.get(key)) for event in selected for key in fields)
                   and all(budget.same(value.get(key), row.get(key)) for key in fields)
                   and all(type(event['attempt_ordinal']) is int and event['attempt_ordinal'] == start['ordinal'] for event in selected)
                   and type(value['attempt_ordinal']) is int and value['attempt_ordinal'] == start['ordinal']
                   and selected[1]['actual_request_hash'] == value['actual_request_hash'] == row['request_hash']
                   and selected[-1]['call_status'] == value['call_status'] and main_terminal_status(value['call_status']),
                   'original terminal binding changed')
    if value.get('response_sha256'):
        packet.require(cohort.sha(directory / 'raw-response.json') == value['response_sha256'], 'original response bytes changed')
    http_status = value.get('http_status')
    packet.require(selected[-1].get('http_status') == http_status
                   and selected[-1].get('response_sha256') == value.get('response_sha256'),
                   'original HTTP/response finish binding changed')
    if value['call_status'].startswith('http_'):
        packet.require(type(http_status) is int and http_status == int(value['call_status'][5:])
                       and value.get('response_sha256') is not None, 'complete non2xx raw response required')
    elif value.get('response_sha256'):
        packet.require(type(http_status) is int and 200 <= http_status < 300, 'raw success HTTP status required')
    else:
        packet.require(http_status is None, 'incomplete transport must not fabricate HTTP response')
    return value, value['call_status']


def _oracle(task, answer, directory, execute):
    code = original_code(answer)
    if code is None:
        return {'label': 'unknown', 'reason': 'single_unmodified_python_fence_required', 'call_eligible': False}
    try:
        packet.request(task['question'], task['starter_code'], code['code'], task['input_output'])
    except ValueError:
        return {'label': 'unknown', 'reason': 'complete_packet_resource_or_scope_rejected', 'call_eligible': False}
    path = directory / 'stdin-candidate-oracle.json'
    marker = directory / 'stdin-candidate-oracle.started'
    if not path.exists():
        if not execute:
            return {'label': 'unknown', 'reason': 'oracle_attempt_incomplete' if marker.exists() else 'independent_oracle_not_recorded',
                    'call_eligible': False}
        durable_new(marker, {'candidate_code_sha256': code['code_sha256'],
            'task_sha256': budget.digest(task), 'input_output_sha256': budget.digest(task['input_output']),
            'runtime': packet.runtime_info(), 'source_graph': packet.dependencies()})
        record = oracle.execute(code['code'], task['input_output'])
        durable_new(path, record)
    record = cohort.sealed(budget.load(path))
    proof = budget.load(marker)
    graph = oracle.source_graph()
    packet.require(record['code_sha256'] == code['code_sha256']
                   and record['input_output_sha256'] == budget.digest(task['input_output'])
                   and proof == {'candidate_code_sha256': code['code_sha256'], 'task_sha256': budget.digest(task),
                                 'input_output_sha256': budget.digest(task['input_output']),
                                 'runtime': packet.runtime_info(), 'source_graph': packet.dependencies()}
                   and budget.same(record['parameters'], contract.LOADED_PARAMETERS)
                   and record['credentials_removed'] is True and record['original_script_unchanged'] is True
                   and all(record['source_graph'][role]['before_sha256'] == record['source_graph'][role]['after_sha256'] == cohort.sha(source)
                           and same_path(record['source_graph'][role]['file'], source) for role, source in graph.items())
                   and all(record['runtime'][key] == value for key, value in packet.runtime_info().items()),
                   'original Oracle source/code/IO/runtime/marker changed')
    result = record['result']
    pairs = contract.ordered_pairs(task['input_output'])
    packet.require(type(result['pair_count']) is int and result['pair_count'] == len(pairs)
                   and len(result['pair_results']) == len(pairs)
                   and record['full_ordered_pairs_retained'] is True, 'full original ordered IO required')
    for index, (row, (input_text, expected)) in enumerate(zip(result['pair_results'], pairs, strict=True)):
        packet.require(type(row['index']) is int and row['index'] == index
                       and row['status'] in ('passed', 'failed', 'unknown') and type(row['passed']) is bool
                       and row['passed'] is (row['status'] == 'passed')
                       and row['input_sha256'] == oracle.raw_sha(input_text.encode())
                       and row['expected_sha256'] == oracle.raw_sha(expected.encode()), 'original Oracle pair identity changed')
    status = 'unknown' if any(row['status'] == 'unknown' for row in result['pair_results']) else (
        'failed' if any(row['status'] == 'failed' for row in result['pair_results']) else 'passed')
    packet.require(result['status'] == status, 'Unknown-priority original Oracle status changed')
    return {'label': {'passed': 'supported', 'failed': 'unsupported', 'unknown': 'unknown'}[status],
            'reason': status, 'call_eligible': status != 'unknown', 'oracle_file': str(path),
            'oracle_sha256': cohort.sha(path), 'candidate_code_sha256': code['code_sha256'], 'candidate_code_span': code}


def collect(cohort_file, prepared_directories, executable, *, execute_new_oracles=False):
    """All 808 outcomes retained; full source once, then each original join."""
    runtime = loaded_check()
    packet.require(type(execute_new_oracles) is bool, 'explicit Oracle option boolean required')
    locked = {'file': str(Path(cohort_file).resolve()), 'sha256': cohort.sha(cohort_file)}
    pack = cohort.verified_pack(cohort_file)
    value, current, finished = pack['cohort'], pack['current'], pack['finished']
    compiled = build(executable)
    packet.require(budget.same(compiled, value['collector_build_info'])
                   and cohort.sha(executable) == value['collector_executable_sha256'],
                   'provided binary differs from original cohort compiled collector')
    starts = {row['group_id']: row for row in current['main_started']}
    own = {key: row for key, row in starts.items() if row['started'].get('artifact_cohort_hash') == value['artifact_hash']}
    packet.require(not execute_new_oracles or all(finished[key] is not None for key in own), 'Oracle prohibited during in-flight main')
    actual, bindings, before_directories = {}, [], {}
    directories = [Path(path).resolve(strict=True) for path in prepared_directories]
    packet.require(len(set(directories)) == len(directories), 'duplicate preparation directory')
    for directory in directories:
        manifest, _ = collection.validate_manifest(directory / 'collection-manifest.json', pack=pack)
        rows = lines(directory / 'prepared.jsonl')
        events_file = directory / 'attempt-events.jsonl'
        events = lines(events_file) if events_file.exists() else []
        packet.require(len(rows) == len(manifest['cases']) and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'complete original preparation/events required')
        captured = directory_bindings(directory)
        before_directories[str(directory)] = captured
        for row, case in zip(rows, manifest['cases'], strict=True):
            task, case_dir = pack['source_pack']['tasks'][case['task_id']], directory / case['id']
            _prepared(row, case, task, manifest, directory, compiled)
            start = own.get(case['group_id'])
            matching = start is not None and same_path(start['started']['case_directory'], case_dir)
            original, status = terminal(row, start if matching else None, finished.get(case['group_id']) if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate actual candidate')
                if status == 'ok':
                    verify(row, case_dir / 'receipt.json', events, executable)
                actual[case['group_id']] = (original, status, case_dir, task)
        bindings += captured
    packet.require(set(actual) == set(own), 'actual started group or preparation directory omitted')
    outcomes = []
    for case in value['cases']:
        item = actual.get(case['group_id'])
        original, status, directory, task = item if item else (None, 'main_not_started', None, None)
        row = {**{key: case[key] for key in collection.history_fields()}, 'main_status': status,
               'label': 'unlabeled' if status in ('main_not_started', 'main_not_finished') else 'unknown',
               'reason': status, 'call_eligible': False}
        if original:
            row.update(main_receipt_file=str(directory / 'receipt.json'), main_receipt_sha256=cohort.sha(directory / 'receipt.json'))
        if status == 'ok':
            row.update(_oracle(task, original['result']['candidate'], directory, execute_new_oracles))
        outcomes.append(row)
    cohort.check(cohort.source_authority._merge_bindings([locked], pack['bindings'], bindings))
    allowed_new = {str(item[2] / name) for item in actual.values() if item[1] == 'ok'
                   for name in ('stdin-candidate-oracle.json', 'stdin-candidate-oracle.started')} if execute_new_oracles else set()
    final_bindings = []
    for directory in directories:
        after = directory_bindings(directory)
        before = before_directories[str(directory)]
        old_paths = {item['file'] for item in before}
        new_paths = {item['file'] for item in after}
        packet.require(old_paths <= new_paths and new_paths - old_paths <= allowed_new,
                       'collection file set changed during intake')
        cohort.check(before)
        final_bindings += after
    packet.require(budget.same(budget._snapshot_chain(current['authorization_chain']), (current, finished))
                   and build(executable) == compiled and loaded_check() == runtime,
                   'budget/executable/loaded runtime changed during intake')
    body = {'schema': SCHEMA, 'cohort_file': str(Path(cohort_file).resolve()), 'cohort_hash': value['artifact_hash'],
            'cohort_sha256': locked['sha256'], 'build_info': compiled, 'collector_executable_file': str(Path(executable).resolve()),
            'cases': outcomes, 'allocated_groups': len(outcomes), 'collected_groups': len(own),
            'pending_groups': sum(row['main_status'] == 'main_not_finished' for row in outcomes),
            'uncollected_groups': len(outcomes) - len(own), 'label_counts': dict(Counter(row['label'] for row in outcomes)),
            'collection_bindings': final_bindings, 'scores_hidden': True, 'jev_probabilities_read': False,
            'formal_registration': False, 'holdout_opened': False, 'production_authorized': False,
            'formal_certification': False, 'certified_threshold': None, 'model_calls': 0, 'jev_calls': 0}
    return cohort.seal(body)
