"""Original full-allocation APPS main intake; no model/Jev or reservation.

One complete source/cohort proof per read-only transaction; every actual start,
raw response/session/candidate and independent Oracle still joined individually.
Default never executes an Oracle. New local execution is exclusive once.
"""
import json
import os
import subprocess
from collections import Counter
from pathlib import Path

from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..collection_receipts import read_lines, verify
from . import development_intake, holdout_collection, intake, packet

ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / 'rust/crates/ncx-core/examples'
SCHEMA = 'buglecat.verification.apps-original-dataset.holdout.v3'


def runner_sources():
    fields = development_intake.runner_sources()
    fields.update(runner_source_sha256=EXAMPLES / 'verification_apps_v3_holdout_collect.rs',
                  support_source_sha256=EXAMPLES / 'verification_apps_v3_holdout_collect/support.rs',
                  collection_engine_source_sha256=EXAMPLES / 'verification_apps_v3_holdout_collect/engine.rs',
                  formal_collection_owner_source_sha256=Path(holdout_collection.formal_collection.__file__),
                  holdout_collection_owner_source_sha256=Path(holdout_collection.__file__),
                  formal_intake_owner_source_sha256=Path(__file__).with_name('formal_intake.py'),
                  holdout_intake_owner_source_sha256=Path(__file__),
                  sampling_proposal_owner_source_sha256=Path(holdout_collection.formal_collection.sampling_proposal.__file__),
                  formal_contract_owner_source_sha256=Path(holdout_collection.formal_collection.formal_contract.__file__),
                  prospective_protocol_owner_source_sha256=Path(holdout_collection.formal_collection.protocol.__file__))
    fields['approved_budget_owner_source_sha256'] = Path(holdout_collection.formal_collection.approved_budget.__file__)
    fields['formal_ledger_source_sha256'] = EXAMPLES / 'verification_apps_v3_formal_collect/ledger.rs'
    fields.update(holdout_source_owner_sha256=Path(holdout_collection.formal_holdout.__file__),
                  calibration_archive_owner_sha256=Path(holdout_collection.formal_archive.__file__))
    return fields


def build(executable):
    executable = Path(executable).resolve(strict=True)
    before = file_hash(executable)
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    child = subprocess.run([str(executable), 'build-info'], capture_output=True, check=True, timeout=10, env=env)
    proof = json.loads(child.stdout)
    require(type(proof['paid_calls']) is int and proof['paid_calls'] == 0
            and type(proof['http_calls']) is int and proof['http_calls'] == 0, 'pure_formal_build_query_required')
    info = proof['build_info']
    require(before == file_hash(executable) == info['executable_sha256']
            and all(info[role] == file_hash(path) for role, path in runner_sources().items()), 'actual_formal_compiled_source_changed')
    return info


def _terminal(row, start, finish, events, case_directory):
    if start is not None:
        fields = (*holdout_collection.BIND_FIELDS, 'id', 'group_id', 'task_id', 'source_kind', 'split', 'task_sha256',
                  'manifest_sha256', 'artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
                  'request_hash', 'settings', 'build_info', 'case_directory', 'workspace', 'session_log',
                  'source_sha256', 'catalog_sha256', 'protocol_sha256', 'authorization_sha256',
                  'main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')
        require(all(holdout_collection._same(start['started'].get(field), row.get(field)) for field in fields),
                'original_pending_or_terminal_start_binding_changed')
    original, status = development_intake.terminal_join(row, start, finish, events, case_directory)
    if original is not None:
        extra = holdout_collection.BIND_FIELDS
        require(all(holdout_collection._same(original.get(field), row[field]) for field in extra)
                and all(holdout_collection._same(event.get(field), row[field]) for event in events if event['id'] == row['id'] for field in extra),
                'original_formal_proposal_terminal_binding_changed')
    return original, status


def _prepared(directory, executable, compiled, pack, starts, finished, marker):
    path = directory / 'collection-manifest.json'
    manifest = holdout_collection.manifest_from_pack(load(path), pack, marker)
    rows = read_lines(directory / 'prepared.jsonl')
    require(len(rows) == len(manifest['cases']) and [r['id'] for r in rows] == [c['id'] for c in manifest['cases']],
            'complete_original_batch_preparation_required')
    events_file = directory / 'attempt-events.jsonl'
    events = read_lines(events_file) if events_file.exists() else []
    require(all(e['id'] in {r['id'] for r in rows} for e in events), 'foreign_formal_batch_event')
    selected = []
    tasks = pack[1]
    for row, case in zip(rows, manifest['cases'], strict=True):
        case_directory = directory / case['id']
        development_intake.prepared_gate(row, case, tasks[case['task_id']], manifest, directory, compiled)
        require(all(holdout_collection._same(row[key], manifest[key]) for key in holdout_collection.BIND_FIELDS)
                and holdout_collection._same(row['settings'], holdout_collection.SETTINGS)
                and holdout_collection._same(row['build_info'], compiled)
                and all(holdout_collection._same(row[key], case[key]) for key in ('id', 'group_id', 'task_id', 'source_kind', 'split', 'public_interface')),
                'prepared_formal_proposal_binding_changed')
        start = starts.get(case['group_id'])
        matching = start is not None and development_intake.same_path(start['started']['case_directory'], case_directory)
        original, status = _terminal(row, start if matching else None, finished.get(case['group_id']) if matching else None, events, case_directory)
        if status == 'ok':
            verify(row, case_directory / 'receipt.json', events, executable)
        selected.append((case, row, original, status, case_directory, matching))
    binding = {'directory': str(directory), 'manifest_sha256': file_hash(path),
               'prepared_sha256': file_hash(directory / 'prepared.jsonl'),
               'events_sha256': file_hash(events_file) if events_file.exists() else None}
    return selected, binding


def _outcome(case, prepared, task, execute):
    original, status, case_directory = (prepared[2], prepared[3], prepared[4]) if prepared else (None, 'main_not_started', None)
    row = {**{key: case[key] for key in ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')},
           'main_status': status, 'label': 'unlabeled' if status in {'main_not_started', 'main_not_finished'} else 'unknown',
           'reason': status, 'call_eligible': False, 'main_receipt_file': str(case_directory / 'receipt.json') if original else None,
           'main_receipt_sha256': file_hash(case_directory / 'receipt.json') if original else None,
           'oracle_file': None, 'oracle_sha256': None, 'candidate_code_sha256': None}
    if status != 'ok':
        return row
    answer = original['result']['candidate']
    path = case_directory / 'apps-holdout-oracle.json'
    extracted = intake.original_code(answer)
    if execute and not path.exists() and extracted and intake.label(task, answer)['reason'] == 'independent_original_oracle_not_recorded':
        intake.create_oracle(path, task, answer)
    record = load(path) if path.exists() else None
    classified = intake.label(task, answer, record)
    row.update({k: classified[k] for k in ('label', 'reason', 'call_eligible')})
    if extracted:
        row['candidate_code_sha256'] = extracted['code_sha256']
    if record:
        row.update(oracle_file=str(path), oracle_sha256=file_hash(path))
    return row


def collect(cohort_file, prepared_directories, executable, *, execute_new_oracles=False, split='holdout', _source_pack=None):
    """Recompute full allocation, including missing preparation/failed/unstarted.

    All actual batch directories must be supplied. An unused preparation can
    recur after an earlier first fault; only one matching real SQLite start can
    supply a candidate. Existing Oracle files are never rewritten or rerun.
    """
    require(split == 'holdout', 'holdout_scope_required')
    require(type(execute_new_oracles) is bool, 'explicit_boolean_oracle_option_required')
    marker, pack = _source_pack if _source_pack is not None else holdout_collection.verified_pack(cohort_file)
    cohort, tasks, _, baseline, finished = pack
    compiled = build(executable)
    directories = [Path(p).resolve(strict=True) for p in prepared_directories]
    require(len(set(directories)) == len(directories), 'duplicate_collection_directory')
    marker_sha = file_hash(cohort_file)
    original_files = {str(p): file_hash(p) for directory in directories for p in directory.rglob('*') if p.is_file()}
    starts = {r['group_id']: r for r in baseline['main_started']}
    fixed = [c for c in cohort['cases'] if c['split'] == split]
    groups = {c['group_id'] for c in fixed}
    own = {g: r for g, r in starts.items() if r['started'].get('artifact_cohort_hash') == cohort['artifact_hash'] and r['started'].get('split') == 'holdout'}
    require(set(own) <= groups, 'foreign_or_holdout_start_in_calibration_scope')
    if execute_new_oracles:
        require(all(finished.get(group) is not None for group in own), 'no_new_oracle_while_main_in_flight')
        require(not groups & {r['group_id'] for r in baseline['jev_reservations']}, 'new_labels_after_jev_reservation_prohibited')
    actual, bindings = {}, []
    for directory in directories:
        prepared, binding = _prepared(directory, executable, compiled, pack, starts, finished, marker)
        bindings.append(binding)
        for item in prepared:
            if item[5]:
                group = item[0]['group_id']
                require(group not in actual, 'duplicate_actual_main_candidate')
                actual[group] = item
    require(set(actual) == set(own), 'original_main_start_or_collection_directory_omitted')
    outcomes = [_outcome(case, actual.get(case['group_id']), tasks[case['task_id']], execute_new_oracles) for case in fixed]
    require(holdout_collection._same(holdout_collection.budget_snapshot(next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization')),
                                    (baseline, finished)), 'shared_ledger_changed_during_intake')
    require(all(file_hash(b['file']) == b['sha256'] for b in cohort['bindings'])
            and file_hash(cohort_file) == marker_sha
            and all(file_hash(b['file']) == b['sha256'] for b in marker['source_bindings'])
            and all(Path(p).is_file() and file_hash(p) == sha for p, sha in original_files.items())
            and build(executable) == compiled and packet.canonical_bytes(packet.runtime_info()) == packet.canonical_bytes(cohort['oracle_runtime_binding']),
            'source_compiled_runtime_changed_during_intake')
    current_files = {str(p) for directory in directories for p in directory.rglob('*') if p.is_file()}
    new_oracles = {r['oracle_file'] for r in outcomes if r['oracle_file'] is not None} - original_files.keys()
    require(current_files - original_files.keys() == new_oracles and (execute_new_oracles or not new_oracles),
            'unexpected_original_holdout_file_created_during_intake')
    body = {'schema': SCHEMA, 'split': split, 'cohort_file': marker['cohort_file'],
            'holdout_source_file': str(Path(cohort_file).resolve()), 'holdout_source_sha256': file_hash(cohort_file),
            'holdout_source_hash': marker['artifact_hash'], 'fixed_threshold': marker['fixed_threshold'],
            'cohort_sha256': marker['cohort_sha256'], 'cohort_hash': cohort['artifact_hash'], 'protocol_file': cohort['protocol_file'],
            'protocol_sha256': cohort['protocol_sha256'], 'oracle_runtime_binding': cohort['oracle_runtime_binding'],
            'collector_executable_file': str(Path(executable).resolve()), 'collector_executable_sha256': compiled['executable_sha256'],
            'build_info': compiled, 'collection_bindings': bindings, 'cases': outcomes, 'allocated_groups': len(fixed),
            'collected_groups': len(own), 'uncollected_groups': len(fixed) - len(own),
            'pending_groups': sum(r['main_status'] == 'main_not_finished' for r in outcomes),
            'label_counts': dict(Counter(r['label'] for r in outcomes)), 'scores_hidden': True, 'jev_probabilities_read': False,
            'formal_registration': False, 'holdout_opened': True, 'formal_certification': False, 'certified_threshold': None,
            'production_authorized': False, 'native_claim_certification': False, 'model_calls': 0, 'jev_calls': 0}
    return {**body, 'artifact_hash': digest(body)}
