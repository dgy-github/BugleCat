"""Registration transaction tests; source/intake are isolated trusted fixtures."""
import copy
import json
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import formal_registration as owner
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash


def write(path, value):
    path.write_text(json.dumps(value), encoding='utf-8')


def seal(value):
    value = {k: v for k, v in value.items() if k != 'artifact_hash'}
    return {**value, 'artifact_hash': digest(value)}


@pytest.fixture
def original(tmp_path, monkeypatch):
    root = tmp_path / 'jev'
    (root / 'reservations').mkdir(parents=True)
    auth = tmp_path / 'auth.json'
    protocol = tmp_path / 'protocol.json'
    cohort_file = tmp_path / 'cohort.json'
    dataset_file = tmp_path / 'dataset.json'
    evidence = tmp_path / 'main'
    evidence.mkdir()
    write(evidence / 'receipt.json', {'raw': 'original'})
    write(auth, {'approved': True})
    write(protocol, owner.prospective_protocol.specification())
    binary = [dict(id=f'case-{n}', group_id=f'group-{n}', label='supported' if n < 150 else 'unsupported',
                   call_eligible=True, main_status='ok', split='calibration') for n in range(200)]
    prior = [dict(group_id=f'old-{n}', reservation_index=n + 1) for n in range(2)]
    baseline = dict(jev_reservations=prior, authorization_chain=dict(
        authorization={'jev': {'ledger_dir': str(root)}}, current_sha256=file_hash(auth),
        ledger_authorization_sha256='original-shared'))
    for row in prior:
        write(root / 'reservations' / (digest(row['group_id']) + '.json'), row)
    write(root / 'authorization.json', {'authorization_sha256': 'original-shared', 'kind': 'jev'})
    cohort = dict(cases=binary, bindings=[dict(role='authorization', file=str(auth), sha256=file_hash(auth))],
                  ledger_snapshot=copy.deepcopy(baseline))
    cohort = seal(cohort)
    write(cohort_file, cohort)
    data = dict(schema=owner.formal_intake.SCHEMA, split='calibration', cases=binary,
        collection_bindings=[dict(directory=str(evidence))], collector_executable_file=str(auth),
        cohort_file=str(cohort_file), cohort_sha256=file_hash(cohort_file), protocol_file=str(protocol),
        protocol_sha256=file_hash(protocol), allocated_groups=200, collected_groups=200,
        uncollected_groups=0, pending_groups=0, formal_registration=False, holdout_opened=False,
        formal_certification=False, production_authorized=False, certified_threshold=None,
        scores_hidden=True, jev_probabilities_read=False)
    data = seal(data)
    write(dataset_file, data)
    state = dict(data=data, cohort=cohort, baseline=baseline)
    monkeypatch.setattr(owner.formal_intake, 'collect', lambda *a, **kw: copy.deepcopy(state['data']))
    monkeypatch.setattr(owner.formal_collection, 'validate_cohort', lambda *a: copy.deepcopy(state['cohort']))
    monkeypatch.setattr(owner.formal_collection, 'budget_snapshot', lambda *a: (
        copy.deepcopy(state['baseline']), {r['group_id']: {'status': 'ok'} for r in state['data']['cases']}))
    return dict(root=root, state=state, dataset=dataset_file, auth=auth, evidence=evidence)


def test_registration_creates_once_in_shared_namespace_and_revalidates_without_execution(original):
    value = owner.register(original['dataset'])
    assert Path(value['registration_file']).is_file()
    assert not (original['root'] / 'reservation.lock').exists()
    assert len(list((original['root'] / 'reservations').glob('*.json'))) == 2
    assert value['oracle_executions'] == value['jev_calls'] == value['model_calls'] == 0
    assert owner.revalidate(value['registration_file'])[0] == value
    with pytest.raises(ValueError, match='already_consumed'):
        owner.register(original['dataset'])


def test_stale_lock_preserved_without_registration_or_paid_slots(original):
    lock = original['root'] / 'reservation.lock'
    lock.write_text('existing-owner')
    with pytest.raises(FileExistsError):
        owner.register(original['dataset'])
    assert lock.read_text() == 'existing-owner'
    assert not (original['root'] / owner.NAMESPACE).exists()


@pytest.mark.parametrize('mutation', ['quota', 'pending', 'uncollected', 'score', 'intake_mismatch', 'reservation'])
def test_prediction_blind_whole_terminal_quota_required(original, mutation):
    data = original['state']['data']
    if mutation == 'quota':
        data['cases'][-1].update(label='unknown', call_eligible=False)
    elif mutation == 'pending':
        data['pending_groups'] = 1
    elif mutation == 'uncollected':
        data['uncollected_groups'] = 1
    elif mutation == 'score':
        data['jev_probabilities_read'] = True
    elif mutation == 'reservation':
        original['state']['baseline']['jev_reservations'].append(dict(group_id='group-0', reservation_index=3))
    else:
        original['state']['data'] = {**data, 'extra': 'changed'}
    if mutation != 'intake_mismatch':
        original['state']['data'] = seal(data)
        write(original['dataset'], original['state']['data'])
    with pytest.raises(ValueError):
        owner.register(original['dataset'])
    assert not (original['root'] / owner.NAMESPACE).exists()


@pytest.mark.parametrize('mutation', ['auth_alias', 'deleted_prefix', 'noncontiguous_prefix', 'raw_evidence', 'owner'])
def test_resealed_or_raw_registration_mutations_fail(original, mutation):
    value = owner.register(original['dataset'])
    marker = Path(value['registration_file'])
    if mutation == 'auth_alias':
        alias = original['auth'].with_name('alias.json')
        alias.write_bytes(original['auth'].read_bytes())
        value['authorization_file'] = str(alias)
    elif mutation == 'deleted_prefix':
        value['original_jev_reservations'] = []
        value['original_jev_reservation_count'] = 0
    elif mutation == 'noncontiguous_prefix':
        value['original_jev_reservations'] = value['original_jev_reservations'][1:]
        value['original_jev_reservation_count'] = 1
    elif mutation == 'raw_evidence':
        write(original['evidence'] / 'receipt.json', {'raw': 'tampered'})
    else:
        value['source_bindings'][0]['sha256'] = 'f' * 64
    write(marker, seal(value))
    with pytest.raises(ValueError):
        owner.revalidate(marker)


def test_appended_failed_reservation_remains_in_prefix_and_cannot_repeat(original):
    value = owner.register(original['dataset'])
    row = dict(group_id='group-0', reservation_index=3, failed=True)
    original['state']['baseline']['jev_reservations'].append(row)
    actual, _, _ = owner.revalidate(value['registration_file'])
    assert actual == value
    assert original['state']['baseline']['jev_reservations'][-1]['failed'] is True


def test_registration_stops_when_namespace_changes_between_proof_and_lock(original, monkeypatch):
    actual = owner.registration_value
    def drift(path):
        value = actual(path)
        write(original['root'] / 'reservations' / (digest('new') + '.json'),
              dict(group_id='new', reservation_index=3))
        return value
    monkeypatch.setattr(owner, 'registration_value', drift)
    with pytest.raises(ValueError, match='namespace_changed'):
        owner.register(original['dataset'])
    assert not (original['root'] / 'reservation.lock').exists()
    assert not (original['root'] / owner.NAMESPACE).exists()
