"""Read-only original incomplete label terminal; no registration or certificate."""
import runpy
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import holdout_evaluation as owner
from evals.verification.apps_campaign_v3 import holdout_intake
from evals.verification.calibration import create, digest, load


@pytest.fixture
def source(tmp_path, monkeypatch):
    fixtures = runpy.run_path(str(Path(__file__).with_name('test_verification_apps_campaign_v3_holdout_collection.py')))
    value = fixtures['source'].__wrapped__(tmp_path, monkeypatch)
    value['helpers'] = fixtures
    return value


def test_progress_unstarted_not_unknown_not_final(source, monkeypatch):
    directories = source['helpers']['batches'](source, monkeypatch)
    actual = owner.terminal_value(source['markerfile'], directories, source['auth'])
    assert actual['status'] == 'in_progress' and actual['whole_allocation_terminal'] is False
    assert actual['collected_groups'] == 3 and actual['uncollected_groups'] == 9
    assert actual['label_counts'] == {'unknown': 3, 'unlabeled': 9}
    assert actual['new_paid_calls'] == actual['oracle_executions'] == actual['reservations'] == 0
    assert actual['certified_threshold'] is None and actual['production_authorized'] is False
    path = source['tmp'] / 'progress.json'
    create(path, actual)
    assert owner.revalidate_terminal(path) == actual


def terminal(source, monkeypatch):
    directory = source['tmp'] / 'complete'
    directory.mkdir()
    create(directory / 'receipt.json', {'call_status': 'transport_error'})
    for n in range(12):
        source['helpers']['start'](source, n)
    prepared = [(c, {}, {'call_status': 'transport_error'}, 'transport_error', directory, True) for c in source['cases']]
    monkeypatch.setattr(holdout_intake, '_prepared', lambda *a: (prepared, {'directory': str(directory)}))
    return [directory]


def test_complete_inadequate_quota_original_revalidation_no_certification(source, monkeypatch):
    directories = terminal(source, monkeypatch)
    actual = owner.terminal_value(source['markerfile'], directories, source['auth'])
    assert actual['status'] == 'incomplete_labels' and actual['whole_allocation_terminal'] is True
    assert actual['independent_label_quota_met'] is False
    assert actual['allocated_groups'] == actual['collected_groups'] == 12 and actual['label_counts'] == {'unknown': 12}
    assert actual['formal_certification'] is actual['APPS_ordered_pair_artifact_threshold_certified'] is False
    assert actual['native_claim_certification'] is actual['overall_agent_benefit_certification'] is actual['overall_goal_complete'] is False
    path = source['tmp'] / 'terminal.json'
    create(path, actual)
    assert owner.revalidate_terminal(path) == actual
    actual['status'] = 'passed'
    actual['artifact_hash'] = digest({k: v for k, v in actual.items() if k != 'artifact_hash'})
    path.write_bytes(owner.holdout_jev.packet.canonical_bytes(actual))
    with pytest.raises(ValueError, match='terminal_changed'):
        owner.revalidate_terminal(path)


def test_labels_already_jev_reserved_never_disguised_as_blind_terminal(source, monkeypatch):
    directories = terminal(source, monkeypatch)
    source['snapshot']['jev_reservations'].append({'group_id': 'group-0'})
    with pytest.raises(ValueError, match='jev_before_label_terminal'):
        owner.terminal_value(source['markerfile'], directories, source['auth'])


def test_original_new_file_during_intake_refused_not_new_valid_terminal_binding(source, monkeypatch):
    directories = terminal(source, monkeypatch)
    collect = holdout_intake.collect
    def changed(*args, **kwargs):
        actual = collect(*args, **kwargs)
        create(directories[0] / 'unbound.json', {'added after genuine intake': True})
        return actual
    monkeypatch.setattr(holdout_intake, 'collect', changed)
    with pytest.raises(ValueError, match='terminal_graph_changed'):
        owner.terminal_value(source['markerfile'], directories, source['auth'])


def test_original_bytes_after_saved_terminal_refused(source, monkeypatch):
    directories = terminal(source, monkeypatch)
    path = source['tmp'] / 'terminal.json'
    create(path, owner.terminal_value(source['markerfile'], directories, source['auth']))
    (directories[0] / 'receipt.json').write_bytes(b'changed original receipt')
    with pytest.raises(ValueError, match='terminal_changed'):
        owner.revalidate_terminal(path)
    assert load(path)['certified_threshold'] is None


def test_budget_added_after_original_intake_refused(source, monkeypatch):
    directories = terminal(source, monkeypatch)
    original = holdout_intake.collect
    def changed(*args, **kwargs):
        actual = original(*args, **kwargs)
        source['snapshot']['jev_reservations'].append({'group_id': 'another_scope'})
        return actual
    monkeypatch.setattr(holdout_intake, 'collect', changed)
    with pytest.raises(ValueError, match='budget_changed_during_terminal'):
        owner.terminal_value(source['markerfile'], directories, source['auth'])
