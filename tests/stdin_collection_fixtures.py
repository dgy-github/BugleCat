"""Synthetic temporary accounting; never real C1 approval or paid provenance."""
import json
import sqlite3
from copy import deepcopy
from pathlib import Path

from evals.verification.apps_stdin_v1 import budget, cohort, frozen, packet


def write(path, value):
    Path(path).write_bytes(packet.canonical_bytes(value))
    return cohort.sha(path)


def campaign(tmp_path, monkeypatch):
    main, jev = tmp_path / 'fixture-main', tmp_path / 'fixture-jev'
    main.mkdir()
    jev.mkdir()
    bindings = []
    for index, (main_cap, jev_cap) in enumerate(budget.OLD_CAPS):
        path = tmp_path / f'fixture-auth-{index}.json'
        auth = {'automatic_retries': 0, 'main_model': {'ledger_dir': str(main), 'max_attempts': main_cap},
                'jev': {'ledger_dir': str(jev), 'max_attempts': jev_cap}}
        bindings.append({'file': str(path), 'sha256': write(path, auth)})
    old = {'authorization': auth, 'current_sha256': bindings[-1]['sha256'],
           'allowed_sha256': [row['sha256'] for row in bindings], 'ledger_authorization_sha256': bindings[0]['sha256'], 'bindings': bindings}
    database = main / 'main-attempts.sqlite3'
    with sqlite3.connect(database) as connection:
        connection.executescript('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);'
            'CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT UNIQUE NOT NULL,split TEXT NOT NULL,started_json TEXT NOT NULL,finished_json TEXT);')
        connection.execute('INSERT INTO metadata VALUES(?,?)', ('authorization_sha256', old['ledger_authorization_sha256']))
        for group, finish in [('old-pending', None), ('old-failed', {'call_status': 'transport_error'})]:
            start = {'group_id': group, 'split': 'development', 'authorization_sha256': old['current_sha256']}
            connection.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES(?,?,?,?)',
                               (group, 'development', json.dumps(start), json.dumps(finish) if finish else None))
    baseline, finished = budget._snapshot_chain(old)
    proposal_path, snapshot_path, authority_path, profile_path = [tmp_path / name for name in ('proposal.json', 'snapshot.json', 'authority.json', 'profile.json')]
    train = tmp_path / 'fixture-train.jsonl'
    train.write_text('fixture-only-not-APPS', encoding='utf-8')
    tasks, cases, counter = {}, {}, 0
    for split, count in [('development', 8), ('calibration', 400), ('holdout', 400)]:
        cases[split] = []
        for _ in range(count):
            task = {'task_id': counter, 'source_kind': 'apps_stdin_restricted_script', 'question': f'Public fixture {counter}',
                    'starter_code': '', 'input_output': {'inputs': ['7\n'], 'outputs': ['7']}, 'reference': 'hidden reference fixture'}
            tasks[counter] = task
            cases[split].append({'id': f'stdin-v1-{split}-main-{counter}', 'group_id': f'fixture-group-{counter}',
                                'task_id': counter, 'task_sha256': budget.digest(task), 'source_kind': task['source_kind'], 'split': split})
            counter += 1
    write(authority_path, {'fixture_only': True})
    profile = {'artifact_hash': 'f' * 64}
    write(profile_path, profile)
    proposal = {'artifact_hash': 'a' * 64, 'cases': cases, 'source_authority_file': str(authority_path),
                'source_authority_sha256': cohort.sha(authority_path), 'main_resource_profile_file': str(profile_path),
                'main_resource_profile_sha256': cohort.sha(profile_path), 'main_resource_profile_hash': profile['artifact_hash'],
                'protocol': {'fixture_only': True}, 'public_system_prompt_sha256': budget.digest('fixture-prompt'),
                'oracle_runtime_binding': packet.runtime_info()}
    proposal_sha = write(proposal_path, proposal)
    snapshot_sha = write(snapshot_path, {'snapshot': baseline, 'finished': finished})
    pack = {'proposal': proposal, 'authority': {'train_file': str(train), 'full_source_sha256': cohort.sha(train)},
            'baseline': baseline, 'finished': finished, 'tasks': tasks, 'train': train,
            'bindings': budget.raw_bindings([proposal_path, snapshot_path, authority_path, profile_path, train]),
            'proposal_file': str(proposal_path), 'snapshot_file': str(snapshot_path)}
    auth_path = tmp_path / 'fixture-future-approval.json'
    current = {'schema': budget.SCHEMA, 'approved_plan': budget.PLAN, 'approval_status': 'approved', 'approved': True,
               'user_authorization': budget.QUOTE, 'previous_authorization_file': bindings[-1]['file'],
               'previous_authorization_sha256': old['current_sha256'], 'additional_main_attempt_cap': 492,
               'additional_jev_attempt_cap': 176, 'automatic_retries': 0, 'per_group_max_main_attempts': 1,
               'per_group_max_jev_attempts': 1, 'old_attempts_and_failures_count': True, 'old_pending_attempts_count': True,
               'production_authorized': False, 'main_model': {'max_attempts': 1642, 'ledger_dir': str(main)},
               'jev': {'max_attempts': 1037, 'ledger_dir': str(jev)}, 'source_proposal_file': str(proposal_path),
               'source_snapshot_file': str(snapshot_path), 'source_proposal_hash': proposal['artifact_hash'],
               'source_proposal_sha256': proposal_sha, 'source_snapshot_sha256': snapshot_sha}
    write(auth_path, current)
    calls = []
    def source(*args):
        calls.append('full_source_proof')
        return deepcopy(pack)
    monkeypatch.setattr(cohort, '_source', source)
    binary = tmp_path / 'fixture-only-not-genuine.exe'
    binary.write_bytes(b'fixture-never-executed-or-accepted-by-live')
    compiled = {'collector_executable_file': str(binary), 'collector_executable_sha256': cohort.sha(binary),
                'collector_build_info': {'fixture_only': True}}
    monkeypatch.setattr(cohort, 'collector_build', lambda: deepcopy(compiled))
    monkeypatch.setattr(cohort, 'loaded_check', lambda: None)
    monkeypatch.setattr(budget, 'loaded_check', lambda: {})
    monkeypatch.setattr(budget, 'PREVIOUS_SHA', old['current_sha256'])
    monkeypatch.setattr(budget, 'previous', lambda _: deepcopy(old))
    monkeypatch.setitem(frozen.RAW_PINS, 'proposal', proposal_sha)
    monkeypatch.setitem(frozen.RAW_PINS, 'snapshot', snapshot_sha)
    monkeypatch.setattr(cohort.protocol, 'validate', lambda value: value)
    output = tmp_path / 'output'
    output.mkdir()
    value = cohort.preregister(proposal_path, snapshot_path, auth_path, output)
    path = tmp_path / 'fixture-cohort.json'
    write(path, value)
    return {'path': path, 'value': value, 'pack': pack, 'database': database, 'calls': calls, 'auth_path': auth_path,
            'output': output, 'old': old, 'chain': value['ledger_snapshot']['authorization_chain']}


def append(fixture, case, *, pending=False):
    start = {**case, 'authorization_sha256': fixture['value']['authorization_sha256'],
             'artifact_cohort_hash': fixture['value']['artifact_hash']}
    with sqlite3.connect(fixture['database']) as connection:
        connection.execute('INSERT INTO attempts(group_id,split,started_json) VALUES(?,?,?)',
                           (case['group_id'], case['split'], json.dumps(start)))
        ordinal = connection.execute('SELECT max(ordinal) FROM attempts').fetchone()[0]
        if not pending:
            connection.execute('UPDATE attempts SET finished_json=? WHERE ordinal=?',
                               (json.dumps({'group_id': case['group_id'], 'global_main_attempt': ordinal, 'call_status': 'transport_error'}), ordinal))
