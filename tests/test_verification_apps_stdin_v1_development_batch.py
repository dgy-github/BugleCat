"""All original allocation and single fresh proof; synthetic temporary fees."""
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    protocol,
)
from evals.verification.apps_stdin_v1 import (
    development_jev as sender,
)
from evals.verification.apps_stdin_v1 import (
    development_observations as observer,
)
from evals.verification.apps_stdin_v1 import (
    development_registration as registration,
)
from evals.verification.apps_stdin_v1 import (
    development_transport as transport,
)
from tests.stdin_development_jev_fixtures import finish_synthetic
from tests.test_verification_apps_stdin_v1_development_registration import original_fixture

EXE = Path(__file__).resolve().parents[1] / 'rust/target/debug/examples/verification_apps_stdin_v1_jev_development.exe'


def test_whole_batch_real_temp_registration_slots_raw_joins_once(tmp_path, monkeypatch):
    genuine_validate = protocol.validate
    fixture = original_fixture(tmp_path, monkeypatch)
    monkeypatch.setattr(protocol, 'validate', genuine_validate)
    registered = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    prepared = sender.prepare(registered['file'], EXE)
    directories = []
    for index, item in enumerate(prepared):
        row, pack = sender.validate(item['file'])
        slot = transport._reserve(item['file'], row, pack)
        _, pack = sender.validate(item['file'])
        transport._claim(item['file'], row, pack, slot['reservation_file'])
        directories.append(finish_synthetic(row, item['file'], slot['reservation_file'], status='http_400' if index == 0 else 'ok'))
    fixture['calls'].clear()
    proof = observer.proof(registered['file'], directories, EXE)
    assert fixture['calls'] == ['full_source_proof']
    assert len(proof['observations']) == 8
    assert proof['pack']['finished']['old-pending'] is None
    assert proof['pack']['finished']['old-failed']['call_status'] == 'transport_error'
    result = observer.collect(registered['file'], directories, EXE)
    assert result['terminal_groups'] == 8 and result['pending_groups'] == 0
    assert result['status_counts'] == {'http_400': 1, 'ok': 7}
    assert result['verified_pass_signals'] == 0 and result['certified_threshold'] is None
    cases = [{'id': row['id'], 'group_id': row['group_id'], 'source_kind': row['source_kind'], 'label': row['label'],
              'candidate_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256']}
             for row in registered['registration']['cases']]
    protocol._verified(cases, result['observations'], protocol.specification())
    assert result['budget_snapshot_digest'] == budget.digest((proof['pack']['current'], proof['pack']['finished']))
    with pytest.raises(ValueError, match='skip/resend'):
        transport.reserve(prepared[0]['file'])
    with pytest.raises(ValueError, match='omitted'):
        observer.collect(registered['file'], directories[1:], EXE)


def test_pending_slot_preserved_not_fake_failure(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    registered = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    prepared = sender.prepare(registered['file'], EXE)
    row, pack = sender.validate(prepared[0]['file'])
    reserved = transport._reserve(prepared[0]['file'], row, pack)
    _, pack = sender.validate(prepared[0]['file'])
    transport._claim(prepared[0]['file'], row, pack, reserved['reservation_file'])
    result = observer.collect(registered['file'], [row['output_directory']], EXE)
    assert result['pending_groups'] == 1 and not result['observations']
    assert result['cases'][0]['jev_status'] == 'jev_pending'
    assert all(case['jev_status'] == 'jev_not_started' for case in result['cases'][1:])


def test_namespace_junction_rejected_before_marker(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    original = Path.is_junction
    monkeypatch.setattr(Path, 'is_junction', lambda path: path.name == registration.NAMESPACE or original(path))
    with pytest.raises(ValueError, match='namespace link'):
        registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    assert not (tmp_path / 'fixture-jev' / registration.NAMESPACE).exists()
