"""Development-only allocation proofs and actual executable, zero network."""
import copy
import json
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import development as owner
from evals.verification.apps_campaign_v3 import packet, resources
from evals.verification.calibration import create, digest
from evals.verification.collection_manifest import file_hash
from evals.verification.collection_receipts import verify

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'rust/target/debug/examples/verification_apps_v3_collect.exe'


@pytest.fixture
def fixture(tmp_path, monkeypatch):
    authority_file, resource_file, auth, profile = [tmp_path / name for name in ('authority.json', 'resources.json', 'auth.json', 'profile.json')]
    for p in (authority_file, resource_file, auth, profile):
        p.write_text('{}')
    protocol_file = tmp_path / 'protocol.json'
    protocol_file.write_bytes(packet.canonical_bytes(owner.protocol.specification()))
    monkeypatch.setattr(owner, 'PROTOCOL_SHA', file_hash(protocol_file))
    baseline = {'main_started': [{'ordinal': n + 1, 'group_id': f'old-{n}', 'started': {'id': f'old-{n}'}} for n in range(510)],
                'jev_reservations': [{'group_id': f'jev-{n}'} for n in range(222)], 'main_reservations': [],
                'excluded_groups': ['group-0'], 'main_metadata': [['authorization_sha256', 'original']],
                'authorization_chain': {'authorization': {'main_model': {'max_attempts': 750}, 'jev': {'max_attempts': 750}}, 'bindings': []},
                'remaining_main_attempts': 240}
    state = {'baseline': baseline, 'finished': {'old-0': {'call_status': 'ok'}}}
    tasks, cases, resource_cases = {}, [], []
    for n in range(20):
        task = {'task_id': n, 'group_id': f'group-{n}', 'question': f'Original {n}', 'starter_code': '',
                'input_output': {'fn_name': 'f', 'inputs': [[1]], 'outputs': [1]},
                'source_kind': packet.FREE if n % 2 else packet.SOLUTION}
        tasks[n] = task
        case = {'task_id': n, 'group_id': task['group_id'], 'source_kind': task['source_kind'],
                'original_difficulty': 'interview' if n < 10 else 'introductory', 'task_sha256': digest(task),
                'reference_sha256': 'never-main-visible', 'pair_count': 1}
        cases.append(case)
        resource_cases.append({**case, 'resource_status': 'qualified', 'resource_receipt': resources.inspect(task)})
    authority = {'cases': cases}
    inventory = {'cases': resource_cases}
    monkeypatch.setattr(owner, 'inputs', lambda *args: (authority, inventory))
    monkeypatch.setattr(owner, 'original_tasks', lambda a, c: ({r['task_id']: tasks[r['task_id']] for r in c}, auth))
    monkeypatch.setattr(owner, 'validate_profile', lambda p: ({'artifact_hash': 'profile'}, []))
    monkeypatch.setattr(owner, 'snapshot', lambda a: copy.deepcopy((state['baseline'], state['finished'])))
    monkeypatch.setattr(owner, 'owners', lambda: [owner.binding('fixture_owner', auth)])
    value = owner.preregister(authority_file, resource_file, auth, profile, tmp_path / 'new-run')
    cohort_file = tmp_path / 'cohort.json'
    create(cohort_file, value)
    return {'value': value, 'file': cohort_file, 'state': state, 'authority': authority, 'inventory': inventory, 'tasks': tasks}


def reseal(value):
    plain = {k: v for k, v in value.items() if k != 'artifact_hash'}
    return {**plain, 'artifact_hash': digest(plain)}


def test_balanced_fixed_allocation_excludes_snapshot_group_no_outcome_selection(fixture):
    value = fixture['value']
    assert len(value['cases']) == 8 and len({c['group_id'] for c in value['cases']}) == 8
    assert [c['original_difficulty'] for c in value['cases']] == ['interview'] * 4 + ['introductory'] * 4
    assert 'group-0' not in {c['group_id'] for c in value['cases']}
    assert owner.validate_cohort(fixture['file']) == value
    assert value['formal_registration'] is value['production_authorized'] is value['holdout_opened'] is False
    assert value['whole_allocation_excluded_from_future_formal'] is True


@pytest.mark.parametrize('count', [0, 9, True, 1.0])
def test_invalid_upper_bound(fixture, count):
    with pytest.raises(ValueError, match='upper bound'):
        owner.choose(fixture['authority'], fixture['inventory'], fixture['state']['baseline'], count)


def test_resource_refusal_never_replaced_based_on_candidate_or_reference_labels(fixture):
    before = owner.choose(fixture['authority'], fixture['inventory'], fixture['state']['baseline'], 8)
    fixture['inventory']['cases'][0]['resource_status'] = 'oversize'
    # Existing excluded group0 is already absent. No outcome is consulted.
    for case in fixture['authority']['cases']:
        case['candidate_label'] = 'unsupported'
    after = owner.choose(fixture['authority'], fixture['inventory'], fixture['state']['baseline'], 8)
    assert [c['group_id'] for c in before] == [c['group_id'] for c in after]


@pytest.mark.parametrize('field', ['formal_registration', 'holdout_opened', 'production_authorized', 'candidate_labels_assigned'])
def test_resealed_promotion_rejected(fixture, field):
    value = copy.deepcopy(fixture['value'])
    value[field] = True
    fixture['file'].write_text(json.dumps(reseal(value)))
    with pytest.raises(ValueError, match='development-only'):
        owner.validate_cohort(fixture['file'])


@pytest.mark.parametrize('mutation', ['settings_float', 'task_float', 'runtime_float', 'start_float'])
def test_exact_original_numeric_types_cannot_alias(fixture, mutation):
    value = copy.deepcopy(fixture['value'])
    if mutation == 'settings_float':
        value['main_collection_settings']['max_tokens'] = 16384.0
    elif mutation == 'task_float':
        value['cases'][0]['task_id'] = float(value['cases'][0]['task_id'])
    elif mutation == 'runtime_float':
        value['oracle_runtime_binding']['version_info'][0] = float(value['oracle_runtime_binding']['version_info'][0])
    else:
        fixture['state']['baseline']['main_started'][0]['ordinal'] = 1.0
    fixture['file'].write_text(json.dumps(reseal(value)))
    with pytest.raises(ValueError):
        owner.validate_cohort(fixture['file'])


def test_resealed_partial_baseline_cannot_reset_shared_history(fixture):
    value = copy.deepcopy(fixture['value'])
    value['ledger_snapshot']['main_started'].pop()
    fixture['file'].write_text(json.dumps(reseal(value)))
    with pytest.raises(ValueError, match='complete original development budget'):
        owner.validate_cohort(fixture['file'])


@pytest.mark.parametrize('change', ['old_start', 'old_finish', 'metadata', 'reservation', 'owner', 'allocation'])
def test_original_history_or_bindings_change_rejected(fixture, change):
    state = fixture['state']
    value = copy.deepcopy(fixture['value'])
    if change == 'old_start':
        state['baseline']['main_started'][0]['started']['id'] = 'altered'
    elif change == 'old_finish':
        state['finished']['old-0']['call_status'] = 'failed'
    elif change == 'metadata':
        state['baseline']['main_metadata'] = []
    elif change == 'reservation':
        state['baseline']['jev_reservations'].pop()
    elif change == 'owner':
        Path(value['bindings'][0]['file']).write_text('changed')
    else:
        value['cases'].reverse()
        fixture['file'].write_text(json.dumps(reseal(value)))
    with pytest.raises(ValueError):
        owner.validate_cohort(fixture['file'])


def test_append_only_real_starts_allowed_and_manifest_has_no_private_task_fields(fixture):
    fixture['state']['baseline']['main_started'].append({'ordinal': 511, 'group_id': fixture['value']['cases'][0]['group_id'], 'started': {'id': 'actual'}})
    manifest = owner.manifest_value(fixture['file'])
    assert len(manifest['public_tasks']) == 8
    for task in manifest['public_tasks']:
        assert set(task) == {'task_id', 'question', 'starter_code', 'fn_name', 'source_kind', 'main_prompt'}
        assert not any(k in task for k in ('reference', 'input_output', 'oracle', 'label'))
    manifest_file = fixture['file'].with_name('manifest.json')
    create(manifest_file, manifest)
    assert owner.validate_manifest(manifest_file) == manifest
    manifest['public_tasks'][0]['reference'] = 'pollution'
    manifest_file.write_text(json.dumps(reseal(manifest)))
    with pytest.raises(ValueError, match='manifest changed'):
        owner.validate_manifest(manifest_file)


def binary(args):
    assert BINARY.exists(), 'build the new APPS collector before binary regression'
    env = {k: v for k, v in os.environ.items() if k.upper() in {'SYSTEMROOT', 'WINDIR', 'TEMP', 'TMP', 'PATH'}}
    return subprocess.run([str(BINARY), *args], cwd=ROOT, env=env, capture_output=True, text=True, timeout=30)


def test_actual_build_info_no_configuration_or_ledger_access():
    child = binary(['build-info'])
    assert child.returncode == 0, child.stderr
    value = json.loads(child.stdout)
    assert value['paid_calls'] == value['http_calls'] == 0
    paths = {'runner_source_sha256': ROOT / 'rust/crates/ncx-core/examples/verification_apps_v3_collect.rs',
             'support_source_sha256': ROOT / 'rust/crates/ncx-core/examples/verification_apps_v3_collect/support.rs',
             'collection_engine_source_sha256': ROOT / 'rust/crates/ncx-core/examples/verification_apps_v3_collect/engine.rs',
             'packet_owner_source_sha256': Path(packet.__file__), 'development_owner_source_sha256': Path(owner.__file__)}
    for field, path in paths.items():
        assert value['build_info'][field] == file_hash(path)
    assert value['build_info']['executable_sha256'] == file_hash(BINARY)


@pytest.mark.parametrize('scenario,status,started', [
    ('success', 'ok', 511), ('failure', 'offline_transport_failure', 511),
    ('malformed', 'response_content_invalid', 511), ('model_mismatch', 'response_model_or_usage_invalid', 511),
    ('invalid_usage', 'response_model_or_usage_invalid', 511), ('timeout', 'timeout', 511),
    ('cancel_before', None, 510), ('cancel_during', 'cancelled', 511), ('tamper', None, 510), ('duplicate', 'ok', 511),
])
def test_actual_binary_original_bytes_failure_cancel_and_shared_ledger(tmp_path, scenario, status, started):
    directory = tmp_path / scenario
    child = binary(['offline-test', str(directory), scenario])
    assert child.returncode == 0, child.stderr
    summary = json.loads(child.stdout)
    assert summary['paid_calls'] == summary['http_calls'] == 0
    assert summary['global_main_attempts'] == started and summary['remaining'] == 750 - started
    with sqlite3.connect(directory / 'offline-ledger/main-attempts.sqlite3') as db:
        assert db.execute('SELECT count(*) FROM attempts').fetchone()[0] == started
        assert db.execute("SELECT value FROM metadata WHERE key='authorization_sha256'").fetchone()[0] == 'offline'
        if status:
            saved = json.loads(db.execute('SELECT finished_json FROM attempts WHERE ordinal=511').fetchone()[0])
            assert saved['call_status'] == status
    if status:
        receipt = json.loads((directory / 'receipt.json').read_bytes())
        assert receipt['call_status'] == status and receipt['attempt_ordinal'] == 511
        events = [json.loads(line) for line in (directory / 'attempt-events.jsonl').read_text().splitlines()]
        assert [e['event'] for e in events] == ['attempt_started', 'request_submitted', 'attempt_finished']
        assert events[1]['actual_request_hash'] == file_hash(directory / 'request.json')
        assert events[-1]['receipt_sha256'] == file_hash(directory / 'receipt.json')
        assert all(e['build_info'] == receipt['build_info'] for e in events)
        assert receipt['formal_registration'] is receipt['production_authorized'] is receipt['holdout_opened'] is False
        if status == 'ok':
            prepared = json.loads((directory / 'offline-prepared.json').read_bytes())
            assert verify(prepared, directory / 'receipt.json', events, BINARY) == receipt
    else:
        assert summary['fake_transport_calls'] == 0


def test_actual_no_implicit_live_or_source_promotion(tmp_path):
    assert binary(['live', str(tmp_path)]).returncode != 0
    manifest = tmp_path / 'bad.json'
    manifest.write_text(json.dumps({'schema': owner.MANIFEST, 'split': 'holdout', 'artifact_cohort_file': str(manifest)}))
    child = binary(['prepare', str(manifest), str(tmp_path / 'new')])
    assert child.returncode != 0 and 'apps_development_only' in child.stderr
    assert not (tmp_path / 'new').exists()


def test_actual_resource_pin_rejected_before_source_or_budget_access(tmp_path, monkeypatch):
    fake = tmp_path / 'changed-inventory.json'
    fake.write_text('{}')
    monkeypatch.setattr(owner.admission_guard, 'pinned', lambda *args: {})
    with pytest.raises(ValueError, match='pinned resource/packet'):
        owner.inputs(tmp_path / 'authority', fake)


def test_preregistration_is_pure_and_snapshot_drift_stops_before_output(fixture, monkeypatch, tmp_path):
    before = set(tmp_path.iterdir())
    by_role = {b['role']: b['file'] for b in fixture['value']['bindings']}
    value = owner.preregister(by_role['source_authority'], by_role['resource_inventory'], by_role['authorization'],
                              by_role['profile'], tmp_path / 'uncreated')
    assert value['new_paid_calls'] == 0 and not (tmp_path / 'uncreated').exists()
    assert set(tmp_path.iterdir()) == before
    original = copy.deepcopy((fixture['state']['baseline'], fixture['state']['finished']))
    calls = []

    def changing_snapshot(path):
        calls.append(path)
        result = copy.deepcopy(original)
        if len(calls) > 1:
            result[0]['main_metadata'].append(['changed', 'true'])
        return result
    monkeypatch.setattr(owner, 'snapshot', changing_snapshot)
    with pytest.raises(ValueError, match='snapshot changed during'):
        owner.preregister(by_role['source_authority'], by_role['resource_inventory'], by_role['authorization'],
                          by_role['profile'], tmp_path / 'still-uncreated')
    assert not (tmp_path / 'still-uncreated').exists()
