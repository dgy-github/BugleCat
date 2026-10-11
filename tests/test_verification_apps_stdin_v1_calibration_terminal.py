"""Synthetic labels prove phase and quota rules, never natural model accuracy."""
from copy import deepcopy
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import calibration_terminal as terminal
from evals.verification.apps_stdin_v1 import cohort, development_review, protocol
from tests.stdin_collection_fixtures import write


def fixture(supported=150, unsupported=50):
    original, outcomes = [], []
    for index in range(808):
        split = 'development' if index < 8 else 'calibration' if index < 408 else 'holdout'
        case = {'id': f'fixture-{index}', 'group_id': f'group-{index}', 'task_id': index,
                'task_sha256': 'e' * 64, 'source_kind': protocol.SOURCE_KIND, 'split': split}
        original.append(case)
        local = index - 8
        label = ('supported' if local < supported else 'unsupported' if local < supported + unsupported else 'unknown')
        if split == 'development':
            label = 'unsupported'
        elif split == 'holdout':
            label = 'unlabeled'
        outcomes.append({**case, 'label': label, 'call_eligible': label in ('supported', 'unsupported'),
                         'main_status': 'main_not_started' if split == 'holdout' else 'ok',
                         'reason': {'supported': 'passed', 'unsupported': 'failed', 'unknown': 'unknown',
                                    'unlabeled': 'main_not_started'}[label],
                         'candidate_code_sha256': 'a' * 64, 'oracle_sha256': 'b' * 64})
    return original, outcomes, protocol.specification()


def test_full_quota_keeps_unknown_and_both_other_phase_allocations():
    result = terminal._decision(*fixture())
    assert result['independent_quotas_met'] is True
    assert result['label_counts'] == {'supported': 150, 'unsupported': 50, 'unknown': 200, 'unlabeled': 0}
    assert result['development_groups_counted_in_formal_quotas'] == 0
    assert result['holdout_groups_unopened'] == 400
    assert result['certified_threshold'] is None
    assert all(result[key] is False for key in terminal.NO_AUTHORITY)


@pytest.mark.parametrize('positive,negative', [(149, 50), (150, 49), (150, 0)])
def test_quota_cannot_borrow_development_negatives_or_unknown(positive, negative):
    result = terminal._decision(*fixture(positive, negative))
    assert result['independent_quotas_met'] is False
    assert result['status'] == 'independent_quotas_not_met'
    assert result['missing_supported'] == max(0, 150 - positive)
    assert result['missing_unsupported'] == 50 - negative


@pytest.mark.parametrize('status', ['main_not_started', 'main_not_finished'])
def test_all_400_must_finish_even_when_labels_already_meet_quota(status):
    original, rows, spec = fixture()
    rows[407].update(main_status=status, label='unlabeled', call_eligible=False, reason=status)
    result = terminal._decision(original, rows, spec)
    assert result['label_counts']['supported'] == 150 and result['label_counts']['unsupported'] == 50
    assert result['whole_allocation_terminal'] is result['independent_quotas_met'] is False
    assert result['status'] == 'calibration_main_incomplete'
    assert result['terminal_groups'] == 399


def test_failed_transport_retained_as_unknown_without_using_it_as_negative():
    original, rows, spec = fixture()
    rows[407].update(main_status='http_400', label='unknown', call_eligible=False, reason='http_400')
    result = terminal._decision(original, rows, spec)
    assert result['terminal_groups'] == 400
    assert result['main_status_counts']['http_400'] == 1
    assert result['label_counts']['unknown'] == 200


@pytest.mark.parametrize('reason', terminal.ORACLE_PENDING)
def test_missing_or_stranded_independent_oracle_blocks_registration_without_retry(reason):
    original, rows, spec = fixture()
    rows[407].update(reason=reason)
    rows[407].pop('oracle_sha256')
    rows[407].pop('candidate_code_sha256')
    result = terminal._decision(original, rows, spec)
    assert result['whole_allocation_terminal'] is True
    assert result['labels_ready'] is result['independent_quotas_met'] is False
    assert result['unresolved_oracle_groups'] == 1
    assert result['status'] == 'independent_labels_incomplete'


@pytest.mark.parametrize('reason', terminal.UNKNOWN_WITHOUT_ORACLE)
def test_scope_rejected_or_missing_code_retained_as_terminal_unknown(reason):
    original, rows, spec = fixture()
    rows[407].update(reason=reason)
    rows[407].pop('oracle_sha256')
    rows[407].pop('candidate_code_sha256')
    result = terminal._decision(original, rows, spec)
    assert result['labels_ready'] is result['independent_quotas_met'] is True
    assert result['unresolved_oracle_groups'] == 0
    assert result['label_counts']['unknown'] == 200


@pytest.mark.parametrize('mutation', ['omit', 'duplicate_group', 'reorder', 'replace_group', 'phase',
                                    'failed_as_negative', 'pending_as_negative', 'numeric_bool',
                                    'wrong_oracle', 'wrong_reason', 'holdout_open', 'development_pending'])
def test_foreign_or_reinterpreted_original_outcomes_rejected(mutation):
    original, rows, spec = fixture()
    original, rows = deepcopy(original), deepcopy(rows)
    if mutation == 'omit':
        rows.pop()
    elif mutation == 'duplicate_group':
        original[9]['group_id'] = original[8]['group_id']
    elif mutation == 'reorder':
        rows[8], rows[9] = rows[9], rows[8]
    elif mutation == 'replace_group':
        rows[8]['group_id'] = 'replacement'
    elif mutation == 'phase':
        rows[8]['split'] = 'development'
    elif mutation == 'failed_as_negative':
        rows[158].update(main_status='http_400', reason='http_400')
    elif mutation == 'pending_as_negative':
        rows[158].update(main_status='main_not_finished', reason='main_not_finished')
    elif mutation == 'numeric_bool':
        rows[8]['call_eligible'] = 1
    elif mutation == 'wrong_oracle':
        rows[8]['oracle_sha256'] = 'foreign'
    elif mutation == 'wrong_reason':
        rows[8]['reason'] = 'failed'
    elif mutation == 'holdout_open':
        rows[408].update(main_status='ok', label='supported', call_eligible=True, reason='passed')
    else:
        rows[0].update(main_status='main_not_finished', label='unlabeled', call_eligible=False,
                       reason='main_not_finished')
    with pytest.raises(ValueError):
        terminal._decision(original, rows, spec)


def replay_fixture(tmp_path, monkeypatch):
    """Only test Root replay/closure orchestration; fake intake is not authority."""
    original, rows, _ = fixture()
    executable = tmp_path / 'synthetic-not-executable.exe'
    executable.write_bytes(b'no executable, HTTP or genuine source proof')
    directory = tmp_path / 'synthetic-calibration-directory'
    directory.mkdir()
    pack = {'cohort': {'cases': original, 'artifact_hash': 'c' * 64},
            'calibration_opening_sha256': 'd' * 64, 'calibration_opening_hash': 'e' * 64,
            'bindings': [], 'current': {'main_started': [], 'jev_reservations': []},
            'finished': {'historical-pending': None}, 'sql_image': [], 'budget_bindings': []}
    decision = terminal._decision(original, rows, protocol.specification())
    data = cohort.seal({'schema': terminal.calibration.SCHEMA, 'scores_hidden': True,
        'jev_probabilities_read': False, 'certified_threshold': None,
        'formal_registration': False, 'holdout_opened': False, 'formal_certification': False,
        'production_authorized': False, 'opening_file': str(tmp_path / 'synthetic-opening.json'),
        'opening_sha256': 'd' * 64, 'opening_hash': 'e' * 64, 'cohort_hash': 'c' * 64,
        'cohort_file': str(tmp_path / 'synthetic-cohort.json'), 'cohort_sha256': 'f' * 64,
        'collector_executable_file': str(executable), 'prepared_directories': [str(directory)],
        'build_info': {}, 'cases': rows, 'allocated_groups': 808, 'calibration_allocated_groups': 400,
        'calibration_collected_groups': 400, 'calibration_terminal_groups': 400,
        'calibration_pending_groups': 0, 'calibration_uncollected_groups': 0,
        'unresolved_oracle_groups': 0, 'labels_ready': True,
        'label_counts': {label: count for label, count in decision['label_counts'].items() if count}})
    path = tmp_path / 'synthetic-dataset.json'
    write(path, data)
    joined = {'pack': pack, 'dataset': data, 'directories': [directory],
              'evidence': development_review.evidence([directory])}
    calls = []

    def replay(opening, directories, image, *, execute_new_oracles):
        assert execute_new_oracles is False
        calls.append('replay')
        return deepcopy(joined)

    monkeypatch.setattr(terminal, 'loaded_check', lambda: None)
    monkeypatch.setattr(terminal.calibration, 'runner_sources', lambda: {})
    monkeypatch.setattr(terminal.calibration, 'proof', replay)
    monkeypatch.setattr(terminal.calibration.originals, 'close', lambda current: calls.append('close'))
    return path, data, joined, calls


def test_genuine_replay_boundary_called_once_readonly_and_report_keeps_no_authority(tmp_path, monkeypatch):
    path, _, _, calls = replay_fixture(tmp_path, monkeypatch)
    before = path.read_bytes()
    result = terminal.value(path)
    assert calls == ['replay', 'close']
    assert path.read_bytes() == before
    assert result['independent_quotas_met'] is True
    assert result['historical_main_pending'] == 1
    assert all(result[key] is False for key in terminal.NO_AUTHORITY)
    assert result['report_oracle_executions'] == result['report_model_calls'] == result['report_jev_calls'] == 0


def test_saved_dataset_cannot_replace_fresh_original_outcome(tmp_path, monkeypatch):
    path, data, _, _ = replay_fixture(tmp_path, monkeypatch)
    changed = deepcopy(data)
    changed['cases'][8]['label'] = 'unsupported'
    changed.pop('artifact_hash')
    write(path, cohort.seal(changed))
    with pytest.raises(ValueError, match='differs from fresh original'):
        terminal.value(path)


def test_saved_report_boolean_cannot_grant_registration(tmp_path, monkeypatch):
    path, _, _, _ = replay_fixture(tmp_path, monkeypatch)
    report = terminal.value(path)
    report['formal_registration'] = True
    saved = tmp_path / 'synthetic-forged-terminal-report.json'
    report.pop('artifact_hash')
    write(saved, cohort.seal(report))
    with pytest.raises(ValueError, match='differs from fresh original'):
        terminal.revalidate(saved)


def test_unstarted_calibration_with_no_directories_retains_all400_without_authority(tmp_path, monkeypatch):
    path, data, joined, _ = replay_fixture(tmp_path, monkeypatch)
    for row in data['cases'][8:408]:
        row.update(main_status='main_not_started', label='unlabeled', call_eligible=False, reason='main_not_started')
    data.update(calibration_collected_groups=0, calibration_terminal_groups=0,
                calibration_uncollected_groups=400, labels_ready=False, label_counts={'unlabeled': 400},
                prepared_directories=[])
    data.pop('artifact_hash')
    sealed = cohort.seal(data)
    data.clear()
    data.update(sealed)
    joined.update(directories=[], evidence=[])
    write(path, data)
    result = terminal.value(path)
    assert result['status'] == 'calibration_main_incomplete'
    assert result['terminal_groups'] == 0 and result['uncollected_groups'] == 400
    assert result['label_counts']['unlabeled'] == 400
    assert result['independent_quotas_met'] is False


@pytest.mark.parametrize('mutation', ['dataset_bytes', 'empty_extra_directory'])
def test_original_file_changes_during_replay_rejected(tmp_path, monkeypatch, mutation):
    path, data, joined, _ = replay_fixture(tmp_path, monkeypatch)

    def changed_replay(*args, **kwargs):
        assert kwargs['execute_new_oracles'] is False
        if mutation == 'dataset_bytes':
            path.write_bytes(path.read_bytes() + b'\n')
        else:
            (joined['directories'][0] / 'unrecorded-empty-directory').mkdir()
        return deepcopy(joined)

    monkeypatch.setattr(terminal.calibration, 'proof', changed_replay)
    with pytest.raises(ValueError):
        terminal.value(path)


def test_temp_original_archive_calibration_intake_and_terminal_registration_gate(tmp_path, monkeypatch):
    """Real new owners/EXE joins on self-made temp data; no real source or HTTP."""
    from evals.verification.apps_stdin_v1 import budget
    from evals.verification.apps_stdin_v1 import calibration_registration as registration
    from tests.stdin_calibration_fixtures import CAL_EXE, add_calibration_record, opened_fixture

    fixture = opened_fixture(tmp_path, monkeypatch)
    failed = add_calibration_record(fixture, tmp_path / 'cal-first-http400', status='http_400')
    scoped = add_calibration_record(fixture, tmp_path / 'cal-second-unknown', answer='No implementation; uncertain.')
    data = terminal.calibration.collect(fixture['opening']['file'],
        [failed['directory'], scoped['directory']], CAL_EXE)
    path = tmp_path / 'synthetic-original-calibration-intake.json'
    write(path, data)
    before_sql = budget._ledger_rows(fixture['database'])
    before_raw = registration.development_originals.binding_images(fixture['chain'])
    fixture['calls'].clear()
    report = terminal.value(path)
    assert fixture['calls'] == ['full_source_proof']
    assert report['terminal_groups'] == 2 and report['uncollected_groups'] == 398
    assert report['label_counts'] == {'supported': 0, 'unsupported': 0, 'unknown': 2, 'unlabeled': 398}
    assert report['status'] == 'calibration_main_incomplete'
    assert report['independent_quotas_met'] is False and report['holdout_groups_unopened'] == 400
    fixture['calls'].clear()
    with pytest.raises(ValueError, match='whole independent calibration quotas'):
        registration.register(path)
    assert fixture['calls'] == ['full_source_proof']
    assert budget._ledger_rows(fixture['database']) == before_sql
    assert registration.development_originals.binding_images(fixture['chain']) == before_raw
    jev_root = Path(fixture['chain']['authorization']['jev']['ledger_dir'])
    assert not (jev_root / registration.NAMESPACE).exists()
