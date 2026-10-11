"""Synthetic score policy and read-only orchestration, not natural accuracy."""
import sqlite3
from collections import Counter
from copy import deepcopy
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    cohort,
    development_originals,
    development_review,
    protocol,
)
from evals.verification.apps_stdin_v1 import formal_decision as decision
from tests.stdin_collection_fixtures import write
from tests.test_verification_apps_stdin_v1_calibration_terminal import fixture


def policy_fixture():
    fixed, outcomes, spec = fixture()
    campaign = {'cases': fixed}
    counts = Counter(row['label'] for row in outcomes if row['split'] == 'calibration')
    dataset = {'cases': outcomes, 'allocated_groups': 808, 'calibration_allocated_groups': 400,
        'calibration_collected_groups': 400, 'calibration_terminal_groups': 400,
        'calibration_pending_groups': 0, 'calibration_uncollected_groups': 0,
        'unresolved_oracle_groups': 0, 'labels_ready': True, 'label_counts': dict(counts)}
    requests = [{key: row[key] for key in (*decision.collection.history_fields(),
                  'label', 'candidate_code_sha256', 'oracle_sha256')}
                for row in outcomes if row['split'] == 'calibration' and row['call_eligible']]
    for index, row in enumerate(requests):
        row['request_hash'] = budget.digest({'synthetic_request': index})
    registered = {'eligible_ids': [row['id'] for row in requests],
                  'request_hashes': [row['request_hash'] for row in requests]}
    observations = [{'id': row['id'], 'group_id': row['group_id'],
        'candidate_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
        'request_hash': row['request_hash'], 'actual_request_hash': row['request_hash'],
        'response_sha256': 'c' * 64, 'model': protocol.MODEL,
        'transport_binding_status': 'original_transport_bytes_verified',
        'call_status': 'ok', 'acceptance_violation': .01 if row['label'] == 'supported' else .8}
        for row in requests]
    return campaign, dataset, registered, requests, observations, spec


def test_perfect_partial_prefix_does_not_select_threshold_or_open_holdout():
    campaign, dataset, registered, requests, observations, spec = policy_fixture()
    result = decision._decision(campaign, dataset, registered, requests, observations[:-1], spec)
    assert result['status'] == 'formal_jev_incomplete'
    assert result['numeric_results'] is result['qualified_calibration_threshold'] is None
    assert result['calibration_gate_met'] is False and result['unresolved_binary_groups'] == 1
    assert all(result[key] is False for key in decision.FALSE_FLAGS)


def test_whole_binary_terminal_retains_unknown_and_freezes_grid_tie_without_certificate():
    result = decision._decision(*policy_fixture())
    assert result['calibration_gate_met'] is True and result['qualified_calibration_threshold'] == .8
    assert result['label_counts'] == {'supported': 150, 'unsupported': 50, 'unknown': 200, 'unlabeled': 0}
    assert result['holdout_groups_unopened'] == 400 and result['development_groups_counted_in_formal_quotas'] == 0
    assert result['certified_threshold'] is None and result['verified_pass_signals'] == 0
    assert all(result[key] is False for key in decision.FALSE_FLAGS)


def test_failed_negative_jev_attempts_remain_negative_denominator_and_fail_gate():
    values = policy_fixture()
    for row in values[4][150:]:
        row.update(call_status='http_400', acceptance_violation=None)
    result = decision._decision(*values)
    assert result['all_registered_binary_terminal'] is True and result['calibration_gate_met'] is False
    assert result['failed_jev_attempts'] == 50 and result['label_counts']['unsupported'] == 50
    assert result['qualified_calibration_threshold'] is None
    assert all(trial['detected_unsupported'] == 0 for trial in result['numeric_results']['grid_results'])


@pytest.mark.parametrize('change', ['duplicate', 'foreign', 'pending', 'candidate', 'request_order',
                                   'label_reinterpretation', 'bad_model', 'bad_probability'])
def test_foreign_or_reinterpreted_terminal_numeric_evidence_rejected(change):
    campaign, dataset, registered, requests, rows, spec = policy_fixture()
    if change == 'duplicate':
        rows.append(deepcopy(rows[0]))
    elif change == 'foreign':
        rows[0]['id'] = 'foreign'
    elif change == 'pending':
        rows[0]['call_status'] = 'pending'
    elif change == 'candidate':
        rows[0]['candidate_sha256'] = 'f' * 64
    elif change == 'request_order':
        requests[0], requests[1] = requests[1], requests[0]
    elif change == 'label_reinterpretation':
        requests[0]['label'] = 'unsupported'
    elif change == 'bad_model':
        rows[0]['model'] = 'jev-other'
    else:
        rows[0]['acceptance_violation'] = True
    with pytest.raises(ValueError):
        decision._decision(campaign, dataset, registered, requests, rows, spec)


def replay_fixture(tmp_path, monkeypatch):
    """Explicit fake observer; original raw SQL closure remains real temp data."""
    campaign, dataset, registered, requests, observations, _ = policy_fixture()
    main, jev = tmp_path / 'synthetic-main', tmp_path / 'synthetic-jev'
    main.mkdir()
    jev.mkdir()
    write(main / 'authorization.json', {'fixture': 'main'})
    write(jev / 'authorization.json', {'fixture': 'jev'})
    database = main / 'main-attempts.sqlite3'
    with sqlite3.connect(database) as connection:
        connection.execute('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT)')
        connection.execute('INSERT INTO metadata VALUES (?,?)', ('fixture', 'temporary-only'))
        connection.execute('CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT,split TEXT,started_json TEXT,finished_json TEXT)')
        connection.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES (?,?,?,?)',
                           ('old-failure', 'old', '{"old":true}', '{"status":"http_400"}'))
        connection.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES (?,?,?,?)',
                           ('old-pending', 'old', '{"old":true}', None))
    chain = {'authorization': {'main_model': {'ledger_dir': str(main), 'max_attempts': 1642},
                              'jev': {'ledger_dir': str(jev), 'max_attempts': 1037}},
             'current_sha256': 'e' * 64}
    campaign_file = tmp_path / 'synthetic-cohort.json'
    campaign = cohort.seal(campaign)
    write(campaign_file, campaign)
    registered.update(cohort_file=str(campaign_file), cohort_sha256=cohort.sha(campaign_file))
    registered = cohort.seal(registered)
    registration_file = tmp_path / 'synthetic-registration.json'
    write(registration_file, registered)
    executable = tmp_path / 'not-a-live-native-image.exe'
    executable.write_bytes(b'self-made fixture; never an actual executable')
    source = tmp_path / 'synthetic-source.txt'
    source.write_bytes(b'no actual source authority')
    dirs = []
    for name in ('synthetic-dev-archive', 'synthetic-cal-archive', 'synthetic-formal-sender'):
        directory = tmp_path / name
        directory.mkdir()
        (directory / 'fixture.txt').write_bytes(b'not original model evidence')
        dirs.append(str(directory))
    bindings = budget.raw_bindings([source, campaign_file, registration_file, executable])
    pack = {'cohort': campaign, 'dataset': dataset, 'registration': registered, 'requests': requests,
        'registration_file': str(registration_file), 'registration_sha256': cohort.sha(registration_file),
        'chain': chain, 'current': {'main_ledger_file': str(database),
            'main_started': [{'group_id': 'old-failure'}, {'group_id': 'old-pending'}],
            'remaining_main_attempts': 400, 'jev_reservations': [], 'excluded_groups': ['old-failure', 'old-pending']},
        'finished': {'old-failure': {'status': 'http_400'}, 'old-pending': None},
        'sql_image': budget._ledger_rows(database), 'budget_bindings': development_originals.binding_images(chain),
        'proof_bindings': bindings,
        'directory_images': [([path], development_review.evidence([path])) for path in dirs[:2]]}
    joined = {'pack': pack, 'observations': observations, 'directories': [dirs[-1]],
              'evidence': development_review.evidence([dirs[-1]]), 'bindings': bindings}
    calls = []
    monkeypatch.setattr(decision, 'loaded_check', lambda: None)

    def fake_proof(*args):
        calls.append('explicit_fake_observer')
        return deepcopy(joined)

    def fake_close(value):
        cohort.check(value['bindings'])
        assert budget._ledger_rows(database) == value['pack']['sql_image']

    monkeypatch.setattr(decision.observer, 'proof', fake_proof)
    monkeypatch.setattr(decision.observer, 'close', fake_close)
    return registration_file, executable, joined, calls, database


def test_readonly_report_rebuilds_fake_observer_once_and_preserves_original_temp_sql(tmp_path, monkeypatch):
    registration, executable, joined, calls, database = replay_fixture(tmp_path, monkeypatch)
    before = budget._ledger_rows(database)
    report = decision.value(registration, joined['directories'], executable)
    assert calls == ['explicit_fake_observer'] and budget._ledger_rows(database) == before
    assert report['calibration_gate_met'] is True and report['formal_certification'] is False
    path = tmp_path / 'synthetic-decision.json'
    write(path, report)
    assert decision.revalidate(path) == report
    assert calls == ['explicit_fake_observer', 'explicit_fake_observer']
    report['qualified_calibration_threshold'] = .99
    report.pop('artifact_hash')
    write(path, cohort.seal(report))
    with pytest.raises(ValueError, match='differs from fresh original'):
        decision.revalidate(path)
    assert budget._ledger_rows(database) == before
    assert not (Path(joined['pack']['chain']['authorization']['jev']['ledger_dir']) / 'reservations').exists()
