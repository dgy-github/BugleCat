"""Isolated original SQLite and exact approval/source-baseline bridge tests."""
import copy
import json
import sqlite3

import pytest

from evals.verification.apps_campaign_v3 import approved_budget as owner
from evals.verification.apps_campaign_v3 import formal_collection as formal
from evals.verification.calibration import digest
from evals.verification.collection_manifest import file_hash
from tests.test_verification_apps_campaign_v3_formal_collection import fixture  # noqa: F401
from tests.test_verification_budget_authorization_v2 import extended


def write(path, value):
    path.write_text(json.dumps(value), encoding='utf-8')


@pytest.fixture
def approved(fixture, monkeypatch):  # noqa: F811
    tmp = fixture['tmp']
    original, previous, auth750, _ = extended(tmp)
    monkeypatch.setattr(owner, 'ORIGINAL750_SHA', file_hash(auth750))
    root = tmp / 'main'
    root.mkdir()
    with sqlite3.connect(root / 'main-attempts.sqlite3') as db:
        db.executescript('CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);'
                         'CREATE TABLE attempts(ordinal INTEGER PRIMARY KEY AUTOINCREMENT,group_id TEXT UNIQUE,'
                         'split TEXT,started_json TEXT,finished_json TEXT);')
        db.execute('INSERT INTO metadata VALUES(?,?)', ('authorization_sha256', file_hash(original)))
        for group, auth, terminal in [('old-0', original, {'call_status': 'transport_error'}), ('old-1', previous, None)]:
            db.execute('INSERT INTO attempts(group_id,split,started_json,finished_json) VALUES(?,?,?,?)',
                       (group, 'development', json.dumps({'group_id': group, 'authorization_sha256': file_hash(auth),
                                                         'split': 'development'}), json.dumps(terminal) if terminal else None))
    baseline, finished = owner.snapshot(auth750)
    proposal = json.loads(fixture['paths']['proposal'].read_text())
    proposal.update(current_ledger_snapshot_sha256=digest(baseline), current_completion_snapshot_sha256=digest(finished),
                    authorization_file=str(auth750), main_ledger_file=baseline['main_ledger_file'],
                    authorization_chain_bindings=baseline['authorization_chain']['bindings'],
                    excluded_started_reserved_or_historical_groups=sorted(set(baseline['excluded_groups'])
                        | set(proposal['excluded_whole_dev_groups'])))
    proposal['artifact_hash'] = digest({k: v for k, v in proposal.items() if k != 'artifact_hash'})
    write(fixture['paths']['proposal'], proposal)
    source = tmp / 'source-snapshot.json'
    write(source, {'snapshot': baseline, 'finished': finished})
    value = {'schema': owner.SCHEMA, 'approval_status': 'approved', 'approved_plan': 'apps-252-per-stage',
             'user_authorization': '批准252组方案：主模型+272、Jev不追加',
             'previous_authorization_file': str(auth750), 'previous_authorization_sha256': file_hash(auth750),
             'additional_main_attempt_cap': 272, 'additional_jev_attempt_cap': 0,
             'main_model': {'max_attempts': 1022, 'ledger_dir': str(root)},
             'jev': {'max_attempts': 750, 'ledger_dir': str(tmp / 'jev')},
             'automatic_retries': 0, 'per_group_max_attempts': 1, 'old_attempts_and_failures_count': True,
             'old_pending_attempts_count': True, 'production_authorized': False,
             'source_proposal_file': str(fixture['paths']['proposal']),
             'source_proposal_sha256': file_hash(fixture['paths']['proposal']), 'source_proposal_hash': proposal['artifact_hash'],
             'source_snapshot_file': str(source), 'source_snapshot_sha256': file_hash(source)}
    auth = tmp / 'approved.json'
    write(auth, value)
    return dict(fixture=fixture, auth=auth, auth750=auth750, original=original, previous=previous, value=value,
                baseline=baseline, finished=finished, source=source, root=root)


def test_exact_approved_chain_preserves_original_failures_pending_and_readonly(approved):
    before = {p: p.read_bytes() for p in approved['root'].glob('*')}
    new, finished = owner.snapshot(approved['auth'])
    assert finished == approved['finished'] and finished['old-1'] is None
    assert new['remaining_main_attempts'] == 1020 and len(new['main_started']) == 2
    assert new['authorization_chain']['ledger_authorization_sha256'] == file_hash(approved['original'])
    assert len(new['authorization_chain']['allowed_sha256']) == 4
    bridge = owner.bridge(approved['baseline'], approved['finished'], new, finished)
    assert bridge['kind'] == 'approved_caps_only_extension'
    assert {p: p.read_bytes() for p in approved['root'].glob('*')} == before


@pytest.mark.parametrize('quote', ['批准316组方案：主模型+400、Jev+111（推荐）', '批准316组方案：主模型+400、Jev+111'])
def test_exact_316_options_only(approved, quote):
    # Independent option typing test: source-plan mismatch still fails until
    # a genuine 316 proposal is supplied, not permission to relabel 252.
    value = approved['value']
    value.update(approved_plan='apps-316-per-stage', user_authorization=quote,
                 additional_main_attempt_cap=400, additional_jev_attempt_cap=111)
    value['main_model']['max_attempts'], value['jev']['max_attempts'] = 1150, 861
    write(approved['auth'], value)
    with pytest.raises(ValueError, match='plan_proposal'):
        owner.resolve(approved['auth'])
    f = approved['fixture']
    proposal = json.loads(f['paths']['proposal'].read_text())
    _, stages = formal.sampling_proposal._selected(f['authority'], f['inventory'],
        set(proposal['excluded_started_reserved_or_historical_groups']), 316, 175)
    proposal.update(cases=stages, count_per_stage=316, introductory_per_stage=141)
    proposal['artifact_hash'] = digest({k: v for k, v in proposal.items() if k != 'artifact_hash'})
    write(f['paths']['proposal'], proposal)
    value.update(source_proposal_sha256=file_hash(f['paths']['proposal']), source_proposal_hash=proposal['artifact_hash'])
    write(approved['auth'], value)
    chain = owner.resolve(approved['auth'])
    assert chain['authorization']['main_model']['max_attempts'] == 1150
    assert chain['authorization']['jev']['max_attempts'] == 861


@pytest.mark.parametrize('mutation', [
    lambda v, p: v.update(approval_status='pending'),
    lambda v, p: v.update(user_authorization='批准'),
    lambda v, p: v.update(user_authorization='批准316组方案：主模型+400、Jev+111'),
    lambda v, p: v.update(approved_plan='anything'),
    lambda v, p: v.update(additional_jev_attempt_cap=True),
    lambda v, p: v.update(automatic_retries=False),
    lambda v, p: v.update(per_group_max_attempts=True),
    lambda v, p: v.update(old_pending_attempts_count=False),
    lambda v, p: v.update(old_attempts_and_failures_count=False),
    lambda v, p: v.update(production_authorized=True),
    lambda v, p: v['main_model'].update(max_attempts=1022.0),
    lambda v, p: v['jev'].update(max_attempts=751),
    lambda v, p: v['main_model'].update(ledger_dir=str(p / 'reset')),
    lambda v, p: v.update(previous_authorization_sha256='a' * 64),
    lambda v, p: v.update(source_proposal_hash='b' * 64),
    lambda v, p: v.update(source_snapshot_sha256='c' * 64),
])
def test_unapproved_quote_type_caps_reset_or_fake_lineage_rejected(approved, mutation):
    value = copy.deepcopy(approved['value'])
    mutation(value, approved['root'])
    write(approved['auth'], value)
    with pytest.raises(ValueError):
        owner.resolve(approved['auth'])


@pytest.mark.parametrize('mutation', ['start', 'finish', 'missing_pending', 'metadata', 'remaining', 'new_start', 'reservation'])
def test_bridge_requires_same_whole_history_not_merely_budget_digest(approved, mutation):
    current, finished = owner.snapshot(approved['auth'])
    if mutation == 'start':
        current['main_started'][0]['started']['extra'] = 'changed'
    elif mutation == 'finish':
        finished['old-0'] = {'call_status': 'ok'}
    elif mutation == 'missing_pending':
        del finished['old-1']
    elif mutation == 'metadata':
        current['main_metadata'].append(['fake', 'binding'])
    elif mutation == 'remaining':
        current['remaining_main_attempts'] += 1
    elif mutation == 'new_start':
        current['main_started'].append({'ordinal': 3})
    else:
        current['jev_reservations'].append({'group_id': 'new'})
    with pytest.raises(ValueError):
        owner.bridge(approved['baseline'], approved['finished'], current, finished)


def test_new_auth_ordinal_allowed_old_cap_ordinal_rejected_same_global_sqlite(approved):
    # Original cap400 at global ordinal751 cannot borrow the new approved cap.
    with sqlite3.connect(approved['root'] / 'main-attempts.sqlite3') as db:
        for ordinal in range(3, 752):
            start = {'group_id': f'group-{ordinal}', 'authorization_sha256': file_hash(approved['auth']), 'split': 'calibration'}
            db.execute('INSERT INTO attempts(group_id,split,started_json) VALUES(?,?,?)',
                       (start['group_id'], 'calibration', json.dumps(start)))
    current, finished = owner.snapshot(approved['auth'])
    assert current['main_started'][-1]['ordinal'] == 751 and finished['group-751'] is None
    with pytest.raises(ValueError):
        owner.snapshot(approved['auth750'])  # Old resolver must not accept new auth starts.
    with sqlite3.connect(approved['root'] / 'main-attempts.sqlite3') as db:
        start['authorization_sha256'] = file_hash(approved['original'])
        db.execute('UPDATE attempts SET started_json=? WHERE ordinal=751', (json.dumps(start),))
    with pytest.raises(ValueError, match='SQLite_start'):
        owner.snapshot(approved['auth'])


def test_deleted_latest_spent_ordinal_and_live_lock_rejected(approved):
    with sqlite3.connect(approved['root'] / 'main-attempts.sqlite3') as db:
        db.execute('DELETE FROM attempts WHERE ordinal=2')
    with pytest.raises(ValueError, match='ordinal_deleted'):
        owner.snapshot(approved['auth'])
    (approved['root'] / 'reservation.lock').write_text('interrupted')
    with pytest.raises(ValueError, match='transaction'):
        owner.snapshot(approved['auth'])


def test_source_member_and_original_raw_snapshot_changed_rejected(approved):
    proposal_file = approved['fixture']['paths']['proposal']
    original = proposal_file.read_bytes()
    proposal_file.write_bytes(original + b'\n')
    with pytest.raises(ValueError, match='bytes_changed'):
        owner.resolve(approved['auth'])
    proposal_file.write_bytes(original)
    record = json.loads(approved['source'].read_text())
    record['finished']['old-0'] = {'call_status': 'ok'}
    write(approved['source'], record)
    approved['value']['source_snapshot_sha256'] = file_hash(approved['source'])
    write(approved['auth'], approved['value'])
    with pytest.raises(ValueError, match='snapshot_digest'):
        owner.resolve(approved['auth'])


def test_formal_source_baseline_remains750_budget_baseline_new_then_append_only(approved, monkeypatch):
    f = approved['fixture']
    monkeypatch.setattr(formal, 'budget_snapshot', owner.snapshot)
    value = formal.preregister(f['paths']['proposal'], approved['auth'], f['paths']['profile'], f['tmp'] / 'uncreated-new',
                              protocol_file=f['paths']['formal_protocol'])
    assert value['source_proposal_baseline']['authorization_chain']['authorization']['main_model']['max_attempts'] == 750
    assert value['ledger_snapshot']['authorization_chain']['authorization']['main_model']['max_attempts'] == 1022
    assert value['approved_budget_bridge']['kind'] == 'approved_caps_only_extension'
    write(f['paths']['cohort'], value)
    monkeypatch.setattr(formal.formal_contract, 'review_proposal', lambda *a: pytest.fail('cannot old-resolve after paid start'))
    first = value['cases'][0]
    record = {k: first[k] for k in ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')}
    record.update(authorization_sha256=file_hash(approved['auth']), artifact_cohort_hash=value['artifact_hash'])
    with sqlite3.connect(approved['root'] / 'main-attempts.sqlite3') as db:
        db.execute('INSERT INTO attempts(group_id,split,started_json) VALUES(?,?,?)',
                   (record['group_id'], record['split'], json.dumps(record)))
    assert formal._same(formal.validate_cohort(f['paths']['cohort']), value)
    manifest = formal.manifest_value(f['paths']['cohort'], count=1)
    assert manifest['cases'][0] == value['cases'][1]
