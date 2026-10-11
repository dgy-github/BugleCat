"""Temporary accounting fixtures only; no real source approval or ledger writes."""
import json
import sqlite3
import sys
import types
from copy import deepcopy
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import budget as b


def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False), encoding='utf-8')
    return b.file_hash(path)


@pytest.fixture
def ledger(tmp_path):
    main, jev = tmp_path / 'main', tmp_path / 'jev'
    main.mkdir()
    jev.mkdir()
    bindings = []
    for i, (main_cap, jev_cap) in enumerate(b.OLD_CAPS):
        path = tmp_path / f'auth-{i}.json'
        value = {'schema': 'fixture-only-original', 'automatic_retries': 0,
                 'main_model': {'max_attempts': main_cap, 'ledger_dir': str(main)},
                 'jev': {'max_attempts': jev_cap, 'ledger_dir': str(jev)}}
        bindings.append({'file': str(path), 'sha256': write(path, value)})
    chain = {'authorization': value, 'current_sha256': bindings[-1]['sha256'],
             'allowed_sha256': [row['sha256'] for row in bindings],
             'ledger_authorization_sha256': bindings[0]['sha256'], 'bindings': bindings}
    dbfile = main / 'main-attempts.sqlite3'
    with sqlite3.connect(dbfile) as db:
        db.executescript('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);'
                        'CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT UNIQUE,'
                        'split TEXT,started_json TEXT,finished_json TEXT);')
        db.execute('INSERT INTO metadata VALUES(?,?)', ('authorization_sha256', bindings[0]['sha256']))
        for group, auth, terminal in [('failed-old', bindings[0]['sha256'], {'call_status': 'transport_error'}),
                                      ('pending-old', bindings[1]['sha256'], None)]:
            start = {'group_id': group, 'authorization_sha256': auth, 'split': 'development'}
            db.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES(?,?,?,?)',
                       (group, 'development', json.dumps(start), json.dumps(terminal) if terminal is not None else None))
    return {'main': main, 'jev': jev, 'chain': chain, 'db': dbfile, 'tmp': tmp_path}


@pytest.fixture
def extension(ledger, monkeypatch):
    """Substitute source proof only inside this synthetic temporary test scope."""
    old = ledger['chain']
    proposal_file, snapshot_file = ledger['tmp'] / 'proposal.json', ledger['tmp'] / 'snapshot.json'
    baseline, finished = b._snapshot_chain(old)
    cases = {split: [{'group_id': f'fixture-{split}-{i}'} for i in range(count)]
             for split, count in [('development', 8), ('calibration', 400), ('holdout', 400)]}
    proposal = {'cases': cases, 'current_ledger_snapshot_sha256': b.digest(baseline),
                'current_completion_snapshot_sha256': b.digest(finished), 'artifact_hash': 'f' * 64}
    proposal_sha = write(proposal_file, proposal)
    snapshot_sha = write(snapshot_file, {'snapshot': baseline, 'finished': finished})
    pack = {'proposal': proposal, 'authority': {'fixture_only': True}, 'baseline': baseline, 'finished': finished,
            'bindings': b.raw_bindings([proposal_file, snapshot_file]), 'tasks': {}, 'train': None}
    # Never changes a real auth, graph, pin, or ledger. Production loaded_check
    # and actual frozen.load_pack are tested separately; this seam covers logic.
    monkeypatch.setattr(b, 'loaded_check', lambda: {'fixture_only': True})
    monkeypatch.setattr(b, 'PREVIOUS_SHA', old['current_sha256'])
    monkeypatch.setattr(b, 'PROPOSAL_SHA', proposal_sha)
    monkeypatch.setattr(b, 'SNAPSHOT_SHA', snapshot_sha)
    monkeypatch.setattr(b.original, 'resolve', lambda path: deepcopy(old))
    fake = types.ModuleType('evals.verification.apps_stdin_v1.frozen')
    fake.load_pack = lambda *args, **kwargs: deepcopy(pack)
    monkeypatch.setitem(sys.modules, fake.__name__, fake)
    path = ledger['tmp'] / 'future-fixture-approval.json'
    value = {'schema': b.SCHEMA, 'approved_plan': b.PLAN, 'approval_status': 'approved', 'approved': True,
        'user_authorization': b.QUOTE, 'previous_authorization_file': old['bindings'][-1]['file'],
        'previous_authorization_sha256': old['current_sha256'], 'additional_main_attempt_cap': 492,
        'additional_jev_attempt_cap': 176, 'automatic_retries': 0, 'per_group_max_main_attempts': 1,
        'per_group_max_jev_attempts': 1, 'old_attempts_and_failures_count': True, 'old_pending_attempts_count': True,
        'production_authorized': False, 'main_model': {'max_attempts': 1642, 'ledger_dir': str(ledger['main'])},
        'jev': {'max_attempts': 1037, 'ledger_dir': str(ledger['jev'])},
        'source_proposal_file': str(proposal_file), 'source_proposal_sha256': proposal_sha,
        'source_proposal_hash': proposal['artifact_hash'], 'source_snapshot_file': str(snapshot_file),
        'source_snapshot_sha256': snapshot_sha}
    write(path, value)
    return {**ledger, 'path': path, 'value': value, 'baseline': baseline, 'finished': finished,
            'pack': pack, 'proposal_file': proposal_file, 'snapshot_file': snapshot_file}


def test_same_original_roots_failure_pending_and_readonly_caps_bridge(extension):
    before = {str(path): path.read_bytes() for path in extension['tmp'].rglob('*') if path.is_file()}
    current, finished = b.snapshot(extension['path'])
    assert len(current['main_started']) == 2 and current['remaining_main_attempts'] == 1640
    assert finished == {'failed-old': {'call_status': 'transport_error'}, 'pending-old': None}
    assert len(current['authorization_chain']['bindings']) == 5
    assert current['authorization_chain']['ledger_authorization_sha256'] == extension['chain']['ledger_authorization_sha256']
    assert b.bridge(extension['baseline'], extension['finished'], current, finished)['kind'] == 'approved_caps_only_extension'
    assert {str(path): path.read_bytes() for path in extension['tmp'].rglob('*') if path.is_file()} == before


def test_unchanged_four_chain_remains_exact(extension):
    current, finished = b.snapshot(extension['chain']['bindings'][-1]['file'])
    assert b.same(current, extension['baseline'])
    assert b.bridge(extension['baseline'], extension['finished'], current, finished)['kind'] == 'unchanged_original_authorization'


@pytest.mark.parametrize('field,value', [
    ('approval_status', 'pending'), ('approved', 1), ('approved_plan', 'other'), ('user_authorization', '批准'),
    ('user_authorization', '批准 STDIN 808 组方案：主模型追加最多 492 次、Jev 追加最多 176 次（推荐）'),
    ('additional_main_attempt_cap', 493), ('additional_jev_attempt_cap', True), ('automatic_retries', False),
    ('per_group_max_main_attempts', 2), ('per_group_max_jev_attempts', True),
    ('old_attempts_and_failures_count', False), ('old_pending_attempts_count', False), ('production_authorized', True),
    ('previous_authorization_sha256', '0' * 64), ('source_proposal_hash', '0' * 64),
    ('source_proposal_sha256', '0' * 64), ('source_snapshot_sha256', '0' * 64),
])
def test_unapproved_quote_reset_retry_or_source_binding_rejected(extension, field, value):
    changed = deepcopy(extension['value'])
    changed[field] = value
    write(extension['path'], changed)
    with pytest.raises(ValueError):
        b.resolve(extension['path'])


@pytest.mark.parametrize('kind,field,value', [('main_model', 'max_attempts', 1642.0),
    ('jev', 'max_attempts', True), ('jev', 'max_attempts', 1038), ('main_model', 'ledger_dir', 'reset-root')])
def test_cap_types_and_original_root_cannot_change(extension, kind, field, value):
    changed = deepcopy(extension['value'])
    changed[kind][field] = value
    write(extension['path'], changed)
    with pytest.raises(ValueError, match='cap or ledger root'):
        b.resolve(extension['path'])


def test_cannot_extend_an_extension_again(extension):
    changed = deepcopy(extension['value'])
    changed['previous_authorization_file'] = str(extension['path'])
    write(extension['path'], changed)
    with pytest.raises(ValueError, match='previous authorization'):
        b.resolve(extension['path'])


@pytest.mark.parametrize('which', ['proposal', 'snapshot'])
def test_original_raw_bytes_drift_even_whitespace_rejected(extension, which):
    file = extension[which + '_file']
    file.write_bytes(file.read_bytes() + b'\n')
    with pytest.raises(ValueError, match='raw bytes differ'):
        b.resolve(extension['path'])


@pytest.mark.parametrize('which', ['member', 'count', 'baseline', 'completion', 'empty'])
def test_frozen_pack_members_and_history_cannot_be_relabelled(extension, which):
    pack = extension['pack']
    if which == 'member':
        pack['proposal']['cases']['holdout'][0] = pack['proposal']['cases']['calibration'][0]
    elif which == 'count':
        pack['proposal']['cases']['holdout'].pop()
    elif which == 'baseline':
        pack['baseline']['main_started'].pop()
    elif which == 'completion':
        pack['finished']['pending-old'] = {'call_status': 'ok'}
    else:
        pack['baseline']['main_started'] = []
    with pytest.raises(ValueError):
        b.resolve(extension['path'])


@pytest.mark.parametrize('which', ['start', 'finish', 'pending', 'metadata', 'main_reservation', 'jev_reservation', 'remaining'])
def test_bridge_no_history_changed_between_four_and_five(extension, which):
    current, finished = b.snapshot(extension['path'])
    if which == 'start':
        current['main_started'][0]['started']['extra'] = 'changed'
    elif which == 'finish':
        finished['failed-old'] = {'call_status': 'ok'}
    elif which == 'pending':
        finished.pop('pending-old')
    elif which == 'metadata':
        current['main_metadata'].append(['new', 'value'])
    elif which in ('main_reservation', 'jev_reservation'):
        current[which + 's'].append({'group_id': 'new'})
    else:
        current['remaining_main_attempts'] += 1
    with pytest.raises(ValueError):
        b.bridge(extension['baseline'], extension['finished'], current, finished)


@pytest.mark.parametrize('which', ['delete_highest', 'empty', 'metadata', 'reset_sequence'])
def test_old_spent_pending_and_metadata_cannot_reset(ledger, which):
    with sqlite3.connect(ledger['db']) as db:
        if which == 'delete_highest':
            db.execute('DELETE FROM attempts WHERE ordinal=2')
        elif which == 'empty':
            db.execute('DELETE FROM attempts')
            db.execute("UPDATE sqlite_sequence SET seq=0 WHERE name='attempts'")
        elif which == 'metadata':
            db.execute("UPDATE metadata SET value='reset' WHERE key='authorization_sha256'")
        else:
            db.execute("UPDATE sqlite_sequence SET seq=1 WHERE name='attempts'")
    with pytest.raises(ValueError):
        b._snapshot_chain(ledger['chain'])


def add_reservation(ledger, kind='jev', group='reserved-group', index=1, auth=None, retry=0):
    root = ledger['jev'] if kind == 'jev' else ledger['main']
    folder = root / 'reservations'
    folder.mkdir(exist_ok=True)
    write(root / 'authorization.json', {'authorization_sha256': ledger['chain']['ledger_authorization_sha256'], 'kind': kind})
    value = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': kind, 'group_id': group,
             'reservation_index': index, 'authorization_sha256': auth or ledger['chain']['current_sha256'],
             'automatic_retries': retry, 'status': 'reserved_before_transport', 'request_hash': 'a' * 64}
    path = folder / (b.digest(group) + '.json')
    write(path, value)
    return path


def test_reserved_only_main_slot_counts_and_marker_never_readds_slot(ledger):
    path = add_reservation(ledger, 'main_model')
    path.with_suffix('.started').write_bytes(b'fixture claimed before transport')
    current, finished = b._snapshot_chain(ledger['chain'])
    assert current['remaining_main_attempts'] == 1147 and len(current['main_started']) == 2
    assert len(current['main_reservations']) == 1 and finished['pending-old'] is None


@pytest.mark.parametrize('which', ['gap', 'duplicate_index', 'bool_retry', 'origin', 'old_cap', 'orphan_marker', 'lock'])
def test_original_reservations_reject_reset_duplicate_cap_and_stale_lock(ledger, which):
    path = add_reservation(ledger)
    if which == 'gap':
        value = b.load(path)
        value['reservation_index'] = 2
        write(path, value)
    elif which == 'duplicate_index':
        add_reservation(ledger, group='second-group')
    elif which == 'bool_retry':
        value = b.load(path)
        value['automatic_retries'] = False
        write(path, value)
    elif which == 'origin':
        write(ledger['jev'] / 'authorization.json', {'authorization_sha256': '0' * 64, 'kind': 'jev'})
    elif which == 'old_cap':
        value = b.load(path)
        value.update(authorization_sha256=ledger['chain']['bindings'][0]['sha256'], reservation_index=401)
        write(path, value)
    elif which == 'orphan_marker':
        path.with_suffix('.started').write_text('fixture spent')
        path.unlink()
    else:
        (ledger['jev'] / 'reservation.lock').write_text('do not clear stale lock')
    with pytest.raises(ValueError):
        b._snapshot_chain(ledger['chain'])


def test_new_auth_ordinal_allowed_old_auth_cap_not_borrowed(extension):
    new_hash = b.file_hash(extension['path'])
    with sqlite3.connect(extension['db']) as db:
        for n in range(3, 1152):
            group = f'new-{n}'
            start = {'group_id': group, 'authorization_sha256': new_hash, 'split': 'calibration'}
            db.execute('INSERT INTO attempts(group_id,split,started_json) VALUES(?,?,?)',
                       (group, 'calibration', json.dumps(start)))
    current, finished = b.snapshot(extension['path'])
    assert current['main_started'][-1]['ordinal'] == 1151 and finished['new-1151'] is None
    with pytest.raises(ValueError, match='SQLite start'):
        b.snapshot(extension['chain']['bindings'][-1]['file'])
    with sqlite3.connect(extension['db']) as db:
        start['authorization_sha256'] = extension['chain']['current_sha256']
        db.execute('UPDATE attempts SET started_json=? WHERE ordinal=1151', (json.dumps(start),))
    with pytest.raises(ValueError, match='SQLite start'):
        b.snapshot(extension['path'])


@pytest.mark.parametrize('which', ['auth_whitespace', 'reservation_whitespace', 'new_reservation', 'marker', 'metadata', 'sql_finish'])
def test_actual_raw_transaction_before_after_closes(ledger, monkeypatch, which):
    path = add_reservation(ledger)
    original = b.reservations
    changed = []
    def reader(root, kind, chain):
        rows = original(root, kind, chain)
        if kind == 'jev' and not changed:
            changed.append(True)
            if which == 'auth_whitespace':
                auth = Path(chain['bindings'][-1]['file'])
                auth.write_bytes(auth.read_bytes() + b'\n')
            elif which == 'reservation_whitespace':
                path.write_bytes(path.read_bytes() + b'\n')
            elif which == 'new_reservation':
                add_reservation(ledger, group='later-group', index=2)
            elif which == 'marker':
                path.with_suffix('.started').write_text('fixture first claim')
            elif which == 'metadata':
                metadata = ledger['jev'] / 'authorization.json'
                metadata.write_bytes(metadata.read_bytes() + b'\n')
            else:
                with sqlite3.connect(ledger['db']) as db:
                    db.execute('UPDATE attempts SET finished_json=? WHERE ordinal=2', (json.dumps({'call_status': 'ok'}),))
        return rows
    monkeypatch.setattr(b, 'reservations', reader)
    with pytest.raises(ValueError, match='drifted|changed during snapshot'):
        b._snapshot_chain(ledger['chain'])


@pytest.mark.parametrize('name,value', [('SCHEMA', 'caller'), ('QUOTE', '批准'), ('PREVIOUS_SHA', '0' * 64),
    ('PROPOSAL_SHA', '0' * 64), ('SNAPSHOT_SHA', '0' * 64), ('OLD_CAPS', ()), ('EXTRAS', {'main_model': 999})])
def test_production_loaded_literal_option_and_pin_drift_rejected(monkeypatch, name, value):
    monkeypatch.setattr(b, name, value)
    with pytest.raises(ValueError, match='constants differ'):
        b.loaded_check()


def test_production_loaded_graph_and_alias_checks(monkeypatch):
    assert b.loaded_check()['packet'] == '8e854531467011072044eef3168bd92ffe7f38e4132da72fce8cf105b1c67445'
    monkeypatch.setattr(b, 'reservations', lambda *args: ([], []))
    with pytest.raises(ValueError, match='aliases differ'):
        b.loaded_check()


@pytest.mark.parametrize('name,value', [
    ('EXTRAS', {'main_model': 492.0, 'jev': 176}),
    ('EXTRAS', {'main_model': 492, 'jev': 176.0}),
    ('OLD_CAPS', ((400.0, 400), (500, 500), (750, 750), (1150, 861))),
    ('OLD_CAPS', ((400, 400), (500, 500), (750, 750), (1150, 861.0))),
    ('OLD_CAPS', ([400, 400], (500, 500), (750, 750), (1150, 861))),
])
def test_loaded_nested_literal_types_are_exact(monkeypatch, name, value):
    monkeypatch.setattr(b, name, value)
    with pytest.raises(ValueError, match='constants differ'):
        b.loaded_check()


def test_reservation_reader_projection_must_equal_captured_original_raw_bytes(ledger, monkeypatch):
    add_reservation(ledger)
    original = b.reservations
    def reader(root, kind, chain):
        rows, bindings = original(root, kind, chain)
        if kind == 'jev':
            rows[0]['extra_caller_projection'] = True
        return rows, bindings
    monkeypatch.setattr(b, 'reservations', reader)
    with pytest.raises(ValueError, match='parsed projection differs'):
        b._snapshot_chain(ledger['chain'])


@pytest.mark.parametrize('encoded', ['{"same":1,"same":2}', '{"n":NaN}', '{"n":Infinity}', '{"n":1e400}'])
def test_original_budget_and_sql_json_are_strict(encoded):
    with pytest.raises(ValueError):
        b.parse(encoded)


def test_source_pack_raw_closure_after_proof_rejects_changed_input(extension, monkeypatch):
    fake = sys.modules['evals.verification.apps_stdin_v1.frozen']
    def load_pack(*args, **kwargs):
        extension['proposal_file'].write_bytes(extension['proposal_file'].read_bytes() + b'\n')
        return deepcopy(extension['pack'])
    monkeypatch.setattr(fake, 'load_pack', load_pack)
    with pytest.raises(ValueError, match='raw bytes drifted'):
        b.resolve(extension['path'])


def test_original_resolver_projection_cannot_disagree_with_auth_file(extension, monkeypatch):
    changed = deepcopy(extension['chain'])
    changed['authorization']['production_authorized'] = True
    monkeypatch.setattr(b.original, 'resolve', lambda _: changed)
    with pytest.raises(ValueError, match='authorization projection differs'):
        b.resolve(extension['path'])
