"""Shared once source authority after genuine complete APPS calibration.

Opening is explicit and durable, never a request or reservation. The frozen
calibration collector cannot send holdout; a separate compiled holdout sender
must consume this exact marker. Production and general certification stay off.
"""
import os
import sqlite3
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..paid_budget import durable_new
from . import formal_collection, formal_evaluation, formal_jev

SCHEMA = 'buglecat.verification.apps-once-holdout-source.v3'
NAMESPACE = 'formal-apps-holdout-source-v3'


def _bindings(calibration):
    paths = {Path(b['file']).resolve(strict=True) for b in calibration['runner_bindings']}
    paths.update(Path(calibration[k]).resolve(strict=True) for k in (
        'registration_file', 'allocation_file', 'observations_file', 'runner_executable_file', 'cohort_file', 'protocol_file'))
    reg = load(calibration['registration_file'])
    paths.update(Path(b['file']).resolve(strict=True) for b in reg['source_bindings'] + reg['original_evidence_bindings'])
    paths.add(Path(reg['dataset_file']).resolve(strict=True))
    for row in formal_jev.read_lines(calibration['observations_file']):
        paths.add(Path(row['prepared_file']).resolve(strict=True))
        directory = Path(row['attempt_directory']).resolve(strict=True)
        for p in directory.rglob('*'):
            require(not p.is_symlink(), 'original_attempt_symlink')
            if p.is_file():
                require(p.resolve(strict=True).is_relative_to(directory), 'original_attempt_path_escape')
                paths.add(p.resolve(strict=True))
    paths.update((Path(__file__).resolve(), Path(__file__).with_name('formal_archive.py').resolve(strict=True),
                  Path(formal_evaluation.__file__).resolve()))
    directory = Path(__file__).parent
    paths.update((directory / name).resolve(strict=True) for name in (
        'holdout_collection.py', 'holdout_intake.py', 'holdout_registration.py', 'holdout_jev.py', 'holdout_evaluation.py'))
    examples = formal_collection.EXAMPLES
    paths.update((examples / name).resolve(strict=True) for name in (
        'verification_apps_v3_holdout_collect.rs', 'verification_apps_v3_holdout_collect/engine.rs',
        'verification_apps_v3_holdout_collect/support.rs', 'verification_apps_v3_jev_holdout.rs'))
    return [{'file': str(p), 'sha256': file_hash(p)} for p in sorted(paths)]


def source_value(calibration_file):
    """Read-only original proof. Returned bytes alone grant no send permission."""
    calibration_sha = file_hash(calibration_file)
    original = sealed(load(calibration_file))
    captured = _bindings(original)
    actual, _, pack = formal_evaluation.proof_transaction(calibration_file)
    threshold = actual['qualified_for_holdout_threshold']
    require(actual['qualified_for_once_holdout'] is True
            and actual['calibration_numeric_gate_met'] is True
            and type(threshold) in {int, float} and threshold in formal_collection.protocol.GRID
            and actual['certified_threshold'] is None and actual['formal_certification'] is False
            and actual['production_authorized'] is False, 'genuine_qualified_original_calibration_required')
    require(file_hash(calibration_file) == calibration_sha and formal_jev.same(actual, original)
            and formal_jev.same(_bindings(actual), captured)
            and all(file_hash(b['file']) == b['sha256'] for b in captured), 'original_calibration_graph_changed_after_proof')
    cohort, _, _, snapshot, finished = pack
    cases = [c for c in cohort['cases'] if c['split'] == 'holdout']
    groups = {c['group_id'] for c in cases}
    require(len(cases) == cohort['funded_allocation']['holdout_main_earmark']
            and groups.isdisjoint({c['group_id'] for c in cohort['cases'] if c['split'] == 'calibration'})
            and groups.isdisjoint(cohort['excluded_groups']), 'whole_fixed_fresh_holdout_required')
    used = {c['group_id'] for c in snapshot['main_started']}
    used |= {r['group_id'] for key in ('main_reservations', 'jev_reservations') for r in snapshot[key]}
    require(groups.isdisjoint(used), 'holdout_already_started_or_reserved')
    auth = snapshot['authorization_chain']['authorization']
    require(snapshot['remaining_main_attempts'] >= len(cases)
            and auth['jev']['max_attempts'] - len(snapshot['jev_reservations']) >= len(cases), 'full_holdout_budget_unavailable')
    root = Path(auth['jev']['ledger_dir']).resolve(strict=True)
    target = root / NAMESPACE / (cohort['artifact_hash'] + '.json')
    require(not target.exists(), 'once_holdout_source_already_consumed')
    body = {'schema': SCHEMA, 'split': 'holdout', 'calibration_file': str(Path(calibration_file).resolve(strict=True)),
            'calibration_sha256': calibration_sha, 'calibration_hash': actual['artifact_hash'],
            'cohort_file': actual['cohort_file'], 'cohort_sha256': actual['cohort_sha256'], 'cohort_hash': cohort['artifact_hash'],
            'registration_file': actual['registration_file'], 'registration_sha256': actual['registration_sha256'],
            'fixed_threshold': threshold, 'cases': cases, 'allocation_groups': [c['group_id'] for c in cases],
            'before_holdout_snapshot': snapshot, 'before_holdout_finished': {g: v for g, v in finished.items() if v is not None},
            'source_bindings': captured, 'marker_file': str(target), 'ledger_root': str(root),
            'holdout_collection_authorized': True, 'qualified_original_calibration_verified': True,
            'threshold_retuning_allowed': False, 'holdout_labels_read': False, 'holdout_probabilities_read': False,
            'production_authorized': False, 'formal_certification': False, 'certified_threshold': None,
            'native_claim_certification': False, 'overall_agent_benefit_certification': False,
            'new_paid_calls': 0, 'reservations': 0, 'oracle_executions': 0}
    return {**body, 'artifact_hash': digest(body)}


def open_source(calibration_file):
    """Explicit Root action; fsync one marker under the original shared lock."""
    value = source_value(calibration_file)
    root = Path(value['ledger_root'])
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.write(descriptor, str(os.getpid()).encode())
        os.fsync(descriptor)
        # The original reservation reader intentionally rejects this lock.
        # Inspect precisely the original namespace while owning it, no new slot.
        old = value['before_holdout_snapshot']['jev_reservations']
        paths = list((root / 'reservations').glob('*.json'))
        expected = {digest(r['group_id']) + '.json': r for r in old}
        require(len(expected) == len(old) and {p.name for p in paths} == set(expected)
                and all(formal_jev.same(load(p), expected[p.name]) for p in paths)
                and load(root / 'authorization.json') == {
                    'authorization_sha256': value['before_holdout_snapshot']['authorization_chain']['ledger_authorization_sha256'],
                    'kind': 'jev'}, 'reservation_changed_before_source_open')
        require(file_hash(calibration_file) == value['calibration_sha256']
                and all(file_hash(b['file']) == b['sha256'] for b in value['source_bindings']), 'original_proof_changed_before_source_open')
        # Freeze the same actual SQLite start/finish rows until marker fsync.
        from . import approved_budget
        with sqlite3.connect(value['before_holdout_snapshot']['main_ledger_file']) as db:
            db.execute('BEGIN IMMEDIATE')
            rows = list(db.execute('SELECT ordinal,group_id,started_json,finished_json FROM attempts ORDER BY ordinal'))
            starts = [{'ordinal': n, 'group_id': g, 'started': approved_budget._json(s)} for n, g, s, _ in rows]
            completed = {g: approved_budget._json(f) if f is not None else None for _, g, _, f in rows}
            metadata = list(db.execute('SELECT key,value FROM metadata ORDER BY key'))
            sequence = list(db.execute("SELECT seq FROM sqlite_sequence WHERE name='attempts'"))
            require(formal_jev.same(starts, value['before_holdout_snapshot']['main_started'])
                    and all(formal_jev.same(completed.get(g), v) for g, v in value['before_holdout_finished'].items())
                    and formal_jev.same(metadata, value['before_holdout_snapshot']['main_metadata'])
                    and sequence == [(len(starts),)], 'main_changed_before_source_open')
            namespace = root / NAMESPACE
            require(not namespace.is_symlink(), 'holdout_namespace_symlink')
            namespace.mkdir(exist_ok=True)
            durable_new(Path(value['marker_file']), value)
    finally:
        os.close(descriptor)
        lock.unlink()
    return value


def validate(marker_file):
    """Recompute original calibration through the independent archive owner."""
    from . import formal_archive
    value = sealed(load(marker_file))
    return formal_archive.validate_source(marker_file, value)
