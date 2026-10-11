"""Exclusive original blind calibration registration before the first score.

This owner writes no reservation and sends no HTTP. Its before-first-reservation
reader deliberately stops once calibration Jev starts; a later sender requires
its independent original archive and actual append-prefix owner.
"""
import hashlib
import os
import sqlite3
import sys
from pathlib import Path

from .. import paid_budget
from . import (
    budget,
    cohort,
    development_originals,
    development_registration,
    development_review,
    packet,
)
from . import calibration_terminal as terminal

SCHEMA = 'buglecat.verification.stdin-blind-calibration-registration.v1'
NAMESPACE = 'stdin-calibration-registrations'
FALSE_FLAGS = ('holdout_opened', 'formal_certification', 'production_authorized',
               'native_claim_certification', 'overall_agent_benefit_certification')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NAMESPACE', 'FALSE_FLAGS'))
    terminal.loaded_check()
    development_registration.loaded_check()
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))


def marker_path(pack):
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).absolute()
    return development_registration.safe_directory(root, NAMESPACE) / (pack['cohort']['artifact_hash'] + '.json')


def _requests(joined):
    """Build original full binary packets from the same fresh source proof."""
    pack, data = joined['proof']['pack'], joined['proof']['dataset']
    rows = [row for row in data['cases'] if row['split'] == 'calibration' and row['call_eligible']]
    packet.require(joined['report']['independent_quotas_met'] is True and 200 <= len(rows) <= 400,
                   'whole independent calibration quotas required before blind registration')
    # Keep the unopened holdout completely funded even if some calibration
    # outcomes are Unknown and do not need a Jev score.
    remaining_jev = pack['chain']['authorization']['jev']['max_attempts'] - len(pack['current']['jev_reservations'])
    packet.require(pack['current']['remaining_main_attempts'] >= 400 and remaining_jev >= len(rows) + 400,
                   'complete fixed calibration and unopened holdout must remain funded')
    result = []
    for row in rows:
        task = pack['source_pack']['tasks'][row['task_id']]
        code = row['candidate_code_span']['code']
        packet.require(type(code) is str and hashlib.sha256(code.encode('utf-8')).hexdigest()
                       == row['candidate_code_sha256'], 'original complete candidate code span changed')
        request = packet.request(task['question'], task['starter_code'], code, task['input_output'],
                                 pack['cohort']['oracle_runtime_binding'])
        result.append({**{key: row[key] for key in development_review.IDENTITY}, 'label': row['label'],
                       'candidate_code_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
                       'request': request, 'request_hash': budget.digest(request),
                       'request_json': packet.canonical_bytes(request).decode('utf-8')})
    return result


def _bindings(joined):
    return cohort.source_authority._merge_bindings(joined['locked'], joined['proof']['pack']['bindings'],
                                                 joined['registration_owner_bindings'])


def value(joined, requests):
    """Body only; caller-generated bodies grant no actual registration."""
    pack, data, report = joined['proof']['pack'], joined['proof']['dataset'], joined['report']
    return cohort.seal({'schema': SCHEMA, 'dataset_file': report['dataset_file'],
        'dataset_sha256': report['dataset_sha256'], 'dataset_hash': data['artifact_hash'],
        'cohort_file': data['cohort_file'], 'cohort_sha256': data['cohort_sha256'], 'cohort_hash': data['cohort_hash'],
        'opening_file': data['opening_file'], 'opening_sha256': data['opening_sha256'], 'opening_hash': data['opening_hash'],
        'collector_executable_file': data['collector_executable_file'], 'collector_build_info': data['build_info'],
        'prepared_directories': data['prepared_directories'], 'original_calibration_evidence': joined['proof']['evidence'],
        'bindings': _bindings(joined), 'terminal_report': report,
        'cases': [row for row in data['cases'] if row['split'] == 'calibration'],
        'whole_original_allocation': 808, 'calibration_groups': 400, 'holdout_groups_unopened': 400,
        'eligible_ids': [row['id'] for row in requests], 'request_hashes': [row['request_hash'] for row in requests],
        'ledger_snapshot': pack['current'], 'original_finished': pack['finished'],
        'original_sql_image': pack['sql_image'], 'original_reservation_bindings': pack['budget_bindings'],
        'authorization_file': pack['cohort']['authorization_file'], 'authorization_sha256': pack['chain']['current_sha256'],
        'registration_owner_sha256': cohort.sha(__file__), 'registration_before_jev': True,
        'scores_hidden': True, 'jev_probabilities_read': False, 'calibration_only': True,
        'formal_registration': True, 'certified_threshold': None, 'new_paid_calls': 0,
        **{key: False for key in FALSE_FLAGS}})


def _locked_close(joined, connection):
    """Compare authenticated actual bytes under our own live reservation lock."""
    pack = joined['proof']['pack']
    cohort.check(_bindings(joined))
    packet.require(connection.in_transaction and
                   budget.same(development_originals.binding_images(pack['chain']), pack['budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(pack['current']['main_ledger_file'])), pack['sql_image']),
                   'original shared accounting changed before blind calibration marker')
    packet.require(budget.same(development_review.evidence(joined['proof']['directories']), joined['proof']['evidence'])
                   and budget.same(development_review.evidence(pack['archive']['main_directories']), pack['archive']['main_evidence'])
                   and budget.same(development_review.evidence(pack['archive']['jev_directories']), pack['archive']['jev_evidence']),
                   'original calibration or development archive changed under lock')
    loaded_check()


def register(dataset_file, output_file=None):
    """Create exactly one marker after fresh independent full400 labels.

    Existing or stranded markers block repetition. All fees, original failures
    and pending attempts remain untouched; the temporary lock is this call's.
    """
    loaded_check()
    owner_bindings = budget.raw_bindings([__file__])
    joined = terminal.proof(dataset_file)
    joined['registration_owner_bindings'] = owner_bindings
    requests = _requests(joined)
    pack = joined['proof']['pack']
    target = marker_path(pack)
    packet.require(output_file is None or Path(output_file).absolute() == target,
                   'blind calibration marker must use fixed original shared namespace')
    body = value(joined, requests)
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.fsync(descriptor)
        with sqlite3.connect(pack['current']['main_ledger_file']) as connection:
            connection.execute('BEGIN IMMEDIATE')
            _locked_close(joined, connection)
            development_registration.safe_directory(root, NAMESPACE, create=True)
            packet.require(marker_path(pack) == target, 'registration namespace changed under lock')
            paid_budget.durable_new(target, body)
            _locked_close(joined, connection)
    finally:
        os.close(descriptor)
        lock.unlink()
    return {'file': str(target), 'sha256': cohort.sha(target), 'registration': body}


def before_first_reservation(registration_file):
    """Fresh original proof before any calibration Jev slot, never after it."""
    loaded_check()
    owner_bindings = budget.raw_bindings([__file__])
    path = Path(registration_file).absolute()
    raw_sha = cohort.sha(path)
    registered = cohort.sealed(budget.load(path))
    packet.require(registered.get('schema') == SCHEMA, 'genuine calibration blind registration required')
    joined = terminal.proof(registered['dataset_file'])
    joined['registration_owner_bindings'] = owner_bindings
    requests = _requests(joined)
    packet.require(path == marker_path(joined['proof']['pack'])
                   and budget.same(registered, value(joined, requests)) and cohort.sha(path) == raw_sha,
                   'registered labels and original blind baseline changed')
    terminal._close(joined['proof'], joined['locked'])
    cohort.check(_bindings(joined))
    return {'registration': registered, 'registration_file': str(path), 'registration_sha256': raw_sha,
            'terminal_proof': joined, 'requests': requests}
