import pytest

from evals.verification.apps_stdin_v1 import cohort, collection
from tests.stdin_collection_fixtures import append, campaign, write


def test_public_manifest_never_projects_hidden_io_reference(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    manifest = collection.manifest_value(fixture['path'])
    assert len(manifest['cases']) == 8 and manifest['split'] == 'development'
    assert 'hidden reference fixture' not in str(manifest)
    assert all(set(task) == {'task_id', 'source_kind', 'main_prompt'} for task in manifest['public_tasks'])
    path = tmp_path / 'manifest.json'
    write(path, manifest)
    before = len(fixture['calls'])
    collection.validate_manifest(path)
    assert len(fixture['calls']) == before + 1


@pytest.mark.parametrize('split', ['calibration', 'holdout', True])
def test_caller_phase_strings_cannot_open_sources(tmp_path, monkeypatch, split):
    fixture = campaign(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='phase owner'):
        collection.manifest_value(fixture['path'], split=split)


def test_fault_suffix_can_be_prepared_again_but_attempted_group_cannot_send(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    original = collection.manifest_value(fixture['path'])
    path = tmp_path / 'old-manifest.json'
    write(path, original)
    append(fixture, fixture['value']['cases'][0])
    collection.validate_manifest(path)
    suffix = collection.manifest_value(fixture['path'])
    assert [case['id'] for case in suffix['cases']] == [case['id'] for case in original['cases'][1:]]
    pack = collection.cohort.verified_pack(fixture['path'])
    with pytest.raises(ValueError, match='next unstarted'):
        collection.next_case(pack, original['cases'][0])
    collection.next_case(pack, original['cases'][1])


def test_pending_group_cannot_be_bypassed(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    append(fixture, fixture['value']['cases'][0], pending=True)
    pack = collection.cohort.verified_pack(fixture['path'])
    with pytest.raises(ValueError, match='in-flight'):
        collection.next_case(pack, fixture['value']['cases'][1])
def test_source_closure_reuses_original_batch_and_never_reselects(tmp_path, monkeypatch):
    from tests.stdin_collection_fixtures import append, campaign, write
    fixture = campaign(tmp_path, monkeypatch)
    manifest = collection.manifest_value(fixture['path'], 1)
    path = tmp_path / 'manifest.json'
    write(path, manifest)
    row = {**manifest['cases'][0], 'artifact_cohort_file': str(fixture['path']),
           'artifact_cohort_sha256': cohort.sha(fixture['path']), 'artifact_cohort_hash': fixture['value']['artifact_hash'],
           'manifest_file': str(path), 'manifest_sha256': cohort.sha(path)}
    append(fixture, manifest['cases'][0])
    before = len(fixture['calls'])
    proof = collection.source_closure(row)
    assert len(fixture['calls']) == before
    assert proof['current']['main_started'][-1]['group_id'] == row['group_id']
