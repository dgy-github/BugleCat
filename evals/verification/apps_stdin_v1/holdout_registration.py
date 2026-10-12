"""Register original complete holdout labels before any holdout Jev score.

The threshold comes from the once-opened holdout, never from these outcomes.
Registration consumes no model attempt and does not certify production.
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
    formal_archive,
    holdout_terminal,
    packet,
    protocol,
)

SCHEMA = 'buglecat.verification.stdin-blind-holdout-registration.v1'
NAMESPACE = 'stdin-holdout-registrations'
FALSE_FLAGS = ('formal_certification', 'production_authorized',
               'native_claim_certification', 'overall_agent_benefit_certification')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NAMESPACE', 'FALSE_FLAGS'))
    holdout_terminal.loaded_check()
    development_registration.loaded_check()
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))


def marker_path(pack):
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).absolute()
    return development_registration.safe_directory(root, NAMESPACE) / (pack['cohort']['artifact_hash'] + '.json')


def _requests(joined):
    pack, data = joined['proof']['pack'], joined['proof']['dataset']
    rows = [row for row in data['cases'] if row['split'] == 'holdout' and row['call_eligible']]
    threshold = pack['opening']['qualified_calibration_threshold']
    packet.require(joined['report']['independent_quotas_met'] is True and 200 <= len(rows) <= 400
                   and type(threshold) in (int, float) and threshold in protocol.GRID
                   and budget.same(data['qualified_calibration_threshold'], threshold),
                   'whole independent holdout quotas and original fixed threshold required')
    remaining = pack['chain']['authorization']['jev']['max_attempts'] - len(pack['current']['jev_reservations'])
    packet.require(remaining >= len(rows), 'all fixed binary holdout requests must remain funded')
    context = formal_archive.packet_context(pack['cohort']['oracle_runtime_binding'])
    result = []
    for row in rows:
        task = pack['source_pack']['tasks'][row['task_id']]
        code = row['candidate_code_span']['code']
        packet.require(type(code) is str and hashlib.sha256(code.encode('utf-8')).hexdigest() == row['candidate_code_sha256'],
                       'original complete holdout candidate code span changed')
        request = formal_archive._packet(task['question'], task['starter_code'], code, task['input_output'], context)
        result.append({**{key: row[key] for key in development_review.IDENTITY}, 'label': row['label'],
            'candidate_code_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
            'request': request, 'request_hash': budget.digest(request),
            'request_json': packet.canonical_bytes(request).decode('utf-8')})
    formal_archive.close_context(context)
    return result


def _bindings(joined):
    return cohort.source_authority._merge_bindings(joined['locked'], joined['proof']['pack']['proof_bindings'],
                                                 joined['registration_owner_bindings'])


def value(joined, requests):
    """Exact historical body rendering, never standalone registration authority."""
    pack, data, report = joined['proof']['pack'], joined['proof']['dataset'], joined['report']
    return cohort.seal({'schema': SCHEMA,
        **{key: data[key] for key in ('cohort_file', 'cohort_sha256', 'cohort_hash', 'opening_file',
            'opening_sha256', 'opening_hash', 'collector_executable_file', 'prepared_directories',
            'qualified_calibration_threshold')},
        'dataset_file': report['dataset_file'], 'dataset_sha256': report['dataset_sha256'],
        'dataset_hash': data['artifact_hash'], 'collector_build_info': data['build_info'],
        'original_holdout_evidence': joined['proof']['evidence'],
        'original_historical_directory_images': pack['directory_images'],
        'bindings': _bindings(joined), 'terminal_report': report,
        'cases': [row for row in data['cases'] if row['split'] == 'holdout'],
        'whole_original_allocation': 808, 'holdout_groups': 400,
        'eligible_ids': [row['id'] for row in requests], 'request_hashes': [row['request_hash'] for row in requests],
        'ledger_snapshot': pack['current'], 'original_finished': pack['finished'],
        'original_sql_image': pack['sql_image'], 'original_reservation_bindings': pack['budget_bindings'],
        'authorization_file': pack['cohort']['authorization_file'], 'authorization_sha256': pack['chain']['current_sha256'],
        'registration_owner_sha256': cohort.sha(__file__), 'registration_before_jev': True,
        'holdout_scores_hidden': True, 'holdout_jev_probabilities_read': False,
        'holdout_opened': True, 'holdout_only': True, 'formal_registration': True,
        'threshold_tuning': False, 'certified_threshold': None, 'new_paid_calls': 0,
        **{key: False for key in FALSE_FLAGS}})


def _locked_close(joined, connection):
    pack = joined['proof']['pack']
    cohort.check(_bindings(joined))
    packet.require(connection.in_transaction
                   and budget.same(development_originals.binding_images(pack['chain']), pack['budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(pack['current']['main_ledger_file'])), pack['sql_image']),
                   'whole actual accounting changed before blind holdout marker')
    for directories, evidence in pack['directory_images']:
        packet.require(budget.same(development_review.evidence(directories), evidence),
                       'original historical archive changed under holdout registration lock')
    packet.require(budget.same(development_review.evidence(joined['proof']['directories']), joined['proof']['evidence']),
                   'original holdout archive changed under registration lock')
    loaded_check()


def register(dataset_file, output_file=None):
    """Create-new once in the real shared namespace; no reservation or request."""
    loaded_check()
    owner_bindings = budget.raw_bindings([__file__])
    joined = holdout_terminal.proof(dataset_file)
    joined['registration_owner_bindings'] = owner_bindings
    requests = _requests(joined)
    pack = joined['proof']['pack']
    target = marker_path(pack)
    packet.require(output_file is None or Path(output_file).absolute() == target,
                   'holdout registration must use fixed original namespace')
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
            packet.require(marker_path(pack) == target, 'holdout registration namespace changed under lock')
            paid_budget.durable_new(target, body)
            _locked_close(joined, connection)
    finally:
        os.close(descriptor)
        lock.unlink()
    return {'file': str(target), 'sha256': cohort.sha(target), 'registration': body}


def before_first_reservation(registration_file):
    """Pre-holdout-score only; later real prefixes require their own archive."""
    loaded_check()
    path = Path(registration_file).absolute()
    raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'original blind holdout registration required')
    joined = holdout_terminal.proof(saved['dataset_file'])
    joined['registration_owner_bindings'] = budget.raw_bindings([__file__])
    requests = _requests(joined)
    packet.require(path == marker_path(joined['proof']['pack'])
                   and budget.same(saved, value(joined, requests)) and cohort.sha(path) == raw,
                   'original holdout registration differs from fresh independent labels')
    holdout_terminal._close(joined['proof'], joined['locked'])
    cohort.check(_bindings(joined))
    return {'registration': saved, 'registration_file': str(path), 'registration_sha256': raw,
            'terminal_proof': joined, 'requests': requests}
