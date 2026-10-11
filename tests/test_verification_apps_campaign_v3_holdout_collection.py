"""Once-source interface and full current-started intake, isolated source proofs."""
import copy

import pytest

from evals.verification.apps_campaign_v3 import holdout_collection as owner
from evals.verification.apps_campaign_v3 import holdout_intake as reader
from evals.verification.apps_campaign_v3 import prospective_protocol
from evals.verification.calibration import create, digest
from evals.verification.collection_manifest import file_hash


@pytest.fixture
def source(tmp_path, monkeypatch):
    markerfile, cohortfile, auth, evidence = [tmp_path / n for n in ('marker.json', 'cohort.json', 'auth.json', 'source.txt')]
    evidence.write_bytes(b'original source')
    auth.write_bytes(b'fixture auth')
    protocol = tmp_path / 'protocol.json'
    create(protocol, prospective_protocol.specification())
    cases = [dict(id=f'hold-{n}', group_id=f'group-{n}', task_id=n, task_sha256=digest(n),
                  source_kind='free_function', split='holdout') for n in range(12)]
    runtime = {'version_info': [3, 13, 0], 'executable_sha256': 'e' * 64}
    cohort = dict(artifact_hash='c' * 64, cases=cases, oracle_runtime_binding=runtime,
                  protocol_file=str(protocol), protocol_sha256=file_hash(protocol), bindings=[
                      dict(role='authorization', file=str(auth), sha256=file_hash(auth))])
    create(cohortfile, cohort)
    marker = dict(cohort_file=str(cohortfile), cohort_sha256=file_hash(cohortfile), cases=cases,
                  fixed_threshold=.9, artifact_hash='s' * 64,
                  source_bindings=[dict(file=str(evidence), sha256=file_hash(evidence))])
    create(markerfile, marker)
    snapshot = dict(main_started=[], main_reservations=[], jev_reservations=[], remaining_main_attempts=316)
    finished = {}
    pack = (cohort, {n: {'source': n} for n in range(12)}, 'original train', snapshot, finished)
    state = dict(pack=pack, marker=marker, proofs=0, matched={})
    def verified(path):
        state['proofs'] += 1
        return copy.deepcopy((marker, pack))
    monkeypatch.setattr(owner, 'verified_pack', verified)
    monkeypatch.setattr(owner, 'owners', lambda: [])
    monkeypatch.setattr(owner.formal_collection, '_manifest', lambda path, coho, tasks, train, chosen: {
        'schema': 'fixture original main', 'artifact_cohort_hash': coho['artifact_hash'], 'cases': copy.deepcopy(chosen)})
    monkeypatch.setattr(owner, 'budget_snapshot', lambda path: copy.deepcopy((snapshot, finished)))
    monkeypatch.setattr(reader, 'build', lambda path: {'executable_sha256': 'b' * 64})
    monkeypatch.setattr(reader.packet, 'runtime_info', lambda: runtime)
    return dict(tmp=tmp_path, markerfile=markerfile, marker=marker, cohort=cohort, cases=cases,
                snapshot=snapshot, finished=finished, state=state, evidence=evidence, auth=auth)


def start(source, index, status='transport_error'):
    case = source['cases'][index]
    snapshot = source['snapshot']
    snapshot['main_started'].append(dict(group_id=case['group_id'], ordinal=len(snapshot['main_started']) + 1,
                                        started={**case, 'artifact_cohort_hash': source['cohort']['artifact_hash']}))
    snapshot['remaining_main_attempts'] -= 1
    source['finished'][case['group_id']] = {'call_status': status} if status else None


def test_once_proof_then_fixed_prefix_no_skip_or_retry(source):
    value = owner.manifest_value(source['markerfile'])
    assert source['state']['proofs'] == 1 and value['cases'] == source['cases'][:8]
    assert value['holdout_source_hash'] == source['marker']['artifact_hash']
    path = source['tmp'] / 'manifest.json'
    create(path, value)
    for n in range(3):
        start(source, n)
    assert owner.validate_manifest(path) == value
    assert owner.manifest_value(source['markerfile'], 2)['cases'] == source['cases'][3:5]
    source['snapshot']['main_reservations'].append({'group_id': source['cases'][3]['group_id']})
    with pytest.raises(ValueError, match='cannot_skip_or_retry'):
        owner.manifest_value(source['markerfile'])


@pytest.mark.parametrize('count', [True, 0, 9])
def test_batch_limit_strict_not_authority_boolean(source, count):
    with pytest.raises(ValueError, match='upper_bound'):
        owner.manifest_value(source['markerfile'], count)


def test_missing_genuine_qualified_source_not_caller_hash(source, monkeypatch):
    monkeypatch.setattr(owner, 'verified_pack', lambda p: (_ for _ in ()).throw(ValueError('genuine proof unavailable')))
    with pytest.raises(ValueError, match='genuine proof'):
        owner.manifest_value(source['markerfile'])


def batches(source, monkeypatch, pending=False):
    for n in range(3):
        start(source, n, None if pending and n == 2 else 'transport_error')
    directories = [source['tmp'] / n for n in ('batch1', 'batch2')]
    for directory in directories:
        directory.mkdir()
        create(directory / 'receipt.json', {'call_status': 'transport_error'})
    def prepared(directory, *args):
        indexes = (0,) if directory == directories[0] else (1, 2)
        values = [(source['cases'][n], {}, {'call_status': 'transport_error'} if source['finished'][f'group-{n}'] else None,
                   'transport_error' if source['finished'][f'group-{n}'] else 'main_not_finished', directory, True) for n in indexes]
        if directory == directories[0]:
            values.append((source['cases'][1], {}, None, 'main_not_started', directory, False))
        return values, {'directory': str(directory)}
    monkeypatch.setattr(reader, '_prepared', prepared)
    return directories


def test_complete_current_started_merge_unknown_and_unstarted_separate(source, monkeypatch):
    directories = batches(source, monkeypatch)
    data = reader.collect(source['markerfile'], directories, source['auth'])
    assert data['allocated_groups'] == 12 and data['collected_groups'] == 3 and data['uncollected_groups'] == 9
    assert data['label_counts'] == {'unknown': 3, 'unlabeled': 9}
    assert data['holdout_opened'] is True and data['formal_registration'] is False
    with pytest.raises(ValueError, match='directory_omitted'):
        reader.collect(source['markerfile'], directories[1:], source['auth'])


def test_new_oracle_refuses_inflight_and_seen_probabilities(source, monkeypatch):
    directories = batches(source, monkeypatch, pending=True)
    with pytest.raises(ValueError, match='in_flight'):
        reader.collect(source['markerfile'], directories, source['auth'], execute_new_oracles=True)
    source['finished']['group-2'] = {'call_status': 'transport_error'}
    source['snapshot']['jev_reservations'].append({'group_id': 'group-0'})
    with pytest.raises(ValueError, match='after_jev'):
        reader.collect(source['markerfile'], directories, source['auth'], execute_new_oracles=True)


@pytest.mark.parametrize('change', ['source', 'marker', 'old_receipt', 'extra_file', 'budget'])
def test_mid_original_intake_byte_or_budget_drift_rejected(source, monkeypatch, change):
    directories = batches(source, monkeypatch)
    original = reader._outcome
    done = []
    def outcome(*args):
        result = original(*args)
        if not done:
            done.append(True)
            if change == 'source':
                source['evidence'].write_bytes(b'changed original source')
            elif change == 'marker':
                source['markerfile'].write_bytes(b'changed source marker')
            elif change == 'old_receipt':
                (directories[0] / 'receipt.json').write_bytes(b'changed original receipt')
            elif change == 'extra_file':
                (directories[0] / 'unbound.txt').write_bytes(b'unexpected file')
            else:
                source['snapshot']['main_reservations'].append({'group_id': 'other'})
        return result
    monkeypatch.setattr(reader, '_outcome', outcome)
    with pytest.raises(ValueError):
        reader.collect(source['markerfile'], directories, source['auth'])


def test_default_oracle_read_only_explicit_create_new_once(source, monkeypatch):
    case = source['cases'][0]
    directory = source['tmp'] / 'oracle-case'
    directory.mkdir()
    original = {'result': {'candidate': '```python\ndef f(x):\n    return x\n```'}}
    create(directory / 'receipt.json', original)
    prepared = (case, {}, original, 'ok', directory, True)
    calls = []
    def label(task, answer, record=None):
        return {'label': 'supported' if record else 'unknown', 'reason': 'recorded' if record else 'independent_original_oracle_not_recorded', 'call_eligible': bool(record)}
    def create_oracle(path, task, answer):
        calls.append(path)
        create(path, {'isolated_original': True})
    monkeypatch.setattr(reader.intake, 'label', label)
    monkeypatch.setattr(reader.intake, 'create_oracle', create_oracle)
    assert reader._outcome(case, prepared, {}, False)['label'] == 'unknown' and not calls
    assert reader._outcome(case, prepared, {}, True)['label'] == 'supported' and len(calls) == 1
    before = (directory / 'apps-holdout-oracle.json').read_bytes()
    assert reader._outcome(case, prepared, {}, True)['label'] == 'supported' and len(calls) == 1
    assert (directory / 'apps-holdout-oracle.json').read_bytes() == before
