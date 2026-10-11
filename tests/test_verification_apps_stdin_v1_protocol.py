from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import protocol as p

H = 'a' * 64


def fixture(supported=150, unsupported=50):
    cases, rows = [], []
    for n in range(supported + unsupported):
        good = n < supported
        case = {'id': str(n), 'group_id': f'group-{n}', 'source_kind': p.SOURCE_KIND,
                'label': 'supported' if good else 'unsupported', 'candidate_sha256': H, 'oracle_sha256': H}
        cases.append(case)
        rows.append({**{k: case[k] for k in ('id', 'group_id', 'candidate_sha256', 'oracle_sha256')},
                     'call_status': 'ok', 'model': p.MODEL, 'acceptance_violation': .1 if good else .9,
                     'transport_binding_status': 'original_transport_bytes_verified',
                     'request_hash': H, 'actual_request_hash': H, 'response_sha256': H})
    return cases, rows


def test_exact_gates_and_tie_do_not_certify():
    cases, rows = fixture()
    spec = p.specification()
    result = p.calibrate_numbers(cases, rows, spec)
    assert len(result['grid_results']) == 41
    assert result['calibration_numeric_gate_met'] and result['qualified_numeric_threshold'] == .9
    assert result['certified_threshold'] is None and result['formal_certification'] is False
    assert result['production_authorized'] is False and result['holdout_status'] == 'not_run'
    assert all(row['verified_pass_signals'] == 0 for row in result['grid_results'])


def test_jev_failures_are_not_removed_from_denominator():
    cases, rows = fixture()
    for row in rows[-50:]:
        row.update(call_status='transport_error', acceptance_violation=None)
    result = p.numbers(cases, rows, .9, p.specification())
    assert result['unsupported_groups'] == 50 and result['detected_unsupported'] == 0
    assert len(result['failed_attempts']) == 50 and not result['gate_met']


@pytest.mark.parametrize('supported,unsupported', [(149, 50), (150, 49), (0, 0)])
def test_quotas_cannot_be_reduced(supported, unsupported):
    cases, rows = fixture(supported, unsupported)
    assert not p.calibrate_numbers(cases, rows, p.specification())['calibration_numeric_gate_met']


@pytest.mark.parametrize('key,value', [('model', 'jev-latest'), ('source_kinds', ['apps_call_based_free_function']),
                                     ('min_unsupported', 25), ('confidence', .9), ('threshold_grid', [.9]),
                                     ('maximum_request_bytes', 48001), ('fresh_child_per_pair', False),
                                     ('candidate_json_literal_byte_limit', 16001)])
def test_protocol_scope_and_thresholds_frozen(key, value):
    spec = p.specification()
    spec[key] = value
    with pytest.raises(ValueError, match='protocol changed'):
        p.validate(spec)


@pytest.mark.parametrize('key,value', [('label', 'unknown'), ('source_kind', 'apps_call_based_free_function'),
                                     ('oracle_sha256', 'x'), ('candidate_sha256', None)])
def test_old_scope_unknown_and_unbound_labels_rejected(key, value):
    cases, rows = fixture()
    cases[0][key] = value
    with pytest.raises(ValueError):
        p.numbers(cases, rows, .9, p.specification())


@pytest.mark.parametrize('key,value', [('model', 'jev-other'), ('actual_request_hash', 'b'*64),
                                     ('candidate_sha256', 'b'*64), ('oracle_sha256', 'b'*64),
                                     ('group_id', 'foreign'), ('acceptance_violation', True),
                                     ('acceptance_violation', float('nan')), ('acceptance_violation', 10**1000),
                                     ('transport_binding_status', 'caller_claimed'), ('response_sha256', '')])
def test_numeric_shape_and_identity_rejected(key, value):
    cases, rows = fixture()
    rows[0][key] = value
    with pytest.raises(ValueError):
        p.numbers(cases, rows, .9, p.specification())


def test_missing_duplicate_observations_and_group_reuse_rejected():
    cases, rows = fixture()
    with pytest.raises(ValueError):
        p.numbers(cases, rows[:-1], .9, p.specification())
    with pytest.raises(ValueError):
        p.numbers(cases, rows + rows[:1], .9, p.specification())
    cases[1]['group_id'] = cases[0]['group_id']
    with pytest.raises(ValueError):
        p.numbers(cases, rows, .9, p.specification())


def test_failed_call_cannot_smuggle_probability():
    cases, rows = fixture()
    rows[0]['call_status'] = 'transport_error'
    with pytest.raises(ValueError, match='failed attempt'):
        p.numbers(cases, rows, .9, p.specification())


@pytest.mark.parametrize('threshold', [True, 0, .013, float('nan')])
def test_unregistered_threshold_rejected(threshold):
    cases, rows = fixture()
    with pytest.raises(ValueError, match='predeclared'):
        p.numbers(cases, rows, threshold, p.specification())


def test_inputs_are_not_rewritten():
    cases, rows = fixture()
    before = deepcopy((cases, rows))
    p.calibrate_numbers(cases, rows, p.specification())
    assert (cases, rows) == before


def test_quality_gate_memory_change_rejected(monkeypatch):
    changed = deepcopy(p.GATES)
    changed['min_unsupported'] = 25
    monkeypatch.setattr(p.artifact_v2_formal, 'GATES', changed)
    with pytest.raises(ValueError, match='constants differ'):
        p.specification()


def test_actual_loaded_confidence_computation_checked(monkeypatch):
    monkeypatch.setattr(p.artifact_gate, 'metrics', lambda *args: {'gate_met': True})
    with pytest.raises(ValueError, match='bytecode differs'):
        p.specification()


def test_untyped_observation_identity_rejected():
    cases, rows = fixture()
    rows[0]['id'] = []
    with pytest.raises(ValueError):
        p.numbers(cases, rows, .9, p.specification())


def test_runtime_grid_change_cannot_self_validate(monkeypatch):
    monkeypatch.setattr(p, 'GRID', (.9,))
    with pytest.raises(ValueError, match='grid changed'):
        p.specification()


def test_typed_nested_quality_gate_not_equal_float(monkeypatch):
    monkeypatch.setitem(p.GATES, 'min_supported', 150.0)
    with pytest.raises(ValueError, match='typed original'):
        p.specification()


@pytest.mark.parametrize('status', ['pending', 'not_started', 'unlabeled', 'unknown', '',
                                   'http_200', 'http_0200', 'http_600', 'http_４００', True])
def test_incomplete_or_invented_call_cannot_enter_zero_false_block_denominator(status):
    cases, rows = fixture()
    rows[0].update(call_status=status, acceptance_violation=None)
    with pytest.raises(ValueError, match='terminal Jev status'):
        p.calibrate_numbers(cases, rows, p.specification())


@pytest.mark.parametrize('status', list(p.TERMINAL_STATUSES[1:]) + ['http_400', 'http_599'])
def test_each_terminal_failure_stays_in_denominator(status):
    cases, rows = fixture()
    rows[-1].update(call_status=status, acceptance_violation=None)
    result = p.numbers(cases, rows, .9, p.specification())
    assert result['unsupported_groups'] == 50 and result['detected_unsupported'] == 49
    assert result['failed_attempts'] == {rows[-1]['id']: status}
