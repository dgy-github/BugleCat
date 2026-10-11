"""One blind development registration in the actual shared Jev namespace.

Originals are reconstructed before any marker. Registration grants only this
ordered binary development allocation; no numeric or subsequent-stage authority.
"""
import os
import sqlite3
import sys
from pathlib import Path

from .. import paid_budget
from . import budget, cohort, packet
from . import development_originals as originals

SCHEMA = 'buglecat.verification.stdin-blind-development-registration.v1'
NAMESPACE = 'stdin-development-registrations'
FALSE_FLAGS = ('formal_registration', 'calibration_opened', 'holdout_opened', 'formal_certification', 'production_authorized')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NAMESPACE', 'FALSE_FLAGS'))
    originals.loaded_check()
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))


def marker_path(pack):
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).absolute()
    return safe_directory(root, NAMESPACE) / (pack['cohort']['artifact_hash'] + '.json')


def safe_directory(root, *parts, create=False):
    """Fixed namespaces must stay in the actual root before any output write."""
    root = Path(root).absolute()
    packet.require(root.is_dir() and not root.is_symlink() and not root.is_junction(), 'shared root link forbidden')
    current = root.resolve(strict=True)
    for part in parts:
        packet.require(type(part) is str and part not in ('', '.', '..') and Path(part).name == part,
                       'fixed namespace component required')
        current = current / part
        packet.require(not current.is_symlink() and not current.is_junction()
                       and current.resolve().is_relative_to(root.resolve(strict=True)), 'shared namespace link or escape forbidden')
        if create:
            current.mkdir(exist_ok=True)
        packet.require(not current.exists() or current.is_dir(), 'shared namespace must be a directory')
    return current


def value(pack, baseline, finished, raw_budget):
    eligible = pack['requests']
    packet.require(eligible and len(eligible) <= 8, 'at least one original binary development group required')
    return cohort.seal({'schema': SCHEMA, 'dataset_file': pack['dataset_file'],
        'dataset_sha256': cohort.sha(pack['dataset_file']), 'dataset_hash': pack['dataset']['artifact_hash'],
        'cohort_file': pack['dataset']['cohort_file'], 'cohort_sha256': pack['dataset']['cohort_sha256'],
        'cohort_hash': pack['cohort']['artifact_hash'], 'main_executable': pack['main_executable'],
        'main_executable_sha256': cohort.sha(pack['main_executable']),
        'prepared_directories': [str(path) for path in pack['directories']], 'bindings': pack['bindings'],
        'original_evidence': pack['evidence'], 'cases': pack['dataset']['cases'][:8],
        'whole_original_allocation': 808, 'unopened_groups': 800,
        'eligible_ids': [row['id'] for row in eligible], 'request_hashes': [row['request_hash'] for row in eligible],
        'authorization_file': pack['cohort']['authorization_file'], 'authorization_sha256': pack['chain']['current_sha256'],
        'registration_owner_sha256': cohort.sha(__file__),
        'ledger_snapshot': baseline, 'original_finished': finished, 'original_reservation_bindings': raw_budget,
        'registration_before_jev': True, 'scores_hidden': True, 'jev_probabilities_read': False,
        'development_only': True, 'certified_threshold': None, 'new_paid_calls': 0,
        **{key: False for key in FALSE_FLAGS}})


def _locked_close(pack, connection):
    """Called under the same exclusive Jev lock and a held main SQL transaction."""
    cohort.check(pack['bindings'])
    packet.require(budget.same(originals.binding_images(pack['chain']), pack['budget_bindings'])
                   and budget._ledger_rows(Path(pack['current']['main_ledger_file'])) == pack['sql_image'],
                   'raw source/accounting changed between proof and exclusive marker')
    metadata = Path(pack['chain']['authorization']['jev']['ledger_dir']) / 'authorization.json'
    expected = {'authorization_sha256': pack['chain']['ledger_authorization_sha256'], 'kind': 'jev'}
    packet.require(metadata.is_file() and budget.same(budget.load(metadata), expected), 'actual shared Jev metadata changed')
    packet.require(budget.same(originals.development_review.evidence(pack['directories']), pack['evidence']),
                   'original directory bytes changed under registration lock')
    # transaction exists solely to hold BEGIN IMMEDIATE, never modifies main rows.
    packet.require(connection.in_transaction, 'main history transaction missing')


def register(dataset_file, directories, executable, output_file=None):
    loaded_check()
    pack = originals.proof(dataset_file, directories, executable)
    packet.require(not pack['new_reservations'], 'blind registration must precede every allocated Jev reservation')
    target = marker_path(pack)
    packet.require(output_file is None or Path(output_file).resolve() == target, 'registration must use fixed shared namespace')
    body = value(pack, pack['current'], pack['finished'], pack['budget_bindings'])
    root = target.parent.parent
    descriptor = os.open(root / 'reservation.lock', os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.fsync(descriptor)
        with sqlite3.connect(pack['current']['main_ledger_file']) as connection:
            connection.execute('BEGIN IMMEDIATE')
            _locked_close(pack, connection)
            safe_directory(root, NAMESPACE, create=True)
            paid_budget.durable_new(target, body)
            _locked_close(pack, connection)
    finally:
        os.close(descriptor)
        (root / 'reservation.lock').unlink()
    return {'file': str(target), 'sha256': cohort.sha(target), 'registration': body}


def revalidate(path):
    loaded_check()
    path = Path(path).resolve(strict=True)
    raw_sha = cohort.sha(path)
    registered = cohort.sealed(budget.load(path))
    packet.require(registered.get('schema') == SCHEMA and registered.get('registration_before_jev') is True
                   and registered.get('scores_hidden') is True and registered.get('jev_probabilities_read') is False
                   and registered.get('development_only') is True and registered.get('certified_threshold') is None
                   and type(registered.get('new_paid_calls')) is int and registered['new_paid_calls'] == 0
                   and all(registered.get(key) is False for key in FALSE_FLAGS), 'genuine blind development registration required')
    pack = originals.proof(registered['dataset_file'], registered['prepared_directories'], registered['main_executable'])
    packet.require(path == marker_path(pack), 'caller registration is not the actual shared fixed marker')
    baseline, complete = registered['ledger_snapshot'], registered['original_finished']
    packet.require(budget.same(baseline['authorization_chain'], pack['chain'])
                   and budget.same(baseline['main_started'], pack['current']['main_started'])
                   and budget.same(complete, pack['finished'])
                   and budget.same(baseline['main_reservations'], pack['current']['main_reservations'])
                   and budget.same(baseline['main_metadata'], pack['current']['main_metadata'])
                   and budget.same(baseline['jev_reservations'], pack['cohort']['ledger_snapshot']['jev_reservations']),
                   'registration original main/JeV baseline is not genuine pre-score history')
    packet.require(budget.same(value(pack, baseline, complete, registered['original_reservation_bindings']), registered),
                   'registered original labels/bytes/allocation/body changed')
    cohort.check(registered['original_reservation_bindings'])
    for item in pack['new_reservations']:
        packet.require(item.get('registration_file') == str(path) and item.get('registration_sha256') == raw_sha
                       and item.get('registration_hash') == registered['artifact_hash'], 'actual reservation lacks original blind registration binding')
    packet.require(cohort.sha(path) == raw_sha, 'registration raw bytes changed during proof')
    originals.close(pack)
    pack['registration'] = registered
    pack['registration_file'] = str(path)
    pack['registration_sha256'] = raw_sha
    return pack
