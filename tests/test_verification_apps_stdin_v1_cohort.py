from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import cohort
from tests.stdin_collection_fixtures import append, campaign, write


def test_entire_funded_allocation_and_one_source_proof_per_transaction(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    assert fixture['calls'] == ['full_source_proof']
    assert len(fixture['value']['cases']) == 808
    assert fixture['value']['collection_authorized_split'] == 'development'
    append(fixture, fixture['value']['cases'][0])
    before = len(fixture['calls'])
    pack = cohort.verified_pack(fixture['path'])
    assert len(fixture['calls']) == before + 1
    assert pack['cohort']['cases'] == fixture['value']['cases']
    assert pack['finished']['old-pending'] is None


@pytest.mark.parametrize('field,value', [('collection_authorized', 1), ('collection_authorized_split', 'calibration'),
    ('formal_registration', True), ('holdout_opened', True), ('production_authorized', True),
    ('new_paid_calls', False), ('certified_threshold', .5)])
def test_resealed_caller_permissions_do_not_open_phase(tmp_path, monkeypatch, field, value):
    fixture = campaign(tmp_path, monkeypatch)
    changed = deepcopy(fixture['value'])
    changed.pop('artifact_hash')
    changed[field] = value
    write(fixture['path'], cohort.seal(changed))
    with pytest.raises(ValueError):
        cohort.verified_pack(fixture['path'])


@pytest.mark.parametrize('change', ['case', 'split', 'drop', 'settings', 'runtime', 'bindings', 'protocol'])
def test_whole_members_and_dependencies_are_immutable(tmp_path, monkeypatch, change):
    fixture = campaign(tmp_path, monkeypatch)
    value = deepcopy(fixture['value'])
    value.pop('artifact_hash')
    if change == 'case':
        value['cases'][0]['task_id'] = 9999
    elif change == 'split':
        value['cases'][0]['split'] = 'holdout'
    elif change == 'drop':
        value['cases'].pop()
    elif change == 'settings':
        value['main_collection_settings']['timeout_s'] = 121
    elif change == 'runtime':
        value['oracle_runtime_binding']['version_info'] = [0, 0, 0]
    elif change == 'bindings':
        value['bindings'].pop()
    else:
        value['protocol']['caller'] = True
    write(fixture['path'], cohort.seal(value))
    with pytest.raises(ValueError):
        cohort.verified_pack(fixture['path'])


@pytest.mark.parametrize('change', ['calibration', 'skipped', 'pending_skipped', 'old_finish'])
def test_actual_shared_prefix_rejects_scope_or_original_history_drift(tmp_path, monkeypatch, change):
    fixture = campaign(tmp_path, monkeypatch)
    cases = fixture['value']['cases']
    if change == 'calibration':
        append(fixture, cases[8])
    elif change == 'skipped':
        append(fixture, cases[1])
    elif change == 'pending_skipped':
        append(fixture, cases[0], pending=True)
        append(fixture, cases[1])
    else:
        import sqlite3
        with sqlite3.connect(fixture['database']) as connection:
            connection.execute('UPDATE attempts SET finished_json=? WHERE ordinal=1', ('{}',))
    with pytest.raises(ValueError):
        cohort.verified_pack(fixture['path'])


def test_original_four_chain_has_no_808_collection_authority(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='approved STDIN extension'):
        cohort.preregister(fixture['pack']['proposal_file'], fixture['pack']['snapshot_file'], fixture['old']['bindings'][-1]['file'], tmp_path / 'other')


def test_production_loaded_literals_and_functions_are_checked():
    cohort.loaded_check()
