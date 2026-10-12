"""Continue the explicit self-made Temp archive; never actual APPS authority.

Original400 calibration/200 score files and path-bound markers stay untouched.
Only new Temp main rows/directories and subsequent holdout reservations append.
No production entry imports this fixture or accepts its source substitutions.
"""
import json
import sqlite3
from pathlib import Path

from evals.verification import collection_receipts
from evals.verification.apps_stdin_v1 import (
    budget,
    cohort,
    formal_archive,
    holdout_collection,
    holdout_jev,
    holdout_observations,
    holdout_originals,
    holdout_transport,
    intake,
    packet,
)
from tests.stdin_collection_fixtures import write
from tests.stdin_formal_calibration_fixtures import restore_synthetic

ROOT = Path(__file__).resolve().parents[1]
MAIN_EXE = ROOT / 'rust/target/debug/examples/verification_apps_stdin_v1_holdout_collect.exe'
JEV_EXE = ROOT / 'rust/target/debug/examples/verification_apps_stdin_v1_jev_holdout.exe'
REAL_VERIFY = collection_receipts.verify


def restore(root, monkeypatch):
    fixture = restore_synthetic(root, monkeypatch)
    # The immutable old self-made calibration archive lacks the generic
    # session/result fields. Retain that explicit historical fixture boundary;
    # every new holdout success uses the genuine independent byte join.
    monkeypatch.setattr(intake, 'verify', lambda row, *args: REAL_VERIFY(row, *args)
                        if row['split'] == 'holdout' else None)
    marker = next((Path(root) / 'fixture-main/stdin-holdout-openings').glob('*.json'))
    fixture.update(holdout_opening=marker)
    return fixture


def prepare_whole_holdout(fixture):
    """400 new exact synthetic raw rows, two self-made once-Oracle templates.

    The full public production readers independently replay every byte; the
    fixture does not supply an ephemeral proof to any production action.
    """
    pack = holdout_originals.proof(fixture['holdout_opening'])
    compiled = holdout_collection.build(MAIN_EXE)
    graph, runtime = packet.dependencies(), packet.runtime_info()
    context = formal_archive.packet_context(runtime)
    context['compiled_bindings'] = budget.raw_bindings([*holdout_collection.runner_sources().values(), MAIN_EXE])
    templates = {code: intake.oracle.execute(code, {'inputs': ['7\n'], 'outputs': ['7']})
                 for code in ('print(input())\n', 'print("wrong")\n')}
    base = fixture['path'].parent
    directories = []
    with sqlite3.connect(fixture['database']) as db:
        for offset in range(0, 400, 8):
            directory = base / ('synthetic-holdout-' + str(offset))
            directory.mkdir()
            directories.append(directory)
            manifest = holdout_collection._manifest(pack, MAIN_EXE, pack['opening']['cases'][offset:offset + 8], compiled, context=context)
            manifest_file = directory / 'collection-manifest.json'
            write(manifest_file, manifest)
            rows, events = [], []
            for index, case in enumerate(manifest['cases'], offset):
                task = fixture['pack']['tasks'][case['task_id']]
                code = 'print(input())\n' if 1 <= index <= 150 else 'print("wrong")\n' if 151 <= index <= 200 else None
                answer = '```python\n' + code + '```\nSynthetic uncertainty.' if code else 'No artifact; synthetic Unknown.'
                case_dir = directory / case['id']
                workspace = case_dir / 'workspace'
                workspace.mkdir(parents=True)
                prompt = formal_archive._public_task(task, context)['main_prompt']
                body = {'messages': [{'role': 'system', 'content': cohort.artifact_v2_development_cohort.system_prompt()},
                                     {'role': 'user', 'content': prompt}], 'model': 'deepseek-flash'}
                write(case_dir / 'request.json', body)
                row = {**case, 'schema': 'buglecat.verification.prepared-main.v1',
                    **{key: manifest[key] for key in holdout_collection.BIND_FIELDS},
                    **{key: manifest[key] for key in ('artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
                        'authorization_file', 'authorization_sha256', 'source_sha256', 'catalog_sha256', 'protocol_sha256',
                        'main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')},
                    'settings': cohort.frozen.EXPECTED_SETTINGS, 'build_info': compiled,
                    'manifest_file': str(manifest_file), 'manifest_sha256': cohort.sha(manifest_file),
                    'case_directory': str(case_dir), 'workspace': str(workspace),
                    'session_log': str(workspace / '.nanocodex/sessions' / (case['id'] + '.jsonl')),
                    'endpoint_sha256': 'e' * 64, 'prompt': prompt, 'prompt_sha256': intake.oracle.raw_sha(prompt.encode()),
                    'body': body, 'request_hash': cohort.sha(case_dir / 'request.json')}
                row['artifact_sha256'] = budget.digest(row)
                write(case_dir / 'prepared-row.json', row)
                rows.append(row)
                start = {**row, 'event': 'attempt_started'}
                db.execute('INSERT INTO attempts(group_id,split,started_json) VALUES(?,?,?)',
                           (case['group_id'], 'holdout', json.dumps(start)))
                ordinal = db.execute('SELECT max(ordinal) FROM attempts').fetchone()[0]
                started = {**start, 'attempt_ordinal': ordinal}
                status = 'http_400' if index == 0 else 'ok'
                raw = {'model': 'deepseek-flash', 'choices': [{'message': {'content': answer}}],
                       'usage': {'prompt_tokens': 1, 'completion_tokens': 2}} if status == 'ok' else {'error': 'synthetic-private-first-fault'}
                write(case_dir / 'raw-response.json', raw)
                receipt = {**row, 'schema': 'buglecat.verification.collected-main.v1', 'attempt_ordinal': ordinal,
                    'call_status': status, 'actual_request_hash': row['request_hash'], 'http_status': 400 if index == 0 else 200,
                    'response_sha256': cohort.sha(case_dir / 'raw-response.json')}
                if status == 'ok':
                    (case_dir / 'candidate.txt').write_bytes(answer.encode())
                    log = Path(row['session_log'])
                    log.parent.mkdir(parents=True)
                    log.write_bytes(b'\n'.join(packet.canonical_bytes(message) for message in [
                        {'role': 'user', 'content': prompt}, {'role': 'assistant', 'content': answer}]) + b'\n')
                    sources = log.with_suffix('.sources.jsonl')
                    sources.write_bytes(packet.canonical_bytes({'fixture_only': True, 'candidate_sha256': intake.oracle.raw_sha(answer.encode())}) + b'\n')
                    # This self-made packet is a synthetic input, not a native
                    # model/claim certificate. Actual native packet creation is
                    # separately tested by the offline compiled tool fixtures.
                    write(case_dir / 'packet.json', {'candidate': answer, 'claims': [], 'fixture_only': True})
                    receipt['result'] = {'candidate': answer, 'candidate_sha256': intake.oracle.raw_sha(answer.encode()),
                        'response_sha256': receipt['response_sha256'], 'confirmed_model': raw['model'], 'usage': raw['usage'],
                        'session_log': str(log), 'session_sha256': cohort.sha(log), 'sources_sha256': cohort.sha(sources),
                        'packet_sha256': cohort.sha(case_dir / 'packet.json'), 'claims': [],
                        'host_B': {'policy_version': compiled['policy_version']}, 'agent_loop_executed': False,
                        'assertions_executed': False, 'tools_executed': 0}
                receipt['artifact_sha256'] = budget.digest({key: child for key, child in receipt.items() if key != 'artifact_sha256'})
                write(case_dir / 'receipt.json', receipt)
                submitted = {**started, 'event': 'request_submitted', 'actual_request_hash': row['request_hash']}
                finish = {**submitted, 'event': 'attempt_finished', 'call_status': status, 'http_status': receipt['http_status'],
                    'receipt_sha256': cohort.sha(case_dir / 'receipt.json'), 'response_sha256': receipt['response_sha256'],
                    'global_main_attempt': ordinal}
                db.execute('UPDATE attempts SET finished_json=? WHERE ordinal=?', (json.dumps(finish), ordinal))
                events += [started, submitted, finish]
                if code:
                    write(case_dir / 'stdin-candidate-oracle.started', {'candidate_code_sha256': intake.oracle.raw_sha(code.encode()),
                        'task_sha256': budget.digest(task), 'input_output_sha256': budget.digest(task['input_output']),
                        'runtime': runtime, 'source_graph': graph})
                    write(case_dir / 'stdin-candidate-oracle.json', templates[code])
            (directory / 'prepared.jsonl').write_bytes(b'\n'.join(packet.canonical_bytes(row) for row in rows) + b'\n')
            (directory / 'attempt-events.jsonl').write_bytes(b'\n'.join(packet.canonical_bytes(row) for row in events) + b'\n')
    formal_archive.close_context(context)
    return directories


def finish_holdout(row, prepared_file, reservation_file, *, status='ok', signal=.1):
    directory = Path(row['output_directory'])
    original = holdout_observations.identity(row, prepared_file, budget.load(reservation_file), reservation_file)
    (directory / 'request.json').write_bytes(row['request_json'].encode())
    raw = {'model': 'jev-1.13.0', 'answers': {'acceptance_violation': {'type': 'noul', 'noul': signal}},
           'usage': {'input_tokens': 12, 'output_tokens': 3}}
    raw_file = directory / 'raw-response.json'
    write(raw_file, raw if status == 'ok' else {'error': 'synthetic-private-fault'})
    terminal = {**original, 'call_status': status, 'http_status': 200 if status == 'ok' else int(status[5:]),
        'raw_response_file': str(raw_file), 'response_sha256': cohort.sha(raw_file), 'response': raw if status == 'ok' else None}
    write(directory / 'receipt.json', terminal)
    events = [{**original, 'event': 'attempt_started'},
              {**original, 'event': 'request_submitted', 'actual_request_hash': row['request_hash']},
              {**terminal, 'event': 'attempt_finished', 'receipt_sha256': cohort.sha(directory / 'receipt.json')}]
    (directory / 'events.jsonl').write_bytes(b'\n'.join(packet.canonical_bytes(event) for event in events) + b'\n')
    return directory


def complete_scores(fixture, registration):
    """Actual first reserve/claim; all200 original synthetic raw slots joined."""
    from evals.verification.paid_budget import durable_new
    prepared = holdout_jev.prepare(registration, JEV_EXE)
    directories = []
    root = Path(fixture['chain']['authorization']['jev']['ledger_dir'])
    for index, item in enumerate(prepared):
        path = Path(item['file'])
        row = cohort.sealed(budget.load(path))
        reserved_file = root / 'reservations' / (budget.digest(row['group_id']) + '.json')
        if index == 0:
            row, pack = holdout_jev.validate(path)
            slot = holdout_transport._reserve(path, row, pack)
            row, pack = holdout_jev.validate(path)
            holdout_transport._claim(path, row, pack, slot['reservation_file'])
        else:
            slot = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
                'group_id': row['group_id'], 'request_hash': row['request_hash'],
                'authorization_sha256': fixture['chain']['current_sha256'], 'reservation_index': 209 + index,
                'status': 'reserved_before_transport', 'automatic_retries': 0,
                'registration_file': row['registration_file'], 'registration_sha256': row['registration_sha256'],
                'registration_hash': row['registration_hash'], 'prepared_file': str(path.resolve()),
                'prepared_sha256': cohort.sha(path), 'sender_executable_sha256': row['sender_build_info']['runner_exe_sha256'],
                'output_directory': row['output_directory'], 'endpoint_sha256': row['endpoint_sha256']}
            durable_new(reserved_file, slot)
            Path(row['output_directory']).mkdir()
            durable_new(reserved_file.with_suffix('.started'), {
                'schema': holdout_transport.SCHEMA, 'group_id': row['group_id'], 'request_hash': row['request_hash'],
                'reservation_file': str(reserved_file.resolve()), 'reservation_sha256': cohort.sha(reserved_file),
                'reservation_index': slot['reservation_index'], 'prepared_file': str(path.resolve()),
                'prepared_sha256': cohort.sha(path), 'output_directory': row['output_directory'], 'new_paid_calls': 0})
        directories.append(finish_holdout(row, path, reserved_file, status='http_400' if index == 0 else 'ok',
                                        signal=.9 if row['label'] == 'unsupported' else .1))
    return directories
