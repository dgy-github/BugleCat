"""Actual whole-image prefix policy, isolated synthetic accounting only."""
import sqlite3
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, holdout_originals, holdout_score_originals
from tests.stdin_collection_fixtures import append, campaign


def main_facts(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    baseline, finished = budget._snapshot_chain(fixture['chain'])
    marker = {'original_accounting_image': {'snapshot': baseline, 'finished': finished,
        'sql_image': budget._ledger_rows(fixture['database'])}, 'cases': fixture['value']['cases'][408:],
        'cohort_hash': fixture['value']['artifact_hash']}
    identity = dict(zip(holdout_originals.OPENING_FIELDS, ('fixture', 'a' * 64, 'b' * 64), strict=True))
    return fixture, marker, identity


def test_actual_prefix_preserves_old_pending_and_failure(tmp_path, monkeypatch):
    fixture, marker, identity = main_facts(tmp_path, monkeypatch)
    case = {**marker['cases'][0], **identity}
    append(fixture, case)
    current, finished = budget._snapshot_chain(fixture['chain'])
    before = deepcopy((current, finished))
    actual = holdout_originals.prefix(marker, identity, current, finished, budget._ledger_rows(fixture['database']))
    assert len(actual) == 1 and all(actual[0]['started'][key] == value for key, value in case.items())
    assert (current, finished) == before
    assert finished['old-pending'] is None and finished['old-failed']['call_status'] == 'transport_error'


@pytest.mark.parametrize('change', ['old_finish', 'raw_sql', 'metadata', 'opening', 'calibration', 'skip', 'delete', 'pending'])
def test_actual_main_prefix_rejects_drift(tmp_path, monkeypatch, change):
    fixture, marker, identity = main_facts(tmp_path, monkeypatch)
    case = {**marker['cases'][0], **identity}
    if change == 'opening':
        case['holdout_opening_sha256'] = 'c' * 64
    if change == 'calibration':
        case = {**fixture['value']['cases'][8], **identity}
    if change == 'skip':
        case = {**marker['cases'][1], **identity}
    append(fixture, case, pending=change == 'pending')
    if change == 'pending':
        append(fixture, {**marker['cases'][1], **identity})
    with sqlite3.connect(fixture['database']) as db:
        if change == 'old_finish':
            db.execute('UPDATE attempts SET finished_json=? WHERE ordinal=2', ('{"call_status":"ok"}',))
        elif change == 'raw_sql':
            db.execute("UPDATE attempts SET started_json=started_json || ' ' WHERE ordinal=1")
        elif change == 'metadata':
            db.execute("UPDATE metadata SET value='reset'")
        elif change == 'delete':
            db.execute('DELETE FROM attempts WHERE ordinal=3')
    with pytest.raises(ValueError):
        current, finished = budget._snapshot_chain(fixture['chain'])
        holdout_originals.prefix(marker, identity, current, finished, budget._ledger_rows(fixture['database']))


def score_facts(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    baseline, finished = budget._snapshot_chain(fixture['chain'])
    rows = [{'id': f'binary-{i}', 'group_id': f'binary-{i}', 'request_hash': budget.digest(i)} for i in range(200)]
    path = tmp_path / 'synthetic-registration.json'
    registered = {'ledger_snapshot': baseline, 'original_finished': deepcopy(finished),
        'original_sql_image': budget._ledger_rows(fixture['database']), 'artifact_hash': 'b' * 64,
        'eligible_ids': [row['id'] for row in rows], 'request_hashes': [row['request_hash'] for row in rows]}
    slot = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev', 'group_id': rows[0]['group_id'],
        'request_hash': rows[0]['request_hash'], 'registration_file': str(path), 'registration_sha256': 'c' * 64,
        'registration_hash': registered['artifact_hash'], 'authorization_sha256': baseline['authorization_chain']['current_sha256'],
        'reservation_index': 1, 'status': 'reserved_before_transport', 'automatic_retries': 0}
    current = deepcopy(baseline)
    current['jev_reservations'] = [slot]
    current['excluded_groups'] = sorted([*baseline['excluded_groups'], slot['group_id']])
    return registered, current, finished, rows, path, slot


@pytest.mark.parametrize('change', [None, 'old_finish', 'new_main', 'raw_sql', 'skip', 'request', 'registration', 'retry', 'exclusion'])
def test_score_current_is_original_plus_authenticated_slots(tmp_path, monkeypatch, change):
    registered, current, finished, rows, path, slot = score_facts(tmp_path, monkeypatch)
    sql = deepcopy(registered['original_sql_image'])
    if change == 'old_finish':
        finished['old-pending'] = {'call_status': 'ok'}
    elif change == 'new_main':
        current['main_started'].append({'ordinal': 3, 'group_id': 'new-main', 'started': {}})
        finished['new-main'] = None
    elif change == 'raw_sql':
        sql[1][0] = (*sql[1][0][:3], sql[1][0][3] + ' ', sql[1][0][4])
    elif change == 'skip':
        slot['group_id'] = rows[1]['group_id']
    elif change == 'request':
        slot['request_hash'] = 'd' * 64
    elif change == 'registration':
        slot['registration_sha256'] = 'd' * 64
    elif change == 'retry':
        slot['automatic_retries'] = False
    elif change == 'exclusion':
        current['excluded_groups'] = []
    if change is None:
        assert holdout_score_originals.current_prefix(registered, current, finished, sql, rows, path, 'c' * 64) == [slot]
    else:
        with pytest.raises(ValueError):
            holdout_score_originals.current_prefix(registered, current, finished, sql, rows, path, 'c' * 64)
def test_manifest_graph_shared_bindings_checked_once_and_raw_closed(tmp_path, monkeypatch):
    """Pure graph kernel fixture, not source/admission or fee authority."""
    from evals.verification.apps_stdin_v1 import cohort, holdout_originals
    shared = tmp_path / 'shared-original-source.txt'
    shared.write_bytes(b'unchanged-fixture-source')
    directories = []
    for index in range(2):
        directory = tmp_path / str(index)
        directory.mkdir()
        value = cohort.seal({'phase_proof_bindings': [{'file': str(shared), 'sha256': cohort.sha(shared)}]})
        (directory / 'collection-manifest.json').write_bytes(budget.packet.canonical_bytes(value))
        directories.append(directory)
    checked, real_check = [], cohort.check
    monkeypatch.setattr(cohort, 'check', lambda bindings: (checked.append(list(bindings)), real_check(bindings))[1])
    image = holdout_originals.manifest_images(directories)
    assert len(checked) == 1 and len(checked[0]) == 1
    assert len(image['manifests']) == len(image['sha256']) == 2
    holdout_originals.close_manifest_images(image)
    manifest = directories[0] / 'collection-manifest.json'
    manifest.write_bytes(manifest.read_bytes() + b' ')
    with pytest.raises(ValueError, match='manifest bytes changed'):
        holdout_originals.close_manifest_images(image)


def test_manifest_graph_union_rejects_conflicting_original_bindings(tmp_path):
    from evals.verification.apps_stdin_v1 import cohort, holdout_originals
    shared = tmp_path / 'original-source.txt'
    shared.write_bytes(b'original')
    directories = []
    for index in range(2):
        directory = tmp_path / str(index)
        directory.mkdir()
        value = cohort.seal({'phase_proof_bindings': [{'file': str(shared), 'sha256': cohort.sha(shared) if index == 0 else '0' * 64}]})
        (directory / 'collection-manifest.json').write_bytes(budget.packet.canonical_bytes(value))
        directories.append(directory)
    with pytest.raises(ValueError):
        holdout_originals.manifest_images(directories)


