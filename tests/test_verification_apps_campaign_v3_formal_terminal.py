"""Original terminal gate tests use isolated trusted intake fixtures."""
import copy
import json

import pytest

from evals.verification.apps_campaign_v3 import formal_registration
from evals.verification.apps_campaign_v3 import formal_terminal as owner
from tests.test_verification_apps_campaign_v3_formal_registration import (  # noqa: F401
    original,
    seal,
    write,
)


@pytest.fixture
def terminal(original, monkeypatch):  # noqa: F811
    monkeypatch.setattr(owner.approved_budget, 'snapshot', lambda *a: (
        {**copy.deepcopy(original['state']['baseline']), 'main_started': [0] * 200},
        {c['group_id']: {'status': 'ok'} for c in original['state']['data']['cases']}))
    return original


def test_sufficient_terminal_does_not_grant_registration_or_fees(terminal):
    actual = owner.value(terminal['dataset'])
    assert actual['independent_quotas_met'] is True
    assert actual['formal_registration'] is actual['holdout_opened'] is actual['production_authorized'] is False
    assert actual['report_model_calls'] == actual['report_oracle_executions'] == actual['report_reservations'] == 0
    assert not (terminal['root'] / formal_registration.NAMESPACE).exists()


def test_unknown_is_retained_and_never_becomes_negative(terminal):
    data = terminal['state']['data']
    data['cases'][-1].update(label='unknown', call_eligible=False)
    terminal['state']['data'] = seal(data)
    write(terminal['dataset'], terminal['state']['data'])
    actual = owner.value(terminal['dataset'])
    assert actual['status'] == 'independent_quotas_not_met'
    assert actual['label_counts'] == {'supported': 150, 'unsupported': 49, 'unknown': 1, 'unlabeled': 0}
    assert actual['missing_unsupported'] == 1 and actual['certified_threshold'] is None
    assert 'dataset_file' not in owner.public_summary(actual)


@pytest.mark.parametrize('mutation', ['pending', 'omission', 'score_read', 'raw_data', 'scope_jev'])
def test_incomplete_or_changed_originals_cannot_form_terminal_report(terminal, monkeypatch, mutation):
    data = terminal['state']['data']
    if mutation == 'pending':
        data['pending_groups'] = 1
    elif mutation == 'omission':
        data['collected_groups'] -= 1
    elif mutation == 'score_read':
        data['jev_probabilities_read'] = True
    elif mutation == 'raw_data':
        terminal['state']['data'] = {**data, 'extra': 'changed'}
    else:
        read = owner.approved_budget.snapshot
        def reserved(*args):
            state, finished = read(*args)
            state['jev_reservations'].append({'group_id': 'group-0'})
            return state, finished
        monkeypatch.setattr(owner.approved_budget, 'snapshot', reserved)
    if mutation not in {'raw_data', 'scope_jev'}:
        terminal['state']['data'] = seal(data)
        write(terminal['dataset'], terminal['state']['data'])
    with pytest.raises(ValueError):
        owner.value(terminal['dataset'])


def test_resealed_report_tampering_rejected(terminal, tmp_path):
    result = owner.value(terminal['dataset'])
    path = tmp_path / 'result.json'
    write(path, result)
    assert owner.revalidate(path) == result
    changed = {**result, 'production_authorized': True}
    path.write_text(json.dumps(seal(changed)), encoding='utf-8')
    with pytest.raises(ValueError, match='terminal_report_changed'):
        owner.revalidate(path)


def test_budget_after_intake_cannot_be_recaptured_as_original(terminal, monkeypatch):
    read = owner.formal_intake.collect
    def changed(*args, **kwargs):
        actual = read(*args, **kwargs)
        terminal['state']['baseline']['new_paid_slot'] = 'outside_transaction'
        return actual
    monkeypatch.setattr(owner.formal_intake, 'collect', changed)
    with pytest.raises(ValueError, match='terminal_evidence_owner_or_budget_changed'):
        owner.value(terminal['dataset'])


@pytest.mark.parametrize('mutation', ['dataset', 'receipt', 'new_file', 'budget'])
def test_report_closure_rejects_changes_after_original_intake(terminal, monkeypatch, mutation):
    read = owner.formal_intake.collect
    def collect(*args, **kwargs):
        actual = read(*args, **kwargs)
        if mutation == 'dataset':
            terminal['dataset'].write_bytes(terminal['dataset'].read_bytes() + b' ')
        elif mutation == 'receipt':
            write(terminal['evidence'] / 'receipt.json', {'raw': 'changed'})
        elif mutation == 'new_file':
            write(terminal['evidence'] / 'new.json', {'new': True})
        return actual
    monkeypatch.setattr(owner.formal_intake, 'collect', collect)
    if mutation == 'budget':
        before = owner.approved_budget.snapshot
        calls = []
        def snapshot(*args):
            state, finished = before(*args)
            calls.append(None)
            if len(calls) > 1:
                state['main_started'].append(0)
            return state, finished
        monkeypatch.setattr(owner.approved_budget, 'snapshot', snapshot)
    with pytest.raises(ValueError, match='terminal_evidence_owner_or_budget_changed'):
        owner.value(terminal['dataset'])
