"""Temporary fixture accounting only; no actual source approval or paid root."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    collection,
    intake,
)
from evals.verification.apps_stdin_v1 import (
    development_originals as originals,
)
from evals.verification.apps_stdin_v1 import (
    development_registration as registration,
)
from tests.stdin_collection_fixtures import append, campaign, write


def original_fixture(tmp_path, monkeypatch, *, unknown_last=False):
    fixture = campaign(tmp_path, monkeypatch)
    write(tmp_path / 'fixture-jev/authorization.json',
          {'authorization_sha256': fixture['chain']['ledger_authorization_sha256'], 'kind': 'jev'})
    value, path = fixture['value'], fixture['path']
    directory = tmp_path / 'prepared'
    directory.mkdir()
    manifest = collection.manifest_value(path, 8)
    write(directory / 'collection-manifest.json', manifest)
    rows, answers = [], {}
    for case in value['cases'][:8]:
        case_dir = directory / case['id']
        case_dir.mkdir()
        write(case_dir / 'receipt.json', {'fixture_only': True})
        append(fixture, {**case, 'case_directory': str(case_dir)})
        rows.append(case)
        task = fixture['pack']['tasks'][case['task_id']]
        # Self-made echo fixtures, not public APPS references or candidates.
        code = 'print(missing_implicit_dependency)\n' if unknown_last and case['task_id'] == 7 else 'print(input())\n'
        answers[case['id']] = '```python\n' + code + '```\nNot executed by a model.'
        intake._oracle(task, answers[case['id']], case_dir, True)
    (directory / 'prepared.jsonl').write_bytes(b'\n'.join(intake.packet.canonical_bytes(row) for row in rows) + b'\n')
    monkeypatch.setattr(intake, 'loaded_check', lambda: intake.packet.runtime_info())
    monkeypatch.setattr(intake, 'build', lambda _: {'fixture_only': True})
    monkeypatch.setattr(intake, '_prepared', lambda *args: None)
    monkeypatch.setattr(intake, 'verify', lambda *args: None)
    monkeypatch.setattr(intake, 'terminal', lambda *args: ({'result': {'candidate': answers[args[0]['id']]}}, 'ok'))
    monkeypatch.setattr(originals, 'loaded_check', lambda: None)
    monkeypatch.setattr(registration, 'loaded_check', lambda: None)
    binary = value['collector_executable_file']
    data = intake.collect(path, [directory], binary)
    dataset = tmp_path / 'original-dataset.json'
    write(dataset, data)
    fixture.update(directory=directory, dataset=dataset, binary=binary)
    return fixture


def test_originals_once_proof_real_current_budget(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    fixture['calls'].clear()
    pack = originals.proof(fixture['dataset'], [fixture['directory']], fixture['binary'])
    assert fixture['calls'] == ['full_source_proof']
    assert len(pack['dataset']['cases']) == 808 and len(pack['requests']) == 8
    assert pack['finished']['old-pending'] is None
    assert pack['finished']['old-failed']['call_status'] == 'transport_error'


def test_real_shared_marker_durable_once(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    result = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    pack = registration.revalidate(result['file'])
    assert pack['registration_sha256'] == result['sha256']
    assert pack['registration']['whole_original_allocation'] == 808
    assert not pack['registration']['formal_registration']
    with pytest.raises(FileExistsError):
        registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])


@pytest.mark.parametrize('change', ['label', 'span', 'split', 'drop', 'seen'])
def test_saved_labels_source_shape_cannot_authorize(tmp_path, monkeypatch, change):
    fixture = original_fixture(tmp_path, monkeypatch)
    data = budget.load(fixture['dataset'])
    if change == 'label':
        data['cases'][0]['label'] = 'unsupported'
    elif change == 'span':
        data['cases'][0]['candidate_code_span']['code'] = 'print("fake")\n'
    elif change == 'split':
        data['cases'][0]['split'] = 'calibration'
    elif change == 'drop':
        data['cases'].pop()
    else:
        data['jev_probabilities_read'] = True
    data.pop('artifact_hash')
    write(fixture['dataset'], originals.cohort.seal(data))
    with pytest.raises(ValueError):
        registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    assert not list((tmp_path / 'fixture-jev').rglob('stdin-development-registrations/*.json'))


def test_read_after_genuine_new_reservation_not_zero_projection(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    result = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    pack = registration.revalidate(result['file'])
    request = pack['requests'][0]
    folder = tmp_path / 'fixture-jev/reservations'
    folder.mkdir()
    row = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
           'group_id': request['group_id'], 'request_hash': request['request_hash'],
           'authorization_sha256': pack['chain']['current_sha256'], 'reservation_index': 1,
           'status': 'reserved_before_transport', 'automatic_retries': 0,
           'registration_file': result['file'], 'registration_sha256': result['sha256'],
           'registration_hash': result['registration']['artifact_hash']}
    write(folder / (budget.digest(row['group_id']) + '.json'), row)
    actual = registration.revalidate(result['file'])
    assert actual['current']['jev_reservations'] == [row]
    assert actual['new_reservations'] == [row]
    assert len(actual['current']['main_started']) == 10  # preserves pending/failure + 8
    row['request_hash'] = 'f' * 64
    write(folder / (budget.digest(row['group_id']) + '.json'), row)
    with pytest.raises(ValueError, match='actual reserved body'):
        registration.revalidate(result['file'])


def test_fixed_shared_namespace_no_caller_marker(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    with pytest.raises(ValueError, match='fixed shared namespace'):
        registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'], tmp_path / 'fake-marker.json')


def test_source_mutation_between_proof_and_lock(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    proof = originals.proof

    def mutate(*args):
        pack = proof(*args)
        (fixture['directory'] / 'new-file').write_text('changed', encoding='utf-8')
        return pack

    monkeypatch.setattr(originals, 'proof', mutate)
    with pytest.raises(ValueError, match='directory bytes changed'):
        registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])


def test_missing_oracle_crash_marker_not_rerun(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    case = fixture['value']['cases'][0]
    receipt = fixture['directory'] / case['id'] / 'stdin-candidate-oracle.json'
    receipt.unlink()
    with pytest.raises(ValueError, match='absent/stranded'):
        originals.proof(fixture['dataset'], [fixture['directory']], fixture['binary'])


def test_self_signed_registration_not_original(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch)
    result = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    value = deepcopy(result['registration'])
    value.pop('artifact_hash')
    value['cases'][0]['label'] = 'unsupported'
    write(result['file'], originals.cohort.seal(value))
    with pytest.raises(ValueError, match='labels/bytes'):
        registration.revalidate(result['file'])


def test_unknown_original_retained_never_eligible(tmp_path, monkeypatch):
    fixture = original_fixture(tmp_path, monkeypatch, unknown_last=True)
    result = registration.register(fixture['dataset'], [fixture['directory']], fixture['binary'])
    pack = registration.revalidate(result['file'])
    assert len(pack['requests']) == 7 and len(pack['dataset']['cases']) == 808
    assert pack['registration']['cases'][7]['label'] == 'unknown'
    assert pack['registration']['cases'][7]['call_eligible'] is False
    assert pack['dataset']['uncollected_groups'] == 800
