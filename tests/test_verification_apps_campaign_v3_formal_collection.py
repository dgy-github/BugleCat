"""Mocked cohort/source/Oracle and actual isolated fake-transport binary tests."""
import copy
import json
import os
import sqlite3
import subprocess
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import formal_collection as owner
from evals.verification.apps_campaign_v3 import formal_intake as reader
from evals.verification.apps_campaign_v3 import packet, sampling_proposal
from evals.verification.calibration import create, digest
from evals.verification.collection_manifest import file_hash
from evals.verification.collection_receipts import verify

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'rust/target/debug/examples/verification_apps_v3_formal_collect.exe'


def seal(value):
    body = {k: v for k, v in value.items() if k != 'artifact_hash'}
    return {**body, 'artifact_hash': digest(body)}


@pytest.fixture
def fixture(tmp_path, monkeypatch):
    paths = {name: tmp_path / (name + '.json') for name in ('authority', 'resources', 'auth', 'profile', 'source', 'protocol', 'proposal', 'cohort')}
    for name, path in paths.items():
        if name not in {'proposal', 'cohort'}:
            path.write_text('{}')
    create(tmp_path / 'spec.json', owner.development.protocol.specification())
    paths['protocol'] = tmp_path / 'spec.json'
    monkeypatch.setattr(owner.development, 'PROTOCOL_SHA', file_hash(paths['protocol']))
    create(tmp_path / 'formal-spec.json', owner.protocol.specification())
    paths['formal_protocol'] = tmp_path / 'formal-spec.json'
    runtime = dict(executable_sha256='e' * 64, version_info=[3, 13, 0], implementation='cpython', platform='win32')
    monkeypatch.setattr(owner.packet, 'runtime_info', lambda: copy.deepcopy(runtime))
    starts = [{'ordinal': n + 1, 'group_id': f'old-{n}', 'started': {'group_id': f'old-{n}'}} for n in range(2)]
    baseline = dict(main_started=starts, main_reservations=[], jev_reservations=[{'group_id': 'old-jev'}],
                    excluded_groups=['old-0', 'old-1', 'old-jev'], main_metadata=[['authorization_sha256', 'original']],
                    main_ledger_file=str(tmp_path / 'ledger.sqlite3'), remaining_main_attempts=748,
                    authorization_chain={'authorization': {'main_model': {'max_attempts': 750}, 'jev': {'max_attempts': 750}},
                                         'bindings': [dict(file=str(paths['auth']), sha256=file_hash(paths['auth']))]})
    finished = {'old-0': {'status': 'ok'}, 'old-1': None}
    state = {'baseline': baseline, 'finished': finished, 'precollection_reviews': []}
    tasks, source_cases, resource_cases = {}, [], []
    for n in range(700):
        task = {'task_id': n, 'question': f'Original public task {n}', 'starter_code': '',
                'source_kind': packet.FREE if n % 2 else packet.SOLUTION,
                'input_output': {'fn_name': 'f', 'inputs': [[1]], 'outputs': [1]}}
        tasks[n] = task
        case = dict(task_id=n, group_id=f'group-{n}', task_sha256=digest(task), pair_count=1,
                    source_kind=task['source_kind'], original_difficulty='interview' if n < 360 else 'introductory')
        source_cases.append(case)
        resource_cases.append(dict(group_id=case['group_id'], resource_status='qualified'))
    authority, inventory = {'cases': source_cases}, {'cases': resource_cases}
    dev = [f'dev-{n}' for n in range(8)]
    _, stages = sampling_proposal._selected(authority, inventory, set(baseline['excluded_groups']) | set(dev), 252, 175)
    proposal = dict(schema=sampling_proposal.SCHEMA, seed=sampling_proposal.SEED, count_per_stage=252, interview_per_stage=175,
                    introductory_per_stage=77, cases=stages, excluded_whole_dev_groups=dev,
                    excluded_started_reserved_or_historical_groups=sorted(set(baseline['excluded_groups']) | set(dev)),
                    current_ledger_snapshot_sha256=digest(baseline), current_completion_snapshot_sha256=digest(finished),
                    source_bindings=[dict(file=str(paths['source']), sha256=file_hash(paths['source']), roles=['fixture-source'])],
                    oracle_runtime_binding=runtime, authorization_file=str(paths['auth']), main_ledger_file=baseline['main_ledger_file'],
                    authorization_chain_bindings=baseline['authorization_chain']['bindings'], development_cohort_hash='d' * 64,
                    all_fixed_allocation_outcomes_retained=True, membership_changes_after_labels_or_probabilities_allowed=False,
                    natural_negative_sufficiency_proven=False, source_only_proposal_not_source_admission_or_certification=True,
                    certified_threshold=None, candidate_labels_read=False, jev_probabilities_read=False,
                    **{k: False for k in owner.formal_contract.FALSE_FIELDS}, **{k: 0 for k in owner.formal_contract.ZERO_FIELDS})
    for name, path_name in [('source_authority', 'authority'), ('resource_inventory', 'resources'), ('development_cohort', 'source'), ('protocol', 'protocol')]:
        proposal[name + '_file'], proposal[name + '_sha256'] = str(paths[path_name]), file_hash(paths[path_name])
    create(paths['proposal'], seal(proposal))
    monkeypatch.setattr(owner.formal_contract, 'review_proposal', lambda path: state['precollection_reviews'].append(str(path)))
    monkeypatch.setattr(owner, 'budget_snapshot', lambda path: copy.deepcopy((state['baseline'], state['finished'])))
    monkeypatch.setattr(owner.development, 'inputs', lambda *args: (authority, inventory))
    monkeypatch.setattr(owner.development, 'original_tasks', lambda a, cases: ({c['task_id']: tasks[c['task_id']] for c in cases}, paths['source']))
    monkeypatch.setattr(owner, 'validate_profile', lambda path: ({'artifact_hash': 'c' * 64}, []))
    monkeypatch.setattr(owner, 'owners', lambda: [owner.development.binding('fixture_owner', paths['source'])])
    value = owner.preregister(paths['proposal'], paths['auth'], paths['profile'], tmp_path / 'uncreated', protocol_file=paths['formal_protocol'])
    create(paths['cohort'], value)
    return dict(paths=paths, value=value, state=state, tasks=tasks, authority=authority, inventory=inventory, tmp=tmp_path)


def add_start(fixture, case, status='ok'):
    current = fixture['state']['baseline']
    event = {k: case[k] for k in ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')}
    event['artifact_cohort_hash'] = fixture['value']['artifact_hash']
    current['main_started'].append(dict(ordinal=len(current['main_started']) + 1, group_id=case['group_id'], started=event))
    current['excluded_groups'].append(case['group_id'])
    current['remaining_main_attempts'] -= 1
    fixture['state']['finished'][case['group_id']] = {'call_status': status} if status else None


def test_genuine_cohort_rechecks_precollection_once_but_later_uses_original_prefix(fixture, monkeypatch):
    value = owner.validate_cohort(fixture['paths']['cohort'])
    assert value['collection_authorized'] is True and value['collection_authorized_split'] == 'calibration'
    assert value['formal_registration'] is value['holdout_opened'] is value['production_authorized'] is False
    assert len(value['cases']) == 504 and value['funded_allocation']['maximum_total_main'] == 504
    assert len(fixture['state']['precollection_reviews']) == 1
    first = value['cases'][0]
    add_start(fixture, first)
    monkeypatch.setattr(owner.formal_contract, 'review_proposal', lambda *a: pytest.fail('cannot rechoose after main starts'))
    assert owner.validate_cohort(fixture['paths']['cohort']) == value
    manifest = owner.manifest_value(fixture['paths']['cohort'])
    assert len(manifest['cases']) == 8 and manifest['cases'][0]['id'] == value['cases'][1]['id']
    assert all(set(task) == {'task_id', 'question', 'starter_code', 'fn_name', 'source_kind', 'main_prompt'} for task in manifest['public_tasks'])


def test_current_realistic_750_remainder_insufficient_rejects_before_output(fixture):
    original = load_proposal = json.loads(fixture['paths']['proposal'].read_text())
    fixture['state']['baseline']['remaining_main_attempts'] = 232
    load_proposal['current_ledger_snapshot_sha256'] = digest(fixture['state']['baseline'])
    fixture['paths']['proposal'].write_text(json.dumps(seal(original)))
    before = set(fixture['tmp'].iterdir())
    with pytest.raises(ValueError, match='cannot_fund_complete'):
        owner.preregister(fixture['paths']['proposal'], fixture['paths']['auth'], fixture['paths']['profile'], fixture['tmp'] / 'never',
                          protocol_file=fixture['paths']['formal_protocol'])
    assert set(fixture['tmp'].iterdir()) == before


@pytest.mark.parametrize('field', ['formal_registration', 'holdout_opened', 'production_authorized', 'candidate_labels_assigned', 'actual_APPS_holdout_owner_available'])
def test_resealed_boolean_authority_promotions_rejected(fixture, field):
    value = copy.deepcopy(fixture['value'])
    value[field] = True
    fixture['paths']['cohort'].write_text(json.dumps(seal(value)))
    with pytest.raises(ValueError):
        owner.validate_cohort(fixture['paths']['cohort'])


@pytest.mark.parametrize('split', ['holdout', 'development', True])
def test_no_caller_consumption_hash_or_boolean_can_enable_holdout(fixture, split):
    with pytest.raises(ValueError, match='once_source_owner_unavailable'):
        owner.manifest_value(fixture['paths']['cohort'], split=split)


@pytest.mark.parametrize('mutation', ['old_start', 'old_finish', 'metadata', 'reservation', 'baseline', 'drop_case', 'swap_split', 'runtime', 'owner_graph'])
def test_source_history_deletion_and_type_drift_rejected(fixture, mutation):
    value = copy.deepcopy(fixture['value'])
    state = fixture['state']
    if mutation == 'old_start':
        state['baseline']['main_started'][0]['started']['group_id'] = 'changed'
    elif mutation == 'old_finish':
        state['finished']['old-0'] = {'status': 'changed'}
    elif mutation == 'metadata':
        state['baseline']['main_metadata'] = []
    elif mutation == 'reservation':
        state['baseline']['jev_reservations'] = []
    elif mutation == 'baseline':
        value['ledger_snapshot']['remaining_main_attempts'] = 747
    elif mutation == 'drop_case':
        value['cases'].pop()
    elif mutation == 'swap_split':
        value['cases'][0]['split'] = 'holdout'
    elif mutation == 'runtime':
        value['oracle_runtime_binding']['version_info'][0] = 3.0
    else:
        value['bindings'] = [b for b in value['bindings'] if b['role'] != 'fixture_owner']
    fixture['paths']['cohort'].write_text(json.dumps(seal(value)))
    with pytest.raises(ValueError):
        owner.validate_cohort(fixture['paths']['cohort'])


def test_existing_manifest_can_revalidate_after_later_starts_without_reselection(fixture):
    original = owner.manifest_value(fixture['paths']['cohort'])
    file = fixture['tmp'] / 'manifest.json'
    create(file, original)
    for case in fixture['value']['cases'][:10]:
        add_start(fixture, case)
    assert owner.validate_manifest(file) == original
    next_manifest = owner.manifest_value(fixture['paths']['cohort'])
    assert next_manifest['cases'][0] == fixture['value']['cases'][10]


def test_unresolved_reservation_is_not_silently_skipped(fixture):
    first = fixture['value']['cases'][0]
    fixture['state']['baseline']['main_reservations'].append({'group_id': first['group_id']})
    with pytest.raises(ValueError, match='no_skip_or_retry'):
        owner.manifest_value(fixture['paths']['cohort'])


def test_actual_starts_cannot_skip_the_frozen_unstarted_prefix(fixture):
    add_start(fixture, fixture['value']['cases'][1])
    with pytest.raises(ValueError, match='whole_frozen_prefix'):
        owner.validate_cohort(fixture['paths']['cohort'])


def test_formal_protocol_must_be_explicit_prospective_not_old_history(fixture):
    with pytest.raises(ValueError, match='prospective_protocol_changed'):
        owner.preregister(fixture['paths']['proposal'], fixture['paths']['auth'], fixture['paths']['profile'],
                          fixture['tmp'] / 'not-created', protocol_file=fixture['paths']['protocol'])
    assert fixture['value']['development_protocol_file'] == str(fixture['paths']['protocol'])
    assert fixture['value']['protocol_file'] == str(fixture['paths']['formal_protocol'])


def test_unknown_and_unprepared_members_remain_full_allocation(fixture, monkeypatch):
    value = fixture['value']
    add_start(fixture, value['cases'][0], 'transport_error')
    add_start(fixture, value['cases'][1], None)
    directory = fixture['tmp'] / 'batch'
    directory.mkdir()
    receipt_path = directory / 'receipt.json'
    create(receipt_path, {'call_status': 'transport_error'})
    compiled = {'executable_sha256': 'b' * 64}
    monkeypatch.setattr(reader, 'build', lambda p: compiled)
    def prep(path, executable, info, pack, starts, finished):
        return [(value['cases'][0], {}, {'call_status': 'transport_error'}, 'transport_error', directory, True),
                (value['cases'][1], {}, None, 'main_not_finished', directory, True)], {'directory': str(directory)}
    monkeypatch.setattr(reader, '_prepared', prep)
    result = reader.collect(fixture['paths']['cohort'], [directory], fixture['paths']['auth'])
    assert result['allocated_groups'] == 252 and result['collected_groups'] == 2 and result['uncollected_groups'] == 250
    assert result['label_counts'] == {'unknown': 1, 'unlabeled': 251} and result['pending_groups'] == 1
    assert result['formal_registration'] is False
    with pytest.raises(ValueError, match='in_flight'):
        reader.collect(fixture['paths']['cohort'], [directory], fixture['paths']['auth'], execute_new_oracles=True)
    with pytest.raises(ValueError, match='directory_omitted'):
        reader.collect(fixture['paths']['cohort'], [], fixture['paths']['auth'])


def test_two_historical_batches_merge_current_starts_including_unused_preparations(fixture, monkeypatch):
    value = fixture['value']
    for case in value['cases'][:3]:
        add_start(fixture, case, 'transport_error')
    directories = [fixture['tmp'] / name for name in ('batch1', 'batch2')]
    for directory in directories:
        directory.mkdir()
        create(directory / 'receipt.json', {'call_status': 'transport_error'})
    monkeypatch.setattr(reader, 'build', lambda p: {'executable_sha256': 'b' * 64})
    def prep(path, executable, info, pack, starts, finished):
        indexes = (0,) if path == directories[0] else (1, 2)
        items = [(value['cases'][n], {}, {'call_status': 'transport_error'}, 'transport_error', path, True) for n in indexes]
        if path == directories[0]:
            items.append((value['cases'][1], {}, None, 'main_not_started', path, False))
        return items, {'directory': str(path)}
    monkeypatch.setattr(reader, '_prepared', prep)
    result = reader.collect(fixture['paths']['cohort'], directories, fixture['paths']['auth'])
    assert result['collected_groups'] == 3 and result['label_counts'] == {'unknown': 3, 'unlabeled': 249}
    with pytest.raises(ValueError, match='directory_omitted'):
        reader.collect(fixture['paths']['cohort'], directories[1:], fixture['paths']['auth'])


def test_oracle_explicit_create_new_once_default_never_executes(fixture, monkeypatch):
    case = fixture['value']['cases'][0]
    directory = fixture['tmp'] / 'oracle-case'
    directory.mkdir()
    original = {'result': {'candidate': '```python\ndef f(x):\n    return x\n```'}}
    create(directory / 'receipt.json', original)
    prepared = (case, {}, original, 'ok', directory, True)
    calls = []
    def label(task, answer, record=None):
        return {'label': 'supported' if record else 'unknown', 'reason': 'recorded' if record else 'independent_original_oracle_not_recorded', 'call_eligible': bool(record)}
    def create_oracle(path, task, answer):
        calls.append(path)
        create(path, {'mock_original': True})
    monkeypatch.setattr(reader.intake, 'label', label)
    monkeypatch.setattr(reader.intake, 'create_oracle', create_oracle)
    task = fixture['tasks'][case['task_id']]
    assert reader._outcome(case, prepared, task, False)['label'] == 'unknown' and not calls
    assert reader._outcome(case, prepared, task, True)['label'] == 'supported' and len(calls) == 1
    before = (directory / 'apps-formal-oracle.json').read_bytes()
    assert reader._outcome(case, prepared, task, True)['label'] == 'supported' and len(calls) == 1
    assert (directory / 'apps-formal-oracle.json').read_bytes() == before


def binary(args):
    if not BINARY.exists():
        pytest.fail('build the new example actual entry first')
    env = {k: v for k, v in os.environ.items() if not any(s in k.upper() for s in ('KEY', 'TOKEN', 'SECRET'))}
    return subprocess.run([str(BINARY), *args], capture_output=True, text=True, env=env, timeout=20)


def test_actual_binary_build_info_proves_all_new_and_legacy_sources():
    result = reader.build(BINARY)
    assert result['executable_sha256'] == file_hash(BINARY)
    assert all(result[role] == file_hash(path) for role, path in reader.runner_sources().items())


@pytest.mark.parametrize('scenario,status,count', [('success', 'ok', 511), ('duplicate', 'ok', 511),
                                                 ('failure', 'offline_transport_failure', 511), ('timeout', 'timeout', 511),
                                                 ('malformed', 'response_content_invalid', 511),
                                                 ('model_mismatch', 'response_model_or_usage_invalid', 511),
                                                 ('invalid_usage', 'response_model_or_usage_invalid', 511),
                                                 ('cancel_during', 'cancelled', 511), ('cancel_before', None, 510), ('tamper', None, 510)])
def test_actual_fake_binary_receipt_sqlite_raw_session_sources_cancel_no_real_budget(tmp_path, scenario, status, count):
    directory = tmp_path / scenario
    child = binary(['offline-test', str(directory), scenario])
    assert child.returncode == 0, child.stderr
    result = json.loads(child.stdout)
    assert result['paid_calls'] == result['http_calls'] == 0 and result['global_main_attempts'] == count
    with sqlite3.connect(directory / 'offline-ledger/main-attempts.sqlite3') as db:
        assert db.execute('select count(*) from attempts').fetchone()[0] == count
    if status:
        receipt = json.loads((directory / 'receipt.json').read_bytes())
        events = [json.loads(line) for line in (directory / 'attempt-events.jsonl').read_text().splitlines()]
        assert receipt['call_status'] == status and events[-1]['receipt_sha256'] == file_hash(directory / 'receipt.json')
        assert events[1]['actual_request_hash'] == receipt['actual_request_hash'] == file_hash(directory / 'request.json')
        if status == 'ok':
            row = json.loads((directory / 'offline-prepared.json').read_bytes())
            assert verify(row, directory / 'receipt.json', events, BINARY) == receipt
    else:
        assert result['fake_transport_calls'] == 0


def test_actual_binary_no_implicit_live_holdout_rejected_before_config_budget(tmp_path):
    assert binary(['live', str(tmp_path)]).returncode != 0
    path = tmp_path / 'manifest.json'
    create(path, {'schema': owner.MANIFEST, 'split': 'holdout', 'artifact_cohort_file': str(path)})
    child = binary(['prepare', str(path), str(tmp_path / 'must-not-create')])
    assert child.returncode != 0 and not (tmp_path / 'must-not-create').exists()


@pytest.mark.parametrize('scenario', ['manifest', 'cohort', 'source', 'holdout'])
def test_actual_binary_per_request_local_source_byte_guard_before_http_or_budget(tmp_path, scenario):
    child = binary(['offline-local-source-test', str(tmp_path / scenario), scenario])
    assert child.returncode == 0, child.stderr
    result = json.loads(child.stdout)
    assert result['scenario'] == scenario and result['paid_calls'] == result['http_calls'] == result['budget_writes'] == 0
    assert not (tmp_path / scenario / 'main-attempts.sqlite3').exists()
