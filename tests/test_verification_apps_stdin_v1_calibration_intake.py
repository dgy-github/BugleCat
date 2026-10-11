"""Calibration self-made temp artifacts; never real APPS or actual fees."""

import pytest

from evals.verification.apps_stdin_v1 import budget, intake
from evals.verification.apps_stdin_v1 import calibration_intake as reader
from tests.stdin_calibration_fixtures import CAL_EXE, add_calibration_record, opened_fixture


def test_full_archive_and_calibration_labels_once_with_all_unstarted_retained(tmp_path, monkeypatch):
    fixture = opened_fixture(tmp_path, monkeypatch)
    failed = add_calibration_record(fixture, tmp_path / 'first', status='http_400')
    passed = add_calibration_record(fixture, tmp_path / 'second')
    directories = [failed['directory'], passed['directory']]
    fixture['calls'].clear()
    readonly = reader.proof(fixture['opening']['file'], directories, CAL_EXE)
    assert fixture['calls'] == ['full_source_proof']
    data = readonly['dataset']
    assert len(data['cases']) == 808 and data['calibration_collected_groups'] == 2
    assert data['calibration_terminal_groups'] == 2 and data['calibration_uncollected_groups'] == 398
    assert data['label_counts'] == {'unknown': 2, 'unlabeled': 398}
    assert data['unresolved_oracle_groups'] == 1 and not data['labels_ready']
    assert data['cases'][8]['reason'] == 'http_400'
    assert data['cases'][9]['reason'] == 'independent_oracle_not_recorded'
    assert all(row['main_status'] == 'main_not_started' and row['label'] == 'unlabeled' for row in data['cases'][408:])
    assert len(readonly['pack']['current']['jev_reservations']) == 8
    assert not list(passed['directory'].rglob('stdin-candidate-oracle*'))
    # This echo fixture runs the restricted child only once, with original IO.
    labeled = reader.proof(fixture['opening']['file'], directories, CAL_EXE, execute_new_oracles=True)
    assert labeled['dataset']['cases'][9]['label'] == 'supported'
    assert labeled['dataset']['label_counts'] == {'unknown': 1, 'supported': 1, 'unlabeled': 398}
    oracle_files = [passed['case_directory'] / name for name in ('stdin-candidate-oracle.started', 'stdin-candidate-oracle.json')]
    before = budget.raw_bindings(oracle_files)
    files = {row['file'] for row in labeled['dataset']['collection_bindings']}
    assert all(str(path) in files for path in oracle_files)
    assert not labeled['dataset']['formal_registration'] and not labeled['dataset']['holdout_opened']
    monkeypatch.setattr(intake.oracle, 'execute', lambda *_: pytest.fail('existing original Oracle executed twice'))
    assert reader.collect(fixture['opening']['file'], directories, CAL_EXE, execute_new_oracles=True) == labeled['dataset']
    assert budget.raw_bindings(oracle_files) == before
    with pytest.raises(ValueError, match='omitted'):
        reader.collect(fixture['opening']['file'], [passed['directory']], CAL_EXE)
    # A permanent crash marker remains unresolved, without retry or fabricated label.
    later = add_calibration_record(fixture, tmp_path / 'third')
    intake.durable_new(later['case_directory'] / 'stdin-candidate-oracle.started', {'fixture_crash': True})
    later_data = reader.collect(fixture['opening']['file'], [*directories, later['directory']], CAL_EXE)
    assert later_data['cases'][10]['reason'] == 'oracle_attempt_incomplete'
    assert not later_data['labels_ready'] and later_data['unresolved_oracle_groups'] == 1
    with pytest.raises(FileExistsError):
        reader.collect(fixture['opening']['file'], [*directories, later['directory']], CAL_EXE, execute_new_oracles=True)


def simple_proof(tmp_path, monkeypatch):
    """Isolate the real bidirectional closure, not a provenance authority."""
    directory = tmp_path / 'synthetic-directory'
    directory.mkdir()
    receipt = directory / 'original'
    receipt.write_bytes(b'original')
    pack = {'new_main': [], 'finished': {}, 'archive': {'dataset': {'cases': []}},
            'source_pack': {'tasks': {}}, 'cohort': {'cases': [], 'artifact_hash': 'fixture'},
            'opening': {'cohort_file': str(receipt)}, 'calibration_opening_file': str(receipt),
            'calibration_opening_sha256': 'fixture', 'calibration_opening_hash': 'fixture'}
    monkeypatch.setattr(reader, 'loaded_check', lambda: {'fixture_only': True})
    monkeypatch.setattr(reader, 'build', lambda *_: {})
    monkeypatch.setattr(reader.collector, 'validate_manifest', lambda *_ , **__: ({'cases': [], 'collector_executable_file': str(receipt), 'collector_build_info': {}}, pack))
    monkeypatch.setattr(intake, 'lines', lambda *_: [])
    monkeypatch.setattr(reader.originals, 'close', lambda *_: None)
    return directory, receipt, pack


@pytest.mark.parametrize('change', ['new_file', 'delete_file', 'changed_bytes', 'empty_directory'])
def test_raw_collection_captured_before_proof_rejects_set_or_byte_drift(tmp_path, monkeypatch, change):
    directory, receipt, pack = simple_proof(tmp_path, monkeypatch)
    def mutate(_):
        if change == 'new_file':
            (directory / 'extra').write_bytes(b'new')
        elif change == 'delete_file':
            receipt.unlink()
        elif change == 'changed_bytes':
            receipt.write_bytes(b'changed')
        else:
            (directory / 'new-empty-directory').mkdir()
        return pack
    monkeypatch.setattr(reader.originals, 'proof', mutate)
    with pytest.raises((ValueError, FileNotFoundError)):
        reader.proof(receipt, [directory], receipt)


def test_duplicate_directory_and_untyped_oracle_option_rejected(tmp_path, monkeypatch):
    directory, receipt, pack = simple_proof(tmp_path, monkeypatch)
    monkeypatch.setattr(reader.originals, 'proof', lambda *_: pack)
    with pytest.raises(ValueError, match='duplicate'):
        reader.proof(receipt, [directory, directory], receipt)
    with pytest.raises(ValueError, match='boolean'):
        reader.proof(receipt, [], receipt, execute_new_oracles=1)


def test_no_calibration_starts_can_project_unstarted_allocation_without_fake_unknown(tmp_path, monkeypatch):
    _, receipt, pack = simple_proof(tmp_path, monkeypatch)
    pack['cohort']['cases'] = [
        {'id': str(i), 'group_id': str(i), 'task_id': i, 'task_sha256': 'fixture',
         'source_kind': 'apps_stdin_restricted_script', 'split': 'calibration' if i < 400 else 'holdout'} for i in range(800)]
    monkeypatch.setattr(reader.originals, 'proof', lambda *_: pack)
    data = reader.collect(receipt, [], receipt)
    assert data['calibration_uncollected_groups'] == 400 and data['calibration_collected_groups'] == 0
    assert data['label_counts'] == {'unlabeled': 400} and data['prepared_directories'] == []
    assert all(row['main_status'] == 'main_not_started' and row['label'] == 'unlabeled' for row in data['cases'])
    assert not data['labels_ready']


@pytest.mark.parametrize('link', ['symlink', 'junction'])
def test_original_input_root_link_rejected_before_resolving_alias(tmp_path, monkeypatch, link):
    from pathlib import Path
    directory, receipt, pack = simple_proof(tmp_path, monkeypatch)
    name = 'is_symlink' if link == 'symlink' else 'is_junction'
    original = getattr(Path, name)
    monkeypatch.setattr(Path, name, lambda path: True if path == directory else original(path))
    monkeypatch.setattr(reader.originals, 'proof', lambda *_: pytest.fail('root alias accepted before raw proof'))
    with pytest.raises(ValueError, match='actual original preparation'):
        reader.proof(receipt, [directory], receipt)


def test_new_intake_loaded_schema_and_terminal_alias_drift(monkeypatch):
    monkeypatch.setattr(reader, 'SCHEMA', 'fake')
    with pytest.raises(ValueError, match='constants'):
        reader.loaded_check()
    monkeypatch.undo()
    monkeypatch.setattr(reader, 'MAIN_TERMINAL_STATUSES', frozenset({'ok'}))
    with pytest.raises(ValueError, match='alias'):
        reader.loaded_check()
