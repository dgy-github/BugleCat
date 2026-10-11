"""Original calibration main and once-only independent Oracle joins.

Read-only by default, full808 outcomes retained. No Jev reservation, registration,
threshold or holdout permission is provided. Original development is archived
and authenticated, not re-executed or counted toward calibration labels.
"""
import sys
from collections import Counter
from pathlib import Path

from . import budget, cohort, collection, intake, packet
from . import calibration_collection as collector
from . import calibration_originals as originals

SCHEMA = 'buglecat.verification.stdin-original-calibration-dataset.v1'
MAIN_TERMINAL_STATUSES = intake.MAIN_TERMINAL_STATUSES


def main_terminal_status(status):
    return intake.main_terminal_status(status)


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    packet.require(MAIN_TERMINAL_STATUSES is intake.MAIN_TERMINAL_STATUSES, 'main terminal policy alias changed')
    collector.loaded_check()
    intake.loaded_check()
    return packet.runtime_info()


def runner_sources():
    return collector.runner_sources()


def build(executable):
    return collector.build(executable)


def _prepared(row, case, task, manifest, directory, compiled):
    intake._prepared(row, case, task, manifest, directory, compiled)
    packet.require(all(budget.same(row.get(key), manifest[key]) for key in originals.OPENING_FIELDS)
                   and row['build_info'] == manifest['collector_build_info'], 'prepared actual calibration opening/EXE changed')


def terminal(row, start, finish, events, directory):
    result, status = intake.terminal(row, start, finish, events, directory)
    selected = [event for event in events if event['id'] == row['id']]
    packet.require(all(budget.same(event.get(key), row.get(key)) for event in selected for key in originals.OPENING_FIELDS),
                   'calibration original events/opening identity changed')
    if result:
        packet.require(all(budget.same(result.get(key), row.get(key)) for key in originals.OPENING_FIELDS),
                       'calibration terminal receipt/opening identity changed')
    return result, status


def proof(opening_file, prepared_directories, executable, *, execute_new_oracles=False):
    runtime = loaded_check()
    packet.require(type(execute_new_oracles) is bool, 'explicit local Oracle boolean required')
    paths = [Path(path).absolute() for path in prepared_directories]
    before_trees = originals.development_review.evidence(paths) if paths else []
    directories = [path.resolve(strict=True) for path in paths]
    packet.require(len(set(directories)) == len(directories), 'duplicate calibration preparation directory')
    captured = {str(path): intake.directory_bindings(path) for path in directories}
    pack = originals.proof(opening_file)
    compiled = build(executable)
    own = {row['group_id']: row for row in pack['new_main']}
    packet.require(not execute_new_oracles or all(pack['finished'][key] is not None for key in own),
                   'independent labeling prohibited while main is pending')
    actual = {}
    for directory in directories:
        manifest, _ = collector.validate_manifest(directory / 'collection-manifest.json', pack=pack)
        packet.require(intake.same_path(manifest['collector_executable_file'], executable)
                       and budget.same(manifest['collector_build_info'], compiled), 'collection belongs to another actual calibration EXE')
        rows = intake.lines(directory / 'prepared.jsonl')
        events = intake.lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
        packet.require(len(rows) == len(manifest['cases'])
                       and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'full calibration preparation/event scope required')
        for row, case in zip(rows, manifest['cases'], strict=True):
            task, case_dir = pack['source_pack']['tasks'][case['task_id']], directory / case['id']
            _prepared(row, case, task, manifest, directory, compiled)
            start = own.get(case['group_id'])
            matching = start is not None and intake.same_path(start['started']['case_directory'], case_dir)
            record, status = terminal(row, start if matching else None,
                pack['finished'][case['group_id']] if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate real calibration candidate')
                if status == 'ok':
                    intake.verify(row, case_dir / 'receipt.json', events, executable)
                actual[case['group_id']] = (record, status, case_dir, task)
    packet.require(set(actual) == set(own), 'actual calibration started candidate omitted')
    dev = {row['group_id']: row for row in pack['archive']['dataset']['cases'][:8]}
    outcomes = []
    for case in pack['cohort']['cases']:
        if case['split'] == 'development':
            outcomes.append(dev[case['group_id']])
            continue
        record, status, directory, task = actual.get(case['group_id'], (None, 'main_not_started', None, None))
        row = {**{key: case[key] for key in collection.history_fields()}, 'main_status': status,
               'label': 'unlabeled' if status in ('main_not_started', 'main_not_finished') else 'unknown',
               'reason': status, 'call_eligible': False}
        if record:
            row.update(main_receipt_file=str(directory / 'receipt.json'), main_receipt_sha256=cohort.sha(directory / 'receipt.json'))
        if status == 'ok':
            row.update(intake._oracle(task, record['result']['candidate'], directory, execute_new_oracles))
        outcomes.append(row)
    allowed = {str(record[2] / name) for record in actual.values() if record[1] == 'ok'
               for name in ('stdin-candidate-oracle.json', 'stdin-candidate-oracle.started')} if execute_new_oracles else set()
    final_bindings = []
    for directory in directories:
        after, before = intake.directory_bindings(directory), captured[str(directory)]
        old_paths, new_paths = {row['file'] for row in before}, {row['file'] for row in after}
        packet.require(old_paths <= new_paths and new_paths - old_paths <= allowed, 'calibration directory set changed outside new once-Oracle files')
        cohort.check(before)
        final_bindings.extend(after)
    after_trees = originals.development_review.evidence(directories) if directories else []
    packet.require([row['directory'] for row in before_trees] == [row['directory'] for row in after_trees]
                   and all(before['directories'] == after['directories'] for before, after in zip(before_trees, after_trees, strict=True)),
                   'calibration directory topology changed during intake')
    originals.close(pack)
    packet.require(build(executable) == compiled and loaded_check() == runtime, 'calibration runtime/compiled image changed')
    rows = [row for row in outcomes if row['split'] == 'calibration']
    pending = sum(row['main_status'] == 'main_not_finished' for row in rows)
    terminal_count = sum(main_terminal_status(row['main_status']) for row in rows)
    unresolved = sum(row['reason'] in ('independent_oracle_not_recorded', 'oracle_attempt_incomplete') for row in rows)
    data = cohort.seal({'schema': SCHEMA, 'opening_file': pack['calibration_opening_file'],
        'opening_sha256': pack['calibration_opening_sha256'], 'opening_hash': pack['calibration_opening_hash'],
        'cohort_file': pack['opening']['cohort_file'], 'cohort_sha256': cohort.sha(pack['opening']['cohort_file']),
        'cohort_hash': pack['cohort']['artifact_hash'], 'build_info': compiled,
        'collector_executable_file': str(Path(executable).resolve()), 'prepared_directories': [str(path) for path in directories],
        'cases': outcomes, 'allocated_groups': 808, 'calibration_allocated_groups': 400,
        'calibration_collected_groups': len(own), 'calibration_terminal_groups': terminal_count,
        'calibration_pending_groups': pending, 'calibration_uncollected_groups': 400 - len(own),
        'unresolved_oracle_groups': unresolved, 'labels_ready': terminal_count == 400 and pending == 0 and unresolved == 0,
        'label_counts': dict(Counter(row['label'] for row in rows)), 'collection_bindings': final_bindings,
        'scores_hidden': True, 'jev_probabilities_read': False, 'formal_registration': False, 'holdout_opened': False,
        'production_authorized': False, 'formal_certification': False, 'certified_threshold': None, 'model_calls': 0, 'jev_calls': 0})
    return {'pack': pack, 'dataset': data, 'directories': directories, 'evidence': after_trees}


def collect(opening_file, prepared_directories, executable, *, execute_new_oracles=False):
    return proof(opening_file, prepared_directories, executable,
                 execute_new_oracles=execute_new_oracles)['dataset']
