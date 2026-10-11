"""Self-made Temp evidence only; no APPS tasks or live-compatible authority.

The source/old generic main verifier substitutes are inherited from the C4b
fixture. Main SQL/event/prepared/finish bindings and new formal sender bytes are
joined by their real readers. Two local echo/wrong child receipts supply
explicit synthetic Oracle templates, not 200 natural independently generated
model candidates. No fixture route exists in the genuine compiled live action.
"""
import json
import sqlite3
from copy import deepcopy
from pathlib import Path

from evals.verification.apps_stdin_v1 import (
    budget,
    calibration_collection,
    calibration_originals,
    calibration_registration,
    cohort,
    development_originals,
    formal_archive,
    formal_originals,
    frozen,
    intake,
    packet,
    protocol,
)
from tests.stdin_calibration_fixtures import CAL_EXE, opened_fixture
from tests.stdin_collection_fixtures import write

FORMAL_EXE = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_stdin_v1_jev_formal.exe'


def full_calibration(tmp_path, monkeypatch):
    fixture = opened_fixture(tmp_path, monkeypatch)
    pack = calibration_originals.proof(fixture['opening']['file'])
    compiled = calibration_collection.build(CAL_EXE)
    graph, runtime = packet.dependencies(), packet.runtime_info()
    context = formal_archive.packet_context(runtime)
    context['compiled_bindings'] = budget.raw_bindings([*calibration_collection.runner_sources().values(), CAL_EXE])
    templates = {code: intake.oracle.execute(code, {'inputs': ['7\n'], 'outputs': ['7']})
                 for code in ('print(input())\n', 'print("wrong")\n')}
    directories = []
    with sqlite3.connect(fixture['database']) as db:
        for offset in range(0, 400, 8):
            directory = tmp_path / ('synthetic-calibration-' + str(offset))
            directory.mkdir()
            directories.append(directory)
            manifest = formal_archive._manifest(pack, CAL_EXE, pack['opening']['cases'][offset:offset + 8], compiled, context)
            manifest_file = directory / 'collection-manifest.json'
            write(manifest_file, manifest)
            rows, events = [], []
            for index, case in enumerate(manifest['cases'], offset):
                task = fixture['pack']['tasks'][case['task_id']]
                code = 'print(input())\n' if index < 150 else 'print("wrong")\n' if index < 200 else None
                answer = '```python\n' + code + '```\nSynthetic assessment.' if code else 'No artifact; synthetic Unknown.'
                case_dir = directory / case['id']
                workspace = case_dir / 'workspace'
                workspace.mkdir(parents=True)
                prompt = formal_archive._public_task(task, context)['main_prompt']
                body = {'messages': [{'role': 'system', 'content': cohort.artifact_v2_development_cohort.system_prompt()},
                                     {'role': 'user', 'content': prompt}], 'model': 'deepseek-flash'}
                write(case_dir / 'request.json', body)
                row = {**case, 'schema': 'buglecat.verification.prepared-main.v1',
                    **{key: manifest[key] for key in calibration_collection.BIND_FIELDS},
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
                           (case['group_id'], 'calibration', json.dumps(start)))
                ordinal = db.execute('SELECT max(ordinal) FROM attempts').fetchone()[0]
                started = {**start, 'attempt_ordinal': ordinal}
                raw = {'model': 'deepseek-flash', 'choices': [{'message': {'content': answer}}],
                       'usage': {'prompt_tokens': 1, 'completion_tokens': 2}}
                write(case_dir / 'raw-response.json', raw)
                receipt = {**row, 'schema': 'buglecat.verification.collected-main.v1', 'attempt_ordinal': ordinal,
                    'call_status': 'ok', 'actual_request_hash': row['request_hash'], 'http_status': 200,
                    'response_sha256': cohort.sha(case_dir / 'raw-response.json'), 'result': {'candidate': answer}}
                receipt['artifact_sha256'] = budget.digest({key: child for key, child in receipt.items() if key != 'artifact_sha256'})
                write(case_dir / 'receipt.json', receipt)
                (case_dir / 'candidate.txt').write_bytes(answer.encode())
                submitted = {**started, 'event': 'request_submitted', 'actual_request_hash': row['request_hash']}
                finish = {**submitted, 'event': 'attempt_finished', 'call_status': 'ok', 'http_status': 200,
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
    # Build the saved fixture input with the new full raw kernel. The frozen
    # Root register below independently replays the complete calibration intake
    # before granting any temporary marker; it is not mocked or bypassed.
    pack['current'], pack['finished'] = budget._snapshot_chain(pack['chain'])
    pack['sql_image'] = budget._ledger_rows(fixture['database'])
    pack['new_main'] = calibration_originals._prefix({**pack['opening'],
        '_identity': {key: pack[key] for key in calibration_originals.OPENING_FIELDS}},
        pack['current'], pack['finished'], pack['sql_image'])
    pack['budget_bindings'] = development_originals.binding_images(pack['chain'])
    data = formal_archive.calibration_dataset(pack, {
        'prepared_directories': [str(path) for path in directories], 'collector_executable_file': str(CAL_EXE),
        'collector_build_info': compiled, 'original_reservation_bindings': pack['budget_bindings']})['dataset']
    dataset_file = tmp_path / 'synthetic-whole400-dataset.json'
    write(dataset_file, data)
    result = calibration_registration.register(dataset_file)
    # Production loaded guards remain mandatory; the fixture source graph is
    # explicitly substituted by the pre-existing Temp fixture and cannot live.
    monkeypatch.setattr(formal_originals, 'loaded_check', lambda: None)
    monkeypatch.setattr(formal_originals.formal_archive, 'loaded_check', lambda: None)
    fixture.update(formal_registration=result, calibration_dataset=dataset_file, calibration_directories=directories)
    return fixture


def finish_formal(row, prepared_file, reservation_file, *, status='ok', signal=.1):
    from evals.verification.apps_stdin_v1 import formal_observations
    directory = Path(row['output_directory'])
    original = formal_observations.identity(row, prepared_file, budget.load(reservation_file), reservation_file)
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


def restore_synthetic(root, monkeypatch):
    """Reuse self-made input bytes; fresh proofs run, no source/ledger authority.

    This is deliberately a test-only reconstruction of the campaign() fixture
    substitutions. It cannot be imported by a compiled live path. Existing
    Temp SQLite/Oracle/registration inputs are never rewritten or re-executed.
    Root may reuse a completed Temp archive to avoid preparing400 rows twice.
    """
    from evals.verification.apps_stdin_v1 import (
        calibration_opening,
        development_registration,
    )
    from tests.stdin_calibration_fixtures import REAL_PREPARED, REAL_TERMINAL
    root = Path(root).resolve(strict=True)
    assert root.is_relative_to(Path(__import__('tempfile').gettempdir()).resolve())
    assert (root / 'fixture-train.jsonl').read_bytes() == b'fixture-only-not-APPS'
    binary = root / 'fixture-only-not-genuine.exe'
    assert binary.read_bytes() == b'fixture-never-executed-or-accepted-by-live'
    value = cohort.sealed(budget.load(root / 'fixture-cohort.json'))
    proposal = budget.load(root / 'proposal.json')
    image = budget.load(root / 'snapshot.json')
    tasks = {index: {'task_id': index, 'source_kind': 'apps_stdin_restricted_script',
        'question': f'Public fixture {index}', 'starter_code': '',
        'input_output': {'inputs': ['7\n'], 'outputs': ['7']}, 'reference': 'hidden reference fixture'} for index in range(808)}
    assert all(case['task_sha256'] == budget.digest(tasks[case['task_id']]) for case in value['cases'])
    source = {'proposal': proposal, 'authority': {'train_file': str(root / 'fixture-train.jsonl'),
        'full_source_sha256': cohort.sha(root / 'fixture-train.jsonl')},
        'baseline': image['snapshot'], 'finished': image['finished'], 'tasks': tasks, 'train': root / 'fixture-train.jsonl',
        'bindings': budget.raw_bindings([root / name for name in ('proposal.json', 'snapshot.json', 'authority.json', 'profile.json', 'fixture-train.jsonl')]),
        'proposal_file': str(root / 'proposal.json'), 'snapshot_file': str(root / 'snapshot.json')}
    auth_files = [root / f'fixture-auth-{index}.json' for index in range(4)]
    auth_bindings = budget.raw_bindings(auth_files)
    old = {'authorization': budget.load(auth_files[-1]), 'current_sha256': cohort.sha(auth_files[-1]),
        'allowed_sha256': [cohort.sha(file) for file in auth_files], 'ledger_authorization_sha256': cohort.sha(auth_files[0]),
        'bindings': auth_bindings}
    # Frozen fixture campaign() preserves creation-order chain, not raw file
    # lexical sorting as a semantic rewrite.
    old['bindings'] = [{'file': str(file), 'sha256': cohort.sha(file)} for file in auth_files]
    calls = []

    def fake_source(*args):
        calls.append('full_source_proof')
        return deepcopy(source)

    monkeypatch.setattr(cohort, '_source', fake_source)
    monkeypatch.setattr(cohort, 'loaded_check', lambda: None)
    monkeypatch.setattr(cohort, 'collector_build', lambda: {
        'collector_executable_file': str(binary), 'collector_executable_sha256': cohort.sha(binary), 'collector_build_info': {'fixture_only': True}})
    monkeypatch.setattr(budget, 'loaded_check', lambda: {})
    monkeypatch.setattr(budget, 'PREVIOUS_SHA', old['current_sha256'])
    monkeypatch.setattr(budget, 'previous', lambda _: deepcopy(old))
    monkeypatch.setitem(frozen.RAW_PINS, 'proposal', cohort.sha(root / 'proposal.json'))
    monkeypatch.setitem(frozen.RAW_PINS, 'snapshot', cohort.sha(root / 'snapshot.json'))
    monkeypatch.setattr(intake, 'loaded_check', lambda: packet.runtime_info())
    monkeypatch.setattr(intake, 'build', lambda _: {'fixture_only': True})
    monkeypatch.setattr(intake, 'verify', lambda *args: None)
    monkeypatch.setattr(intake, '_prepared', lambda *args: None if args[0]['split'] == 'development' else REAL_PREPARED(*args))
    monkeypatch.setattr(intake, 'terminal', lambda *args: ({'result': {'candidate': '```python\nprint(input())\n```\nNot executed by a model.'}}, 'ok')
        if args[0]['split'] == 'development' else REAL_TERMINAL(*args))
    for owner in (development_originals, development_registration, calibration_opening,
                  calibration_opening.acceptance, calibration_originals, formal_archive, formal_originals):
        monkeypatch.setattr(owner, 'loaded_check', lambda: None)
    # Do not substitute protocol.validate: real protocol numeric/loaded checks
    # remain active, as do225 actual compiled source and selfEXE checks.
    assert protocol.validate.__name__ == 'validate'
    registered = next((root / 'fixture-jev/stdin-calibration-registrations').glob('*.json'))
    result = {'file': str(registered), 'sha256': cohort.sha(registered), 'registration': cohort.sealed(budget.load(registered))}
    return {'path': root / 'fixture-cohort.json', 'value': value, 'pack': source,
        'database': root / 'fixture-main/main-attempts.sqlite3', 'calls': calls, 'binary': binary,
        'chain': value['ledger_snapshot']['authorization_chain'], 'formal_registration': result,
        'calibration_dataset': Path(result['registration']['dataset_file']),
        'calibration_directories': [Path(path) for path in result['registration']['prepared_directories']]}


def complete_synthetic_formal(fixture, monkeypatch):
    """All200 explicit synthetic slots, not model outputs or production authority.

    First two slots use actual new reserve/claim transactions; remaining slots
    are test-generated exact raw inputs (like the400 original fixture rows).
    The final public observer must independently authenticate every byte, ordinal,
    original candidate and full current accounting prefix without a cache.
    """
    from evals.verification.apps_stdin_v1 import formal_jev, formal_transport
    from evals.verification.paid_budget import durable_new
    registered = fixture['formal_registration']['file']
    prepared = formal_jev.prepare(registered, FORMAL_EXE)
    directories = []
    root = Path(fixture['chain']['authorization']['jev']['ledger_dir'])
    for index, item in enumerate(prepared):
        path = Path(item['file'])
        row = cohort.sealed(budget.load(path))
        reservation_file = root / 'reservations' / (budget.digest(row['group_id']) + '.json')
        if index < 2:
            row, pack = formal_jev.validate(path)
            slot = formal_transport._reserve(path, row, pack)
            row, pack = formal_jev.validate(path)
            formal_transport._claim(path, row, pack, slot['reservation_file'])
        else:
            slot = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
                'group_id': row['group_id'], 'request_hash': row['request_hash'],
                'authorization_sha256': fixture['chain']['current_sha256'], 'reservation_index': 9 + index,
                'status': 'reserved_before_transport', 'automatic_retries': 0,
                'registration_file': row['registration_file'], 'registration_sha256': row['registration_sha256'],
                'registration_hash': row['registration_hash'], 'prepared_file': str(path.resolve()),
                'prepared_sha256': cohort.sha(path), 'sender_executable_sha256': row['sender_build_info']['runner_exe_sha256'],
                'output_directory': row['output_directory'], 'endpoint_sha256': row['endpoint_sha256']}
            durable_new(reservation_file, slot)
            Path(row['output_directory']).mkdir()
            durable_new(reservation_file.with_suffix('.started'), {
                'schema': formal_transport.SCHEMA, 'group_id': row['group_id'], 'request_hash': row['request_hash'],
                'reservation_file': str(reservation_file.resolve()), 'reservation_sha256': cohort.sha(reservation_file),
                'reservation_index': slot['reservation_index'], 'prepared_file': str(path.resolve()),
                'prepared_sha256': cohort.sha(path), 'output_directory': row['output_directory'], 'new_paid_calls': 0})
        directories.append(finish_formal(row, path, reservation_file,
            status='http_400' if index == 0 else 'ok', signal=.9 if row['label'] == 'unsupported' else .1))
    return directories
