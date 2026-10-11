from collections import Counter
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import sampling as s


def sources():
    result = []
    for d, count in (('interview', 778), ('competition', 137), ('introductory', 73)):
        for n in range(count):
            identity = len(result)
            result.append({'group_id': f'{d}-{n}', 'task_id': identity, 'original_difficulty': d,
                           'original_members': [f'row-{identity}'], 'task_sha256': 'a'*64})
    return result


def test_actual_inventory_counts_support_fixed_complete_strata():
    cases = sources()
    before = deepcopy(cases)
    chosen = s.choose(cases, [])
    assert [len(chosen[k]) for k in s.STAGES] == [8, 400, 400]
    for split in s.STAGES:
        assert Counter(c['original_difficulty'] for c in chosen[split]) == s.STRATA[split]
    all_cases = [c for split in s.STAGES for c in chosen[split]]
    assert len({c['group_id'] for c in all_cases}) == 808
    assert len({c['id'] for c in all_cases}) == 808
    assert [c['allocation_ordinal'] for c in all_cases] == list(range(1, 809))
    assert chosen == s.choose(list(reversed(cases)), []) and before == cases


def test_complete_old_groups_excluded_even_if_not_started():
    cases = sources()
    first = s.choose(cases, [])
    exclude = [c['group_id'] for c in first['holdout'][:30]]
    chosen = s.choose(cases, exclude)
    assert not set(exclude).intersection(c['group_id'] for stage in chosen.values() for c in stage)


@pytest.mark.parametrize('difficulty,keep', [('interview', 683), ('competition', 123)])
def test_full_pool_shortage_cannot_shrink_or_fill_with_intro(difficulty, keep):
    cases = sources()
    # Keep exactly one fewer than the full 8+400+400 allocation requires.
    selected = [c for c in cases if c['original_difficulty'] != difficulty]
    selected += [c for c in cases if c['original_difficulty'] == difficulty][:keep]
    with pytest.raises(ValueError, match='insufficient'):
        s.choose(selected, [])


@pytest.mark.parametrize('field', ['group_id', 'task_id', 'original_members'])
def test_duplicate_representative_and_cross_group_members_rejected(field):
    cases = sources()
    cases[1][field] = cases[0][field]
    with pytest.raises(ValueError):
        s.choose(cases, [])


def test_rank_is_exact_predeclared_hash():
    import hashlib
    assert s.rank('group') == hashlib.sha256(b'stdin-natural-400-v1:group').hexdigest()


@pytest.mark.parametrize('value', [True, -1, '123'])
def test_original_task_id_is_typed(value):
    cases = sources()
    cases[0]['task_id'] = value
    with pytest.raises(ValueError):
        s.choose(cases, [])


def test_allocation_strata_cannot_drift_in_memory(monkeypatch):
    changed = deepcopy(s.STRATA)
    changed['calibration']['interview'] = 339
    monkeypatch.setattr(s, 'STRATA', changed)
    with pytest.raises(ValueError, match='constants differ'):
        s.choose(sources(), [])


@pytest.mark.parametrize('phase', ['authority', 'profile', 'authorization'])
def test_input_bytes_cannot_change_after_validation_before_binding_capture(tmp_path, monkeypatch, phase):
    from evals.verification.apps_stdin_v1 import source_authority
    paths = {kind: tmp_path / (kind + '.json') for kind in ('authority', 'authorization', 'profile')}
    for path in paths.values():
        path.write_text('{}', encoding='utf-8')

    def change(kind):
        if phase == kind:
            paths[kind].write_text('{"changed":true}', encoding='utf-8')

    def authority(_):
        change('authority')
        return {}

    def profile(_):
        change('profile')
        return {}, []

    def authorization(_):
        change('authorization')
        return {}, {}

    monkeypatch.setattr(source_authority, 'validate', authority)
    monkeypatch.setattr(s, 'validate_profile', profile)
    monkeypatch.setattr(s.approved_budget, 'snapshot', authorization)
    with pytest.raises(ValueError, match='original bound bytes drifted'):
        s.build(paths['authority'], paths['authorization'], paths['profile'])
