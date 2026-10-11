"""Complete synthetic archives exercise real new joins, never source authority."""
import json
import os
import time
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    formal_decision,
    formal_observations,
    formal_originals,
    holdout_opening,
)
from tests.stdin_collection_fixtures import write
from tests.stdin_formal_calibration_fixtures import (
    FORMAL_EXE,
    complete_synthetic_formal,
    full_calibration,
    restore_synthetic,
)


def test_whole400_original_archive_once_no_current_projection(tmp_path, monkeypatch):
    reused = os.environ.get('STDIN_C4C_SYNTHETIC_ARCHIVE')
    fixture = restore_synthetic(reused, monkeypatch) if reused else full_calibration(tmp_path, monkeypatch)
    fixture['calls'].clear()
    started = time.perf_counter()
    original = formal_originals.proof(fixture['formal_registration']['file'])
    elapsed = time.perf_counter() - started
    (tmp_path / 'synthetic-first-proof-timing.json').write_text(json.dumps({
        'new_original_proof_seconds': elapsed, 'actual_model_calls': 0, 'actual_jev_calls': 0}), encoding='utf-8')
    assert fixture['calls'] == ['full_source_proof']
    assert len(original['dataset']['cases']) == 808 and len(original['requests']) == 200
    assert original['dataset']['label_counts'] == {'supported': 150, 'unsupported': 50, 'unknown': 200}
    assert len(original['current']['main_started']) == 410 and len(original['current']['jev_reservations']) == 8
    assert original['finished']['old-pending'] is None
    assert original['finished']['old-failed']['call_status'] == 'transport_error'
    assert all(row['main_status'] == 'main_not_started' for row in original['dataset']['cases'][408:])
    directories = complete_synthetic_formal(fixture, monkeypatch)
    fixture['calls'].clear()
    joined = formal_observations.proof(fixture['formal_registration']['file'], directories, FORMAL_EXE)
    assert fixture['calls'] == ['full_source_proof']
    assert len(joined['observations']) == 200 and joined['observations'][0]['call_status'] == 'http_400'
    assert len(joined['pack']['current']['jev_reservations']) == 208
    assert joined['pack']['current']['main_started'] == original['current']['main_started']
    fixture['calls'].clear()
    decided = formal_decision.proof(fixture['formal_registration']['file'], directories, FORMAL_EXE)
    assert fixture['calls'] == ['full_source_proof']
    assert decided['report']['calibration_gate_met'] is True
    assert decided['report']['failed_jev_attempts'] == 1
    assert decided['report']['certified_threshold'] is None
    report_file = Path(fixture['calibration_dataset']).parent / 'synthetic-qualified-formal-report.json'
    write(report_file, decided['report'])
    before = budget._snapshot_chain(fixture['chain'])
    opened = holdout_opening.open_holdout(report_file)
    assert len(opened['opening']['cases']) == 400
    assert opened['opening']['certified_threshold'] is None
    assert budget._snapshot_chain(fixture['chain']) == before
    holdout_opening.before_first_request(opened['file'])
    with pytest.raises(FileExistsError):
        holdout_opening.open_holdout(report_file)
    (tmp_path / 'synthetic-proof-timing.json').write_text(json.dumps({
        'new_original_proof_seconds': elapsed, 'original_groups': 808, 'binary_groups': 200,
        'source_proofs_per_action': 1, 'actual_model_calls': 0, 'actual_jev_calls': 0,
        'reused_synthetic_archive': bool(reused)}), encoding='utf-8')
    # A self-sealed saved label cannot authorize a changed original candidate.
    path = fixture['calibration_dataset']
    data = budget.load(path)
    data['cases'][8]['label'] = 'unsupported'
    data.pop('artifact_hash')
    raw = path.read_bytes()
    try:
        write(path, formal_originals.cohort.seal(data))
        with pytest.raises(ValueError):
            formal_originals.proof(fixture['formal_registration']['file'])
    finally:
        path.write_bytes(raw)  # restore only this explicit self-made fixture input
