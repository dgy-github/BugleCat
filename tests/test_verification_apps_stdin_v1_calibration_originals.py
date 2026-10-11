"""Free fixture archive/current-prefix checks; no public APPS execution."""
import sqlite3
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, cohort
from evals.verification.apps_stdin_v1 import calibration_originals as originals
from tests.stdin_calibration_fixtures import add_calibration_record, opened_fixture
from tests.stdin_collection_fixtures import append


def test_actual_original_archive_remains_valid_after_terminal_calibration_start(tmp_path, monkeypatch):
    fixture = opened_fixture(tmp_path, monkeypatch)
    fixture['calls'].clear()
    pack = originals.proof(fixture['opening']['file'])
    assert fixture['calls'] == ['full_source_proof']
    assert len(pack['archive']['dataset']['cases']) == 808
    assert len(pack['current']['jev_reservations']) == 8  # never projected zero
    assert pack['finished']['old-pending'] is None
    assert pack['finished']['old-failed']['call_status'] == 'transport_error'
    record = add_calibration_record(fixture, tmp_path / 'calibration-first', status='http_400')
    case = record['case']
    # No old acceptance/before_first_request is invoked after main appends.
    monkeypatch.setattr(originals.opening, 'before_first_request', lambda *_: pytest.fail('old first request guard after append'))
    # No acceptance loaded function is changed; actual new compiled guards bind
    # those bytes. The new owner does not invoke its historical public entry.
    fixture['calls'].clear()
    newer = originals.proof(fixture['opening']['file'])
    assert fixture['calls'] == ['full_source_proof']
    assert len(newer['new_main']) == 1 and len(newer['current']['jev_reservations']) == 8
    assert newer['finished'][case['group_id']]['call_status'] == 'http_400'
    # A pending new attempt permanently blocks skipping to another group.
    pending = add_calibration_record(fixture, tmp_path / 'calibration-pending', pending=True)
    second = pending['case']
    assert originals.proof(fixture['opening']['file'])['finished'][second['group_id']] is None
    third = {**fixture['value']['cases'][10], **{key: pack[key] for key in originals.OPENING_FIELDS}}
    append(fixture, third)
    with pytest.raises(ValueError, match='pending'):
        originals.proof(fixture['opening']['file'])


@pytest.mark.parametrize('change', ['old_finish', 'raw_sql', 'metadata', 'opening_identity', 'holdout', 'skip', 'delete'])
def test_exact_current_history_rejects_mutation_without_archive_reexecution(tmp_path, monkeypatch, change):
    # Small typed prefix fixture exercises actual SQLite and original byte image;
    # full actual archive proof is covered separately above.
    from tests.stdin_collection_fixtures import campaign
    fixture = campaign(tmp_path, monkeypatch)
    baseline, finished = budget._snapshot_chain(fixture['chain'])
    marker = {'original_accounting_image': {'snapshot': baseline, 'finished': finished,
        'sql_image': budget._ledger_rows(fixture['database']), 'raw_budget_bindings': []}, 'cases': fixture['value']['cases'][8:408],
        'cohort_hash': fixture['value']['artifact_hash'], '_identity': {'calibration_opening_file': 'fixture',
        'calibration_opening_sha256': 'a' * 64, 'calibration_opening_hash': 'b' * 64}}
    case = {**marker['cases'][0], **marker['_identity']}
    if change == 'opening_identity':
        case['calibration_opening_sha256'] = 'c' * 64
    if change == 'holdout':
        case = {**fixture['value']['cases'][408], **marker['_identity']}
    if change == 'skip':
        case = {**marker['cases'][1], **marker['_identity']}
    append(fixture, case)
    with sqlite3.connect(fixture['database']) as db:
        if change == 'old_finish':
            db.execute('UPDATE attempts SET finished_json=? WHERE ordinal=2', ('{"call_status":"ok"}',))
        elif change == 'raw_sql':
            # Same parsed value, different historical original bytes.
            db.execute("UPDATE attempts SET started_json=started_json || ' ' WHERE ordinal=1")
        elif change == 'metadata':
            db.execute("UPDATE metadata SET value='reset'")
        elif change == 'delete':
            db.execute('DELETE FROM attempts WHERE ordinal=3')
    with pytest.raises(ValueError):
        current, after = budget._snapshot_chain(fixture['chain'])
        originals._prefix(marker, current, after, budget._ledger_rows(fixture['database']))


def test_raw_original_reservations_cannot_disappear_from_calibration_image(tmp_path, monkeypatch):
    from tests.stdin_collection_fixtures import campaign
    fixture = campaign(tmp_path, monkeypatch)
    baseline, finished = budget._snapshot_chain(fixture['chain'])
    marker = {'original_accounting_image': {'snapshot': baseline, 'finished': finished,
        'sql_image': budget._ledger_rows(fixture['database']), 'raw_budget_bindings': []}, 'cases': fixture['value']['cases'][8:408],
        'cohort_hash': fixture['value']['artifact_hash'], '_identity': {}}
    forged = deepcopy(baseline)
    forged['jev_reservations'] = [{'group_id': 'fake'}]
    with pytest.raises(ValueError, match='future genuine registration'):
        originals._prefix(marker, forged, finished, budget._ledger_rows(fixture['database']))


def test_new_owner_loaded_declared_constants_drift_is_rejected(monkeypatch):
    monkeypatch.setattr(originals, 'SCHEMA', 'forged')
    with pytest.raises(ValueError, match='constants'):
        originals.loaded_check()
    assert cohort.SCHEMA != originals.SCHEMA
