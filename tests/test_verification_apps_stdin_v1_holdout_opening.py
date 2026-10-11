"""One marker lifecycle on explicit fake observation and real temporary SQL."""
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import budget, cohort, development_originals
from evals.verification.apps_stdin_v1 import holdout_opening as opening
from tests.stdin_collection_fixtures import write
from tests.test_verification_apps_stdin_v1_formal_decision import replay_fixture


def setup(tmp_path, monkeypatch):
    registration, executable, joined, calls, database = replay_fixture(tmp_path, monkeypatch)
    report = opening.decision.value(registration, joined['directories'], executable)
    report_file = tmp_path / 'synthetic-qualified-decision.json'
    write(report_file, report)
    monkeypatch.setattr(opening, 'loaded_check', lambda: None)
    calls.clear()
    return report_file, joined, calls, database


def test_unique_holdout_freezes_original_400_and_threshold_without_certificate(tmp_path, monkeypatch):
    report_file, joined, calls, database = setup(tmp_path, monkeypatch)
    before_sql = budget._ledger_rows(database)
    before_raw = development_originals.binding_images(joined['pack']['chain'])
    result = opening.open_holdout(report_file)
    assert calls == ['explicit_fake_observer']
    marker = result['opening']
    assert marker['allocation_groups'] == len(marker['cases']) == 400
    assert marker['qualified_calibration_threshold'] == .8 and marker['threshold_frozen_before_holdout'] is True
    assert marker['holdout_max_consumptions'] == 1 and marker['holdout_opened'] is True
    assert marker['certified_threshold'] is None and marker['formal_certification'] is False
    assert marker['production_authorized'] is False
    assert budget.same(opening.before_first_request(result['file'])['opening'], marker)
    with pytest.raises(FileExistsError):
        opening.open_holdout(report_file)
    assert budget._ledger_rows(database) == before_sql
    assert development_originals.binding_images(joined['pack']['chain']) == before_raw
    jev = Path(joined['pack']['chain']['authorization']['jev']['ledger_dir'])
    assert not (jev / 'reservation.lock').exists() and not (jev / 'reservations').exists()


@pytest.mark.parametrize('change', ['main_funds', 'jev_funds', 'already_started', 'missing_group'])
def test_whole_fixed_holdout_must_remain_unopened_and_funded(tmp_path, monkeypatch, change):
    report_file, joined, _, _ = setup(tmp_path, monkeypatch)
    if change == 'main_funds':
        joined['pack']['current']['remaining_main_attempts'] = 399
    elif change == 'jev_funds':
        joined['pack']['chain']['authorization']['jev']['max_attempts'] = 399
    elif change == 'already_started':
        joined['pack']['current']['excluded_groups'].append(joined['pack']['cohort']['cases'][-1]['group_id'])
    else:
        joined['pack']['cohort']['cases'].pop()
    # A genuine changed current accounting also changes the read-only report.
    # Re-render this explicit fixture so the opening's own funding check is hit.
    if change != 'missing_group':
        registered = joined['pack']['registration_file']
        executable = budget.load(report_file)['sender_executable_file']
        write(report_file, opening.decision.value(registered, joined['directories'], executable))
    with pytest.raises(ValueError):
        opening.open_holdout(report_file)
    assert not opening.marker_path(joined['pack']['chain'], joined['pack']['cohort']['artifact_hash']).exists()


def test_saved_numeric_claim_and_altered_threshold_cannot_open_holdout(tmp_path, monkeypatch):
    report_file, joined, _, _ = setup(tmp_path, monkeypatch)
    saved = budget.load(report_file)
    saved['qualified_calibration_threshold'] = .99
    saved.pop('artifact_hash')
    write(report_file, cohort.seal(saved))
    with pytest.raises(ValueError, match='fresh qualified calibration'):
        opening.open_holdout(report_file)
    assert not opening.marker_path(joined['pack']['chain'], joined['pack']['cohort']['artifact_hash']).exists()


def test_caller_namespace_and_stranded_real_marker_cannot_repeat(tmp_path, monkeypatch):
    report_file, joined, _, _ = setup(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='fixed original shared namespace'):
        opening.open_holdout(report_file, tmp_path / 'caller-opening.json')
    target = opening.marker_path(joined['pack']['chain'], joined['pack']['cohort']['artifact_hash'])
    target.parent.mkdir()
    target.write_bytes(b'stranded create-new marker; do not repair or overwrite')
    before = target.read_bytes()
    with pytest.raises(FileExistsError):
        opening.open_holdout(report_file)
    assert target.read_bytes() == before


@pytest.mark.parametrize('change', ['raw_budget', 'archive_bytes', 'sender_directory'])
def test_actual_temp_file_drift_between_proof_and_locked_write_rejected(tmp_path, monkeypatch, change):
    report_file, joined, _, _ = setup(tmp_path, monkeypatch)
    genuine_proof = opening._proof

    def changed_proof(*args):
        proved = genuine_proof(*args)
        if change == 'raw_budget':
            root = Path(joined['pack']['chain']['authorization']['jev']['ledger_dir'])
            path = root / 'authorization.json'
            path.write_bytes(path.read_bytes() + b'\n')
        elif change == 'archive_bytes':
            path = Path(joined['pack']['directory_images'][0][0][0]) / 'fixture.txt'
            path.write_bytes(b'changed old archive')
        else:
            (Path(joined['directories'][0]) / 'unrecorded-empty-directory').mkdir()
        return proved

    monkeypatch.setattr(opening, '_proof', changed_proof)
    with pytest.raises(ValueError):
        opening.open_holdout(report_file)
    assert not opening.marker_path(joined['pack']['chain'], joined['pack']['cohort']['artifact_hash']).exists()
    assert not (Path(joined['pack']['chain']['authorization']['jev']['ledger_dir']) / 'reservation.lock').exists()
