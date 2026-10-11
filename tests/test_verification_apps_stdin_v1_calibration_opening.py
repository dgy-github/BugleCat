"""Temporary opening transactions; no real funded source or fee permission."""
import sqlite3
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import budget, cohort
from evals.verification.apps_stdin_v1 import calibration_opening as opening
from tests.stdin_collection_fixtures import campaign, write
from tests.stdin_development_jev_fixtures import finish_synthetic
from tests.test_verification_apps_stdin_v1_development_registration import original_fixture


def setup(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    registered = {'dataset_file': str(tmp_path / 'dataset.json'),
                  'cohort_hash': fixture['value']['artifact_hash'], 'ledger_snapshot': fixture['value']['ledger_snapshot'],
                  'bindings': budget.raw_bindings([fixture['path']]),
                  'prepared_directories': [], 'original_evidence': []}
    registration_file = tmp_path / 'synthetic-blind-registration.json'
    write(registration_file, cohort.seal(registered))
    write(registered['dataset_file'], cohort.seal({'cohort_file': str(fixture['path']),
                                               'cohort_hash': fixture['value']['artifact_hash']}))
    chain = fixture['chain']
    image = opening.acceptance._image(chain)
    review = cohort.seal({'pipeline_verified': True, 'reason': 'development_pipeline_verified',
        'registration_file': str(registration_file), 'registration_sha256': cohort.sha(registration_file),
        'cohort_hash': fixture['value']['artifact_hash'], 'budget_snapshot_digest': budget.digest(image),
        'prepared_directories': [], 'original_directory_evidence': []})
    path = tmp_path / 'synthetic-pipeline-review.json'
    write(path, review)
    # Real approval is never delegated to these test patches. Only temporary
    # locks, durable creation, raw accounting and caller-path rejection are used.
    monkeypatch.setattr(opening, 'loaded_check', lambda: None)
    monkeypatch.setattr(opening.acceptance, 'revalidate', lambda _: budget.load(path))
    return fixture, path


def test_exclusive_marker_preserves_accounting_and_only_opens_fixed_calibration(tmp_path, monkeypatch):
    fixture, path = setup(tmp_path, monkeypatch)
    before = opening.acceptance._image(fixture['chain'])
    result = opening.open_calibration(path)
    assert result['opening']['allocation_groups'] == 400
    assert result['opening']['holdout_opened'] is False
    assert result['opening']['certified_threshold'] is None
    assert opening.acceptance._image(fixture['chain']) == before
    assert budget.same(opening.before_first_request(result['file'])['opening'], result['opening'])
    with pytest.raises(FileExistsError):
        opening.open_calibration(path)
    assert opening.acceptance._image(fixture['chain']) == before


def test_caller_output_and_stranded_shared_lock_cannot_change_namespace(tmp_path, monkeypatch):
    fixture, path = setup(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='fixed shared namespace'):
        opening.open_calibration(path, tmp_path / 'caller-marker.json')
    lock = tmp_path / 'fixture-jev/reservation.lock'
    lock.write_bytes(b'stranded')
    with pytest.raises(ValueError, match='transaction in progress'):
        opening.open_calibration(path)
    assert lock.read_bytes() == b'stranded'
    assert not (tmp_path / 'fixture-main' / opening.NAMESPACE).exists()
    assert fixture['database'].exists()


def test_main_change_between_proof_and_lock_blocks_marker(tmp_path, monkeypatch):
    fixture, path = setup(tmp_path, monkeypatch)
    original_proof = opening._proof
    def changed_proof(review_file):
        pack = original_proof(review_file)
        with sqlite3.connect(fixture['database']) as connection:
            connection.execute('UPDATE attempts SET finished_json=? WHERE ordinal=1', ('{"status":"changed"}',))
        return pack
    monkeypatch.setattr(opening, '_proof', changed_proof)
    with pytest.raises(ValueError, match='accounting changed'):
        opening.open_calibration(path)
    assert not (tmp_path / 'fixture-main' / opening.NAMESPACE).exists()
    assert not (tmp_path / 'fixture-jev/reservation.lock').exists()


def test_failed_development_review_cannot_open_calibration(tmp_path, monkeypatch):
    _, path = setup(tmp_path, monkeypatch)
    review = budget.load(path)
    review['pipeline_verified'] = False
    write(path, cohort.seal({key: value for key, value in review.items() if key != 'artifact_hash'}))
    with pytest.raises(ValueError, match='complete verified'):
        opening.open_calibration(path)
    assert not (tmp_path / 'fixture-main' / opening.NAMESPACE).exists()


def test_complete_temp_original_sender_review_and_calibration_marker(tmp_path, monkeypatch):
    """Actual new EXE image/raw join; sources and transport stay synthetic."""
    from evals.verification.apps_stdin_v1 import development_jev as sender
    from evals.verification.apps_stdin_v1 import development_transport as transport
    from evals.verification.apps_stdin_v1 import protocol

    original_validate = protocol.validate
    fixture = original_fixture(tmp_path, monkeypatch)
    monkeypatch.setattr(protocol, 'validate', original_validate)
    monkeypatch.setattr(opening.acceptance, 'loaded_check', lambda: None)
    monkeypatch.setattr(opening, 'loaded_check', lambda: None)
    executable = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_stdin_v1_jev_development.exe'
    registered = opening.acceptance.registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    prepared = sender.prepare(registered['file'], executable)
    directories = []
    for index, item in enumerate(prepared):
        row, pack = sender.validate(item['file'])
        slot = transport._reserve(item['file'], row, pack)
        _, pack = sender.validate(item['file'])
        transport._claim(item['file'], row, pack, slot['reservation_file'])
        directories.append(finish_synthetic(row, item['file'], slot['reservation_file'],
                                            status='http_400' if index == 0 else 'ok'))
    before = opening.acceptance._image(fixture['chain'])
    fixture['calls'].clear()
    review = opening.acceptance.review(registered['file'], directories, executable)
    assert fixture['calls'] == ['full_source_proof']
    assert review['pipeline_verified'] is True and review['jev_status_counts'] == {'http_400': 1, 'ok': 7}
    assert review['development_groups_counted_in_formal_quotas'] == 0
    review_file = tmp_path / 'synthetic-complete-review.json'
    write(review_file, review)
    opened = opening.open_calibration(review_file)
    assert len(opened['opening']['cases']) == 400 and opened['opening']['holdout_opened'] is False
    assert budget.same(opening.before_first_request(opened['file'])['opening'], opened['opening'])
    assert opening.acceptance._image(fixture['chain']) == before
