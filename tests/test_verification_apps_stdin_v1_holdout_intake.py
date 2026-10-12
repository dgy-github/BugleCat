"""Only the explicit once-Oracle append is allowed in isolated raw graph tests."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, cohort, holdout_intake, packet
from tests.stdin_collection_fixtures import write


def temporary_originals(tmp_path, monkeypatch):
    directory = tmp_path / 'explicit-synthetic'
    case = {'id': 'holdout-fixture', 'group_id': 'synthetic-only-group', 'task_id': 408,
            'split': 'holdout', 'source_kind': 'apps_stdin_restricted_script', 'task_sha256': 'a' * 64,
            'source_authority_sha256': 'b' * 64}
    cases = [{**case, 'id': f'historical-{i}', 'group_id': f'historical-{i}', 'task_id': i,
              'split': 'development' if i < 8 else 'calibration'} for i in range(408)] + [case]
    task = {'task_id': 408, 'question': 'Self-made echo fixture', 'starter_code': '',
            'input_output': {'inputs': ['7\n'], 'outputs': ['7']}}
    answer = '```python\nprint(input())\n```\nAn honest fixture uncertainty.'
    child = directory / case['id']
    child.mkdir(parents=True)
    manifest = cohort.seal({'cases': [case], 'collector_executable_file': str(tmp_path / 'synthetic-not-live.exe'),
                           'collector_build_info': {}, 'phase_proof_bindings': []})
    (tmp_path / 'synthetic-not-live.exe').write_bytes(b'not-a-native-authority')
    row = {**case, 'schema': 'explicit-fixture-not-live'}
    write(directory / 'collection-manifest.json', manifest)
    (directory / 'prepared.jsonl').write_bytes(packet.canonical_bytes(row) + b'\n')
    write(child / 'receipt.json', {'explicit_synthetic': True})
    own = {'group_id': case['group_id'], 'started': {'case_directory': str(child)}}
    source_file = tmp_path / 'fixture-cohort.json'
    write(source_file, {'explicit_synthetic': True})
    before = holdout_intake.originals.development_review.evidence([directory])
    pack = {'cohort': {'cases': cases, 'artifact_hash': 'c' * 64, 'oracle_runtime_binding': packet.runtime_info()},
        'new_main': [own], 'finished': {case['group_id']: {'call_status': 'ok'}},
        'source_pack': {'tasks': {408: task}},
        'calibration_dataset': {'cases': [{**item, 'main_status': 'ok', 'label': 'unknown', 'reason': 'fixture',
                                         'call_eligible': False} for item in cases]},
        'opening': {'cohort_file': str(source_file), 'qualified_calibration_threshold': .5},
        'holdout_opening_file': 'explicit-fixture-only', 'holdout_opening_sha256': 'd' * 64, 'holdout_opening_hash': 'e' * 64,
        'directory_images': [([str(directory)], before)], 'proof_bindings': before[0]['files']}
    # These boundaries supply self-made inputs only, never genuine source or
    # native permission. Actual native/source guards are tested separately.
    monkeypatch.setattr(holdout_intake, 'loaded_check', packet.runtime_info)
    monkeypatch.setattr(holdout_intake, 'build', lambda _: {})
    monkeypatch.setattr(holdout_intake.collector, 'runner_sources', lambda: {})
    monkeypatch.setattr(holdout_intake.collector, '_validate_manifest', lambda *args, **kwargs: (manifest, pack))
    monkeypatch.setattr(holdout_intake, '_prepared', lambda *args, **kwargs: None)
    monkeypatch.setattr(holdout_intake, 'terminal', lambda *args: ({'result': {'candidate': answer}}, 'ok'))
    monkeypatch.setattr(holdout_intake.intake, 'verify', lambda *args: None)
    calls = []

    def close(value):
        calls.append('actual_file_closure')
        cohort.check(value['proof_bindings'])
        for dirs, graph in value['directory_images']:
            assert budget.same(holdout_intake.originals.development_review.evidence(dirs), graph)

    monkeypatch.setattr(holdout_intake.originals, 'close', close)
    return directory, child, pack, calls, manifest['collector_executable_file']


def test_real_once_oracle_writer_and_full_increment_graph(tmp_path, monkeypatch):
    directory, child, pack, calls, executable = temporary_originals(tmp_path, monkeypatch)
    # A real restricted child executes only this self-made script; no APPS code.
    joined = holdout_intake._proof('fixture', [directory], executable, pack=pack, execute_new_oracles=True)
    assert joined['dataset']['cases'][-1]['label'] == 'supported'
    assert (child / 'stdin-candidate-oracle.started').stat().st_size > 0
    receipt = child / 'stdin-candidate-oracle.json'
    raw = receipt.read_bytes()
    assert {'file': str(receipt), 'sha256': cohort.sha(receipt)} in joined['dataset']['collection_bindings']
    assert calls == ['actual_file_closure', 'actual_file_closure']
    # Read-only replay uses original receipt and never executes another child.
    monkeypatch.setattr(holdout_intake.intake.oracle, 'execute', lambda *_: pytest.fail('Oracle replay forbidden'))
    repeated = holdout_intake._proof('fixture', [directory], executable, pack=pack)
    assert repeated['dataset'] == joined['dataset'] and receipt.read_bytes() == raw
    assert all(joined['dataset'][key] is False for key in ('native_claim_certification', 'overall_agent_benefit_certification'))


def test_stranded_marker_is_not_retried(tmp_path, monkeypatch):
    directory, child, pack, _, executable = temporary_originals(tmp_path, monkeypatch)
    write(child / 'stdin-candidate-oracle.started', {'explicit_synthetic_crash': True})
    pack['directory_images'] = [([str(directory)], holdout_intake.originals.development_review.evidence([directory]))]
    pack['proof_bindings'] = deepcopy(pack['directory_images'][0][1][0]['files'])
    monkeypatch.setattr(holdout_intake.intake.oracle, 'execute', lambda *_: pytest.fail('stranded Oracle retry forbidden'))
    result = holdout_intake._proof('fixture', [directory], executable, pack=pack)
    assert result['dataset']['cases'][-1]['reason'] == 'oracle_attempt_incomplete'
    with pytest.raises(FileExistsError):
        holdout_intake._proof('fixture', [directory], executable, pack=pack, execute_new_oracles=True)
