"""Rebuild original-source grouping and receipt joins before a new campaign.

No candidate execution, transport, reservation, split or threshold authority.
The new transaction supplies its own complete local owner snapshot. It does
not retroactively upgrade the incomplete original grouping execution graph.
"""
import gzip
import json
import sys
from collections import Counter
from pathlib import Path

from ..apps_solution_v3 import oracle as solution_oracle
from ..apps_solution_v3 import source as solution_source
from ..apps_v3 import dedup, oracle, source

ROOT = Path(__file__).resolve().parents[3]
SCHEMA = "buglecat.verification.apps-source-authority.v3"
HISTORY_SHA256 = "b99856e8a59d7305894e310c9cd1a193cdc50485dcb60da70e3f04dccb727205"
POOL_KINDS = {"mbpp_sanitized": 427, "mbpp_full": 974, "humaneval_raw": 164}


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def load(path):
    return json.loads(Path(path).read_bytes(), parse_constant=source.reject_nonfinite)


def binding(path):
    path = Path(path).resolve(strict=True)
    return {"file": str(path), "sha256": source.file_sha(path)}


def check(bindings):
    require(all(source.file_sha(item['file']) == item['sha256'] for item in bindings),
            "source_authority_binding_changed")


def owners():
    """Conservatively bind all flat verification helpers used by fingerprints.

    Stdlib provenance is the actual interpreter binding; no third-party Oracle
    dependency or remote evaluator is imported. Packet/collector have a separate
    later campaign freeze and are deliberately not granted authority here.
    """
    folder = Path(__file__).resolve().parents[1]
    paths = set(folder.glob('*.py'))
    paths.update((folder.parent / '__init__.py', folder / '__init__.py', Path(__file__).resolve()))
    for name in ('apps_v3', 'apps_solution_v3'):
        paths.update((folder / name).glob('*.py'))
    marker = Path(__file__).with_name('__init__.py')
    if marker.is_file():
        paths.add(marker)
    paths.add(Path(sys.executable).resolve())
    return [binding(path) for path in sorted(paths, key=str) if path.is_file()]


def history_records(inventory, bindings):
    records, kinds = [], set()
    for pool in inventory['source_pools']:
        kind = pool['kind']
        if kind not in POOL_KINDS:
            continue
        require(kind not in kinds, "duplicate_historical_original_pool")
        kinds.add(kind)
        item = binding(pool['file'])
        require(item['sha256'] == pool['sha256'], "historical_source_changed")
        bindings.append(item)
        raw = Path(item['file']).read_bytes()
        if kind == 'humaneval_raw':
            rows = [json.loads(line) for line in gzip.decompress(raw).splitlines()]
        elif kind == 'mbpp_full':
            rows = [json.loads(line) for line in raw.splitlines()]
        else:
            rows = json.loads(raw)
        require(len(rows) == pool['rows'] == POOL_KINDS[kind], "historical_original_count_changed")
        for number, row in enumerate(rows, 1):
            code = row['prompt'] + row['canonical_solution'] if kind == 'humaneval_raw' else row['code']
            records.append({'source_id': f"{kind}:{row['task_id']}", 'original_line_number': number,
                            'question': row[pool['question_field']], 'references': [code], 'url': None})
    require(kinds == set(POOL_KINDS), "complete_historical_original_pools_required")
    return records


def reference_join(folder, source_owner, execution_owner, original_rows, bindings):
    folder = Path(folder).resolve(strict=True)
    for name in ('start.json', 'structural-selection.json', 'public-aggregate.json',
                 'reference-pass-index.json', 'join-recheck.json', '_screen.py', '_verify.py'):
        bindings.append(binding(folder / name))
    start = load(folder / 'start.json')
    graph = {**execution_owner.source_graph(), 'source_owner': Path(source_owner.__file__),
             'screen_operator': folder / '_screen.py'}
    if source_owner is solution_source:
        graph['original_source_owner'] = Path(source.__file__)
    require({role: source.file_sha(path) for role, path in graph.items()} == start['source_graph_sha256'],
            "original_screen_owner_changed")
    require(start['full_train_sha256'] == source.TRAIN_SHA256 and start['reference_policy'] == source.REFERENCE_POLICY
            and start['timeout_seconds'] == 5 and start['paid_calls'] == 0, "screen_contract_changed")
    tasks = []
    for number, raw in original_rows:
        try:
            tasks.append(source_owner.parse_row(raw, number))
        except (ValueError, SyntaxError, TypeError, KeyError, RecursionError, MemoryError):
            pass
    selection = load(folder / 'structural-selection.json')
    require([task['task_id'] for task in tasks] == selection['original_task_ids_in_source_order'],
            "complete_reference_selection_changed")
    counts, hashes, passed, originals = Counter(), {}, [], {}
    for task in tasks:
        path = folder / 'receipts' / f"{task['task_id']}.json"
        item = binding(path)
        bindings.append(item)
        record = load(path)
        receipt = record['execution_receipt']
        require(record['task_id'] == task['task_id'] and record['group_id'] == task['group_id']
                and record['original_task_sha256'] == oracle.digest(task)
                and record['original_line_sha256'] == task['original_line_sha256']
                and record['reference_index'] == 0 and record['reference_policy'] == source.REFERENCE_POLICY
                and record['original_split'] == 'train' and record['source_admitted'] is False
                and record['candidate_label'] is None and record['paid_calls'] == 0,
                "original_reference_record_changed")
        require(receipt['code_sha256'] == task['reference_sha256']
                and receipt['input_output_sha256'] == task['input_output_sha256']
                and receipt['pair_count'] == len(task['input_output']['inputs'])
                and receipt['source_graph_sha256'] == {role: start['source_graph_sha256'][role]
                                                       for role in execution_owner.source_graph()}
                and receipt['timeout_seconds'] == 5 and receipt['credentials_removed'] is True
                and receipt['independent_of_jev'] is True and receipt['paid_calls'] == 0,
                "original_reference_execution_changed")
        result = receipt['result']
        require(oracle.checked_result(result, receipt['pair_count']) == result, "reference_result_invalid")
        counts[result['status']] += 1
        if result['status'] == 'passed':
            passed.append({'task_id': task['task_id'], 'group_id': task['group_id'],
                           'original_line_sha256': task['original_line_sha256'],
                           'original_task_sha256': record['original_task_sha256'],
                           'reference_sha256': task['reference_sha256'],
                           'input_output_sha256': task['input_output_sha256']})
            originals[f"apps-train-{task['task_id']}"] = (task, item)
        hashes[path.name] = item['sha256']
    require(set(hashes) == {path.name for path in (folder / 'receipts').glob('*.json')},
            "reference_receipt_set_changed")
    index, checked, aggregate = [load(folder / name) for name in
                                 ('reference-pass-index.json', 'join-recheck.json', 'public-aggregate.json')]
    require(index['cases'] == passed and index['source_admitted'] is False
            and index['independent_group_proven'] is False and checked['actual_receipts_joined'] == len(tasks)
            and checked['receipt_hashes_digest'] == oracle.digest(hashes)
            and checked['screen_aggregate_sha256'] == source.file_sha(folder / 'public-aggregate.json')
            and checked['source_graph_sha256'] == start['source_graph_sha256']
            and checked['actual_reference_results'] == dict(counts)
            and aggregate['reference_results'] == dict(counts), "reference_full_join_changed")
    return originals, dict(counts)


def fixed_pool(groups, passed, original_rows):
    """Earliest representative only. Never substitute a later passing member."""
    rows = {f"apps-train-{json.loads(raw)['id']}": (number, json.loads(raw)) for number, raw in original_rows}
    covered, ids, cases = set(), set(), []
    for group in groups:
        members = group['members']
        require(members == sorted(members, key=lambda identity: rows[identity][0])
                and group['representative'] == members[0] and not covered.intersection(members)
                and group['group_id'] not in ids and group['source_admitted'] is False
                and group['evaluation_split'] is None
                and group['excluded_by_history'] is bool(group['historical_source_matches']),
                "source_group_partition_or_representative_changed")
        covered.update(members)
        ids.add(group['group_id'])
        identity = group['representative']
        if group['excluded_by_history'] or identity not in passed:
            continue
        task, receipt_binding = passed[identity]
        row = rows[identity][1]
        cases.append({'group_id': group['group_id'], 'original_source_id': identity,
                      'task_id': task['task_id'], 'original_line_number': task['original_line_number'],
                      'task_sha256': oracle.digest(task), 'original_line_sha256': task['original_line_sha256'],
                      'source_kind': task['source_kind'], 'original_difficulty': row['difficulty'],
                      'reference_index': 0, 'reference_sha256': task['reference_sha256'],
                      'input_output_sha256': task['input_output_sha256'],
                      'pair_count': len(task['input_output']['inputs']),
                      'reference_receipt_binding': receipt_binding, 'candidate_label': None,
                      'evaluation_split': None})
    require(covered == set(rows), "complete_original_source_partition_required")
    return cases


def rebuild(train, historical_inventory, grouping_folder, free_screen, solution_screen):
    """New source-only transaction; all owner/data bytes checked before/after."""
    dependencies = owners()
    bindings = [binding(train), binding(historical_inventory)]
    require(bindings[0]['sha256'] == source.TRAIN_SHA256
            and bindings[1]['sha256'] == HISTORY_SHA256, "pinned_original_sources_required")
    history = history_records(load(historical_inventory), bindings)
    original_rows = list(source.verified_rows(train))
    records = []
    for number, raw in original_rows:
        row = json.loads(raw)
        records.append({'source_id': f"apps-train-{row['id']}", 'original_line_number': number,
                        'question': row['question'], 'references': json.loads(row['solutions']), 'url': row['url']})
    grouping_folder = Path(grouping_folder)
    for name in ('groups.json', 'start.json', 'public-aggregate.json', 'provenance-supplement.json'):
        bindings.append(binding(grouping_folder / name))
    stored = load(grouping_folder / 'groups.json')
    grouped = dedup.groups(records, history)
    require(all(stored[key] == value for key, value in grouped.items())
            and stored['original_record_set_sha256'] == oracle.digest(records)
            and stored['historical_record_set_sha256'] == oracle.digest(history),
            "full_original_group_rebuild_mismatch")
    passed, free_counts = reference_join(free_screen, source, oracle, original_rows, bindings)
    class_passed, class_counts = reference_join(solution_screen, solution_source, solution_oracle, original_rows, bindings)
    require(not set(passed).intersection(class_passed), "ambiguous_original_call_mode")
    passed.update(class_passed)
    cases = fixed_pool(grouped['groups'], passed, original_rows)
    check(bindings + dependencies)
    body = {'schema': SCHEMA, 'policy': dedup.POLICY, 'full_source_sha256': source.TRAIN_SHA256,
            'source_revision': source.REVISION, 'original_split': 'train', 'source_rows': 5000,
            'historical_original_rows': len(history), 'source_groups': grouped['source_group_count'],
            'historical_excluded_groups': grouped['excluded_groups'], 'cases': cases,
            'known_duplicate_policy_rebuild_verified': True, 'semantic_uniqueness_proven': False,
            'original_reference_policy': source.REFERENCE_POLICY,
            'reference_results': {'free': free_counts, 'solution': class_counts},
            'bindings': bindings, 'owner_bindings': dependencies,
            'new_transaction_before_and_after_hashes_match': True,
            'original_grouping_before_load_graph_retroactively_complete': False,
            'reference_qualified_groups': len(cases), 'source_only': True,
            'candidate_labels_assigned': False, 'protocol_or_resource_or_cohort_frozen': False,
            'collection_authorized': False, 'formal_registration': False,
            'threshold_certified': False, 'holdout_opened': False, 'production_authorized': False,
            'oracle_executions': 0, 'model_calls': 0, 'jev_calls': 0, 'paid_calls': 0}
    return {**body, 'artifact_hash': oracle.digest(body)}


def validate(value):
    """Authenticate a completed rebuild for later, separately frozen stages.

    Caller must pin this artifact's raw bytes/hash; self-sealing alone does not
    prove that an arbitrary replacement manifest was originally admitted.
    """
    plain = {key: item for key, item in value.items() if key != 'artifact_hash'}
    require(value.get('artifact_hash') == oracle.digest(plain) and value.get('schema') == SCHEMA,
            "source_authority_seal_invalid")
    require(value['collection_authorized'] is False and value['formal_registration'] is False
            and value['production_authorized'] is False and value['candidate_labels_assigned'] is False
            and value['source_only'] is True, "source_only_authority_required")
    check(value['bindings'] + value['owner_bindings'])
    return value
