"""One final holdout evaluation using the already frozen calibration threshold.

Only the original scoped ordered-pair benchmark may receive a certificate.
Production, native claims and overall agent benefit remain separate.
"""
import os
import sqlite3
import sys
from collections import Counter
from pathlib import Path

from .. import paid_budget
from . import (
    budget,
    cohort,
    collection,
    development_originals,
    development_registration,
    development_review,
    holdout_terminal,
    packet,
    protocol,
)

SCHEMA = 'buglecat.verification.stdin-once-holdout-evaluation.v1'
NAMESPACE = 'stdin-holdout-evaluations'
FALSE_FLAGS = ('formal_certification', 'production_authorized',
               'native_claim_certification', 'overall_agent_benefit_certification')


def _observer():
    from . import holdout_observations
    return holdout_observations


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NAMESPACE', 'FALSE_FLAGS'))
    _observer().loaded_check()
    holdout_terminal.loaded_check()
    protocol.dependencies()
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))


def _decision(campaign, dataset, original_calibration, registered, requests, observations, threshold, spec):
    """Numbers and completeness only; never select a threshold on holdout."""
    fixed = [{key: case[key] for key in collection.history_fields()} for case in campaign['cases']]
    labels = holdout_terminal._decision(fixed, dataset['cases'], original_calibration['cases'], spec)
    holdout_terminal._dataset_counts(dataset, labels)
    packet.require(labels['independent_quotas_met'] is True
                   and type(threshold) in (int, float) and threshold in protocol.GRID
                   and budget.same(registered['qualified_calibration_threshold'], threshold)
                   and budget.same(dataset['qualified_calibration_threshold'], threshold),
                   'complete original holdout quotas and fixed calibrated threshold required')
    rows = [row for row in dataset['cases'] if row['split'] == 'holdout' and row['call_eligible']]
    packet.require(type(requests) is list and type(observations) is list
                   and [row['id'] for row in requests] == [row['id'] for row in rows] == registered['eligible_ids']
                   and [row['request_hash'] for row in requests] == registered['request_hashes'],
                   'original complete ordered holdout binary registration required')
    cases = []
    for row, request in zip(rows, requests, strict=True):
        packet.require(all(budget.same(request.get(key), row[key]) for key in
                           (*collection.history_fields(), 'label', 'candidate_code_sha256', 'oracle_sha256')),
                       'original independent holdout candidate/label request changed')
        cases.append({**row, 'candidate_sha256': row['candidate_code_sha256']})
    by_id = {row['id']: row for row in cases}
    seen = set()
    for row in observations:
        packet.require(type(row) is dict and row.get('id') in by_id and row['id'] not in seen
                       and protocol.terminal_status(row.get('call_status')),
                       'one original terminal Jev outcome per holdout group required')
        case = by_id[row['id']]
        packet.require(row.get('group_id') == case['group_id']
                       and row.get('candidate_sha256') == case['candidate_sha256']
                       and row.get('oracle_sha256') == case['oracle_sha256'],
                       'holdout Jev observation differs from original independent candidate')
        seen.add(row['id'])
    complete = len(seen) == len(cases)
    numeric = protocol.numbers(cases, observations, threshold, spec) if complete else None
    met = numeric is not None and numeric['gate_met'] is True
    return {'status': 'holdout_passed' if met else 'holdout_failed' if complete else 'holdout_jev_incomplete',
        'allocated_groups': 400, 'label_counts': labels['label_counts'],
        'binary_groups': len(cases), 'terminal_binary_groups': len(seen),
        'unresolved_binary_groups': len(cases) - len(seen), 'all_registered_binary_terminal': complete,
        'holdout_numeric_gate_met': met, 'fixed_threshold': threshold, 'threshold_tuning': False,
        'numeric_results': numeric, 'jev_status_counts': dict(Counter(row['call_status'] for row in observations)),
        'failed_jev_attempts': sum(row['call_status'] != 'ok' for row in observations),
        'failed_jev_attempts_retained_in_denominators': True,
        'unknown_is_not_negative': True, 'automatic_retries': 0, 'verified_pass_signals': 0,
        'certified_threshold': None, **{key: False for key in FALSE_FLAGS}}


def _proof(registration_file, directories, executable):
    loaded_check()
    owned = budget.raw_bindings([__file__, executable])
    joined = _observer().proof(registration_file, directories, executable)
    pack = joined['pack']
    decision = _decision(pack['cohort'], pack['dataset'], pack['calibration_dataset'], pack['registration'],
        pack['requests'], joined['observations'], pack['opening']['qualified_calibration_threshold'], protocol.specification())
    packet.require(decision['all_registered_binary_terminal'] is True,
                   'complete original terminal binary holdout required before one final evaluation')
    bindings = cohort.source_authority._merge_bindings(owned, joined['bindings'], pack['proof_bindings'])
    cohort.check(bindings)
    _observer().close(joined)
    return {'proof': joined, 'decision': decision, 'bindings': bindings,
        'executable_file': str(Path(executable).resolve(strict=True)), 'executable_sha256': cohort.sha(executable),
        'image': {'snapshot': pack['current'], 'finished': pack['finished'],
                  'sql_image': pack['sql_image'], 'raw_budget_bindings': pack['budget_bindings']}}


def marker_path(pack):
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).absolute()
    return development_registration.safe_directory(root, NAMESPACE) / (pack['cohort']['artifact_hash'] + '.json')


def _body(joined):
    proof, decision = joined['proof'], joined['decision']
    pack = proof['pack']
    met = decision['holdout_numeric_gate_met']
    return cohort.seal({'schema': SCHEMA, **decision,
        'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
        'registration_hash': pack['registration']['artifact_hash'], 'cohort_hash': pack['cohort']['artifact_hash'],
        'opening_file': pack['holdout_opening_file'], 'opening_sha256': pack['holdout_opening_sha256'],
        'opening_hash': pack['holdout_opening_hash'],
        'sender_executable_file': joined['executable_file'], 'sender_executable_sha256': joined['executable_sha256'],
        'prepared_directories': [str(Path(path).resolve(strict=True)) for path in proof['directories']],
        'original_observations_hash': budget.digest(proof['observations']),
        'original_sender_evidence': proof['evidence'], 'original_source_bindings': joined['bindings'],
        'whole_original_accounting_digest': budget.digest(joined['image']),
        'actual_main_count': len(pack['current']['main_started']),
        'actual_phase_jev_count': len(pack['current']['jev_reservations']),
        'historical_main_pending': sum(row is None for row in pack['finished'].values()),
        'protocol_hash': budget.digest(protocol.specification()), 'evaluation_owner_sha256': cohort.sha(__file__),
        'holdout_evaluation_consumptions': 1, 'source_replacements_allowed': False,
        'STDIN_ordered_pair_artifact_threshold_certified': met,
        'certified_STDIN_ordered_pair_artifact_threshold': decision['fixed_threshold'] if met else None,
        'new_main_calls': 0, 'new_jev_calls': 0, 'new_oracle_executions': 0,
        'overall_goal_complete': False,
        'scope': 'Restricted original STDIN ordered-whitespace-token pairs only; no general correctness or agent benefit'})


def _locked_close(joined, connection):
    proof, image = joined['proof'], joined['image']
    pack = proof['pack']
    cohort.check(joined['bindings'])
    packet.require(connection.in_transaction
                   and budget.same(development_originals.binding_images(pack['chain']), image['raw_budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(image['snapshot']['main_ledger_file'])), image['sql_image']),
                   'actual complete accounting changed before final evaluation')
    for directories, evidence in pack['directory_images']:
        packet.require(budget.same(development_review.evidence(directories), evidence),
                       'original holdout archive changed under evaluation lock')
    packet.require(budget.same(development_review.evidence(proof['directories']), proof['evidence']),
                   'original final sender evidence changed under evaluation lock')
    loaded_check()


def evaluate(registration_file, directories, executable, output_file=None):
    """Create-new final result is also the fixed once-consumption marker."""
    joined = _proof(registration_file, directories, executable)
    pack = joined['proof']['pack']
    target = marker_path(pack)
    packet.require(output_file is None or Path(output_file).absolute() == target,
                   'final evaluation requires fixed original once-consumption namespace')
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.fsync(descriptor)
        with sqlite3.connect(pack['current']['main_ledger_file']) as connection:
            connection.execute('BEGIN IMMEDIATE')
            _locked_close(joined, connection)
            development_registration.safe_directory(root, NAMESPACE, create=True)
            packet.require(marker_path(pack) == target, 'evaluation namespace changed under lock')
            body = _body(joined)
            paid_budget.durable_new(target, body)
            _locked_close(joined, connection)
    finally:
        os.close(descriptor)
        lock.unlink()
    return {'file': str(target), 'sha256': cohort.sha(target), 'evaluation': body}


def revalidate(evaluation_file):
    path = Path(evaluation_file).absolute()
    raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'original once holdout evaluation required')
    joined = _proof(saved['registration_file'], saved['prepared_directories'], saved['sender_executable_file'])
    packet.require(path == marker_path(joined['proof']['pack']) and budget.same(saved, _body(joined))
                   and cohort.sha(path) == raw, 'saved final result differs from original fixed-threshold evidence')
    cohort.check(joined['bindings'])
    _observer().close(joined['proof'])
    return saved
