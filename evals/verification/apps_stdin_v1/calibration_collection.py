"""Calibration public manifests and current-prefix guards after genuine opening.

Full original source/archive proof is performed at batch entry. Between requests
strict raw closures and the real current prefix are checked; no persistent proof
cache, zero-Jev projection, source reselection or caller phase flag exists.
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from . import (
    budget,
    cohort,
    collection,
    development_jev,
    development_originals,
    development_review,
    packet,
)
from . import calibration_originals as originals

SCHEMA = 'buglecat.verification.stdin-calibration-main-manifest.v1'
EXAMPLE = 'verification_apps_stdin_v1_calibration_collect'
BIND_FIELDS = ('source_proposal_file', 'source_proposal_sha256', 'source_proposal_hash',
               'source_snapshot_file', 'source_snapshot_sha256', 'source_authority_file',
               'source_authority_sha256', 'oracle_runtime_binding',
               'calibration_opening_file', 'calibration_opening_sha256', 'calibration_opening_hash')


def runner_sources():
    result = dict(development_jev.runner_sources())
    result['development_jev_runner_source_sha256'] = result.pop('runner_source_sha256')
    for name in ('development_acceptance', 'calibration_opening', 'calibration_originals', 'calibration_collection', 'calibration_intake'):
        result['stdin_' + name + '_source_sha256'] = Path(__file__).with_name(name + '.py')
    root = Path(__file__).resolve().parents[3]
    folder = root / 'rust/crates/ncx-core/examples'
    result['runner_source_sha256'] = folder / (EXAMPLE + '.rs')
    for name in ('engine', 'support', 'ledger', 'fixture', 'provenance'):
        result['calibration_' + name + '_source_sha256'] = folder / EXAMPLE / (name + '.rs')
    return result


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'EXAMPLE', 'BIND_FIELDS'))
    packet.require(BIND_FIELDS == (*collection.BIND_FIELDS, *originals.OPENING_FIELDS), 'original source/opening binding aliases changed')
    originals.loaded_check()


def build(executable):
    loaded_check()
    executable = Path(executable).resolve(strict=True)
    env = {key: value for key, value in os.environ.items() if not any(word in key.upper() for word in ('KEY', 'TOKEN', 'SECRET'))}
    done = subprocess.run([str(executable), 'build-info'], capture_output=True, timeout=20, env=env)
    packet.require(done.returncode == 0, 'actual calibration tool unavailable')
    output = budget.parse(done.stdout)
    info = output['build_info']
    packet.require(output['paid_calls'] == 0 and output['http_calls'] == 0
                   and info['executable_sha256'] == cohort.sha(executable)
                   and all(info.get(key) == cohort.sha(file) for key, file in runner_sources().items()),
                   'actual compiled calibration graph/EXE mismatch')
    return info


def _manifest(pack, executable, cases, compiled):
    value = pack['cohort']
    graph = budget.raw_bindings([*runner_sources().values(), executable])
    return cohort.seal({'schema': SCHEMA, 'split': 'calibration', 'cases': cases,
        **value['main_collection_settings'],
        'artifact_cohort_file': pack['opening']['cohort_file'], 'artifact_cohort_sha256': cohort.sha(pack['opening']['cohort_file']),
        'artifact_cohort_hash': value['artifact_hash'], 'authorization_file': value['authorization_file'],
        'authorization_sha256': value['authorization_sha256'],
        'source_file': str(pack['source_pack']['train'].resolve()), 'source_sha256': pack['source_pack']['authority']['full_source_sha256'],
        'catalog_file': value['source_authority_file'], 'catalog_sha256': value['source_authority_sha256'],
        'protocol_sha256': value['protocol_sha256'],
        'main_resource_profile_file': value['main_resource_profile_file'], 'main_resource_profile_sha256': value['main_resource_profile_sha256'],
        'main_resource_profile_hash': value['main_resource_profile_hash'],
        'public_tasks': [collection.public_task(pack['source_pack']['tasks'][case['task_id']]) for case in cases],
        **{key: value[key] for key in collection.BIND_FIELDS}, **{key: pack[key] for key in originals.OPENING_FIELDS},
        'collector_executable_file': str(Path(executable).resolve()), 'collector_build_info': compiled,
        'phase_proof_bindings': cohort.source_authority._merge_bindings(pack['bindings'], pack['budget_bindings'], graph),
        'original_main_directory_evidence': pack['archive']['main_evidence'],
        'original_jev_directory_evidence': pack['archive']['jev_evidence'],
        'formal_registration': False, 'holdout_opened': False, 'production_authorized': False,
        'reference_and_assertions_sent': False})


def manifest_value(opening_file, executable, count=8):
    packet.require(type(count) is int and 1 <= count <= 8, 'calibration batch limit eight')
    pack = originals.proof(opening_file)
    compiled = build(executable)
    started = {row['group_id'] for row in pack['current']['main_started']}
    cases = [case for case in pack['opening']['cases'] if case['group_id'] not in started][:count]
    packet.require(cases, 'fixed calibration allocation exhausted')
    packet.require(not pack['new_main'] or pack['finished'][pack['new_main'][-1]['group_id']] is not None,
                   'pending calibration cannot be skipped')
    result = _manifest(pack, executable, cases, compiled)
    originals.close(pack)
    return result


def validate_manifest(path, *, pack=None):
    loaded_check()
    raw = cohort.sha(path)
    value = cohort.sealed(budget.load(path))
    packet.require(value.get('schema') == SCHEMA and value.get('split') == 'calibration'
                   and type(value.get('cases')) is list and 1 <= len(value['cases']) <= 8,
                   'genuine calibration manifest only; fixture/holdout rejected')
    pack = originals.proof(value['calibration_opening_file']) if pack is None else pack
    fixed = pack['opening']['cases']
    positions = [fixed.index(case) for case in value['cases']]
    packet.require(positions == list(range(positions[0], positions[0] + len(positions))), 'fixed contiguous calibration prefix required')
    packet.require(budget.same(value, _manifest(pack, value['collector_executable_file'], value['cases'], build(value['collector_executable_file']))),
                   'actual opening/public manifest/source/compiled bindings changed')
    packet.require(cohort.sha(path) == raw, 'manifest bytes changed during proof')
    originals.close(pack)
    return value, pack


def source_closure(row):
    """Batch-proved raw graph closure, not another source selection proof."""
    loaded_check()
    manifest = cohort.sealed(budget.load(row['manifest_file']))
    packet.require(manifest.get('schema') == SCHEMA and manifest['split'] == row['split'] == 'calibration'
                   and cohort.sha(row['manifest_file']) == row['manifest_sha256'], 'original calibration manifest changed')
    marker = cohort.sealed(budget.load(row['calibration_opening_file']))
    chain = marker['authorization_chain']
    packet.require(Path(row['calibration_opening_file']).absolute() == originals.opening.marker_path(chain, row['artifact_cohort_hash'])
                   and cohort.sha(row['calibration_opening_file']) == row['calibration_opening_sha256']
                   and marker['artifact_hash'] == row['calibration_opening_hash']
                   and all(budget.same(row[key], manifest[key]) for key in originals.OPENING_FIELDS), 'genuine opening byte identity changed')
    cohort.check(manifest['phase_proof_bindings'])
    for key in ('original_main_directory_evidence', 'original_jev_directory_evidence'):
        evidence = manifest[key]
        packet.require(budget.same(development_review.evidence([scope['directory'] for scope in evidence]), evidence),
                       'original development directory set/bytes changed')
    current, finished = budget._snapshot_chain(chain)
    marker['_identity'] = {key: row[key] for key in originals.OPENING_FIELDS}
    new = originals._prefix(marker, current, finished, budget._ledger_rows(Path(current['main_ledger_file'])))
    originals.verify_main_prefix({'opening': marker, 'current': current, 'finished': finished, 'new_main': new,
                                 **{key: row[key] for key in originals.OPENING_FIELDS}})
    packet.require(budget.same(development_originals.binding_images(chain), marker['original_accounting_image']['raw_budget_bindings']),
                   'original Jev accounting bytes changed during main batch')
    cohort.check(manifest['phase_proof_bindings'])
    return {'opening': marker, 'current': current, 'finished': finished, 'new_main': new}


def row_proof(path):
    row = budget.load(path)
    packet.require(row.get('artifact_sha256') == budget.digest({key: child for key, child in row.items() if key != 'artifact_sha256'}), 'prepared bytes seal changed')
    pack = source_closure(row)
    attempted = {item['group_id'] for item in pack['current']['main_started']}
    remaining = [case for case in pack['opening']['cases'] if case['group_id'] not in attempted]
    packet.require(remaining and all(budget.same(row.get(key), remaining[0][key]) for key in collection.history_fields()), 'only next fixed calibration group may start')
    packet.require(not pack['new_main'] or pack['finished'][pack['new_main'][-1]['group_id']] is not None, 'pending cannot be skipped')
    return {'status': 'stdin_next_calibration_group_verified', 'row_hash': row['artifact_sha256'], 'paid_calls': 0}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('validate-manifest', 'validate-row', 'close-batch'))
    parser.add_argument('--file', required=True)
    args = parser.parse_args()
    if args.action == 'validate-manifest':
        value, _ = validate_manifest(args.file)
        result = {'status': 'stdin_calibration_source_verified', 'hash': value['artifact_hash'], 'paid_calls': 0}
    elif args.action == 'validate-row':
        result = row_proof(args.file)
    else:
        source_closure(budget.load(args.file))
        result = {'status': 'stdin_original_calibration_batch_closure_verified', 'paid_calls': 0}
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
