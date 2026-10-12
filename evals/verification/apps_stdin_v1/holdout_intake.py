"""Original holdout main and once-only independent Oracle joins.

Read-only by default, full808 outcomes retained. No Jev reservation, registration,
threshold or holdout permission is provided. Original development is archived
and authenticated, not re-executed or counted toward holdout labels.
"""
import re
import sys
from collections import Counter
from pathlib import Path

from . import budget, cohort, collection, formal_archive, intake, packet
from . import holdout_collection as collector
from . import holdout_originals as originals

SCHEMA = 'buglecat.verification.stdin-original-holdout-dataset.v1'
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


def _prepared(row, case, task, manifest, directory, compiled, *, context=None):
    if context is None:
        intake._prepared(row, case, task, manifest, directory, compiled)
    else:
        # Same frozen prepared/public-request algorithm, with this action's
        # already authenticated policy context. No persistent or caller cache.
        intake.receipt_seal(row)
        case_dir = directory / case['id']
        fields = (*collection.BIND_FIELDS, 'artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
                  'authorization_sha256', 'source_sha256', 'catalog_sha256', 'protocol_sha256',
                  'main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')
        manifest_path = str((directory / 'collection-manifest.json').resolve(strict=True))
        manifest_sha = context['manifest_sha256'][manifest_path] if 'manifest_sha256' in context else cohort.sha(manifest_path)
        packet.require(row['schema'] == 'buglecat.verification.prepared-main.v1'
            and type(row.get('endpoint_sha256')) is str and re.fullmatch('[0-9a-f]{64}', row['endpoint_sha256'])
            and all(budget.same(row.get(key), case[key]) for key in (*collection.history_fields(), 'public_interface'))
            and all(budget.same(row.get(key), manifest[key]) for key in fields)
            and budget.same(row['settings'], cohort.frozen.EXPECTED_SETTINGS) and budget.same(row['build_info'], compiled)
            and row['manifest_sha256'] == manifest_sha
            and intake.same_path(row['case_directory'], case_dir) and intake.same_path(row['workspace'], case_dir / 'workspace')
            and intake.same_path(row['session_log'], case_dir / 'workspace/.nanocodex/sessions' / (case['id'] + '.jsonl'))
            and intake.same_path(row['manifest_file'], directory / 'collection-manifest.json'), 'prepared scope/source/path/build changed')
        prompt = formal_archive._public_task(task, context)['main_prompt']
        body = budget.load(case_dir / 'request.json')
        packet.require(row['prompt'] == prompt and row['prompt_sha256'] == intake.oracle.raw_sha(prompt.encode())
            and body['messages'] == [{'role': 'system', 'content': cohort.artifact_v2_development_cohort.system_prompt()},
                                     {'role': 'user', 'content': prompt}]
            and budget.same(row['body'], body) and row['request_hash'] == cohort.sha(case_dir / 'request.json'),
            'public-only actual request body changed')
    packet.require(all(budget.same(row.get(key), manifest[key]) for key in originals.OPENING_FIELDS)
                   and row['build_info'] == manifest['collector_build_info'], 'prepared actual holdout opening/EXE changed')


def terminal(row, start, finish, events, directory):
    result, status = intake.terminal(row, start, finish, events, directory)
    selected = [event for event in events if event['id'] == row['id']]
    packet.require(all(budget.same(event.get(key), row.get(key)) for event in selected for key in originals.OPENING_FIELDS),
                   'holdout original events/opening identity changed')
    if result:
        packet.require(all(budget.same(result.get(key), row.get(key)) for key in originals.OPENING_FIELDS),
                       'holdout terminal receipt/opening identity changed')
    return result, status


def _proof(opening_file, prepared_directories, executable, *, execute_new_oracles=False, pack=None):
    runtime = loaded_check()
    packet.require(type(execute_new_oracles) is bool, 'explicit local Oracle boolean required')
    paths = [Path(path).absolute() for path in prepared_directories]
    before_trees = originals.development_review.evidence(paths) if paths else []
    directories = [path.resolve(strict=True) for path in paths]
    packet.require(len(set(directories)) == len(directories), 'duplicate holdout preparation directory')
    captured = {str(path): intake.directory_bindings(path) for path in directories}
    pack = originals.proof(opening_file) if pack is None else pack
    compiled = build(executable)
    context = formal_archive.packet_context(pack['cohort']['oracle_runtime_binding'])
    context['compiled_bindings'] = budget.raw_bindings([*collector.runner_sources().values(), executable])
    image = pack.get('holdout_manifest_images')
    if image is None or set(image['manifests']) != {str(path) for path in directories}:
        image = originals.manifest_images(directories)
    context['manifest_sha256'] = image['sha256']
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], image['bindings'])
    own = {row['group_id']: row for row in pack['new_main']}
    packet.require(not execute_new_oracles or all(pack['finished'][key] is not None for key in own),
                   'independent labeling prohibited while main is pending')
    actual = {}
    for directory in directories:
        manifest, _ = collector._validate_manifest(directory / 'collection-manifest.json', pack=pack, context=context, compiled=compiled,
                                                   image=image)
        packet.require(intake.same_path(manifest['collector_executable_file'], executable)
                       and budget.same(manifest['collector_build_info'], compiled), 'collection belongs to another actual holdout EXE')
        rows = intake.lines(directory / 'prepared.jsonl')
        events = intake.lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
        packet.require(len(rows) == len(manifest['cases'])
                       and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'full holdout preparation/event scope required')
        for row, case in zip(rows, manifest['cases'], strict=True):
            task, case_dir = pack['source_pack']['tasks'][case['task_id']], directory / case['id']
            _prepared(row, case, task, manifest, directory, compiled, context=context)
            start = own.get(case['group_id'])
            matching = start is not None and intake.same_path(start['started']['case_directory'], case_dir)
            record, status = terminal(row, start if matching else None,
                pack['finished'][case['group_id']] if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate real holdout candidate')
                if status == 'ok':
                    intake.verify(row, case_dir / 'receipt.json', events, executable)
                actual[case['group_id']] = (record, status, case_dir, task)
    packet.require(set(actual) == set(own), 'actual holdout started candidate omitted')
    # Close the authenticated archive before the only permitted local writes.
    # Below, a successful execute=True may exclusively append two known Oracle
    # files. It may not change any prior bytes, directories or accounting.
    originals.close(pack)
    historical = {row['group_id']: row for row in pack['calibration_dataset']['cases'][:408]}
    outcomes = []
    for case in pack['cohort']['cases']:
        if case['split'] != 'holdout':
            outcomes.append(historical[case['group_id']])
            continue
        record, status, directory, task = actual.get(case['group_id'], (None, 'main_not_started', None, None))
        row = {**{key: case[key] for key in collection.history_fields()}, 'main_status': status,
               'label': 'unlabeled' if status in ('main_not_started', 'main_not_finished') else 'unknown',
               'reason': status, 'call_eligible': False}
        if record:
            row.update(main_receipt_file=str(directory / 'receipt.json'), main_receipt_sha256=cohort.sha(directory / 'receipt.json'))
        if status == 'ok':
            if execute_new_oracles:
                row.update(intake._oracle(task, record['result']['candidate'], directory, True))
            else:
                row.update(formal_archive._oracle(task, record['result']['candidate'], directory, context))
        outcomes.append(row)
    allowed = {str(record[2] / name) for record in actual.values() if record[1] == 'ok'
               for name in ('stdin-candidate-oracle.json', 'stdin-candidate-oracle.started')} if execute_new_oracles else set()
    final_bindings = []
    for directory in directories:
        after, before = intake.directory_bindings(directory), captured[str(directory)]
        old_paths, new_paths = {row['file'] for row in before}, {row['file'] for row in after}
        packet.require(old_paths <= new_paths and new_paths - old_paths <= allowed, 'holdout directory set changed outside new once-Oracle files')
        cohort.check(before)
        final_bindings.extend(after)
    after_trees = originals.development_review.evidence(directories) if directories else []
    packet.require([row['directory'] for row in before_trees] == [row['directory'] for row in after_trees]
                   and all(before['directories'] == after['directories'] for before, after in zip(before_trees, after_trees, strict=True)),
                   'holdout directory topology changed during intake')
    if execute_new_oracles:
        for index, (old_dirs, _) in enumerate(pack['directory_images']):
            if {str(Path(path).resolve()) for path in old_dirs} == {str(path) for path in directories}:
                pack['directory_images'][index] = ([str(path) for path in directories], after_trees)
        pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], final_bindings)
    originals.close(pack)
    formal_archive.close_context(context)
    originals.close_manifest_images(image)
    packet.require(build(executable) == compiled and loaded_check() == runtime, 'holdout runtime/compiled image changed')
    rows = [row for row in outcomes if row['split'] == 'holdout']
    pending = sum(row['main_status'] == 'main_not_finished' for row in rows)
    terminal_count = sum(main_terminal_status(row['main_status']) for row in rows)
    unresolved = sum(row['reason'] in ('independent_oracle_not_recorded', 'oracle_attempt_incomplete') for row in rows)
    data = cohort.seal({'schema': SCHEMA, 'opening_file': pack['holdout_opening_file'],
        'opening_sha256': pack['holdout_opening_sha256'], 'opening_hash': pack['holdout_opening_hash'],
        'qualified_calibration_threshold': pack['opening']['qualified_calibration_threshold'],
        'cohort_file': pack['opening']['cohort_file'], 'cohort_sha256': cohort.sha(pack['opening']['cohort_file']),
        'cohort_hash': pack['cohort']['artifact_hash'], 'build_info': compiled,
        'collector_executable_file': str(Path(executable).resolve()), 'prepared_directories': [str(path) for path in directories],
        'cases': outcomes, 'allocated_groups': 808, 'holdout_allocated_groups': 400,
        'holdout_collected_groups': len(own), 'holdout_terminal_groups': terminal_count,
        'holdout_pending_groups': pending, 'holdout_uncollected_groups': 400 - len(own),
        'unresolved_oracle_groups': unresolved, 'labels_ready': terminal_count == 400 and pending == 0 and unresolved == 0,
        'label_counts': dict(Counter(row['label'] for row in rows)), 'collection_bindings': final_bindings,
        'holdout_scores_hidden': True, 'holdout_jev_probabilities_read': False, 'formal_registration': False, 'holdout_opened': True,
        'production_authorized': False, 'formal_certification': False, 'native_claim_certification': False,
        'overall_agent_benefit_certification': False, 'certified_threshold': None, 'model_calls': 0, 'jev_calls': 0})
    return {'pack': pack, 'dataset': data, 'directories': directories, 'evidence': after_trees}


def collect(opening_file, prepared_directories, executable, *, execute_new_oracles=False):
    return proof(opening_file, prepared_directories, executable,
                 execute_new_oracles=execute_new_oracles)['dataset']


def proof(opening_file, prepared_directories, executable, *, execute_new_oracles=False):
    """Public fresh main-only proof; never accepts a caller ephemeral pack."""
    return _proof(opening_file, prepared_directories, executable, execute_new_oracles=execute_new_oracles)
