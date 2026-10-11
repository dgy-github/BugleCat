"""Source-only gates: no replacement representative or retroactive authority."""
import copy
import json

import pytest

from evals.verification.apps_campaign_v3 import source_authority as owner
from evals.verification.apps_v3 import oracle


def fixtures():
    rows = [(1, json.dumps({'id': 1, 'difficulty': 'interview'}).encode()),
            (2, json.dumps({'id': 2, 'difficulty': 'introductory'}).encode())]
    group = {'group_id': 'group-1', 'members': ['apps-train-1', 'apps-train-2'],
             'representative': 'apps-train-1', 'excluded_by_history': False,
             'historical_source_matches': [], 'source_admitted': False, 'evaluation_split': None}
    task = {'task_id': 1, 'original_line_number': 1, 'original_line_sha256': 'a' * 64,
            'source_kind': 'apps_call_based_free_function', 'reference_sha256': 'b' * 64,
            'input_output_sha256': 'c' * 64, 'input_output': {'inputs': [[1]], 'outputs': [1]}}
    return rows, group, task


def test_no_later_passing_member_replaces_failed_earliest():
    rows, group, task = fixtures()
    assert owner.fixed_pool([group], {'apps-train-2': (task, {})}, rows) == []


def test_historical_match_excludes_entire_component():
    rows, group, task = fixtures()
    group.update(excluded_by_history=True, historical_source_matches=['mbpp_full:5'])
    assert owner.fixed_pool([group], {'apps-train-1': (task, {})}, rows) == []


def test_original_task_and_receipt_binding_retained_without_candidate_label():
    rows, group, task = fixtures()
    receipt = {'file': 'original.json', 'sha256': 'd' * 64}
    result = owner.fixed_pool([group], {'apps-train-1': (task, receipt)}, rows)
    assert len(result) == 1 and result[0]['group_id'] == group['group_id']
    assert result[0]['task_sha256'] == oracle.digest(task)
    assert result[0]['reference_receipt_binding'] == receipt
    assert result[0]['candidate_label'] is None and result[0]['evaluation_split'] is None


@pytest.mark.parametrize('change', ['representative', 'order', 'exposure', 'split', 'admission'])
def test_group_changes_rejected(change):
    rows, group, _ = fixtures()
    if change == 'representative':
        group['representative'] = 'apps-train-2'
    elif change == 'order':
        group['members'].reverse()
    elif change == 'exposure':
        group['historical_source_matches'] = ['old']
    elif change == 'split':
        group['evaluation_split'] = 'calibration'
    else:
        group['source_admitted'] = True
    with pytest.raises(ValueError, match='partition_or_representative'):
        owner.fixed_pool([group], {}, rows)


def test_duplicate_or_incomplete_partition_rejected():
    rows, group, _ = fixtures()
    with pytest.raises(ValueError, match='partition_or_representative'):
        owner.fixed_pool([group, copy.deepcopy(group)], {}, rows)
    group['members'] = ['apps-train-1']
    with pytest.raises(ValueError, match='complete_original_source_partition'):
        owner.fixed_pool([group], {}, rows)


def sealed(value):
    return {**value, 'artifact_hash': oracle.digest(value)}


def test_changed_owner_refuses_source_authority(tmp_path):
    path = tmp_path / 'owner.py'
    path.write_text('original', encoding='utf-8')
    binding = owner.binding(path)
    value = {'schema': owner.SCHEMA, 'source_only': True, 'collection_authorized': False,
             'formal_registration': False, 'production_authorized': False,
             'candidate_labels_assigned': False, 'bindings': [], 'owner_bindings': [binding]}
    assert owner.validate(sealed(value))['source_only'] is True
    path.write_text('replacement', encoding='utf-8')
    with pytest.raises(ValueError, match='binding_changed'):
        owner.validate(sealed(value))


@pytest.mark.parametrize('field', ['collection_authorized', 'formal_registration',
                                 'production_authorized', 'candidate_labels_assigned'])
def test_resealing_cannot_promote_source_only_record(field):
    value = {'schema': owner.SCHEMA, 'source_only': True, 'collection_authorized': False,
             'formal_registration': False, 'production_authorized': False,
             'candidate_labels_assigned': False, 'bindings': [], 'owner_bindings': []}
    value[field] = True
    with pytest.raises(ValueError, match='source_only_authority_required'):
        owner.validate(sealed(value))
