"""Original APPS holdout manifests consume a genuine shared once source proof."""
import argparse
import json
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from . import formal_archive, formal_collection, formal_holdout

SETTINGS = formal_collection.SETTINGS
development = formal_collection.development
packet = formal_collection.packet
BIND_FIELDS = (*formal_collection.BIND_FIELDS, 'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash')
MANIFEST = 'buglecat.verification.apps-holdout-main-collection.v3'
EXAMPLES = formal_collection.EXAMPLES


def owners():
    paths = {'holdout_collection_owner': Path(__file__), 'holdout_intake_owner': Path(__file__).with_name('holdout_intake.py'),
             'holdout_source_owner': Path(formal_holdout.__file__), 'calibration_archive_owner': Path(formal_archive.__file__),
             'holdout_collector_source': EXAMPLES / 'verification_apps_v3_holdout_collect.rs',
             'holdout_collector_engine': EXAMPLES / 'verification_apps_v3_holdout_collect/engine.rs',
             'holdout_collector_support': EXAMPLES / 'verification_apps_v3_holdout_collect/support.rs'}
    return [development.binding(role, path) for role, path in sorted(paths.items())]


def verified_pack(marker_file):
    marker, pack = formal_archive.proof_transaction(marker_file)
    require(all(any(b['file'] == expected['file'] and b['sha256'] == expected['sha256'] for b in marker['source_bindings'])
                for expected in owners()), 'complete_original_holdout_collector_owners_required')
    return marker, pack


def _manifest(marker_file, marker, pack, cases):
    cohort, tasks, train, _, _ = pack
    original = formal_collection._manifest(marker['cohort_file'], cohort, tasks, train, cases)
    body = {k: v for k, v in original.items() if k != 'artifact_hash'}
    body.update(schema=MANIFEST, split='holdout', holdout_source_file=str(Path(marker_file).resolve(strict=True)),
                holdout_source_sha256=file_hash(marker_file), holdout_source_hash=marker['artifact_hash'],
                holdout_owner_bindings=owners())
    return {**body, 'artifact_hash': digest(body)}


def manifest_value(marker_file, count=8):
    require(type(count) is int and 1 <= count <= 8, 'batch_upper_bound_eight')
    marker, pack = verified_pack(marker_file)
    current = pack[3]
    attempted = {r['group_id'] for r in current['main_started']}
    cases = [c for c in marker['cases'] if c['group_id'] not in attempted][:count]
    require(cases and current['remaining_main_attempts'] >= len(cases), 'holdout_exhausted_or_budget_unavailable')
    reserved = {r['group_id'] for key in ('main_reservations', 'jev_reservations') for r in current[key]}
    require(not reserved & {c['group_id'] for c in cases}, 'unresolved_holdout_slot_cannot_skip_or_retry')
    return _manifest(marker_file, marker, pack, cases)


def manifest_from_pack(value, pack, marker):
    sealed(value)
    require(value['schema'] == MANIFEST and value['split'] == 'holdout' and type(value['cases']) is list
            and 1 <= len(value['cases']) <= 8, 'actual_once_holdout_manifest_required')
    positions = {c['id']: n for n, c in enumerate(marker['cases'])}
    indexes = [positions.get(c['id'], -1) for c in value['cases']]
    require(indexes == list(range(indexes[0], indexes[0] + len(indexes))) and indexes[0] >= 0,
            'original_contiguous_holdout_prefix_required')
    require(formal_collection._same(value, _manifest(value['holdout_source_file'], marker, pack, value['cases']))
            and all(formal_collection._same(c, marker['cases'][positions[c['id']]]) for c in value['cases']), 'exact_original_holdout_manifest_changed')
    return value


def validate_manifest(path):
    value = load(path)
    marker, pack = verified_pack(value['holdout_source_file'])
    return manifest_from_pack(value, pack, marker)


def budget_snapshot(auth):
    return formal_collection.budget_snapshot(auth)


_same = formal_collection._same


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['validate-manifest'])
    parser.add_argument('--manifest', required=True, type=Path)
    args = parser.parse_args()
    value = validate_manifest(args.manifest)
    print(json.dumps({'status': 'apps_original_holdout_source_verified', 'hash': value['artifact_hash'],
                      'formal_registration': False, 'new_paid_calls': 0}))


if __name__ == '__main__':
    main()
