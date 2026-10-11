from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import history as h

H = 'a' * 64
OLD = 'b' * 64
CAMPAIGN = 'c' * 64


def fixture():
    cases = [{'id': f'new-{i}', 'group_id': f'g-{i}', 'task_id': i, 'task_sha256': H,
              'source_kind': 'apps_stdin_restricted_script', 'split': split}
             for i, split in enumerate(('development', 'development', 'calibration', 'holdout'))]
    original = [{'ordinal': i + 1, 'group_id': f'old-{i}', 'started': {'group_id': f'old-{i}'}} for i in range(2)]
    reservation = {'group_id': 'old-jev', 'reservation_index': 1, 'authorization_sha256': OLD}
    baseline = {'main_started': original, 'main_metadata': [('authorization_sha256', OLD)],
                'main_ledger_file': 'original/main-attempts.sqlite3', 'main_reservations': [],
                'jev_reservations': [reservation], 'authorization_chain': {'current_sha256': H}}
    before = {'old-0': None, 'old-1': {'group_id': 'old-1', 'call_status': 'transport_error'}}
    return baseline, before, deepcopy(baseline), deepcopy(before), cases


def append_main(current, finished, case, *, pending=False):
    ordinal = len(current['main_started']) + 1
    current['main_started'].append({'ordinal': ordinal, 'group_id': case['group_id'],
                                    'started': {**case, 'authorization_sha256': H, 'artifact_cohort_hash': CAMPAIGN}})
    finished[case['group_id']] = None if pending else {'group_id': case['group_id'], 'global_main_attempt': ordinal}


def append_jev(current, case):
    current['jev_reservations'].append({'group_id': case['group_id'],
        'reservation_index': len(current['jev_reservations']) + 1,
        'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
        'authorization_sha256': H, 'status': 'reserved_before_transport',
        'automatic_retries': 0, 'request_hash': H})


def test_original_failed_and_pending_records_retained_and_result_grants_no_authority():
    baseline, before, current, finished, cases = fixture()
    append_main(current, finished, cases[0], pending=True)
    result = h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN)
    assert result['main_new_starts'] == 1 and result['jev_new_reservations'] == 0
    assert all(result[key] is False for key in ('collection_authorized', 'formal_registration', 'holdout_opened',
                                               'production_authorized', 'original_provenance_authenticated_by_this_helper'))


@pytest.mark.parametrize('change', ['old_pending', 'old_failure', 'old_start', 'metadata', 'chain', 'fresh_ledger',
                                  'foreign_main_reservation', 'deleted_original', 'deleted_highest'])
def test_original_shared_history_cannot_be_reset_or_reinterpreted(change):
    baseline, before, current, finished, cases = fixture()
    append_main(current, finished, cases[0])
    if change == 'old_pending':
        finished['old-0'] = {'group_id': 'old-0', 'call_status': 'ok'}
    elif change == 'old_failure':
        finished['old-1']['call_status'] = 'ok'
    elif change == 'old_start':
        current['main_started'][0]['started']['new'] = True
    elif change == 'metadata':
        current['main_metadata'].append(('new-origin', 'fresh'))
    elif change == 'chain':
        current['authorization_chain']['current_sha256'] = OLD
    elif change == 'fresh_ledger':
        current['main_ledger_file'] = 'fresh/main-attempts.sqlite3'
    elif change == 'foreign_main_reservation':
        current['main_reservations'].append({'group_id': 'foreign'})
    elif change == 'deleted_original':
        current['main_started'].pop(0)
    else:
        current['main_started'].pop()
    with pytest.raises(ValueError):
        h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN)


@pytest.mark.parametrize('field,value', [('id', 'foreign'), ('group_id', 'other'), ('task_id', True),
                                       ('task_sha256', OLD), ('source_kind', 'apps_call_based'),
                                       ('split', 'holdout'), ('artifact_cohort_hash', OLD),
                                       ('authorization_sha256', OLD)])
def test_new_main_identity_cannot_change_scope_or_funded_authorization(field, value):
    baseline, before, current, finished, cases = fixture()
    append_main(current, finished, cases[0])
    current['main_started'][-1]['started'][field] = value
    with pytest.raises(ValueError, match='frozen source prefix'):
        h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN)


def test_skipped_or_resent_pending_main_cannot_form_prefix():
    baseline, before, current, finished, cases = fixture()
    append_main(current, finished, cases[0], pending=True)
    append_main(current, finished, cases[1])
    with pytest.raises(ValueError, match='pending attempt'):
        h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN)


def test_unopened_calibration_and_holdout_cannot_start():
    baseline, before, current, finished, cases = fixture()
    for case in cases[:3]:
        append_main(current, finished, case)
    with pytest.raises(ValueError, match='opened phase'):
        h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN)
    result = h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN,
                               allowed_splits=('development', 'calibration'))
    assert result['main_new_starts'] == 3 and result['collection_authorized'] is False


@pytest.mark.parametrize('changes', ['duplicate_group', 'swapped_phase', 'replaced_case'])
def test_complete_case_identity_and_phase_order_required(changes):
    baseline, before, current, finished, cases = fixture()
    if changes == 'duplicate_group':
        cases[1]['group_id'] = cases[0]['group_id']
    elif changes == 'swapped_phase':
        cases[0], cases[-1] = cases[-1], cases[0]
    else:
        cases[0]['task_id'] = True
    with pytest.raises(ValueError):
        h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN)


def test_binary_jev_prefix_retains_old_reservation_and_finished_main_association():
    baseline, before, current, finished, cases = fixture()
    for case in cases[:2]:
        append_main(current, finished, case)
    eligible = [{**case, 'label': label} for case, label in zip(cases[:2], ('supported', 'unsupported'), strict=True)]
    append_jev(current, cases[0])
    append_jev(current, cases[1])
    result = h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN, jev_cases=eligible)
    assert result['jev_new_reservations'] == 2 and result['original_jev_reservations'] == 1


@pytest.mark.parametrize('change', ['no_registration', 'unknown', 'pending_main', 'duplicate', 'reorder',
                                  'before_main', 'changed_original', 'skipped', 'wrong_auth', 'retries'])
def test_jev_cannot_escape_blind_registered_binary_prefix(change):
    baseline, before, current, finished, cases = fixture()
    for case in cases[:2]:
        append_main(current, finished, case)
    eligible = [{**case, 'label': 'supported'} for case in cases[:2]]
    append_jev(current, cases[0])
    if change == 'no_registration':
        eligible = []
    elif change == 'unknown':
        eligible[0]['label'] = 'unknown'
    elif change == 'pending_main':
        finished[cases[1]['group_id']] = None
    elif change == 'duplicate':
        eligible.append(eligible[0])
    elif change == 'reorder':
        eligible.reverse()
    elif change == 'before_main':
        eligible.append({**cases[2], 'label': 'supported'})
    elif change == 'changed_original':
        current['jev_reservations'][0]['authorization_sha256'] = H
    elif change == 'skipped':
        current['jev_reservations'][-1]['group_id'] = cases[1]['group_id']
    elif change == 'wrong_auth':
        current['jev_reservations'][-1]['authorization_sha256'] = OLD
    else:
        current['jev_reservations'][-1]['automatic_retries'] = True
    with pytest.raises(ValueError):
        h.validate_prefix(baseline, before, current, finished, cases, CAMPAIGN, jev_cases=eligible)


def test_input_snapshots_not_changed():
    values = fixture()
    before = deepcopy(values)
    h.validate_prefix(*values, CAMPAIGN)
    assert values == before
