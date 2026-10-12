"""Synthetic fixed-holdout labels exercise policy, never natural accuracy."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import holdout_terminal as terminal
from tests.test_verification_apps_stdin_v1_calibration_terminal import (
    fixture as calibration_fixture,
)


def fixture(positive=150, negative=50):
    fixed, original, spec = calibration_fixture()
    rows = deepcopy(original)
    for index, row in enumerate(rows[408:]):
        label = 'supported' if index < positive else 'unsupported' if index < positive + negative else 'unknown'
        row.update(main_status='ok', label=label, call_eligible=label != 'unknown',
                   reason={'supported': 'passed', 'unsupported': 'failed', 'unknown': 'unknown'}[label])
    return fixed, rows, original, spec


def test_complete_holdout_labels_do_not_certify_or_tune_threshold():
    value = terminal._decision(*fixture())
    assert value['independent_quotas_met'] is True
    assert value['label_counts'] == {'supported': 150, 'unsupported': 50, 'unknown': 200, 'unlabeled': 0}
    assert value['development_or_calibration_labels_counted_in_holdout_quotas'] == 0
    assert value['threshold_tuning'] is False and value['certified_threshold'] is None
    assert all(value[key] is False for key in terminal.NO_AUTHORITY)


@pytest.mark.parametrize('positive,negative', [(149, 50), (150, 49), (0, 0)])
def test_historical_calibration_labels_cannot_fill_holdout_quotas(positive, negative):
    value = terminal._decision(*fixture(positive, negative))
    assert value['independent_quotas_met'] is False
    assert value['status'] == 'independent_quotas_not_met'


@pytest.mark.parametrize('status', ['main_not_started', 'main_not_finished'])
def test_every_fixed_holdout_group_must_finish_after_binary_quota_is_met(status):
    fixed, rows, original, spec = fixture()
    rows[-1].update(main_status=status, label='unlabeled', call_eligible=False, reason=status)
    value = terminal._decision(fixed, rows, original, spec)
    assert value['status'] == 'holdout_main_incomplete'
    assert value['terminal_groups'] == 399 and value['independent_quotas_met'] is False


@pytest.mark.parametrize('reason', ['independent_oracle_not_recorded', 'oracle_attempt_incomplete'])
def test_stranded_or_missing_oracle_blocks_labels_ready(reason):
    fixed, rows, original, spec = fixture()
    rows[-1].update(reason=reason)
    value = terminal._decision(fixed, rows, original, spec)
    assert value['whole_allocation_terminal'] is True and value['labels_ready'] is False
    assert value['unresolved_oracle_groups'] == 1


def test_http_failure_is_unknown_and_kept_in_whole_allocation():
    fixed, rows, original, spec = fixture()
    rows[-1].update(main_status='http_400', reason='http_400')
    value = terminal._decision(fixed, rows, original, spec)
    assert value['terminal_groups'] == 400 and value['label_counts']['unknown'] == 200
    rows[-1].update(label='unsupported', call_eligible=True)
    with pytest.raises(ValueError, match='never a negative'):
        terminal._decision(fixed, rows, original, spec)


@pytest.mark.parametrize('mutation', ['calibration_label', 'development_status', 'omit', 'holdout_reorder'])
def test_original_identity_and_historical_outcomes_are_immutable(mutation):
    fixed, rows, original, spec = fixture()
    if mutation == 'calibration_label':
        rows[8].update(label='unsupported', reason='failed')
    elif mutation == 'development_status':
        rows[0].update(main_status='http_400', label='unknown', call_eligible=False, reason='http_400')
    elif mutation == 'omit':
        rows.pop()
    else:
        rows[-1], rows[-2] = rows[-2], rows[-1]
    with pytest.raises(ValueError):
        terminal._decision(fixed, rows, original, spec)
