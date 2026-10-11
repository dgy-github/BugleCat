"""Original observer transaction ordering and unchanged exact numeric gates."""
import copy
import json

import pytest

from evals.verification.apps_campaign_v3 import holdout_evaluation as owner
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
    markerfile = tmp_path / 'source-marker.json'
    write(markerfile, {'actual_original_once': True})
    data = {'cohort_file': str(cohortfile), 'cohort_sha256': file_hash(cohortfile), 'protocol_file': str(specfile),
            'protocol_sha256': file_hash(specfile), 'allocated_groups': 202, 'collected_groups': 202,
            'cohort_hash': 'c' * 64, 'holdout_source_file': str(markerfile), 'holdout_source_sha256': file_hash(markerfile), 'holdout_source_hash': 's' * 64,
            'cases': cases + [{'id': 'unknown-a', 'label': 'unknown'}, {'id': 'unknown-b', 'label': 'unknown'}]}
    datasetfile = tmp_path / 'dataset.json'
    write(datasetfile, data)
    evidence = tmp_path / 'original-oracle.json'
    write(evidence, {'original': True})
    reg = {'artifact_hash': 'r' * 64, 'cohort_hash': 'c' * 64, 'eligible_ids': [c['id'] for c in cases],
           'fixed_threshold': .9, 'dataset_file': str(datasetfile), 'dataset_sha256': file_hash(datasetfile), 'authorization_file': 'actual approved auth',
           'source_bindings': [], 'original_evidence_bindings': [{'file': str(evidence), 'sha256': file_hash(evidence)}]}
    allocation = {'registration_hash': reg['artifact_hash'], 'registration_sha256': file_hash(regfile), 'artifact_hash': 'p' * 64}
    state = {'proofs': 0, 'joins': 0, 'rows': rows}
    def transaction(path):
        state['proofs'] += 1
        return (reg, data, cases), ('actual verified cohort', {}, 'train', {'real snapshot': True}, {'real finishes': True})
    monkeypatch.setattr(owner.holdout_registration, 'proof_transaction', transaction)
    monkeypatch.setattr(owner.holdout_collection, 'budget_snapshot',
                        lambda auth: ({'real snapshot': True}, {'real finishes': True}))
    monkeypatch.setattr(owner.holdout_jev, 'validate_pack', lambda *a, **k: allocation)
    monkeypatch.setattr(owner.holdout_jev, 'build_info', lambda p: {'actual_compiled': True})
    monkeypatch.setattr(owner.holdout_jev, 'runner_sources', lambda: {'fixture': exe})
    monkeypatch.setattr(owner.holdout_jev, 'validate', lambda p, **k: json.loads(p.read_text()) if hasattr(p, 'read_text') else json.loads(open(p, encoding='utf8').read()))
    def join(value, *a, **k):
        state['joins'] += 1
        return copy.deepcopy(next(r for r in state['rows'] if r['id'] == value['id']))
    monkeypatch.setattr(owner.holdout_jev, '_observe', join)
    return dict(reg=regfile, allocation=allocationfile, obs=obsfile, exe=exe, state=state, evidence=evidence, reg_value=reg)


def value(fixture):
    return owner.evaluation_value(fixture['reg'], fixture['allocation'], fixture['obs'], fixture['exe'])


def test_one_actual_source_transaction_each_raw_join_fixed_threshold_unknown_independent(original):
    result = value(original)
    assert original['state']['proofs'] == 1 and original['state']['joins'] == 200
    assert result['allocated_groups'] == result['collected_groups'] == 202 and result['eligible_groups'] == 200
    assert result['label_counts']['unknown'] == 2 and result['original_observations'] == 200
    assert result['fixed_threshold'] == .9 and result['holdout_status'] == 'passed'
    assert result['APPS_ordered_pair_artifact_threshold_certified'] is True
    assert result['certified_APPS_ordered_pair_artifact_threshold'] == .9
    assert result['certified_threshold'] is None and result['production_authorized'] is False
    assert result['formal_certification'] is False and result['overall_goal_complete'] is False
    assert result['native_claim_certification'] is False and result['overall_agent_benefit_certification'] is False
    assert 'grid_results' not in result and result['threshold_retuning_allowed'] is False


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


def test_failures_remain_unsupported_denominator_no_certificate(original):
    rows = original['state']['rows']
    for row in rows[150:]:
        row.update(call_status='timeout', acceptance_violation=None, model=None)
    original['obs'].write_text('\n'.join(json.dumps(r) for r in rows), encoding='utf-8')
    result = value(original)
    assert result['holdout_status'] == 'failed' and result['certified_APPS_ordered_pair_artifact_threshold'] is None
    assert result['unsupported_groups'] == 50 and result['detected_unsupported'] == 0
    assert result['original_observations'] == 200 and result['label_counts']['unknown'] == 2


def test_original_already_read_evidence_changes_during_later_join_refused(original, monkeypatch):
    join = owner.holdout_jev._observe
    def changed(*args, **kwargs):
        result = join(*args, **kwargs)
        if original['state']['joins'] == 100:
            original['evidence'].write_bytes(b'changed after earlier original read')
        return result
    monkeypatch.setattr(owner.holdout_jev, '_observe', changed)
    with pytest.raises(ValueError, match='evidence_or_budget'):
        value(original)


def test_budget_changes_during_original_joins_refused(original, monkeypatch):
    monkeypatch.setattr(owner.holdout_collection, 'budget_snapshot', lambda p: ({'changed': True}, {}))
    with pytest.raises(ValueError, match='evidence_or_budget'):
        value(original)


def test_shared_final_once_consumption_and_original_revalidation(original, monkeypatch):
    target = original['reg'].parent / 'shared' / 'scope.json'
    monkeypatch.setattr(owner, '_target', lambda r: target)
    result = owner.evaluate(original['reg'], original['allocation'], original['obs'], original['exe'])
    assert target.is_file() and target.with_suffix('.started').is_file()
    assert owner.revalidate_evaluation(target) == result
    with pytest.raises(FileExistsError):
        owner.evaluate(original['reg'], original['allocation'], original['obs'], original['exe'])
    actual = json.loads(target.read_bytes())
    actual['fixed_threshold'] = .5
    actual['artifact_hash'] = digest({k: v for k, v in actual.items() if k != 'artifact_hash'})
    write(target, actual)
    with pytest.raises(ValueError, match='once_result'):
        owner.revalidate_evaluation(target)


def test_failed_consumption_stays_no_retry_or_alternate_output(original, monkeypatch):
    target = original['reg'].parent / 'shared' / 'scope.json'
    monkeypatch.setattr(owner, '_target', lambda r: target)
    original['obs'].write_text('', encoding='utf-8')
    with pytest.raises(ValueError):
        owner.evaluate(original['reg'], original['allocation'], original['obs'], original['exe'])
    assert target.with_suffix('.started').is_file() and target.with_suffix('.failed').is_file() and not target.exists()
    with pytest.raises(FileExistsError):
        owner.evaluate(original['reg'], original['allocation'], original['obs'], original['exe'])


def test_copied_result_cannot_be_used_as_shared_certificate(original, monkeypatch):
    target = original['reg'].parent / 'shared' / 'scope.json'
    monkeypatch.setattr(owner, '_target', lambda r: target)
    owner.evaluate(original['reg'], original['allocation'], original['obs'], original['exe'])
    alias = original['reg'].parent / 'copied-result.json'
    alias.write_bytes(target.read_bytes())
    with pytest.raises(ValueError, match='shared_once_result'):
        owner.revalidate_evaluation(alias)


def test_non_predeclared_locked_threshold_refused(original):
    original['reg_value']['fixed_threshold'] = .123456
    with pytest.raises(ValueError, match='predeclared'):
        value(original)
