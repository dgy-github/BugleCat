"""Isolated source/prefix associations, not real candidate or fee evidence."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import frozen as f


def snapshot_fixture():
    chain = {'bindings': [{'file': f'auth-{i}', 'sha256': str(i) * 64} for i in range(4)],
             'allowed_sha256': [str(i) * 64 for i in range(4)],
             'current_sha256': f.RAW_PINS['authorization'], 'ledger_authorization_sha256': '0' * 64}
    starts = [{'ordinal': i + 1, 'group_id': f'old-{i}', 'started': {'group_id': f'old-{i}'}}
              for i in range(834)]
    baseline = {'main_started': starts, 'jev_reservations': [{'group_id': f'old-jev-{i}'} for i in range(229)],
                'remaining_main_attempts': 316, 'main_metadata': [('authorization_sha256', '0' * 64)],
                'main_ledger_file': 'original/main-attempts.sqlite3', 'authorization_chain': chain,
                'excluded_groups': [row['group_id'] for row in starts]}
    record = {'snapshot': baseline, 'finished': {row['group_id']: None for row in starts}}
    proposal = {'current_ledger_snapshot_sha256': f.source_authority.digest(baseline),
                'current_completion_snapshot_sha256': f.source_authority.digest(record['finished']),
                'main_ledger_file': baseline['main_ledger_file']}
    return proposal, record, chain


def refresh(proposal, record):
    proposal['current_ledger_snapshot_sha256'] = f.source_authority.digest(record['snapshot'])
    proposal['current_completion_snapshot_sha256'] = f.source_authority.digest(record['finished'])


def allocation_fixture():
    cases = []
    for difficulty, count in (('interview', 778), ('competition', 137), ('introductory', 73)):
        for n in range(count):
            index = len(cases)
            cases.append({'group_id': f'{difficulty}-{n}', 'task_id': index,
                          'original_members': [f'row-{index}'], 'original_difficulty': difficulty})
    baseline = {'excluded_groups': ['old-start', 'old-reservation']}
    legacy = {'old-untouched-holdout', 'old-development'}
    excluded = legacy | set(baseline['excluded_groups'])
    proposal = {'cases': f.sampling.choose(cases, excluded),
                'excluded_started_reserved_or_complete_old_allocation_groups': sorted(excluded)}
    return proposal, {'cases': cases}, baseline, legacy


def test_frozen_original_keeps_pending_completions_and_json_tuple_representation():
    proposal, record, chain = snapshot_fixture()
    import json
    restored = json.loads(f.packet.canonical_bytes(record))
    baseline, finished = f.snapshot_gate(proposal, restored, chain)
    assert len(baseline['main_started']) == 834 and len(finished) == 834
    assert all(value is None for value in finished.values())


@pytest.mark.parametrize('field', ['snapshot', 'finished'])
def test_proposal_digest_rejects_new_starts_or_reinterpreted_old_pending(field):
    proposal, record, chain = snapshot_fixture()
    if field == 'snapshot':
        record[field]['main_started'].append({'ordinal': 835, 'group_id': 'new', 'started': {}})
    else:
        record[field]['old-0'] = {'call_status': 'ok'}
    with pytest.raises(ValueError, match='snapshot digest'):
        f.snapshot_gate(proposal, record, chain)


@pytest.mark.parametrize('change', ['metadata', 'reset_count', 'duplicate_group', 'deleted_ordinal',
                                  'bool_ordinal', 'duplicate_metadata', 'ledger_root'])
def test_original_snapshot_structure_not_repaired_by_caller_reseal(change):
    proposal, record, chain = snapshot_fixture()
    snapshot = record['snapshot']
    if change == 'metadata':
        snapshot['main_metadata'][0] = ('authorization_sha256', 'new-origin')
    elif change == 'duplicate_metadata':
        snapshot['main_metadata'].append(snapshot['main_metadata'][0])
    elif change == 'reset_count':
        snapshot['remaining_main_attempts'] = 1150
    elif change == 'duplicate_group':
        snapshot['main_started'][1]['group_id'] = snapshot['main_started'][0]['group_id']
        record['finished'].pop('old-1')
    elif change == 'deleted_ordinal':
        snapshot['main_started'].pop()
    elif change == 'bool_ordinal':
        snapshot['main_started'][0]['ordinal'] = True
    elif change == 'ledger_root':
        snapshot['main_ledger_file'] = 'fresh/main-attempts.sqlite3'
    refresh(proposal, record)
    with pytest.raises(ValueError):
        f.snapshot_gate(proposal, record, chain)


def test_original_chain_cannot_be_replaced_by_new_approved_caps_in_source_proof():
    proposal, record, chain = snapshot_fixture()
    new = deepcopy(chain)
    new['bindings'].append({'file': 'new-auth', 'sha256': 'f' * 64})
    new['allowed_sha256'].append('f' * 64)
    record['snapshot']['authorization_chain'] = new
    refresh(proposal, record)
    with pytest.raises(ValueError, match='four-link'):
        f.snapshot_gate(proposal, record, new)


def test_complete_pre_candidate_allocation_reproduced_from_original_baseline():
    proposal, authority, baseline, legacy = allocation_fixture()
    before = deepcopy((proposal, authority, baseline, legacy))
    cases = f.allocation_gate(proposal, authority, baseline, legacy)
    assert len(cases) == len({case['group_id'] for case in cases}) == 808
    assert [case['allocation_ordinal'] for case in cases] == list(range(1, 809))
    assert (proposal, authority, baseline, legacy) == before


@pytest.mark.parametrize('change', ['reorder', 'swap_split', 'reuse', 'replacement', 'drop_unused_history'])
def test_paid_outcomes_cannot_reselect_repair_or_reorder_frozen_sources(change):
    proposal, authority, baseline, legacy = allocation_fixture()
    if change == 'reorder':
        proposal['cases']['calibration'].reverse()
    elif change == 'swap_split':
        proposal['cases']['calibration'][0], proposal['cases']['holdout'][0] = (
            proposal['cases']['holdout'][0], proposal['cases']['calibration'][0])
    elif change == 'reuse':
        proposal['cases']['calibration'][0] = deepcopy(proposal['cases']['development'][0])
    elif change == 'replacement':
        selected = {c['group_id'] for stage in proposal['cases'].values() for c in stage}
        proposal['cases']['calibration'][0] = next(c for c in authority['cases'] if c['group_id'] not in selected)
    else:
        proposal['excluded_started_reserved_or_complete_old_allocation_groups'].remove('old-untouched-holdout')
    with pytest.raises(ValueError):
        f.allocation_gate(proposal, authority, baseline, legacy)


@pytest.mark.parametrize('role', list(f.RAW_PINS))
def test_caller_seal_cannot_replace_known_original_raw_input(tmp_path, role):
    path = tmp_path / 'self-signed.json'
    path.write_text('{"artifact_hash":"fake"}', encoding='utf-8')
    with pytest.raises(ValueError, match='pinned original'):
        f.input_binding(path, role)


def test_original_loaded_pin_change_rejected(monkeypatch):
    monkeypatch.setitem(f.RAW_PINS, 'proposal', 'f' * 64)
    with pytest.raises(ValueError, match='constants differ'):
        f.loaded_check()


def test_loaded_old_reader_cannot_be_replaced_by_empty_shared_history(monkeypatch):
    monkeypatch.setattr(f.approved_budget, 'resolve', lambda _: {})
    with pytest.raises(ValueError, match='bytecode differs'):
        f.loaded_check()


@pytest.mark.parametrize('with_tasks', [1, None, 'true'])
def test_batch_tasks_switch_is_typed_before_any_io(with_tasks):
    with pytest.raises(ValueError, match='with_tasks'):
        f.load_pack('unused', 'unused', with_tasks=with_tasks)
