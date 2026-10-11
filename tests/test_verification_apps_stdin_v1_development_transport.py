"""Shared temporary reservation transactions; no actual auth or paid root."""
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
)
from evals.verification.apps_stdin_v1 import (
    development_observations as observer,
)
from evals.verification.apps_stdin_v1 import (
    development_originals as originals,
)
from evals.verification.apps_stdin_v1 import (
    development_transport as transport,
)
from tests.stdin_collection_fixtures import campaign, write


def temporary_pack(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    root = tmp_path / 'fixture-jev'
    write(root / 'authorization.json', {'authorization_sha256': fixture['chain']['ledger_authorization_sha256'], 'kind': 'jev'})
    (root / 'reservations').mkdir()
    registered = root / 'stdin-development-registrations' / (fixture['value']['artifact_hash'] + '.json')
    registered.parent.mkdir()
    write(registered, {'synthetic_provenance': True, 'artifact_hash': 'f' * 64})
    current, finished = budget._snapshot_chain(fixture['chain'])
    requests = [{'id': f'fixture-{n}', 'group_id': f'fixture-new-{n}', 'request_hash': str(n + 1) * 64} for n in range(3)]
    rows = []
    for request in requests:
        row = {**request, 'output_directory': str(root / 'attempts' / request['id']),
               'sender_build_info': {'runner_exe_sha256': 'a' * 64}, 'endpoint_sha256': 'b' * 64,
               'sender_executable_file': 'synthetic-only.exe'}
        path = tmp_path / (request['id'] + '.json')
        write(path, row)
        rows.append((path, row))
    (root / 'attempts').mkdir()
    pack = {'chain': fixture['chain'], 'current': current, 'finished': finished, 'requests': requests,
            'new_reservations': [], 'registration_file': str(registered), 'registration_sha256': originals.cohort.sha(registered),
            'registration': {'artifact_hash': 'f' * 64}, 'bindings': budget.raw_bindings([registered]),
            'sql_image': budget._ledger_rows(fixture['database']), 'directories': [fixture['output']],
            'evidence': originals.development_review.evidence([fixture['output']]),
            'budget_bindings': originals.binding_images(fixture['chain'])}
    return fixture, root, rows, pack


def refresh(pack):
    pack['current'], pack['finished'] = budget._snapshot_chain(pack['chain'])
    pack['new_reservations'] = sorted(pack['current']['jev_reservations'], key=lambda v: v['reservation_index'])
    pack['budget_bindings'] = originals.binding_images(pack['chain'])


def test_unique_shared_slot_and_crash_marker(tmp_path, monkeypatch):
    fixture, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    before = budget._ledger_rows(fixture['database'])
    path, row = rows[0]
    reserved = transport._reserve(path, row, pack)
    assert reserved['reservation_index'] == 1 and reserved['automatic_retries'] == 0
    assert before == budget._ledger_rows(fixture['database'])
    refresh(pack)
    marker = transport._claim(path, row, pack, reserved['reservation_file'])
    assert Path(reserved['reservation_file']).with_suffix('.started').is_file()
    assert marker['new_paid_calls'] == 0
    refresh(pack)
    with pytest.raises(FileExistsError):
        transport._claim(path, row, pack, reserved['reservation_file'])
    with pytest.raises(ValueError):
        transport._reserve(*rows[1], pack)
    assert len(list((root / 'reservations').glob('*.json'))) == 1


def test_shared_lock_does_not_clear_stale_transaction(tmp_path, monkeypatch):
    _, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    lock = root / 'reservation.lock'
    lock.write_bytes(b'synthetic-other-process')
    with pytest.raises(FileExistsError):
        transport._reserve(*rows[0], pack)
    assert lock.read_bytes() == b'synthetic-other-process'
    assert not list((root / 'reservations').glob('*.json'))


def test_no_cross_group_skip(tmp_path, monkeypatch):
    _, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='skip/resend'):
        transport._reserve(*rows[1], pack)
    assert not list((root / 'reservations').glob('*.json'))


@pytest.mark.parametrize('status', ['http_401', 'http_403', 'http_429', 'timeout', 'transport_error'])
def test_prior_terminal_failure_retained_next_new_group_allowed(tmp_path, monkeypatch, status):
    _, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    transport._reserve(*rows[0], pack)
    refresh(pack)
    output = Path(rows[0][1]['output_directory'])
    output.mkdir()
    write(output / 'receipt.json', {'synthetic_provenance': True})
    monkeypatch.setattr(observer, 'join', lambda *args, **kwargs: {'call_status': status})
    transport._reserve(*rows[1], pack)
    assert len(list((root / 'reservations').glob('*.json'))) == 2
    refresh(pack)
    with pytest.raises(ValueError, match='skip/resend'):
        transport._reserve(*rows[0], pack)


def test_old_pending_failure_retained_and_success_prefix(tmp_path, monkeypatch):
    fixture, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    original_sql = budget._ledger_rows(fixture['database'])
    transport._reserve(*rows[0], pack)
    refresh(pack)
    output = Path(rows[0][1]['output_directory'])
    output.mkdir()
    write(output / 'receipt.json', {'synthetic_provenance': True})
    monkeypatch.setattr(observer, 'join', lambda *args, **kwargs: {'call_status': 'ok'})
    next_slot = transport._reserve(*rows[1], pack)
    assert next_slot['reservation_index'] == 2
    assert original_sql == budget._ledger_rows(fixture['database'])
    assert pack['finished']['old-pending'] is None
    assert pack['finished']['old-failed']['call_status'] == 'transport_error'
    assert len(list((root / 'reservations').glob('*.json'))) == 2


def test_metadata_changed_between_proof_and_lock_rejected(tmp_path, monkeypatch):
    _, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    write(root / 'authorization.json', {'authorization_sha256': 'a' * 64, 'kind': 'jev'})
    with pytest.raises(ValueError, match='source/accounting changed'):
        transport._reserve(*rows[0], pack)
    assert not list((root / 'reservations').glob('*.json'))


def test_actual_cap_checked_before_slot(tmp_path, monkeypatch):
    _, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    pack['chain']['authorization']['jev']['max_attempts'] = 0
    with pytest.raises(ValueError, match='budget exhausted'):
        transport._reserve(*rows[0], pack)
    assert not list((root / 'reservations').glob('*.json'))


def test_prepared_mutation_after_reservation_prevents_claim(tmp_path, monkeypatch):
    _, root, rows, pack = temporary_pack(tmp_path, monkeypatch)
    slot = transport._reserve(*rows[0], pack)
    refresh(pack)
    path, row = rows[0]
    path.write_bytes(path.read_bytes() + b' ')
    with pytest.raises(ValueError, match='compiled body changed'):
        transport._claim(path, row, pack, slot['reservation_file'])
    assert not Path(slot['reservation_file']).with_suffix('.started').exists()
    assert len(list((root / 'reservations').glob('*.json'))) == 1


def test_public_reserve_needs_genuine_validation(tmp_path, monkeypatch):
    from evals.verification.apps_stdin_v1 import development_jev

    calls = []

    def rejected(path):
        calls.append(path)
        raise ValueError('synthetic cannot become genuine')

    monkeypatch.setattr(development_jev, 'validate', rejected)
    with pytest.raises(ValueError, match='cannot become genuine'):
        transport.reserve(tmp_path / 'caller-self-signed.json')
    assert len(calls) == 1
    assert not list(tmp_path.iterdir())
