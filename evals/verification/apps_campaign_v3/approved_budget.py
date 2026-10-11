"""Read-only approved APPS extension and original shared-ledger bridge.

No authorization, reservation, marker, ledger or directory is created here.
Quoted options are supported contracts, not evidence anyone approved them.
"""
import json
import sqlite3
from pathlib import Path

from ..artifact_cohort import reservations
from ..budget_authorization_v2 import resolve as resolve750
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from . import packet

SCHEMA = 'buglecat.verification.apps-approved-budget-extension.v3'
ORIGINAL750_SHA = '2baa8582e6021cabf161e1c1837c0e2fb270f9cc2ffe318e93151619777b475c'
OPTIONS = {
    'apps-316-per-stage': (316, 400, 111, {
        '批准316组方案：主模型+400、Jev+111（推荐）', '批准316组方案：主模型+400、Jev+111'}),
    'apps-252-per-stage': (252, 272, 0, {'批准252组方案：主模型+272、Jev不追加'}),
}


def _same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def _chain(path):
    """Original 400 -> 500 -> 750 validated by its unchanged owners."""
    chain = resolve750(path)
    require(len(chain['bindings']) == len(chain['allowed_sha256']) == 3
            and chain['current_sha256'] == ORIGINAL750_SHA, 'pinned_original_750_lineage_required')
    for item, cap in zip(chain['bindings'], (400, 500, 750), strict=True):
        original = load(item['file'])
        require(item['sha256'] == file_hash(item['file'])
                and all(type(original[k]['max_attempts']) is int and original[k]['max_attempts'] == cap
                        for k in ('main_model', 'jev'))
                and type(original['automatic_retries']) is int and original['automatic_retries'] == 0,
                'strict_original_authorization_caps_and_retries_required')
    return chain


def source_snapshot(path, proposal_file):
    """Authenticate original raw snapshot against both sealed proposal digests."""
    from .formal_contract import _proposal
    proposal = _proposal(load(proposal_file))
    record = load(path)
    require(type(record) is dict and set(record) == {'snapshot', 'finished'}, 'original_source_snapshot_shape_required')
    baseline, finished = record['snapshot'], record['finished']
    require(digest(baseline) == proposal['current_ledger_snapshot_sha256']
            and digest(finished) == proposal['current_completion_snapshot_sha256'], 'original_source_snapshot_digest_changed')
    chain = _chain(proposal['authorization_file'])
    require(_same(baseline['authorization_chain'], chain)
            and _same(chain['bindings'], proposal['authorization_chain_bindings'])
            and Path(baseline['main_ledger_file']).resolve() == Path(proposal['main_ledger_file']).resolve(),
            'original_source_budget_lineage_changed')
    return proposal, baseline, finished


def resolve(path):
    """Existing750 default, or one exactly quoted, plan-bound approved extension."""
    path = Path(path).resolve(strict=True)
    value = load(path)
    if value.get('schema') != SCHEMA:
        return resolve750(path)
    require(value.get('approved_plan') in OPTIONS and value.get('approval_status') == 'approved',
            'explicit_approved_plan_required')
    count, main_extra, jev_extra, quotes = OPTIONS[value['approved_plan']]
    require(type(value.get('user_authorization')) is str and value['user_authorization'] in quotes,
            'exact_quoted_user_approval_required')
    require(all(type(value.get(k)) is int and value[k] == expected for k, expected in (
        ('additional_main_attempt_cap', main_extra), ('additional_jev_attempt_cap', jev_extra),
        ('automatic_retries', 0), ('per_group_max_attempts', 1)))
        and value.get('old_attempts_and_failures_count') is True
        and value.get('old_pending_attempts_count') is True
        and value.get('production_authorized') is False,
        'extension_cannot_reset_failures_pending_or_allow_retries')
    previous_file = Path(value['previous_authorization_file']).resolve(strict=True)
    require(previous_file != path and file_hash(previous_file) == value['previous_authorization_sha256'] == ORIGINAL750_SHA,
            'pinned_previous_750_authorization_changed')
    previous = _chain(previous_file)
    for kind, extra in (('main_model', main_extra), ('jev', jev_extra)):
        require(type(value[kind]['max_attempts']) is int and value[kind]['max_attempts'] == 750 + extra
                and Path(value[kind]['ledger_dir']).resolve() == Path(previous['authorization'][kind]['ledger_dir']).resolve(),
                'approved_cap_or_original_ledger_root_changed')
    proposal_file = Path(value['source_proposal_file']).resolve(strict=True)
    snapshot_file = Path(value['source_snapshot_file']).resolve(strict=True)
    require(file_hash(proposal_file) == value['source_proposal_sha256']
            and file_hash(snapshot_file) == value['source_snapshot_sha256'], 'approved_original_proposal_or_snapshot_bytes_changed')
    proposal, _, _ = source_snapshot(snapshot_file, proposal_file)
    require(proposal['artifact_hash'] == value['source_proposal_hash']
            and proposal['count_per_stage'] == count
            and Path(proposal['authorization_file']).resolve() == previous_file,
            'approved_plan_proposal_membership_or_original_auth_changed')
    current = file_hash(path)
    return {'authorization': value, 'current_sha256': current,
            'allowed_sha256': [*previous['allowed_sha256'], current],
            'ledger_authorization_sha256': previous['ledger_authorization_sha256'],
            'bindings': [*previous['bindings'], {'file': str(path), 'sha256': current}]}


def _json(encoded):
    def invalid(_):
        raise ValueError('nonfinite_original_ledger_json')
    value = json.loads(encoded, parse_constant=invalid)
    packet.canonical_bytes(value)
    return value


def snapshot(authorization):
    """Same SQLite schema and original reservation roots, read-only transaction."""
    chain = resolve(authorization)
    auth = chain['authorization']
    caps = {b['sha256']: load(b['file'])['main_model']['max_attempts'] for b in chain['bindings']}
    root = Path(auth['main_model']['ledger_dir']).resolve(strict=True)
    require(not (root / 'reservation.lock').exists(), 'main_reservation_transaction_in_progress')
    ledger = (root / 'main-attempts.sqlite3').resolve(strict=True)
    with sqlite3.connect(ledger.as_uri() + '?mode=ro', uri=True) as db:
        db.execute('BEGIN')
        metadata = list(db.execute('SELECT key,value FROM metadata ORDER BY key'))
        raw = list(db.execute('SELECT ordinal,group_id,split,started_json,finished_json FROM attempts ORDER BY ordinal'))
        # Reject deletion of latest historical ordinals even if remaining rows
        # still look contiguous; SQLite AUTOINCREMENT retains the spent prefix.
        sequence = list(db.execute("SELECT seq FROM sqlite_sequence WHERE name='attempts'"))
    require(len(dict(metadata)) == len(metadata)
            and dict(metadata).get('authorization_sha256') == chain['ledger_authorization_sha256'],
            'original_shared_SQLite_metadata_lineage_changed')
    starts, finished, groups = [], {}, set()
    for ordinal, group, split, encoded, complete in raw:
        start = _json(encoded)
        require(type(group) is str and bool(group) and group not in groups
                and type(ordinal) is int and ordinal == len(starts) + 1
                and type(start) is dict and start.get('group_id') == group
                and start.get('authorization_sha256') in caps and ordinal <= caps[start['authorization_sha256']]
                and (start.get('split') is None or start['split'] == split), 'invalid_original_or_new_authorization_SQLite_start')
        terminal = _json(complete) if complete is not None else None
        require(terminal is None or (type(terminal) is dict
                and ('group_id' not in terminal or terminal['group_id'] == group)
                and ('global_main_attempt' not in terminal or type(terminal['global_main_attempt']) is int
                     and terminal['global_main_attempt'] == ordinal)), 'original_finished_identity_changed')
        groups.add(group)
        starts.append({'ordinal': ordinal, 'group_id': group, 'started': start})
        finished[group] = terminal
    require((not raw and (not sequence or sequence == [(0,)])) or sequence == [(len(raw),)],
            'historical_main_ordinal_deleted_or_reset')
    main_reserved, _ = reservations(root, 'main_model', chain)
    jev_reserved, _ = reservations(auth['jev']['ledger_dir'], 'jev', chain)
    for rows in (main_reserved, jev_reserved):
        require(sorted(r['reservation_index'] for r in rows) == list(range(1, len(rows) + 1)),
                'original_reservation_index_prefix_changed')
        require(all(type(r['automatic_retries']) is int and r['automatic_retries'] == 0 for r in rows),
                'strict_zero_retry_reservations_required')
    main_groups = groups | {r['group_id'] for r in main_reserved}
    require(len(main_groups) <= auth['main_model']['max_attempts'], 'shared_main_cap_exceeded')
    body = {'main_ledger_file': str(ledger), 'main_metadata': metadata, 'main_started': starts,
            'main_reservations': main_reserved, 'jev_reservations': jev_reserved,
            'authorization_chain': chain,
            'excluded_groups': sorted(main_groups | {r['group_id'] for r in jev_reserved}),
            'remaining_main_attempts': auth['main_model']['max_attempts'] - len(main_groups)}
    require(not (root / 'reservation.lock').exists(), 'shared_budget_changed_during_snapshot')
    return body, finished


def bridge(source, source_finished, current, finished):
    """Before collection, only explicitly approved chain/caps may change."""
    stable = ('main_ledger_file', 'main_metadata', 'main_started', 'main_reservations', 'jev_reservations', 'excluded_groups')
    require(all(_same(source[k], current[k]) for k in stable) and _same(source_finished, finished),
            'source_baseline_history_changed_before_approved_extension')
    old, new = source['authorization_chain'], current['authorization_chain']
    if _same(old, new):
        require(_same(source, current), 'unchanged_authorization_budget_snapshot_drift')
        return {'kind': 'unchanged_original_authorization', 'source_snapshot_hash': digest(source),
                'budget_snapshot_hash': digest(current), 'source_completion_hash': digest(source_finished)}
    require(len(old['bindings']) == 3 and len(new['bindings']) == 4
            and _same(new['bindings'][:3], old['bindings'])
            and new['allowed_sha256'][:3] == old['allowed_sha256']
            and new['ledger_authorization_sha256'] == old['ledger_authorization_sha256']
            and new['authorization']['schema'] == SCHEMA, 'only_one_approved_lineage_extension_allowed')
    actual = resolve(new['bindings'][-1]['file'])
    require(_same(actual, new), 'actual_approved_budget_chain_changed')
    extra = new['authorization']['additional_main_attempt_cap']
    require(type(source['remaining_main_attempts']) is int and type(current['remaining_main_attempts']) is int
            and current['remaining_main_attempts'] == source['remaining_main_attempts'] + extra,
            'approved_remaining_budget_drift')
    return {'kind': 'approved_caps_only_extension', 'source_snapshot_hash': digest(source),
            'budget_snapshot_hash': digest(current), 'source_completion_hash': digest(source_finished),
            'source_authorization_sha256': old['current_sha256'], 'approved_authorization_sha256': new['current_sha256']}
