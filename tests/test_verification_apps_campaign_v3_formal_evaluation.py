"""Original observer transaction ordering and unchanged exact numeric gates."""
import copy
import json

import pytest

from evals.verification.apps_campaign_v3 import formal_evaluation as owner
from evals.verification.apps_campaign_v3 import prospective_protocol
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash


def write(path, value):
    path.write_text(json.dumps(value), encoding='utf-8')


@pytest.fixture
def original(tmp_path, monkeypatch):
    exe, regfile, allocationfile, cohortfile, specfile, obsfile = [tmp_path / n for n in
        ('runner.exe', 'reg.json', 'allocation.json', 'cohort.json', 'protocol.json', 'observations.jsonl')]
    exe.write_bytes(b'isolated fake exe')
    for path in (regfile, allocationfile, cohortfile):
        write(path, {'fixture': True})
    write(specfile, prospective_protocol.specification())
    cases = [{'id': f'c-{i}', 'group_id': f'g-{i}', 'label': 'supported' if i < 150 else 'unsupported'} for i in range(200)]
    rows = [{'id': c['id'], 'group_id': c['group_id'], 'call_status': 'ok', 'model': 'jev-1.13.0',
             'acceptance_violation': .01 if c['label'] == 'supported' else .99,
             'request_hash': 'a' * 64, 'actual_request_hash': 'a' * 64, 'response_sha256': 'b' * 64,
             'transport_binding_status': 'original_transport_bytes_verified',
             'raw_response_verified': True, 'original_reservation_verified': True,
             'runner_executable_file': str(exe), 'prepared_file': str(tmp_path / c['id']),
             'attempt_directory': str(tmp_path / ('attempt-' + c['id']))} for c in cases]
    for row in rows:
        write(tmp_path / row['id'], {'id': row['id']})
    obsfile.write_text('\n'.join(json.dumps(r) for r in rows), encoding='utf-8')
    data = {'cohort_file': str(cohortfile), 'cohort_sha256': file_hash(cohortfile), 'protocol_file': str(specfile),
            'protocol_sha256': file_hash(specfile), 'allocated_groups': 202,
            'cases': cases + [{'id': 'unknown-a', 'label': 'unknown'}, {'id': 'unknown-b', 'label': 'unknown'}]}
    datasetfile = tmp_path / 'dataset.json'
    write(datasetfile, data)
    evidence = tmp_path / 'original-oracle.json'
    write(evidence, {'original': True})
    reg = {'artifact_hash': 'r' * 64, 'cohort_hash': 'c' * 64, 'eligible_ids': [c['id'] for c in cases],
           'dataset_file': str(datasetfile), 'dataset_sha256': file_hash(datasetfile), 'authorization_file': 'actual approved auth',
           'source_bindings': [], 'original_evidence_bindings': [{'file': str(evidence), 'sha256': file_hash(evidence)}]}
    allocation = {'registration_hash': reg['artifact_hash'], 'registration_sha256': file_hash(regfile), 'artifact_hash': 'p' * 64}
    state = {'proofs': 0, 'joins': 0, 'rows': rows}
    def transaction(path):
        state['proofs'] += 1
        return (reg, data, cases), ('actual verified cohort', {}, 'train', {'real snapshot': True}, {'real finishes': True})
    monkeypatch.setattr(owner.formal_registration, 'proof_transaction', transaction)
    monkeypatch.setattr(owner.formal_registration.formal_collection, 'budget_snapshot',
                        lambda auth: ({'real snapshot': True}, {'real finishes': True}))
    monkeypatch.setattr(owner.formal_jev, 'validate_pack', lambda *a, **k: allocation)
    monkeypatch.setattr(owner.formal_jev, 'build_info', lambda p: {'actual_compiled': True})
    monkeypatch.setattr(owner.formal_jev, 'runner_sources', lambda: {'fixture': exe})
    monkeypatch.setattr(owner.formal_jev, 'validate', lambda p, **k: json.loads(p.read_text()) if hasattr(p, 'read_text') else json.loads(open(p, encoding='utf8').read()))
    def join(value, *a, **k):
        state['joins'] += 1
        return copy.deepcopy(next(r for r in state['rows'] if r['id'] == value['id']))
    monkeypatch.setattr(owner.formal_jev, '_observe', join)
    return dict(reg=regfile, allocation=allocationfile, obs=obsfile, exe=exe, state=state, evidence=evidence)


def value(fixture):
    return owner.calibration_value(fixture['reg'], fixture['allocation'], fixture['obs'], fixture['exe'])


def test_one_original_transaction_each_raw_join_unknown_independent(original):
    result = value(original)
    assert original['state']['proofs'] == 1 and original['state']['joins'] == 200
    assert result['allocated_groups'] == 202 and result['eligible_groups'] == 200
    assert result['label_counts']['unknown'] == 2
    assert result['qualified_for_once_holdout'] is True and result['qualified_for_holdout_threshold'] in prospective_protocol.GRID
    assert result['actual_holdout_source_authorization'] is False
    assert result['certified_threshold'] is None and result['production_authorized'] is False
    assert result['native_claim_certification'] is False and result['overall_agent_benefit_certification'] is False
    assert len(result['grid_results']) == 41


@pytest.mark.parametrize('change', ['missing', 'order', 'raw_probability', 'raw_status', 'wrong_exe'])
def test_original_observation_omission_or_tampering_refused(original, change):
    rows = copy.deepcopy(original['state']['rows'])
    if change == 'missing':
        rows.pop()
    elif change == 'order':
        rows.reverse()
    elif change == 'raw_probability':
        rows[0]['acceptance_violation'] = .9
    elif change == 'raw_status':
        rows[0]['call_status'] = 'timeout'
    else:
        rows[0]['runner_executable_file'] = str(original['reg'])
    original['obs'].write_text('\n'.join(json.dumps(r) for r in rows), encoding='utf-8')
    with pytest.raises(ValueError):
        value(original)


def test_failure_counts_in_denominator_no_certification(original):
    rows = original['state']['rows']
    for row in rows[150:]:
        row['call_status'] = 'timeout'
        row['acceptance_violation'] = None
        row['model'] = None
    original['obs'].write_text('\n'.join(json.dumps(r) for r in rows), encoding='utf-8')
    result = value(original)
    assert result['qualified_for_once_holdout'] is False and result['qualified_for_holdout_threshold'] is None
    assert all(r['unsupported_groups'] == 50 and r['detected_unsupported'] == 0 for r in result['grid_results'])
    assert result['original_observations'] == 200


def test_resealed_threshold_or_authority_cannot_change_original_result(original):
    path = original['reg'].parent / 'calibration-result.json'
    actual = value(original)
    actual['qualified_for_holdout_threshold'] = .5
    actual['artifact_hash'] = digest({k: v for k, v in actual.items() if k != 'artifact_hash'})
    write(path, actual)
    with pytest.raises(ValueError, match='original_calibration'):
        owner.revalidate_calibration(path)


def test_original_already_read_evidence_changes_during_later_join_refused(original, monkeypatch):
    join = owner.formal_jev._observe
    def changed(*args, **kwargs):
        result = join(*args, **kwargs)
        if original['state']['joins'] == 100:
            original['evidence'].write_bytes(b'changed after earlier original read')
        return result
    monkeypatch.setattr(owner.formal_jev, '_observe', changed)
    with pytest.raises(ValueError, match='registered_originals_or_budget'):
        value(original)


def test_budget_changes_during_original_joins_refused(original, monkeypatch):
    monkeypatch.setattr(owner.formal_registration.formal_collection, 'budget_snapshot', lambda p: ({'changed': True}, {}))
    with pytest.raises(ValueError, match='registered_originals_or_budget'):
        value(original)
