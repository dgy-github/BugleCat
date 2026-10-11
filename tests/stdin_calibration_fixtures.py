"""Self-made temporary accounting/evidence; never actual source authority."""
import json
import sqlite3
from pathlib import Path

from evals.verification.apps_stdin_v1 import (
    budget,
    calibration_collection,
    calibration_opening,
    cohort,
    intake,
    protocol,
)
from evals.verification.apps_stdin_v1 import calibration_originals as originals
from evals.verification.apps_stdin_v1 import development_jev as sender
from evals.verification.apps_stdin_v1 import development_registration as registration
from evals.verification.apps_stdin_v1 import development_transport as transport
from tests.stdin_collection_fixtures import write
from tests.stdin_development_jev_fixtures import finish_synthetic
from tests.test_verification_apps_stdin_v1_development_registration import original_fixture

ROOT = Path(__file__).resolve().parents[1]
DEV_EXE = ROOT / 'rust/target/debug/examples/verification_apps_stdin_v1_jev_development.exe'
CAL_EXE = ROOT / 'rust/target/debug/examples/verification_apps_stdin_v1_calibration_collect.exe'
REAL_TERMINAL = intake.terminal
REAL_PREPARED = intake._prepared


def opened_fixture(tmp_path, monkeypatch):
    genuine_validate = protocol.validate
    fixture = original_fixture(tmp_path, monkeypatch)
    development_terminal, development_prepared = intake.terminal, intake._prepared
    monkeypatch.setattr(intake, 'terminal', lambda *args: development_terminal(*args) if args[0]['split'] == 'development' else REAL_TERMINAL(*args))
    monkeypatch.setattr(intake, '_prepared', lambda *args: development_prepared(*args) if args[0]['split'] == 'development' else REAL_PREPARED(*args))
    monkeypatch.setattr(protocol, 'validate', genuine_validate)
    monkeypatch.setattr(calibration_opening, 'loaded_check', lambda: None)
    monkeypatch.setattr(calibration_opening.acceptance, 'loaded_check', lambda: None)
    monkeypatch.setattr(originals, 'loaded_check', lambda: None)
    registered = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    prepared = sender.prepare(registered['file'], DEV_EXE)
    directories = []
    for index, item in enumerate(prepared):
        row, pack = sender.validate(item['file'])
        slot = transport._reserve(item['file'], row, pack)
        _, pack = sender.validate(item['file'])
        transport._claim(item['file'], row, pack, slot['reservation_file'])
        directories.append(finish_synthetic(row, item['file'], slot['reservation_file'], status='http_400' if index == 0 else 'ok'))
    review = calibration_opening.acceptance.review(registered['file'], directories, DEV_EXE)
    review_file = tmp_path / 'synthetic-complete-development-review.json'
    write(review_file, review)
    opened = calibration_opening.open_calibration(review_file)
    fixture.update(opening=opened, registered=registered, sender_directories=directories, review_file=review_file)
    return fixture


def reseal(path, changes):
    value = budget.load(path)
    value.update(changes)
    value.pop('artifact_hash')
    return write(path, originals.cohort.seal(value))


def add_calibration_record(fixture, directory, *, status='ok', pending=False, answer='```python\nprint(input())\n```\nUncertain assessment.'):
    """Exact temp prepared/events/raw/SQL joins; no actual HTTP or source API."""
    directory.mkdir()
    manifest = calibration_collection.manifest_value(fixture['opening']['file'], CAL_EXE, 1)
    write(directory / 'collection-manifest.json', manifest)
    case = manifest['cases'][0]
    task = fixture['pack']['tasks'][case['task_id']]
    case_dir = directory / case['id']
    workspace = case_dir / 'workspace'
    workspace.mkdir(parents=True)
    prompt = calibration_collection.collection.public_task(task)['main_prompt']
    body = {'messages': [{'role': 'system', 'content': cohort.artifact_v2_development_cohort.system_prompt()},
                         {'role': 'user', 'content': prompt}], 'model': 'deepseek-flash'}
    write(case_dir / 'request.json', body)
    row = {**case, 'schema': 'buglecat.verification.prepared-main.v1',
        **{key: manifest[key] for key in calibration_collection.BIND_FIELDS},
        **{key: manifest[key] for key in ('artifact_cohort_file', 'artifact_cohort_sha256', 'artifact_cohort_hash',
            'authorization_file', 'authorization_sha256', 'source_sha256', 'catalog_sha256', 'protocol_sha256',
            'main_resource_profile_file', 'main_resource_profile_sha256', 'main_resource_profile_hash')},
        'settings': cohort.frozen.EXPECTED_SETTINGS, 'build_info': manifest['collector_build_info'],
        'manifest_file': str(directory / 'collection-manifest.json'), 'manifest_sha256': cohort.sha(directory / 'collection-manifest.json'),
        'case_directory': str(case_dir), 'workspace': str(workspace),
        'session_log': str(workspace / '.nanocodex/sessions' / (case['id'] + '.jsonl')),
        'endpoint_sha256': 'e' * 64, 'prompt': prompt, 'prompt_sha256': intake.oracle.raw_sha(prompt.encode()),
        'body': body, 'request_hash': cohort.sha(case_dir / 'request.json')}
    row['artifact_sha256'] = budget.digest(row)
    write(case_dir / 'prepared-row.json', row)
    (directory / 'prepared.jsonl').write_bytes(intake.packet.canonical_bytes(row) + b'\n')
    start = {**row, 'event': 'attempt_started'}
    with sqlite3.connect(fixture['database']) as db:
        db.execute('INSERT INTO attempts(group_id,split,started_json) VALUES(?,?,?)',
                   (case['group_id'], 'calibration', json.dumps(start)))
        ordinal = db.execute('SELECT max(ordinal) FROM attempts').fetchone()[0]
    started = {**start, 'attempt_ordinal': ordinal}
    events = [started]
    if not pending:
        raw = {'model': 'deepseek-flash', 'choices': [{'message': {'content': answer}}],
               'usage': {'prompt_tokens': 1, 'completion_tokens': 2}}
        if status == 'http_400':
            raw = {'error': 'temporary fixture only'}
        write(case_dir / 'raw-response.json', raw)
        receipt = {**row, 'schema': 'buglecat.verification.collected-main.v1', 'attempt_ordinal': ordinal,
            'call_status': status, 'actual_request_hash': row['request_hash'], 'http_status': 400 if status == 'http_400' else 200,
            'response_sha256': cohort.sha(case_dir / 'raw-response.json')}
        if status == 'ok':
            receipt['result'] = {'candidate': answer}
            (case_dir / 'candidate.txt').write_bytes(answer.encode())
        receipt['artifact_sha256'] = budget.digest({key: value for key, value in receipt.items() if key != 'artifact_sha256'})
        write(case_dir / 'receipt.json', receipt)
        submitted = {**started, 'event': 'request_submitted', 'actual_request_hash': row['request_hash']}
        finish = {**submitted, 'event': 'attempt_finished', 'call_status': status,
            'receipt_sha256': cohort.sha(case_dir / 'receipt.json'), 'response_sha256': receipt['response_sha256'],
            'http_status': receipt['http_status'], 'global_main_attempt': ordinal}
        with sqlite3.connect(fixture['database']) as db:
            db.execute('UPDATE attempts SET finished_json=? WHERE ordinal=?', (json.dumps(finish), ordinal))
        events += [submitted, finish]
    (directory / 'attempt-events.jsonl').write_bytes(b'\n'.join(intake.packet.canonical_bytes(event) for event in events) + b'\n')
    return {'directory': directory, 'case_directory': case_dir, 'row': row, 'events': events, 'case': case}
