"""Development fixtures never certify natural samples or authorize transport."""
import sqlite3
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, cohort, protocol
from evals.verification.apps_stdin_v1 import development_acceptance as acceptance
from tests.stdin_collection_fixtures import write
from tests.test_verification_apps_stdin_v1_development_registration import original_fixture


def fixture():
    cases, observed = [], []
    for index in range(8):
        label = 'supported' if index < 4 else 'unsupported' if index < 6 else 'unknown'
        row = {'id': f'fixture-{index}', 'group_id': f'fixture-group-{index}',
               'split': 'development', 'source_kind': protocol.SOURCE_KIND,
               'label': label, 'call_eligible': label != 'unknown',
               'candidate_code_sha256': 'a' * 64, 'oracle_sha256': 'b' * 64}
        cases.append(row)
        if row['call_eligible']:
            observed.append({'id': row['id'], 'group_id': row['group_id'],
                'candidate_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
                'call_status': 'ok', 'model': protocol.MODEL, 'acceptance_violation': .1,
                'transport_binding_status': 'original_transport_bytes_verified',
                'request_hash': 'c' * 64, 'actual_request_hash': 'c' * 64, 'response_sha256': 'd' * 64})
    outcomes = [{**case, 'jev_status': 'ok' if case['call_eligible'] else 'jev_not_eligible'} for case in cases]
    return {'cases': cases, 'eligible_ids': [row['id'] for row in observed]}, {'observations': observed, 'cases': outcomes}


def test_development_scores_cannot_select_threshold_or_formal_quota():
    registered, observed = fixture()
    result = acceptance._decision(registered, observed, protocol.specification())
    assert result['pipeline_verified'] is True
    assert result['development_groups_counted_in_formal_quotas'] == 0
    assert result['development_data_used_for_threshold_selection'] is False
    assert result['development_label_counts'] == {'supported': 4, 'unsupported': 2, 'unknown': 2}
    changed = deepcopy(observed)
    for row in changed['observations']:
        row['acceptance_violation'] = .99
    assert acceptance._decision(registered, changed, protocol.specification()) == result


@pytest.mark.parametrize('status', ['jev_not_started', 'jev_pending'])
def test_incomplete_attempt_blocks_pipeline_review(status):
    registered, observed = fixture()
    observed['observations'].pop()
    observed['cases'][5]['jev_status'] = status
    result = acceptance._decision(registered, observed, protocol.specification())
    assert result['pipeline_verified'] is False
    assert result['reason'] == 'development_jev_incomplete'


def test_completed_transport_failures_retained_but_do_not_validate_numeric_pipeline():
    registered, observed = fixture()
    for row in observed['observations']:
        row.update(call_status='http_400', acceptance_violation=None)
    for row in observed['cases'][:6]:
        row['jev_status'] = 'http_400'
    result = acceptance._decision(registered, observed, protocol.specification())
    assert result['pipeline_verified'] is False
    assert result['terminal_jev_groups'] == 6
    assert result['jev_status_counts'] == {'http_400': 6}


@pytest.mark.parametrize('mutation', ['omit_failed_group', 'probability_on_failure', 'wrong_model', 'wrong_candidate'])
def test_original_protocol_shape_and_complete_allocation_required(mutation):
    registered, observed = fixture()
    if mutation == 'omit_failed_group':
        observed['observations'].pop()
    elif mutation == 'probability_on_failure':
        observed['observations'][0]['call_status'] = 'timeout'
        observed['cases'][0]['jev_status'] = 'timeout'
    elif mutation == 'wrong_model':
        observed['observations'][0]['model'] = 'jev-latest'
    else:
        observed['observations'][0]['candidate_sha256'] = 'e' * 64
    with pytest.raises(ValueError):
        acceptance._decision(registered, observed, protocol.specification())


def review_fixture(tmp_path, monkeypatch, mutation=None):
    """Actual temp original/accounting proof; transport delegation is synthetic."""
    original_validate = protocol.validate
    main = original_fixture(tmp_path, monkeypatch)
    monkeypatch.setattr(protocol, 'validate', original_validate)
    registered = acceptance.registration.register(main['dataset'], [main['directory']], main['binary'])
    sender_directory = tmp_path / 'synthetic-sender-directory'
    sender_directory.mkdir()
    (sender_directory / 'fixture-evidence').write_bytes(b'temporary-only')
    calls = []
    def collect(path, directories, executable):
        pack = acceptance.registration.revalidate(path)
        calls.append('fresh_original_proof')
        rows = [{key: request[key] for key in ('id', 'group_id', 'oracle_sha256', 'request_hash')}
                | {'candidate_sha256': request['candidate_code_sha256'], 'call_status': 'ok',
                   'actual_request_hash': request['request_hash'], 'response_sha256': 'd' * 64,
                   'acceptance_violation': .1, 'model': protocol.MODEL,
                   'transport_binding_status': 'original_transport_bytes_verified'} for request in pack['requests']]
        data = cohort.seal({'schema': acceptance.observations.SCHEMA, 'registration_file': pack['registration_file'],
            'registration_sha256': pack['registration_sha256'], 'registration_hash': pack['registration']['artifact_hash'],
            'cohort_hash': pack['cohort']['artifact_hash'], 'sender_build_info': {'fixture_only': True},
            'cases': [{**case, 'jev_status': 'ok'} for case in pack['registration']['cases']],
            'observations': rows, 'budget_snapshot_digest': budget.digest((pack['current'], pack['finished'])),
            'formal_registration': False, 'holdout_opened': False, 'formal_certification': False,
            'production_authorized': False, 'certified_threshold': None})
        if mutation == 'directory_set':
            (sender_directory / 'unexpected-empty-directory').mkdir()
        elif mutation == 'sql_bytes':
            with sqlite3.connect(main['database']) as connection:
                connection.execute('UPDATE attempts SET finished_json=? WHERE ordinal=1', ('{"fixture":"changed"}',))
        return data
    monkeypatch.setattr(acceptance, 'loaded_check', lambda: None)
    monkeypatch.setattr(acceptance.observations, 'collect', collect)
    return main, registered, sender_directory, calls


def test_review_transaction_is_read_only_and_saved_flags_carry_no_authority(tmp_path, monkeypatch):
    main, registered, directory, calls = review_fixture(tmp_path, monkeypatch)
    before = acceptance._image(main['chain'])
    result = acceptance.review(registered['file'], [directory], main['binary'])
    assert result['pipeline_verified'] is True
    assert all(result[key] is False for key in acceptance.NO_AUTHORITY)
    assert result['certified_threshold'] is None
    assert acceptance._image(main['chain']) == before
    assert calls == ['fresh_original_proof']
    saved = tmp_path / 'synthetic-review.json'
    write(saved, result)
    changed = deepcopy(result)
    changed.pop('artifact_hash')
    changed['calibration_opened'] = True
    write(saved, cohort.seal(changed))
    with pytest.raises(ValueError, match='fresh original evidence'):
        acceptance.revalidate(saved)
    assert acceptance._image(main['chain']) == before


@pytest.mark.parametrize('mutation', ['directory_set', 'sql_bytes'])
def test_review_closes_actual_temp_directory_and_sql_image(tmp_path, monkeypatch, mutation):
    main, registered, directory, _ = review_fixture(tmp_path, monkeypatch, mutation)
    with pytest.raises(ValueError, match='directory or accounting changed'):
        acceptance.review(registered['file'], [directory], main['binary'])
