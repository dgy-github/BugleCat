"""Temp blind marker lifecycle; synthetic source labels confer no permission."""
import hashlib
import sqlite3
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, cohort, packet
from evals.verification.apps_stdin_v1 import calibration_registration as registration
from tests.stdin_collection_fixtures import write
from tests.test_verification_apps_stdin_v1_calibration_terminal import replay_fixture


def setup(tmp_path, monkeypatch):
    path, data, joined, _ = replay_fixture(tmp_path, monkeypatch)
    main, jev = tmp_path / 'synthetic-main', tmp_path / 'synthetic-jev'
    main.mkdir()
    jev.mkdir()
    write(main / 'authorization.json', {'fixture': 'main'})
    write(jev / 'authorization.json', {'fixture': 'jev'})
    database = main / 'main-attempts.sqlite3'
    with sqlite3.connect(database) as connection:
        connection.execute('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT)')
        connection.execute('INSERT INTO metadata VALUES (?,?)', ('fixture', 'temporary-only'))
        connection.execute('CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT,split TEXT,started_json TEXT,finished_json TEXT)')
        connection.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES (?,?,?,?)',
                           ('old-failure', 'old', '{"old":true}', '{"status":"http_400"}'))
        connection.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES (?,?,?,?)',
                           ('old-pending', 'old', '{"old":true}', None))
    chain = {'authorization': {'main_model': {'ledger_dir': str(main), 'max_attempts': 1642},
                              'jev': {'ledger_dir': str(jev), 'max_attempts': 1037}},
             'current_sha256': 'e' * 64}
    pack = joined['pack']
    old_main, old_jev = tmp_path / 'synthetic-old-main-archive', tmp_path / 'synthetic-old-jev-archive'
    old_main.mkdir()
    old_jev.mkdir()
    (old_main / 'synthetic-original-bytes.txt').write_bytes(b'not genuine main evidence')
    (old_jev / 'synthetic-original-bytes.txt').write_bytes(b'not genuine Jev evidence')
    pack.update(chain=chain, sql_image=budget._ledger_rows(database),
                budget_bindings=registration.development_originals.binding_images(chain),
                archive={'main_directories': [old_main], 'main_evidence': registration.development_review.evidence([old_main]),
                         'jev_directories': [old_jev], 'jev_evidence': registration.development_review.evidence([old_jev])},
                source_pack={'tasks': {}})
    pack['current'].update(main_ledger_file=str(database), remaining_main_attempts=400)
    pack['cohort'].update(authorization_file=str(tmp_path / 'synthetic-auth.json'), oracle_runtime_binding=packet.runtime_info())
    for row in data['cases']:
        if row['split'] != 'calibration' or not row['call_eligible']:
            continue
        code = 'print(input())\n' if row['label'] == 'supported' else 'print("wrong")\n'
        row.update(candidate_code_sha256=hashlib.sha256(code.encode()).hexdigest(), candidate_code_span={'code': code})
        pack['source_pack']['tasks'][row['task_id']] = {'question': 'Synthetic echo one line only.',
            'starter_code': '', 'input_output': {'inputs': ['2\n'], 'outputs': ['2\n']}}
    data.pop('artifact_hash')
    sealed = cohort.seal(data)
    data.clear()
    data.update(sealed)
    write(path, data)
    monkeypatch.setattr(registration, 'loaded_check', lambda: None)
    return path, joined, database, jev


def test_original_binary_packets_and_unique_marker_leave_fee_rows_and_pending_unchanged(tmp_path, monkeypatch):
    path, joined, database, jev = setup(tmp_path, monkeypatch)
    before = budget._ledger_rows(database)
    result = registration.register(path)
    assert budget._ledger_rows(database) == before
    assert not (jev / 'reservation.lock').exists()
    assert not (jev / 'reservations').exists()
    marker = result['registration']
    assert marker['calibration_groups'] == marker['holdout_groups_unopened'] == 400
    assert len(marker['eligible_ids']) == 200
    assert marker['certified_threshold'] is None and marker['holdout_opened'] is False
    reconstructed = registration.before_first_reservation(result['file'])
    assert len(reconstructed['requests']) == 200
    candidates = {case['id']: case['candidate_code_span']['code'] for case in joined['dataset']['cases']
                  if case['split'] == 'calibration' and case['call_eligible']}
    for row in reconstructed['requests']:
        assert row['request_json'].encode() == packet.canonical_bytes(row['request'])
        assert row['candidate_code_sha256'] == hashlib.sha256(candidates[row['id']].encode()).hexdigest()
    with pytest.raises(FileExistsError):
        registration.register(path)
    assert budget._ledger_rows(database) == before


@pytest.mark.parametrize('resource', ['main', 'jev'])
def test_unopened_holdout_funding_cannot_be_spent_by_calibration(tmp_path, monkeypatch, resource):
    path, joined, _, jev = setup(tmp_path, monkeypatch)
    if resource == 'main':
        joined['pack']['current']['remaining_main_attempts'] = 399
    else:
        joined['pack']['chain']['authorization']['jev']['max_attempts'] = 599
    with pytest.raises(ValueError, match='must remain funded'):
        registration.register(path)
    assert not (jev / registration.NAMESPACE).exists()


def test_unqualified_labels_cannot_create_registration(tmp_path, monkeypatch):
    path, _, _, jev = setup(tmp_path, monkeypatch)
    real_proof = registration.terminal.proof

    def unqualified(value):
        joined = real_proof(value)
        joined['report']['independent_quotas_met'] = False
        return joined

    monkeypatch.setattr(registration.terminal, 'proof', unqualified)
    with pytest.raises(ValueError, match='quotas required'):
        registration.register(path)
    assert not (jev / registration.NAMESPACE).exists()


def test_caller_namespace_and_existing_stranded_lock_rejected(tmp_path, monkeypatch):
    path, _, _, jev = setup(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='fixed original shared namespace'):
        registration.register(path, tmp_path / 'caller.json')
    lock = jev / 'reservation.lock'
    lock.write_bytes(b'stranded original lock')
    with pytest.raises(FileExistsError):
        registration.register(path)
    assert lock.read_bytes() == b'stranded original lock'


@pytest.mark.parametrize('mutation', ['old_failure', 'old_pending', 'directory'])
def test_original_archive_or_sql_drift_between_proof_and_lock_blocks_marker(tmp_path, monkeypatch, mutation):
    path, joined, database, jev = setup(tmp_path, monkeypatch)
    real_value = registration.value

    def changed(proof, requests):
        body = real_value(proof, requests)
        if mutation == 'directory':
            (joined['directories'][0] / 'unrecorded-empty').mkdir()
        else:
            with sqlite3.connect(database) as connection:
                ordinal = 1 if mutation == 'old_failure' else 2
                connection.execute('UPDATE attempts SET finished_json=? WHERE ordinal=?', ('{"changed":true}', ordinal))
        return body

    monkeypatch.setattr(registration, 'value', changed)
    with pytest.raises(ValueError, match='changed'):
        registration.register(path)
    assert not (jev / registration.NAMESPACE).exists()
    assert not (jev / 'reservation.lock').exists()


def test_forged_saved_marker_cannot_change_binary_eligibility(tmp_path, monkeypatch):
    path, _, _, _ = setup(tmp_path, monkeypatch)
    result = registration.register(path)
    changed = deepcopy(result['registration'])
    changed['eligible_ids'].pop()
    changed.pop('artifact_hash')
    write(result['file'], cohort.seal(changed))
    with pytest.raises(ValueError, match='original blind baseline changed'):
        registration.before_first_reservation(result['file'])
