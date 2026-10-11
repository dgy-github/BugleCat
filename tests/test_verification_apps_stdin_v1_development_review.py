"""Synthetic temporary review fixtures; never original paid certification."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import development_review as r


def save(path, value):
    path.write_bytes(r.packet.canonical_bytes(value))
    return path


def fixture_rows():
    fixed, rows = [], []
    for split, count in (('development', 8), ('calibration', 400), ('holdout', 400)):
        for i in range(count):
            case = {'id': f'fixture-{split}-{i}', 'group_id': f'fixture-group-{split}-{i}',
                    'task_id': len(fixed), 'task_sha256': '1' * 64,
                    'source_kind': 'apps_stdin_restricted_script', 'split': split}
            fixed.append(case)
            row = {**case, 'main_status': 'main_not_started', 'label': 'unlabeled',
                   'reason': 'main_not_started', 'call_eligible': False}
            if split == 'development':
                row.update(main_status='ok', label='supported', reason='passed', call_eligible=True,
                    main_receipt_file='fixture-receipt.json', main_receipt_sha256='2' * 64,
                    oracle_file='fixture-oracle.json', oracle_sha256='3' * 64,
                    candidate_code_sha256='4' * 64, candidate_code_span={'code': 'print(1)\n'})
            rows.append(row)
    return fixed, rows


def data_value(rows):
    from collections import Counter
    started = sum(row['main_status'] != 'main_not_started' for row in rows)
    return {'cases': rows, 'allocated_groups': 808, 'collected_groups': started,
            'uncollected_groups': 808 - started, 'label_counts': dict(Counter(row['label'] for row in rows))}


def test_loaded_image_check():
    r.loaded_check()


def test_replaced_intake_cannot_bypass_actual_original_proof(monkeypatch):
    monkeypatch.setattr(r.intake, 'collect', lambda *args, **kwargs: {})
    with pytest.raises(ValueError):
        r.loaded_check()


@pytest.mark.parametrize('status', [value for value in r.intake.MAIN_TERMINAL_STATUSES if value != 'ok'])
def test_main_failures_are_terminal_but_do_not_acquire_binary_labels(status):
    fixed, rows = fixture_rows()
    rows[0].update(main_status=status, label='unknown', reason=status, call_eligible=False)
    assert r.main_terminal(status)
    assert r.allocation(data_value(rows), fixed)[0]['label'] == 'unknown'


def test_whole_allocation_and_unopened_groups_retained():
    fixed, rows = fixture_rows()
    assert len(r.allocation(data_value(rows), fixed)) == 8


@pytest.mark.parametrize('change', ['omit', 'reorder', 'holdout_label', 'holdout_receipt',
                                  'unknown_binary', 'pending_label', 'binary_no_oracle',
                                  'foreign_task', 'false_coverage', 'nonterminal_status'])
def test_incomplete_or_labeled_unopened_evidence_rejected(change):
    fixed, rows = fixture_rows()
    if change == 'omit':
        rows.pop()
    elif change == 'reorder':
        rows[0], rows[1] = rows[1], rows[0]
    elif change == 'holdout_label':
        rows[-1].update(label='unknown')
    elif change == 'holdout_receipt':
        rows[-1]['main_receipt_file'] = 'forged.json'
    elif change == 'unknown_binary':
        rows[0]['label'] = 'unknown'
    elif change == 'pending_label':
        rows[0]['main_status'] = 'main_not_finished'
    elif change == 'binary_no_oracle':
        del rows[0]['oracle_sha256']
    elif change == 'foreign_task':
        rows[0]['task_id'] = 9999
    elif change == 'nonterminal_status':
        rows[0]['main_status'] = 'http_200'
    data = data_value(rows)
    if change == 'false_coverage':
        data['collected_groups'] = True
    with pytest.raises(ValueError):
        r.allocation(data, fixed)


def test_complete_failures_and_unknown_are_preserved():
    fixed, rows = fixture_rows()
    rows[0].update(main_status='http_400', label='unknown', reason='http_400', call_eligible=False)
    rows[1].update(label='unknown', reason='unknown', call_eligible=False)
    rows[2].update(label='unsupported', reason='failed')
    checked = r.allocation(data_value(rows), fixed)
    assert [row['label'] for row in checked[:3]] == ['unknown', 'unknown', 'unsupported']


def test_complete_directory_set_has_no_suffix_exclusion(tmp_path):
    directory = tmp_path / 'preparation'
    directory.mkdir()
    (directory / 'empty').mkdir()
    (directory / 'request.db').write_bytes(b'not ignored')
    initial = r.evidence([directory])
    assert initial[0]['files'][0]['file'].endswith('request.db')
    assert initial[0]['directories'] == [str(directory / 'empty')]
    (directory / 'new-receipt.json').write_bytes(b'new')
    assert not r.budget.same(initial, r.evidence([directory]))


def test_overlapping_directory_or_duplicate_rejected(tmp_path):
    child = tmp_path / 'child'
    child.mkdir()
    for paths in ([child, child], [tmp_path, child]):
        with pytest.raises(ValueError):
            r.evidence(paths)


@pytest.fixture
def original_join(tmp_path, monkeypatch):
    """Mock original-proof seam only in this test; no real authorizations."""
    fixed, rows = fixture_rows()
    directory = tmp_path / 'preparation'
    directory.mkdir()
    (directory / 'receipt.json').write_bytes(b'fixture original')
    executable = tmp_path / 'fixture-executable.exe'
    executable.write_bytes(b'fixture-only-not-an-executable')
    cohort_file = save(tmp_path / 'fixture-cohort.json', r.cohort.seal({
        'cases': fixed, 'ledger_snapshot': {'authorization_chain': {'fixture_only': True}}}))
    data = r.cohort.seal({**data_value(rows), 'schema': r.intake.SCHEMA,
        'cohort_file': str(cohort_file), 'cohort_sha256': r.cohort.sha(cohort_file),
        'cohort_hash': r.budget.load(cohort_file)['artifact_hash'],
        'collector_executable_file': str(executable), 'scores_hidden': True,
        'jev_probabilities_read': False, 'formal_registration': False, 'holdout_opened': False,
        'formal_certification': False, 'production_authorized': False, 'certified_threshold': None,
        'model_calls': 0, 'jev_calls': 0})
    dataset_file = save(tmp_path / 'fixture-dataset.json', data)
    calls = []

    def fake_collect(cohort, paths, exe, *, execute_new_oracles):
        calls.append(execute_new_oracles)
        assert execute_new_oracles is False
        return deepcopy(data)

    monkeypatch.setattr(r.intake, 'collect', fake_collect)
    monkeypatch.setattr(r, 'loaded_check', lambda: None)
    monkeypatch.setattr(r.budget, '_snapshot_chain', lambda chain: ({'fixture_only': True}, {}))
    return {'dataset': dataset_file, 'directory': directory, 'exe': executable, 'data': data, 'calls': calls}


def test_readonly_original_reconstruction_grants_no_authority(original_join):
    f = original_join
    before = {path: path.read_bytes() for path in f['dataset'].parent.rglob('*') if path.is_file()}
    result = r.review(f['dataset'], [f['directory']], f['exe'])
    assert f['calls'] == [False]
    assert result['development_main_oracle_evidence_complete'] is True
    assert all(result[key] is False for key in r.NO_AUTHORITY)
    assert result['dev_jev_pipeline_verified'] is False and result['certified_threshold'] is None
    assert result['new_model_calls'] == result['new_jev_calls'] == result['oracle_executions'] == 0
    assert before == {path: path.read_bytes() for path in f['dataset'].parent.rglob('*') if path.is_file()}


def test_forged_saved_labels_do_not_replace_original_intake(original_join):
    f = original_join
    changed = deepcopy(f['data'])
    changed['cases'][0]['label'] = 'unsupported'
    save(f['dataset'], r.cohort.seal({key: value for key, value in changed.items() if key != 'artifact_hash'}))
    with pytest.raises(ValueError, match='actual original intake'):
        r.review(f['dataset'], [f['directory']], f['exe'])


@pytest.mark.parametrize('mutation', ['new_file', 'deleted_file', 'new_directory', 'changed_bytes', 'budget'])
def test_inflight_original_changes_close_review(original_join, monkeypatch, mutation):
    f = original_join
    original_collect = r.intake.collect

    def changed_collect(*args, **kwargs):
        result = original_collect(*args, **kwargs)
        if mutation == 'new_file':
            (f['directory'] / 'extra.json').write_bytes(b'extra')
        elif mutation == 'deleted_file':
            (f['directory'] / 'receipt.json').unlink()
        elif mutation == 'new_directory':
            (f['directory'] / 'newdir').mkdir()
        elif mutation == 'changed_bytes':
            (f['directory'] / 'receipt.json').write_bytes(b'changed')
        else:
            monkeypatch.setattr(r.budget, '_snapshot_chain', lambda chain: ({'fixture_only': False}, {}))
        return result

    monkeypatch.setattr(r.intake, 'collect', changed_collect)
    with pytest.raises(ValueError):
        r.review(f['dataset'], [f['directory']], f['exe'])


@pytest.mark.parametrize('reason', ['independent_oracle_not_recorded', 'oracle_attempt_incomplete'])
def test_unrecorded_oracle_does_not_complete_review(original_join, reason):
    f = original_join
    row = f['data']['cases'][0]
    row.update(label='unknown', reason=reason, call_eligible=False)
    f['data'].update(data_value(f['data']['cases']))
    sealed = r.cohort.seal({key: value for key, value in f['data'].items() if key != 'artifact_hash'})
    f['data'].clear()
    f['data'].update(sealed)
    save(f['dataset'], f['data'])
    # The fixture seam returns the updated value; actual proof is never bypassed
    # outside this temporary test.
    f['data']['cases'][0]['reason'] = reason
    result = r.review(f['dataset'], [f['directory']], f['exe'])
    assert result['all_development_main_terminal'] is True
    assert result['development_main_oracle_evidence_complete'] is False
