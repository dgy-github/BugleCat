"""Public-only development manifests and batch-local pre-request proof.

stdout never contains hidden IO, reference code, labels or original full tasks.
Cal/holdout strings cannot grant authority; their phase owners do not exist yet.
"""
import argparse
import json
import sys
from pathlib import Path

from . import budget, cohort, packet

SCHEMA = 'buglecat.verification.stdin-development-main-manifest.v1'
BIND_FIELDS = ('source_proposal_file', 'source_proposal_sha256', 'source_proposal_hash',
               'source_snapshot_file', 'source_snapshot_sha256', 'source_authority_file',
               'source_authority_sha256', 'oracle_runtime_binding')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'BIND_FIELDS'))


def public_task(task):
    """Only original question/starter; never reference-derived interfaces."""
    return {'task_id': task['task_id'], 'source_kind': task['source_kind'],
            'main_prompt': packet.main_prompt(task['question'], task['starter_code'])}


def _manifest(cohort_file, pack, cases):
    value = pack['cohort']
    tasks = pack['source_pack']['tasks']
    return cohort.seal({'schema': SCHEMA, 'split': 'development', 'cases': cases,
        **value['main_collection_settings'],
        'artifact_cohort_file': str(Path(cohort_file).resolve()), 'artifact_cohort_sha256': cohort.sha(cohort_file),
        'artifact_cohort_hash': value['artifact_hash'], 'authorization_file': value['authorization_file'],
        'authorization_sha256': value['authorization_sha256'],
        'source_file': str(pack['source_pack']['train'].resolve()), 'source_sha256': pack['source_pack']['authority']['full_source_sha256'],
        'catalog_file': value['source_authority_file'], 'catalog_sha256': value['source_authority_sha256'],
        'protocol_sha256': value['protocol_sha256'],
        'main_resource_profile_file': value['main_resource_profile_file'],
        'main_resource_profile_sha256': value['main_resource_profile_sha256'],
        'main_resource_profile_hash': value['main_resource_profile_hash'],
        'public_tasks': [public_task(tasks[case['task_id']]) for case in cases],
        **{key: value[key] for key in BIND_FIELDS},
        'formal_registration': False, 'holdout_opened': False, 'production_authorized': False,
        'reference_and_assertions_sent': False})


def manifest_value(cohort_file, count=8, split='development'):
    packet.require(split == 'development', 'calibration/holdout genuine phase owner unavailable')
    packet.require(type(count) is int and 1 <= count <= 8, 'development batch limit eight')
    pack = cohort.verified_pack(cohort_file)
    starts = {row['group_id'] for row in pack['current']['main_started']}
    cases = [case for case in pack['cohort']['cases'] if case['split'] == split and case['group_id'] not in starts][:count]
    packet.require(cases, 'development allocation exhausted')
    return _manifest(cohort_file, pack, cases)


def validate_manifest(manifest_file, *, pack=None):
    """Genuine source proof once, then pure public manifest reconstruction."""
    loaded_check()
    raw_sha = cohort.sha(manifest_file)
    value = cohort.sealed(budget.load(manifest_file))
    packet.require(value.get('schema') == SCHEMA and value.get('split') == 'development'
                   and type(value.get('cases')) is list and 1 <= len(value['cases']) <= 8,
                   'only genuine STDIN development manifests accepted')
    pack = cohort.verified_pack(value['artifact_cohort_file']) if pack is None else pack
    fixed = [case for case in pack['cohort']['cases'] if case['split'] == 'development']
    positions = [fixed.index(case) for case in value['cases']]
    packet.require(positions == list(range(positions[0], positions[0] + len(positions))), 'fixed contiguous source prefix required')
    packet.require(budget.same(value, _manifest(value['artifact_cohort_file'], pack, value['cases'])),
                   'public manifest or immutable bindings changed')
    packet.require(cohort.sha(manifest_file) == raw_sha, 'manifest raw bytes changed during proof')
    return value, pack


def next_case(pack, row):
    """No caller can jump the only actually opened source prefix."""
    attempted = {item['group_id'] for item in pack['current']['main_started']}
    remaining = [case for case in pack['cohort']['cases'] if case['split'] == 'development' and case['group_id'] not in attempted]
    packet.require(remaining and all(budget.same(row.get(key), remaining[0][key]) for key in history_fields()),
                   'requested group is not the next unstarted development group')
    own = [item for item in pack['current']['main_started'] if item['started'].get('artifact_cohort_hash') == pack['cohort']['artifact_hash']]
    packet.require(not own or pack['finished'][own[-1]['group_id']] is not None, 'in-flight main cannot be skipped')


def history_fields():
    return ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')


def source_closure(row):
    """Raw closure plus current prefix, never a source-selection authority."""
    loaded_check()
    value = cohort.sealed(budget.load(row['artifact_cohort_file']))
    packet.require(cohort.sha(row['artifact_cohort_file']) == row['artifact_cohort_sha256']
                   and value['artifact_hash'] == row['artifact_cohort_hash'], 'cohort changed before request')
    cohort.check(value['bindings'])
    chain = value['ledger_snapshot']['authorization_chain']
    budget.loaded_check()
    cohort.check(chain['bindings'])
    current, finished = budget._snapshot_chain(chain)
    cohort.history.validate_prefix(value['ledger_snapshot'], value['original_finished'], current, finished,
                                   value['cases'], value['artifact_hash'])
    packet.require(cohort.sha(row['manifest_file']) == row['manifest_sha256'], 'manifest changed before request')
    cohort.check(value['bindings'])
    return {'cohort': value, 'current': current, 'finished': finished}


def row_proof(row_file):
    """Fresh budget/prefix plus bytes; full source is checked by batch entry."""
    row = budget.load(row_file)
    packet.require(row.get('artifact_sha256') == budget.digest({key: child for key, child in row.items() if key != 'artifact_sha256'}),
                   'prepared row seal changed')
    next_case(source_closure(row), row)
    return {'status': 'stdin_next_development_group_verified', 'row_hash': row['artifact_sha256'], 'paid_calls': 0}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('validate-manifest', 'validate-row', 'close-batch'))
    parser.add_argument('--file', required=True)
    args = parser.parse_args()
    if args.action == 'close-batch':
        row = budget.load(args.file)
        source_closure(row)
        result = {'status': 'stdin_original_batch_closure_verified', 'paid_calls': 0}
    elif args.action == 'validate-row':
        result = row_proof(args.file)
    else:
        value, _ = validate_manifest(args.file)
        result = {'status': 'stdin_development_source_verified', 'hash': value['artifact_hash'], 'paid_calls': 0}
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
