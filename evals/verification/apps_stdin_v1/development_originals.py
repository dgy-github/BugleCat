"""Fresh original STDIN main/Oracle proof, independently of later Jev slots.

No projected zero-reservation snapshot, Oracle execution or spending. Sources
are rebuilt once per transaction and original transport helpers join each row.
"""
import sys
from collections import Counter
from pathlib import Path

from . import budget, cohort, collection, development_review, history, intake, packet

SCHEMA = 'buglecat.verification.stdin-development-originals.v1'


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    cohort.loaded_check()
    intake.loaded_check()
    development_review.loaded_check()


def binding_images(chain):
    """Hash all actual reservation bytes/markers; not the short lock itself."""
    paths = []
    for kind in ('main_model', 'jev'):
        root = Path(chain['authorization'][kind]['ledger_dir']).resolve(strict=True)
        packet.require(not root.is_symlink() and not root.is_junction(), 'budget root links forbidden')
        metadata = root / 'authorization.json'
        if metadata.exists():
            paths.append(metadata)
        folder = root / 'reservations'
        if folder.exists():
            packet.require(not folder.is_symlink() and not folder.is_junction(), 'reservation folder link forbidden')
            for child in folder.iterdir():
                packet.require(child.is_file() and not child.is_symlink() and not child.is_junction(), 'reservation node forbidden')
                paths.append(child)
    return budget.raw_bindings(paths)


def close(pack):
    cohort.check(pack['bindings'])
    packet.require(budget.same(development_review.evidence(pack['directories']), pack['evidence'])
                   and budget.same(budget._snapshot_chain(pack['chain']), (pack['current'], pack['finished']))
                   and budget._ledger_rows(Path(pack['current']['main_ledger_file'])) == pack['sql_image']
                   and budget.same(binding_images(pack['chain']), pack['budget_bindings']),
                   'original directory/source/accounting closure changed')
    loaded_check()


def _facts(value, source):
    """Authenticate the original funded cohort facts; never call C3 collect."""
    packet.require(value.get('schema') == cohort.SCHEMA and value.get('collection_authorized') is True
                   and value.get('collection_authorized_split') == 'development'
                   and value.get('before_main_requests') is True and value.get('certified_threshold') is None
                   and type(value.get('new_paid_calls')) is int and value['new_paid_calls'] == 0
                   and all(value.get(key) is False for key in cohort.FALSE_FLAGS), 'original genuine development cohort required')
    packet.require(budget.same(cohort._cases(source), value['cases'])
                   and value['source_proposal_hash'] == source['proposal']['artifact_hash']
                   and value['source_proposal_sha256'] == cohort.sha(value['source_proposal_file'])
                   and value['source_snapshot_sha256'] == cohort.sha(value['source_snapshot_file'])
                   and all(budget.same(value[key], source['proposal'][key]) for key in (
                       'source_authority_file', 'source_authority_sha256', 'main_resource_profile_file',
                       'main_resource_profile_sha256', 'main_resource_profile_hash', 'protocol', 'oracle_runtime_binding'))
                   and value['protocol_sha256'] == budget.digest(value['protocol'])
                   and budget.same(value['main_collection_settings'], cohort.frozen.EXPECTED_SETTINGS)
                   and value['main_system_prompt_sha256'] == source['proposal']['public_system_prompt_sha256'],
                   'original immutable source/profile/protocol/runtime drift')
    chain = cohort._chain(value['authorization_file'], source)
    packet.require(len(chain['bindings']) == 5 and value['authorization_sha256'] == chain['current_sha256']
                   and budget.same(value['ledger_snapshot']['authorization_chain'], chain)
                   and budget.same(cohort._bridge(source, chain, value['ledger_snapshot'], value['original_finished']),
                                   value['approved_budget_bridge'])
                   and budget.same(cohort._funding(value['ledger_snapshot'], value['cases']), value['funded_allocation']),
                   'original approved funding bridge changed')
    expected = {str(path.resolve()): cohort.sha(path) for path in cohort.owners().values()}
    recorded = {row['file']: row['sha256'] for row in value['bindings']}
    packet.require(all(recorded.get(path) == sha for path, sha in expected.items()), 'original complete compiled owner graph omitted')
    return chain


def _main(value, source, current, finished, directories, executable):
    """Direct genuine current main checks, with all actual Jev rows retained."""
    _, new = history._original(value['ledger_snapshot'], value['original_finished'], current, finished)
    selected = history._cases(value['cases'], ('development',))
    history._new_main(new, finished, selected, value['artifact_hash'], value['authorization_sha256'])
    packet.require(len(new) == 8 and all(finished[row['group_id']] is not None for row in new),
                   'whole development main must be terminal before blind labeling/registration')
    compiled = intake.build(executable)
    packet.require(budget.same(compiled, value['collector_build_info'])
                   and cohort.sha(executable) == value['collector_executable_sha256'], 'original actual main binary changed')
    own = {row['group_id']: row for row in new}
    actual = {}
    local = {'cohort': value, 'source_pack': source, 'current': current, 'finished': finished}
    for directory in directories:
        manifest, _ = collection.validate_manifest(directory / 'collection-manifest.json', pack=local)
        rows = intake.lines(directory / 'prepared.jsonl')
        events = intake.lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
        packet.require(len(rows) == len(manifest['cases'])
                       and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'original whole preparation/event scope omitted')
        for row, case in zip(rows, manifest['cases'], strict=True):
            task, case_dir = source['tasks'][case['task_id']], directory / case['id']
            intake._prepared(row, case, task, manifest, directory, compiled)
            start = own.get(case['group_id'])
            matching = start is not None and intake.same_path(start['started']['case_directory'], case_dir)
            original, status = intake.terminal(row, start if matching else None,
                finished[case['group_id']] if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate actual original candidate')
                if status == 'ok':
                    intake.verify(row, case_dir / 'receipt.json', events, executable)
                actual[case['group_id']] = (original, status, case_dir, task)
    packet.require(set(actual) == set(own), 'original started candidate/preparation omitted')
    outcomes = []
    for case in value['cases']:
        original, status, directory, task = actual.get(case['group_id'], (None, 'main_not_started', None, None))
        row = {**{key: case[key] for key in collection.history_fields()}, 'main_status': status,
               'label': 'unlabeled' if status == 'main_not_started' else 'unknown', 'reason': status, 'call_eligible': False}
        if original:
            row.update(main_receipt_file=str(directory / 'receipt.json'), main_receipt_sha256=cohort.sha(directory / 'receipt.json'))
        if status == 'ok':
            row.update(intake._oracle(task, original['result']['candidate'], directory, False))
            packet.require(row['reason'] not in ('independent_oracle_not_recorded', 'oracle_attempt_incomplete'),
                           'original Oracle attempt absent/stranded; never rerun it')
        outcomes.append(row)
    bindings = [row for scope in development_review.evidence(directories) for row in scope['files']]
    return outcomes, bindings, compiled


def proof(dataset_file, prepared_directories, executable):
    """Typed ephemeral proof, never caller-supplied eligibility or cached tasks."""
    loaded_check()
    data = cohort.sealed(budget.load(dataset_file))
    packet.require(data['schema'] == intake.SCHEMA and data['scores_hidden'] is True
                   and data['jev_probabilities_read'] is False and data['certified_threshold'] is None
                   and all(data[key] is False for key in ('formal_registration', 'holdout_opened', 'formal_certification', 'production_authorized')),
                   'original prediction blind C3 dataset required')
    cohort_file = Path(data['cohort_file']).resolve(strict=True)
    value = cohort.sealed(budget.load(cohort_file))
    directories = [Path(path).resolve(strict=True) for path in prepared_directories]
    evidence = development_review.evidence(directories)
    bindings = cohort.source_authority._merge_bindings(value['bindings'], budget.raw_bindings([dataset_file, cohort_file, executable, __file__]),
                                                       *[scope['files'] for scope in evidence])
    source = cohort._source(value['source_proposal_file'], value['source_snapshot_file'])
    chain = _facts(value, source)
    current, finished = budget._snapshot_chain(chain)
    budget_bindings = binding_images(chain)
    sql_image = budget._ledger_rows(Path(current['main_ledger_file']))
    rows, collection_bindings, compiled = _main(value, source, current, finished, directories, executable)
    rebuilt = cohort.seal({'schema': intake.SCHEMA, 'cohort_file': str(cohort_file), 'cohort_hash': value['artifact_hash'],
        'cohort_sha256': cohort.sha(cohort_file), 'build_info': compiled, 'collector_executable_file': str(Path(executable).resolve()),
        'cases': rows, 'allocated_groups': 808, 'collected_groups': 8, 'pending_groups': 0, 'uncollected_groups': 800,
        'label_counts': dict(Counter(row['label'] for row in rows)), 'collection_bindings': collection_bindings,
        'scores_hidden': True, 'jev_probabilities_read': False, 'formal_registration': False, 'holdout_opened': False,
        'production_authorized': False, 'formal_certification': False, 'certified_threshold': None, 'model_calls': 0, 'jev_calls': 0})
    packet.require(budget.same(data, rebuilt), 'saved labels/raw dataset differ from independently reconstructed originals')
    eligible = [row for row in rows[:8] if row['call_eligible']]
    history.validate_prefix(value['ledger_snapshot'], value['original_finished'], current, finished,
                            value['cases'], value['artifact_hash'], jev_cases=eligible)
    requests = []
    for row in eligible:
        task = source['tasks'][row['task_id']]
        code = row['candidate_code_span']['code']
        request = packet.request(task['question'], task['starter_code'], code, task['input_output'], value['oracle_runtime_binding'])
        requests.append({**{key: row[key] for key in development_review.IDENTITY}, 'label': row['label'],
                         'candidate_code_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
                         'request': request, 'request_hash': budget.digest(request), 'request_json': packet.canonical_bytes(request).decode()})
    baseline = value['ledger_snapshot']['jev_reservations']
    new = sorted(current['jev_reservations'], key=lambda row: row['reservation_index'])[len(baseline):]
    packet.require(all(row['request_hash'] == requests[index]['request_hash'] for index, row in enumerate(new)),
                   'actual reserved body differs from authentic original binary request')
    result = {'schema': SCHEMA, 'dataset': data, 'cohort': value, 'source': source, 'chain': chain,
              'current': current, 'finished': finished, 'budget_bindings': budget_bindings,
              'requests': requests, 'new_reservations': new, 'directories': directories,
              'evidence': evidence, 'bindings': bindings, 'sql_image': sql_image, 'dataset_file': str(Path(dataset_file).resolve()),
              'main_executable': str(Path(executable).resolve())}
    close(result)
    return result
