"""Isolated once-source/append-only archive tests; never real holdout opening."""
import copy
import json
import sqlite3
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import formal_archive as archive
from evals.verification.apps_campaign_v3 import formal_holdout as owner
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash


def write(path, value):
    Path(path).write_text(json.dumps(value), encoding='utf-8')


@pytest.fixture
def source(tmp_path, monkeypatch):
    root = tmp_path / 'jev'
    root.mkdir()
    (root / 'reservations').mkdir()
    write(root / 'authorization.json', {'authorization_sha256': 'fixture-auth', 'kind': 'jev'})
    ledger = tmp_path / 'main.sqlite3'
    start = {'group_id': 'cal', 'split': 'calibration'}
    finish = {'group_id': 'cal', 'status': 'ok'}
    with sqlite3.connect(ledger) as db:
        db.execute('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT)')
        db.execute('CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT,started_json TEXT,finished_json TEXT)')
        db.execute('INSERT INTO metadata VALUES (?,?)', ('authorization_sha256', 'fixture-auth'))
        db.execute('INSERT INTO attempts VALUES (?,?,?,?)', (1, 'cal', json.dumps(start), json.dumps(finish)))
    calfile, cohortfile = tmp_path / 'calibration.json', tmp_path / 'cohort.json'
    cal = {'qualified_for_once_holdout': True, 'calibration_numeric_gate_met': True, 'qualified_for_holdout_threshold': .9,
           'certified_threshold': None, 'formal_certification': False, 'production_authorized': False,
           'artifact_hash': 'c' * 64, 'cohort_file': str(cohortfile), 'cohort_sha256': 'rawfixture',
           'registration_file': str(tmp_path / 'registration.json'), 'registration_sha256': 'r' * 64}
    cal['artifact_hash'] = digest({k: v for k, v in cal.items() if k != 'artifact_hash'})
    write(calfile, cal)
    cohort = {'artifact_hash': 'h' * 64, 'cases': [
        {'group_id': 'cal', 'split': 'calibration'},
        {'id': 'hold', 'group_id': 'hold', 'task_id': 42, 'task_sha256': 't' * 64, 'source_kind': 'free_function', 'split': 'holdout'}],
        'excluded_groups': ['old-dev'], 'funded_allocation': {'holdout_main_earmark': 1}}
    write(cohortfile, cohort)
    before = {'authorization_chain': {'ledger_authorization_sha256': 'fixture-auth', 'authorization': {'jev': {'ledger_dir': str(root), 'max_attempts': 861},
             'main_model': {'max_attempts': 1150}}}, 'main_ledger_file': str(ledger),
             'main_metadata': [['authorization_sha256', 'fixture-auth']],
             'main_started': [{'ordinal': 1, 'group_id': 'cal', 'started': start}],
             'main_reservations': [], 'jev_reservations': [], 'remaining_main_attempts': 1149}
    finished = {'cal': finish}
    state = {'cal': cal, 'cohort': cohort, 'before': before, 'finished': finished, 'proofs': 0}
    def proof(path):
        state['proofs'] += 1
        return copy.deepcopy(cal), ('genuine registered tuple',), copy.deepcopy((cohort, {}, 'train', before, finished))
    monkeypatch.setattr(owner.formal_evaluation, 'proof_transaction', proof)
    monkeypatch.setattr(owner, '_bindings', lambda c: [{'file': str(calfile), 'sha256': file_hash(calfile)}])
    return dict(root=root, calfile=calfile, cohortfile=cohortfile, ledger=ledger, state=state)


def test_readonly_default_and_durable_same_scope_once_no_reservation(source):
    root = source['root']
    value = owner.source_value(source['calfile'])
    assert not (root / owner.NAMESPACE).exists() and value['fixed_threshold'] == .9
    result = owner.open_source(source['calfile'])
    assert json.loads((root / owner.NAMESPACE / ('h' * 64 + '.json')).read_text()) == result
    assert result['new_paid_calls'] == result['reservations'] == result['oracle_executions'] == 0
    assert result['formal_certification'] is False and result['certified_threshold'] is None
    assert not list((root / 'reservations').iterdir())
    with pytest.raises(ValueError, match='already_consumed'):
        owner.open_source(source['calfile'])


@pytest.mark.parametrize('mutation', ['unqualified', 'missing_threshold', 'typed_threshold', 'production', 'budget', 'already_used', 'history_overlap'])
def test_unqualified_budget_seen_source_or_forged_boolean_never_opens(source, mutation):
    state = source['state']
    if mutation == 'unqualified':
        state['cal']['qualified_for_once_holdout'] = False
    elif mutation == 'missing_threshold':
        state['cal']['qualified_for_holdout_threshold'] = None
    elif mutation == 'typed_threshold':
        state['cal']['qualified_for_holdout_threshold'] = True
    elif mutation == 'production':
        state['cal']['production_authorized'] = True
    elif mutation == 'budget':
        state['before']['remaining_main_attempts'] = 0
    elif mutation == 'already_used':
        state['before']['jev_reservations'] = [{'group_id': 'hold'}]
    else:
        state['cohort']['excluded_groups'].append('hold')
    with pytest.raises(ValueError):
        owner.open_source(source['calfile'])
    assert not (source['root'] / owner.NAMESPACE).exists()


def test_stale_lock_is_not_deleted_and_main_drift_blocks_marker(source):
    lock = source['root'] / 'reservation.lock'
    lock.write_bytes(b'original stale owner')
    with pytest.raises(FileExistsError):
        owner.open_source(source['calfile'])
    assert lock.read_bytes() == b'original stale owner'
    lock.unlink()
    with sqlite3.connect(source['ledger']) as db:
        db.execute('UPDATE attempts SET finished_json=?', (json.dumps({'changed': True}),))
    with pytest.raises(ValueError, match='main_changed'):
        owner.open_source(source['calfile'])
    assert not lock.exists() and not (source['root'] / owner.NAMESPACE).exists()


@pytest.mark.parametrize('mutation', ['calibration_raw', 'graph_raw', 'graph_member'])
def test_source_projection_never_captures_changed_after_proof_bytes_as_new_valid_hash(source, monkeypatch, mutation):
    evidence = source['calfile'].parent / 'original-calibration-event.json'
    write(evidence, {'original': True})
    def graph(actual):
        paths = [source['calfile'], evidence]
        if mutation == 'graph_member' and (evidence.parent / 'added-event.json').exists():
            paths.append(evidence.parent / 'added-event.json')
        return [{'file': str(p), 'sha256': file_hash(p)} for p in paths]
    monkeypatch.setattr(owner, '_bindings', graph)
    proof = owner.formal_evaluation.proof_transaction
    def changed(path):
        original = proof(path)
        if mutation == 'calibration_raw':
            source['calfile'].write_bytes(source['calfile'].read_bytes() + b' ')
        elif mutation == 'graph_raw':
            write(evidence, {'changed after original proof': True})
        else:
            write(evidence.parent / 'added-event.json', {'unbound': True})
        return original
    monkeypatch.setattr(owner.formal_evaluation, 'proof_transaction', changed)
    with pytest.raises(ValueError, match='graph_changed_after_proof'):
        owner.open_source(source['calfile'])
    assert not (source['root'] / owner.NAMESPACE).exists()


def archived_prefix(source):
    marker = owner.open_source(source['calfile'])
    # Fixture canonical identity and exact source marker bytes are preserved.
    before = marker['before_holdout_snapshot']
    current, finished = copy.deepcopy(before), copy.deepcopy(source['state']['finished'])
    case = marker['cases'][0]
    started = {**case, 'artifact_cohort_hash': source['state']['cohort']['artifact_hash'],
               'holdout_source_hash': marker['artifact_hash'], 'holdout_source_file': marker['marker_file'],
               'holdout_source_sha256': file_hash(marker['marker_file'])}
    current['main_started'].append({'ordinal': 2, 'group_id': 'hold', 'started': started})
    current['remaining_main_attempts'] -= 1
    finished['hold'] = {'status': 'ok'}
    return marker, current, finished


def test_archive_accepts_only_original_prefix_and_actual_bound_ordered_holdout(source):
    marker, current, finished = archived_prefix(source)
    archive._prefix(marker, current, finished, source['state']['cohort'])
    assert marker['allocation_groups'] == ['hold']


@pytest.mark.parametrize('mutation', ['old_start', 'old_finish', 'old_metadata', 'root', 'wrong_group', 'wrong_order', 'wrong_scope',
                                     'missing_marker', 'raw_marker', 'unbound_reservation', 'reservation_prefix'])
def test_archive_changed_original_or_unbound_append_rejected(source, mutation):
    marker, current, finished = archived_prefix(source)
    if mutation == 'old_start':
        current['main_started'][0]['started']['extra'] = 'changed'
    elif mutation == 'old_finish':
        finished['cal']['status'] = 'failed'
    elif mutation == 'old_metadata':
        current['main_metadata'] = []
    elif mutation == 'root':
        current['main_ledger_file'] = 'different.sqlite3'
    elif mutation == 'wrong_group':
        current['main_started'][-1]['group_id'] = 'old-dev'
    elif mutation == 'wrong_order':
        current['main_started'].append(copy.deepcopy(current['main_started'][-1]))
    elif mutation == 'wrong_scope':
        current['main_started'][-1]['started']['artifact_cohort_hash'] = 'wrong'
    elif mutation == 'missing_marker':
        current['main_started'][-1]['started'].pop('holdout_source_hash')
    elif mutation == 'raw_marker':
        current['main_started'][-1]['started']['holdout_source_sha256'] = 'changed'
    elif mutation == 'unbound_reservation':
        current['jev_reservations'] = [{'group_id': 'hold', 'reservation_index': 1}]
    else:
        marker['before_holdout_snapshot']['jev_reservations'] = [{'group_id': 'old', 'reservation_index': 1, 'status': 'failed'}]
        current['jev_reservations'] = [{'group_id': 'old', 'reservation_index': 1, 'status': 'changed'}]
    with pytest.raises(ValueError):
        archive._prefix(marker, current, finished, source['state']['cohort'])


def test_archive_registration_replays_each_original_not_just_sealed_numbers(tmp_path, monkeypatch):
    directory = tmp_path / 'batch'
    directory.mkdir()
    cases = [{'id': f'case-{i}', 'group_id': f'g-{i}', 'task_id': i, 'split': 'calibration'} for i in range(2)]
    outcomes = [{'id': c['id'], 'label': 'supported', 'main_status': 'ok'} for c in cases]
    data = {'cases': outcomes, 'collection_bindings': [{'directory': str(directory), 'original': True}],
            'collector_executable_file': 'exe', 'collector_executable_sha256': 'e', 'build_info': {'executable_sha256': 'e'},
            'allocated_groups': 2, 'collected_groups': 2, 'uncollected_groups': 0, 'pending_groups': 0,
            'label_counts': {'supported': 2}}
    data['artifact_hash'] = digest(data)
    file = tmp_path / 'data.json'
    write(file, data)
    current = {'main_started': [{'group_id': c['group_id']} for c in cases]}
    pack = ({'cases': cases}, {0: {}, 1: {}}, None, current, {'g-0': {}, 'g-1': {}})
    monkeypatch.setattr(archive.formal_intake, 'build', lambda p: {'executable_sha256': 'e'})
    items = [(c, None, None, 'ok', None, True) for c in cases]
    monkeypatch.setattr(archive.formal_intake, '_prepared', lambda *a: (items, data['collection_bindings'][0]))
    executed = []
    def original(case, item, task, execute):
        executed.append(execute)
        return copy.deepcopy(outcomes[case['task_id']])
    monkeypatch.setattr(archive.formal_intake, '_outcome', original)
    assert archive.replay_dataset(file, pack=pack) == data
    assert executed == [False, False]
    modified = copy.deepcopy(data)
    modified['cases'][0]['label'] = 'unsupported'
    modified['artifact_hash'] = digest({k: v for k, v in modified.items() if k != 'artifact_hash'})
    write(file, modified)
    with pytest.raises(ValueError, match='oracle_changed'):
        archive.replay_dataset(file, pack=pack)


@pytest.fixture
def archived_source(source, monkeypatch):
    cal = source['state']['cal']
    write(cal['registration_file'], {'original': True})
    cal['registration_sha256'] = file_hash(cal['registration_file'])
    cal.update(allocation_file='original allocation', observations_file='original observations', runner_executable_file='original exe')
    cal['artifact_hash'] = digest({k: v for k, v in cal.items() if k != 'artifact_hash'})
    write(source['calfile'], cal)
    marker = owner.open_source(source['calfile'])
    pack = (source['state']['cohort'], {}, 'original source', source['state']['before'], source['state']['finished'])
    source['state']['cohort']['bindings'] = [{'role': 'authorization', 'file': 'original auth'}]
    monkeypatch.setattr(archive, '_pack', lambda value: pack)
    calls = []
    def registration(path, **kwargs):
        calls.append('all original main and oracle')
        return ('real registration', 'real dataset', 'real binary')
    def calibration(*args, **kwargs):
        calls.append('all original Jev byte joins and numeric gate')
        return copy.deepcopy(cal)
    monkeypatch.setattr(archive, 'revalidate_registration', registration)
    monkeypatch.setattr(archive.formal_evaluation, 'calibration_value', calibration)
    monkeypatch.setattr(archive.formal_collection, 'budget_snapshot', lambda p: copy.deepcopy(pack[3:]))
    return dict(source=source, marker=marker, calls=calls)


def test_validate_once_marker_replays_originals_not_permission_bool(archived_source):
    fixture = archived_source
    actual = owner.validate(fixture['marker']['marker_file'])
    assert actual == fixture['marker']
    assert fixture['calls'] == ['all original main and oracle', 'all original Jev byte joins and numeric gate']


@pytest.mark.parametrize('mutation', ['threshold', 'typed_threshold', 'false_numeric_gate', 'source_binding', 'claimed_cert', 'wrong_path'])
def test_resealed_marker_cannot_replace_threshold_or_original_proof(archived_source, mutation):
    fixture = archived_source
    value = copy.deepcopy(fixture['marker'])
    if mutation == 'threshold':
        value['fixed_threshold'] = .5
    elif mutation == 'typed_threshold':
        value['fixed_threshold'] = True
    elif mutation == 'false_numeric_gate':
        fixture['source']['state']['cal']['qualified_for_once_holdout'] = False
    elif mutation == 'source_binding':
        value['source_bindings'] = []
    elif mutation == 'claimed_cert':
        value['formal_certification'] = True
    else:
        value['marker_file'] = str(fixture['source']['root'] / 'another.json')
    value['artifact_hash'] = digest({k: v for k, v in value.items() if k != 'artifact_hash'})
    write(fixture['marker']['marker_file'], value)
    with pytest.raises(ValueError):
        owner.validate(fixture['marker']['marker_file'])


def test_missing_original_archive_join_fail_closed(archived_source, monkeypatch):
    monkeypatch.setattr(archive, 'revalidate_registration', lambda *a, **k: (_ for _ in ()).throw(ValueError('original raw missing')))
    with pytest.raises(ValueError, match='original raw missing'):
        owner.validate(archived_source['marker']['marker_file'])


def test_caller_value_cannot_replace_actual_fixed_marker(archived_source):
    value = copy.deepcopy(archived_source['marker'])
    value['fixed_threshold'] = .5
    value['artifact_hash'] = digest({k: v for k, v in value.items() if k != 'artifact_hash'})
    with pytest.raises(ValueError, match='actual_fixed_source_marker_changed'):
        archive._source_proof(archived_source['marker']['marker_file'], value=value)
    assert archived_source['calls'] == []


def test_actual_marker_changed_during_archive_proof_refused(archived_source, monkeypatch):
    original = archive.formal_evaluation.calibration_value
    def changed(*args, **kwargs):
        actual = original(*args, **kwargs)
        value = copy.deepcopy(archived_source['marker'])
        value['fixed_threshold'] = .5
        value['artifact_hash'] = digest({k: v for k, v in value.items() if k != 'artifact_hash'})
        write(value['marker_file'], value)
        return actual
    monkeypatch.setattr(archive.formal_evaluation, 'calibration_value', changed)
    with pytest.raises(ValueError, match='source_or_shared_history_changed'):
        archive._source_proof(archived_source['marker']['marker_file'], value=archived_source['marker'])
