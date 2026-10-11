"""Pinned APPS development allocation and public-only collection proof.

Pure preregistration returns bytes for the Root operator to create exclusively.
No execution, transport, reservations, formal registration or holdout authority.
"""
import argparse
import json
from pathlib import Path

from ..apps_solution_v3 import source as solution_source
from ..apps_v3 import source
from ..artifact_profile_cohort import SETTINGS, validate_profile
from ..artifact_scoring import sealed
from ..artifact_v2_development_cohort import snapshot, system_prompt
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from . import admission_guard, packet, protocol, resources, source_authority

SCHEMA = "buglecat.verification.apps-development-cohort.v3"
MANIFEST = "buglecat.verification.apps-development-collection.v3"
SEED = "apps-development-balanced-public-difficulty-v3-20261011"
AUTHORITY_SHA = "6fab7f355a9de9c5f83102b3a99938d54216cca33ccccda3d6e5c4789a20c625"
RESOURCE_SHA = "d779a64b57445458b1b7a732c30806fb5d7cca91a95ef45c2ba4a295f6118237"
PACKET_SHA = "eaf0ba877ca9d9c4d510e7244398c9d648617dfb2792c9a0967bc43fdd4c391e"
PROTOCOL_SHA = "4c8ed8427fbe57a0902d66b759f0c267f83933b96a7d8f1bf4aa09e769e419db"
ROOT = Path(__file__).resolve().parents[3]
EXAMPLES = ROOT / "rust/crates/ncx-core/examples"
BIND_FIELDS = ("source_authority_file", "source_authority_sha256", "resource_inventory_file",
               "resource_inventory_sha256", "packet_source_sha256", "oracle_runtime_binding")


def binding(role, path):
    path = Path(path).resolve(strict=True)
    return {"role": role, "file": str(path), "sha256": file_hash(path)}


def owners():
    files = {"development_owner": Path(__file__), "admission_guard": Path(admission_guard.__file__),
             "resources_owner": Path(resources.__file__), "authority_owner": Path(source_authority.__file__),
             "protocol_owner": Path(protocol.__file__),
             "collector_source": EXAMPLES / "verification_apps_v3_collect.rs",
             "collector_engine": EXAMPLES / "verification_apps_v3_collect/engine.rs",
             "collector_support": EXAMPLES / "verification_apps_v3_collect/support.rs",
             "shared_ledger_support": EXAMPLES / "verification_artifact_v2_collect/support.rs",
             "shared_profile_support": EXAMPLES / "verification_artifact_profile_collect/support.rs",
             "shared_frozen_support": EXAMPLES / "verification_artifact_collect/support.rs",
             "shared_legacy_support": EXAMPLES / "verification_collect/support.rs",
             "request_builder": ROOT / "rust/crates/ncx-provider/src/request.rs",
             "response_parser": ROOT / "rust/crates/ncx-provider/src/response.rs"}
    for kind in sorted(packet.KINDS):
        files.update({kind + ':' + role: path for role, path in packet.dependency_files(kind).items()})
    # Snapshot/profile/resolver imports are original owners, conservatively bind
    # every flat helper. No new record changes an original owner or fee authority.
    files.update({'shared_python:' + p.name: p for p in Path(__file__).resolve().parent.parent.glob('*.py')})
    return [binding(role, path) for role, path in sorted(files.items())]


def inputs(authority_file, resource_inventory_file):
    authority = admission_guard.pinned(authority_file, AUTHORITY_SHA)
    require(file_hash(resource_inventory_file) == RESOURCE_SHA
            and file_hash(packet.__file__) == PACKET_SHA, "pinned resource/packet bytes changed")
    inventory = load(resource_inventory_file)
    require(inventory['schema'] == resources.SCHEMA and inventory['source_authority_sha256'] == AUTHORITY_SHA
            and inventory['collection_authorized'] is False
            and type(inventory['paid_calls']) is int and inventory['paid_calls'] == 0
            and type(inventory['candidate_oracle_executions']) is int and inventory['candidate_oracle_executions'] == 0,
            "source-only resources required")
    paths = {'resource_owner': Path(resources.__file__), 'source_authority_owner': Path(source_authority.__file__),
             'source_admission_guard': Path(admission_guard.__file__),
             'operator': Path(resource_inventory_file).resolve().with_name('_inspect.py')}
    for kind in sorted(packet.KINDS):
        paths.update({kind + ':' + role: path for role, path in packet.dependency_files(kind).items()})
    import sys
    paths['runtime'] = Path(sys.executable).resolve()
    require(inventory['source_graph_sha256'] == {role: file_hash(path) for role, path in paths.items()},
            "original complete resource graph changed")
    a = {c['group_id']: c for c in authority['cases']}
    r = {c['group_id']: c for c in inventory['cases']}
    require(len(a) == len(r) == len(authority['cases']) == len(inventory['cases']) == 2278
            and set(a) == set(r), "complete resource inventory required")
    for group, row in r.items():
        require(all(row[k] == a[group][k] for k in ('task_id', 'source_kind', 'original_difficulty'))
                and row['original_task_sha256'] == a[group]['task_sha256']
                and row['candidate_label'] is None and row['evaluation_split'] is None,
                "resource original source association changed")
    return authority, inventory


def choose(authority, inventory, baseline, count):
    require(type(count) is int and 1 <= count <= 8, "development allocation upper bound eight")
    qualified = {c['group_id'] for c in inventory['cases'] if c['resource_status'] == 'qualified'}
    excluded = set(baseline['excluded_groups'])
    allocation = []
    for difficulty, n in (('interview', (count + 1) // 2), ('introductory', count // 2)):
        pool = [c for c in authority['cases'] if c['group_id'] in qualified
                and c['group_id'] not in excluded and c['original_difficulty'] == difficulty]
        ordered = sorted(pool, key=lambda c: digest({'seed': SEED, 'group_id': c['group_id']}))
        require(len(ordered) >= n, "insufficient fresh development difficulty stratum")
        allocation.extend(ordered[:n])
    return [{**case, 'id': f"apps-v3-dev-main-{case['task_id']}", 'split': 'development'} for case in allocation]


def original_tasks(authority, cases):
    train = next(b['file'] for b in authority['bindings'] if b['sha256'] == source.TRAIN_SHA256)
    selected = {c['task_id']: c for c in cases}
    tasks = {}
    for number, raw in source.verified_rows(train):
        task_id = json.loads(raw)['id']
        if task_id not in selected:
            continue
        case = selected[task_id]
        task = (solution_source if case['source_kind'] == packet.SOLUTION else source).parse_row(raw, number)
        require(digest(task) == case['task_sha256'], "original task changed")
        tasks[task_id] = task
    require(set(tasks) == set(selected), "complete original selected source required")
    return tasks, train


def public_task(task):
    fn = task['input_output']['fn_name']
    return {'task_id': task['task_id'], 'question': task['question'], 'starter_code': task['starter_code'],
            'fn_name': fn, 'source_kind': task['source_kind'],
            'main_prompt': packet.main_prompt(task['question'], task['starter_code'], fn, task['source_kind'])}


def preregister(authority_file, resource_inventory_file, authorization, profile_file, output_directory, count=8, protocol_file=None):
    authority, inventory = inputs(authority_file, resource_inventory_file)
    protocol_file = Path(protocol_file) if protocol_file else Path(resource_inventory_file).with_name('protocol.json')
    require(file_hash(protocol_file) == PROTOCOL_SHA, "original APPS protocol pin changed")
    protocol.validate(load(protocol_file))
    profile, profile_dependencies = validate_profile(profile_file)
    baseline, finished = snapshot(authorization)
    require(len(baseline['main_started']) == 510 and len(baseline['jev_reservations']) == 222
            and baseline['authorization_chain']['authorization']['main_model']['max_attempts'] == 750
            and baseline['authorization_chain']['authorization']['jev']['max_attempts'] == 750
            and baseline['remaining_main_attempts'] >= count, "original 510/222 shared 750 snapshot required")
    cases = choose(authority, inventory, baseline, count)
    tasks, train = original_tasks(authority, cases)
    resource_map = {c['group_id']: c for c in inventory['cases']}
    for c in cases:
        require(resources.inspect(tasks[c['task_id']]) == resource_map[c['group_id']]['resource_receipt'],
                "selected original resource receipt changed")
        c['public_interface'] = {'starter_code': tasks[c['task_id']]['starter_code'],
                                 'fn_name': tasks[c['task_id']]['input_output']['fn_name'], 'source_kind': c['source_kind']}
    dependencies = owners() + [binding(role, path) for role, path in (
        ('source_authority', authority_file), ('resource_inventory', resource_inventory_file),
        ('source', train), ('authorization', authorization), ('profile', profile_file), ('protocol', protocol_file))]
    dependencies += [binding('authorization_chain', b['file']) for b in baseline['authorization_chain']['bindings']]
    dependencies += [binding('profile_original', b['file']) for b in profile_dependencies]
    body = {'schema': SCHEMA, 'split': 'development', 'seed': SEED, 'cases': cases, 'bindings': dependencies,
            'output_directory': str(Path(output_directory).resolve()), 'ledger_snapshot': baseline,
            'original_completed': {g: f for g, f in finished.items() if f is not None},
            'source_authority_file': str(Path(authority_file).resolve()), 'source_authority_sha256': AUTHORITY_SHA,
            'resource_inventory_file': str(Path(resource_inventory_file).resolve()), 'resource_inventory_sha256': RESOURCE_SHA,
            'packet_source_sha256': PACKET_SHA, 'oracle_runtime_binding': packet.runtime_info(),
            'main_resource_profile_file': str(Path(profile_file).resolve()), 'main_resource_profile_sha256': file_hash(profile_file),
            'main_resource_profile_hash': profile['artifact_hash'], 'main_collection_settings': SETTINGS,
            'main_system_prompt_sha256': source.raw_sha(system_prompt().encode()),
            'protocol_file': str(protocol_file.resolve()), 'protocol_sha256': PROTOCOL_SHA,
            'collection_authorized': True, 'before_main_requests': True, 'candidate_labels_assigned': False,
            'formal_registration': False, 'holdout_opened': False, 'production_authorized': False,
            'whole_allocation_excluded_from_future_formal': True, 'new_paid_calls': 0}
    require(all(file_hash(b['file']) == b['sha256'] for b in dependencies), "owner changed during allocation")
    require(snapshot(authorization) == (baseline, finished), "shared snapshot changed during allocation")
    return {**body, 'artifact_hash': digest(body)}


def validate_cohort(path):
    value = sealed(load(path))
    require(value['schema'] == SCHEMA and value['split'] == 'development' and value['seed'] == SEED
            and value['collection_authorized'] is True and value['before_main_requests'] is True
            and value['whole_allocation_excluded_from_future_formal'] is True
            and all(value[k] is False for k in ('formal_registration', 'holdout_opened', 'production_authorized', 'candidate_labels_assigned'))
            and type(value['new_paid_calls']) is int and value['new_paid_calls'] == 0
            and packet.canonical_bytes(value['main_collection_settings']) == packet.canonical_bytes(SETTINGS), "development-only contract required")
    baseline = value['ledger_snapshot']
    require(len(baseline['main_started']) == 510 and len(baseline['jev_reservations']) == 222
            and [r['ordinal'] for r in baseline['main_started']] == list(range(1, 511))
            and baseline['authorization_chain']['authorization']['main_model']['max_attempts'] == 750
            and baseline['authorization_chain']['authorization']['jev']['max_attempts'] == 750,
            "complete original development budget snapshot required")
    require(all(file_hash(b['file']) == b['sha256'] for b in value['bindings']), "development binding changed")
    require(file_hash(value['protocol_file']) == value['protocol_sha256'] == PROTOCOL_SHA,
            "original APPS protocol changed")
    protocol.validate(load(value['protocol_file']))
    authority, inventory = inputs(value['source_authority_file'], value['resource_inventory_file'])
    profile, _ = validate_profile(value['main_resource_profile_file'])
    require(value['main_system_prompt_sha256'] == source.raw_sha(system_prompt().encode()), "original SYSTEM changed")
    require(value['main_resource_profile_hash'] == profile['artifact_hash']
            and value['main_resource_profile_sha256'] == file_hash(value['main_resource_profile_file'])
            and packet.canonical_bytes(value['oracle_runtime_binding']) == packet.canonical_bytes(packet.runtime_info()) and value['packet_source_sha256'] == PACKET_SHA,
            "original profile/runtime changed")
    expected = choose(authority, inventory, value['ledger_snapshot'], len(value['cases']))
    tasks, _ = original_tasks(authority, expected)
    for c in expected:
        c['public_interface'] = {'starter_code': tasks[c['task_id']]['starter_code'],
                                 'fn_name': tasks[c['task_id']]['input_output']['fn_name'], 'source_kind': c['source_kind']}
    require(packet.canonical_bytes(value['cases']) == packet.canonical_bytes(expected), "frozen allocation changed")
    by_role = {b['role']: b['file'] for b in value['bindings']}
    current, finished = snapshot(by_role['authorization'])
    current_starts = {r['group_id']: r for r in current['main_started']}
    require(all(packet.canonical_bytes(current_starts.get(r['group_id'])) == packet.canonical_bytes(r) for r in value['ledger_snapshot']['main_started'])
            and all(packet.canonical_bytes(finished.get(g)) == packet.canonical_bytes(f) for g, f in value['original_completed'].items())
            and all(dict(current['main_metadata']).get(k) == v for k, v in value['ledger_snapshot']['main_metadata']),
            "original shared history removed or changed")
    for key in ('main_reservations', 'jev_reservations'):
        require(all(packet.canonical_bytes(row) in {packet.canonical_bytes(c) for c in current[key]}
                    for row in value['ledger_snapshot'][key]), "original reservation changed")
    return value


def manifest_value(cohort_file):
    cohort = validate_cohort(cohort_file)
    authority, _ = inputs(cohort['source_authority_file'], cohort['resource_inventory_file'])
    tasks, train = original_tasks(authority, cohort['cases'])
    by_role = {b['role']: b for b in cohort['bindings']}
    body = {'schema': MANIFEST, 'split': 'development', 'formal_registration': False, 'cases': cohort['cases'], **SETTINGS,
            'artifact_cohort_file': str(Path(cohort_file).resolve()), 'artifact_cohort_sha256': file_hash(cohort_file),
            'artifact_cohort_hash': cohort['artifact_hash'], 'source_file': str(Path(train).resolve()), 'source_sha256': source.TRAIN_SHA256,
            'catalog_file': cohort['source_authority_file'], 'catalog_sha256': AUTHORITY_SHA,
            'protocol_file': cohort['protocol_file'], 'protocol_sha256': cohort['protocol_sha256'], 'authorization_file': by_role['authorization']['file'],
            'authorization_sha256': by_role['authorization']['sha256'],
            'main_resource_profile_file': cohort['main_resource_profile_file'],
            'main_resource_profile_sha256': cohort['main_resource_profile_sha256'], 'main_resource_profile_hash': cohort['main_resource_profile_hash'],
            'public_tasks': [public_task(tasks[c['task_id']]) for c in cohort['cases']],
            **{key: cohort[key] for key in BIND_FIELDS}}
    return {**body, 'artifact_hash': digest(body)}


def validate_manifest(path):
    value = sealed(load(path))
    require(packet.canonical_bytes(value) == packet.canonical_bytes(manifest_value(value['artifact_cohort_file'])), "public collection manifest changed")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['validate-manifest', 'validate-cohort'])
    parser.add_argument('--manifest', type=Path)
    parser.add_argument('--cohort', type=Path)
    args = parser.parse_args()
    value = validate_manifest(args.manifest) if args.action == 'validate-manifest' else validate_cohort(args.cohort)
    print(json.dumps({'status': 'apps_development_source_verified', 'hash': value['artifact_hash'],
                      'formal_registration': False, 'new_paid_calls': 0}))


if __name__ == '__main__':
    main()
