"""Authenticate every original APPS development outcome, then local labels.

No transport or budget write. Never read Jev probabilities. Failed, pending,
unstarted and Unknown outcomes retain different states. Label execution is an
explicit local option; existing Oracle files are checked and never rerun.
"""
import json
import os
import subprocess
from collections import Counter
from pathlib import Path

from ..annotation_audit import within_root
from ..artifact_v2_development_cohort import snapshot, system_prompt
from ..collection_receipts import read_lines, seal_check, verify
from . import intake, packet, source_authority
from .source_authority import load, require

ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / 'rust/crates/ncx-core/examples'
SCHEMA = 'buglecat.verification.apps-development-original-dataset.v3'


def runner_sources():
    folder = Path(__file__).resolve().parent
    return {'runner_source_sha256': EXAMPLES / 'verification_apps_v3_collect.rs',
            'support_source_sha256': EXAMPLES / 'verification_apps_v3_collect/support.rs',
            'collection_engine_source_sha256': EXAMPLES / 'verification_apps_v3_collect/engine.rs',
            'frozen_collector_support_sha256': EXAMPLES / 'verification_artifact_collect/support.rs',
            'profile_collector_validator_source_sha256': EXAMPLES / 'verification_artifact_profile_collect/support.rs',
            'legacy_support_source_sha256': EXAMPLES / 'verification_collect/support.rs',
            'request_builder_source_sha256': ROOT / 'rust/crates/ncx-provider/src/request.rs',
            'response_parser_source_sha256': ROOT / 'rust/crates/ncx-provider/src/response.rs',
            'development_owner_source_sha256': folder / 'development.py',
            'budget_v2_owner_source_sha256': folder.parent / 'budget_authorization_v2.py',
            'shared_ledger_source_sha256': EXAMPLES / 'verification_artifact_v2_collect/support.rs',
            'shared_development_owner_source_sha256': folder.parent / 'artifact_v2_development_cohort.py',
            'packet_owner_source_sha256': folder / 'packet.py', 'resource_owner_source_sha256': folder / 'resources.py',
            'admission_guard_source_sha256': folder / 'admission_guard.py',
            'authority_owner_source_sha256': folder / 'source_authority.py',
            'protocol_owner_source_sha256': folder / 'protocol.py'}


def build(executable):
    executable = Path(executable).resolve(strict=True)
    actual = source_authority.source.file_sha(executable)
    environment = {key: os.environ[key] for key in ('SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP') if key in os.environ}
    child = subprocess.run([str(executable), 'build-info'], capture_output=True, text=True, check=True,
                           timeout=10, env=environment)
    proof = json.loads(child.stdout)
    require(type(proof.get('paid_calls')) is int and proof['paid_calls'] == 0
            and type(proof.get('http_calls')) is int and proof['http_calls'] == 0, 'pure_build_query_required')
    facts = proof['build_info']
    require(source_authority.source.file_sha(executable) == actual == facts['executable_sha256']
            and all(facts[role] == source_authority.source.file_sha(path) for role, path in runner_sources().items()),
            'actual_compiled_APPS_sources_changed')
    return facts


def same_path(left, right):
    return within_root(left, right) and within_root(right, left)


def prepared_gate(row, case, task, manifest, directory, compiled):
    from .development import BIND_FIELDS, SETTINGS
    seal_check(row)
    case_dir = Path(directory) / case['id']
    require(row['schema'] == 'buglecat.verification.prepared-main.v1'
            and all(row[key] == case[key] for key in ('id', 'group_id', 'task_id', 'source_kind', 'split', 'public_interface'))
            and row['task_sha256'] == case['task_sha256'] == intake.oracle.digest(task)
            and row['settings'] == SETTINGS and row['build_info'] == compiled,
            'original_development_preparation_changed')
    require(same_path(row['case_directory'], case_dir) and within_root(row['workspace'], case_dir)
            and within_root(row['session_log'], row['workspace'])
            and same_path(row['manifest_file'], Path(directory) / 'collection-manifest.json'),
            'original_development_paths_changed')
    fields = (*BIND_FIELDS, 'artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
              'authorization_sha256', 'source_sha256', 'catalog_sha256', 'protocol_sha256',
              'main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')
    require(all(row[key] == manifest[key] for key in fields)
            and row['manifest_sha256'] == source_authority.source.file_sha(Path(directory) / 'collection-manifest.json'),
            'original_development_manifest_binding_changed')
    prompt = packet.main_prompt(task['question'], task['starter_code'], task['input_output']['fn_name'], task['source_kind'])
    request_path = case_dir / 'request.json'
    require(row['prompt'] == prompt and row['prompt_sha256'] == intake.oracle.raw_sha(prompt.encode())
            and row['body']['messages'] == [{'role': 'system', 'content': system_prompt()},
                                           {'role': 'user', 'content': prompt}]
            and row['request_hash'] == source_authority.source.file_sha(request_path)
            and packet.canonical_bytes(load(request_path)) == packet.canonical_bytes(row['body']),
            'original_public_only_request_changed')


def terminal_join(row, started, finished, events, directory):
    """Strict source/SQLite/events/receipt join includes transport failures."""
    path = Path(directory) / 'receipt.json'
    selected = [event for event in events if event['id'] == row['id']]
    if started is None:
        require(not path.exists() and not selected and finished is None, 'proof_without_original_main_start')
        return None, 'main_not_started'
    require(selected and started['started'] == {key: value for key, value in selected[0].items() if key != 'attempt_ordinal'}
            and type(selected[0]['attempt_ordinal']) is int
            and selected[0]['attempt_ordinal'] == started['ordinal'], 'original_SQLite_start_changed')
    if not path.exists():
        require(finished is None, 'finish_without_original_receipt')
        return None, 'main_not_finished'
    receipt = load(path)
    seal_check(receipt)
    require(receipt['schema'] == 'buglecat.verification.collected-main.v1'
            and [event['event'] for event in selected] == ['attempt_started', 'request_submitted', 'attempt_finished']
            and selected[-1] == finished and selected[-1]['receipt_sha256'] == source_authority.source.file_sha(path),
            'original_SQLite_terminal_changed')
    from .development import BIND_FIELDS
    fields = (*BIND_FIELDS, 'id', 'group_id', 'task_id', 'source_kind', 'split', 'task_sha256',
              'manifest_sha256', 'artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
              'request_hash', 'settings', 'build_info', 'source_sha256', 'catalog_sha256',
              'protocol_sha256', 'authorization_sha256', 'case_directory', 'workspace', 'session_log')
    require(all(event.get(key) == row.get(key) for event in selected for key in fields)
            and all(receipt.get(key) == row.get(key) for key in fields)
            and all(type(event['attempt_ordinal']) is int and event['attempt_ordinal'] == started['ordinal']
                    for event in selected)
            and type(receipt['attempt_ordinal']) is int and receipt['attempt_ordinal'] == started['ordinal']
            and selected[1]['actual_request_hash'] == receipt['actual_request_hash'] == row['request_hash']
            and selected[-1]['call_status'] == receipt['call_status'], 'original_terminal_identity_changed')
    if receipt.get('response_sha256'):
        require(source_authority.source.file_sha(Path(directory) / 'raw-response.json') == receipt['response_sha256'],
                'original_failed_response_bytes_changed')
    return receipt, receipt['call_status']


def collect(directory, executable, *, execute_new_oracles=False):
    """Return every original allocated group; Jev registration remains separate."""
    from . import development
    directory = Path(directory).resolve(strict=True)
    executable = Path(executable).resolve(strict=True)
    compiled = build(executable)
    manifest = development.validate_manifest(directory / 'collection-manifest.json')
    cohort = development.validate_cohort(manifest['artifact_cohort_file'])
    authority, _ = development.inputs(manifest['source_authority_file'], manifest['resource_inventory_file'])
    tasks, _ = development.original_tasks(authority, cohort['cases'])
    rows = read_lines(directory / 'prepared.jsonl')
    require(len(rows) == len(manifest['cases'])
            and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']],
            'complete_development_preparation_required')
    events = read_lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
    require(all(event['id'] in {row['id'] for row in rows} for event in events), 'foreign_development_event')
    baseline, finished = snapshot(manifest['authorization_file'])
    starts = {row['group_id']: row for row in baseline['main_started']}
    outcomes = []
    for row, case in zip(rows, manifest['cases']):
        task, case_dir = tasks[case['task_id']], directory / case['id']
        prepared_gate(row, case, task, manifest, directory, compiled)
        original_start = starts.get(case['group_id'])
        receipt, status = terminal_join(row, original_start, finished.get(case['group_id']), events, case_dir)
        outcome = {'id': case['id'], 'group_id': case['group_id'], 'task_id': case['task_id'],
                   'source_kind': case['source_kind'], 'split': 'development', 'main_status': status,
                   'label': 'unlabeled' if original_start is None else 'unknown', 'reason': status,
                   'call_eligible': False, 'main_receipt_file': str(case_dir / 'receipt.json'),
                   'main_receipt_sha256': source_authority.source.file_sha(case_dir / 'receipt.json') if receipt else None,
                   'oracle_file': None, 'oracle_sha256': None, 'candidate_code_sha256': None}
        if status == 'ok':
            verify(row, case_dir / 'receipt.json', events, executable)
            answer = receipt['result']['candidate']
            extracted = intake.original_code(answer)
            path = case_dir / 'apps-development-oracle.json'
            if extracted is not None and not path.exists() and execute_new_oracles:
                # Reject a resource Unknown before creating an Oracle filename.
                if intake.label(task, answer)['reason'] == 'independent_original_oracle_not_recorded':
                    intake.create_oracle(path, task, answer)
            record = load(path) if path.exists() else None
            classified = intake.label(task, answer, record)
            outcome.update({key: classified[key] for key in ('label', 'reason', 'call_eligible')})
            if extracted:
                outcome['candidate_code_sha256'] = extracted['code_sha256']
            if record:
                outcome.update(oracle_file=str(path), oracle_sha256=source_authority.source.file_sha(path))
        outcomes.append(outcome)
    own_starts = {row['started']['id'] for row in baseline['main_started']
                  if row['started'].get('artifact_cohort_hash') == cohort['artifact_hash']}
    require(own_starts <= {row['id'] for row in outcomes}, 'original_started_group_omitted')
    development.validate_cohort(manifest['artifact_cohort_file'])
    require(build(executable) == compiled, 'collector_changed_during_independent_labels')
    body = {'schema': SCHEMA, 'split': 'development', 'cohort_hash': cohort['artifact_hash'],
            'collection_directory': str(directory), 'collector_executable_sha256': compiled['executable_sha256'],
            'main_manifest_sha256': source_authority.source.file_sha(directory / 'collection-manifest.json'),
            'prepared_sha256': source_authority.source.file_sha(directory / 'prepared.jsonl'),
            'cases': outcomes, 'allocated_groups': len(outcomes), 'started_groups': len(own_starts),
            'label_counts': dict(Counter(row['label'] for row in outcomes)),
            'scores_hidden': True, 'jev_probabilities_read': False,
            'whole_development_allocation_excluded_from_formal': True,
            'formal_registration': False, 'formal_certification': False, 'production_authorized': False,
            'model_calls': 0, 'jev_calls': 0}
    return {**body, 'artifact_hash': intake.oracle.digest(body)}
