"""Publication must retain the genuine owner gate and limited certificate scope."""
import importlib.util
import json
import re
from pathlib import Path
from types import SimpleNamespace

import pytest


@pytest.fixture
def package():
    path = Path(__file__).resolve().parents[1] / 'scripts/package-apps-evaluation-v3.py'
    spec = importlib.util.spec_from_file_location('apps_package_contract_test', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_unrecognized_or_caller_certificate_cannot_stage(package, monkeypatch):
    monkeypatch.setattr(package, 'load', lambda _: {'schema': 'caller_supplied', 'gate_met': True})
    with pytest.raises(ValueError, match='original_APPS_terminal_result_required'):
        package.summary('unused')


def test_ready_independent_labels_do_not_replace_calibration(package, monkeypatch):
    monkeypatch.setattr(package, 'load', lambda _: {'schema': package.formal_terminal.SCHEMA})
    monkeypatch.setattr(package.formal_terminal, 'revalidate', lambda _: {'independent_quotas_met': True})
    with pytest.raises(ValueError, match='ready_calibration_requires_actual_Jev_evaluation'):
        package.summary('unused')


def test_qualified_calibration_does_not_replace_once_holdout(package, monkeypatch):
    monkeypatch.setattr(package, 'load', lambda _: {'schema': package.formal_evaluation.SCHEMA})
    monkeypatch.setattr(package.formal_evaluation, 'revalidate_calibration',
                        lambda _: {'qualified_for_once_holdout': True})
    with pytest.raises(ValueError, match='qualified_calibration_requires_actual_once_holdout'):
        package.summary('unused')


@pytest.mark.parametrize('passed', [True, False])
def test_scope_certificate_cannot_enable_production_or_generic_claims(package, monkeypatch, passed):
    result = {'schema': package.holdout_evaluation.SCHEMA, 'holdout_status': 'passed' if passed else 'failed',
              'APPS_ordered_pair_artifact_threshold_certified': passed,
              'certified_APPS_ordered_pair_artifact_threshold': .6 if passed else None,
              'fixed_threshold': .6, 'allocated_groups': 316, 'label_counts': {'supported': 200, 'unsupported': 60},
              'original_observations': 260, 'artifact_hash': 'original-owner-fixture',
              'production_authorized': False, 'certified_threshold': None}
    monkeypatch.setattr(package, 'load', lambda _: result)
    monkeypatch.setattr(package.holdout_evaluation, 'revalidate_evaluation', lambda _: result)
    monkeypatch.setattr(package, 'file_hash', lambda _: 'fixture-byte-hash')
    actual = package.summary('fixture')
    assert actual['holdout_opened'] is True
    assert actual['APPS_ordered_pair_artifact_threshold_certified'] is passed
    assert actual['certified_APPS_ordered_pair_artifact_threshold'] == (.6 if passed else None)
    assert actual['production_authorized'] is False
    assert actual['certified_threshold'] is None
    assert actual['formal_certification'] is False
    assert actual['native_claim_certification'] is False
    assert actual['overall_agent_benefit_certification'] is False
    assert actual['overall_goal_complete'] is False


def test_revalidation_failure_cannot_be_hidden_by_resealed_input(package, monkeypatch):
    monkeypatch.setattr(package, 'load', lambda _: {'schema': package.holdout_evaluation.SCHEMA})
    def reject(_):
        raise ValueError('original_once_result_or_threshold_changed')
    monkeypatch.setattr(package.holdout_evaluation, 'revalidate_evaluation', reject)
    with pytest.raises(ValueError, match='original_once_result_or_threshold_changed'):
        package.summary('fixture')


@pytest.mark.parametrize('complete,quota', [(False, False), (True, True), (True, False)])
def test_holdout_incomplete_package_requires_terminal_quota_failure(package, monkeypatch, complete, quota):
    result = dict(schema=package.holdout_evaluation.TERMINAL, whole_allocation_terminal=complete,
        independent_label_quota_met=quota, status='incomplete_labels' if complete and not quota else 'in_progress',
        allocated_groups=316, collected_groups=316 if complete else 100,
        label_counts={'supported': 250, 'unsupported': 40, 'unknown': 26}, artifact_hash='fixture-original',
        production_authorized=False, certified_threshold=None)
    monkeypatch.setattr(package, 'load', lambda _: result)
    monkeypatch.setattr(package.holdout_evaluation, 'revalidate_terminal', lambda _: result)
    monkeypatch.setattr(package, 'file_hash', lambda _: 'fixture-byte-hash')
    if complete and not quota:
        actual = package.summary('fixture')
        assert actual['holdout_opened'] is True
        assert actual['APPS_ordered_pair_artifact_threshold_certified'] is False
        assert actual['certified_APPS_ordered_pair_artifact_threshold'] is None
    else:
        with pytest.raises(ValueError, match='whole_terminal_holdout_labels_required'):
            package.summary('fixture')


def test_changed_copy_during_fake_child_cannot_receive_original_attestation(package, monkeypatch, tmp_path):
    binary = tmp_path / 'fixture.exe'
    binary.write_bytes(b'original fixture')
    compiled = {'executable_sha256': package.file_hash(binary)}
    monkeypatch.setattr(package.formal_intake, 'build', lambda _: compiled)
    def transport(*args, **kwargs):
        binary.write_bytes(b'changed after original build proof')
        return SimpleNamespace(returncode=0, stdout=b'{"paid_calls":0,"http_calls":0}')
    monkeypatch.setattr(package.subprocess, 'run', transport)
    with pytest.raises(ValueError, match='copied_APPS_executable_or_sources_changed_during_fixture'):
        package.offline_copy(binary, package.MAIN, compiled, tmp_path / 'fake')


def test_zip_phase_fee_drift_rejects_whole_stage(package, monkeypatch, tmp_path):
    """A real ZIP is created; isolated source/runtime proofs are test fixtures."""
    root = tmp_path / 'project'
    parent = root / 'releases/parent'
    parent.mkdir(parents=True)
    (parent / 'ncx.exe').write_bytes(b'fixture native')
    (parent / 'snapshot.json').write_text('{}', encoding='utf-8')
    source = root / 'evals/source.py'
    source.parent.mkdir()
    source.write_text('# fixture source\n', encoding='utf-8')
    actual_helper = package.support()
    state = {'slots': 0}
    def write_json(path, value):
        path.write_text(json.dumps(value), encoding='utf-8')
    def zip_then_drift(directory, archive, files):
        actual_helper.checked_zip(directory, archive, files)
        state['slots'] += 1
    helper = SimpleNamespace(NATIVE={'ncx.exe'}, PARENT_SHA='fixture-parent',
        verify_parent=lambda _: {'source_snapshot': 'snapshot.json', 'policy_version': 'fixture'},
        write_json=write_json, safe_file=lambda path: path,
        old_packager=lambda: SimpleNamespace(SECRET=re.compile('credential-fixture-never-in-files')),
        isolated_runtime_smoke=lambda *args: {}, checked_zip=zip_then_drift)
    public = {'status': 'incomplete_labels', 'APPS_ordered_pair_artifact_threshold_certified': False,
              'certified_APPS_ordered_pair_artifact_threshold': None}
    monkeypatch.setattr(package, 'ROOT', root)
    monkeypatch.setattr(package, 'support', lambda: helper)
    monkeypatch.setattr(package, 'summary', lambda _: public)
    monkeypatch.setattr(package, 'tools', lambda: ())
    monkeypatch.setattr(package, 'sources', lambda: [source])
    monkeypatch.setattr(package, 'load', lambda path: {'cohort_file': 'fixture-cohort',
                        'bindings': [{'role': 'authorization', 'file': 'fixture-auth'}]})
    monkeypatch.setattr(package.formal_collection, 'budget_snapshot', lambda _: dict(state))
    output = root / 'releases/checked'
    with pytest.raises(ValueError, match='real_fee_or_reservation_changed_during_zip_creation'):
        package.stage(parent, 'fixture-result', output)
    assert output.with_suffix('.zip').is_file()
