"""Read-only qualified calibration decision from fresh original Jev bytes.

The report is not a permission or final certificate. The independent holdout
opening must rebuild it from the same original archive and current accounting.
"""
import sys
from collections import Counter
from pathlib import Path

from . import budget, calibration_terminal, cohort, collection, packet, protocol
from . import formal_observations as observer

SCHEMA = 'buglecat.verification.stdin-formal-calibration-decision.v1'
FALSE_FLAGS = ('formal_certification', 'production_authorized', 'holdout_opened',
               'native_claim_certification', 'overall_agent_benefit_certification')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'FALSE_FLAGS'))
    calibration_terminal.loaded_check()
    observer.loaded_check()
    protocol.dependencies()


def _decision(campaign, dataset, registered, requests, observations, spec):
    """Pure policy only; synthetic callers cannot obtain phase authority."""
    fixed = [{key: case[key] for key in collection.history_fields()} for case in campaign['cases']]
    labels = calibration_terminal._decision(fixed, dataset['cases'], spec)
    calibration_terminal._dataset_counts(dataset, labels)
    packet.require(labels['independent_quotas_met'] is True, 'original whole400 binary quotas required')
    rows = [row for row in dataset['cases'] if row['split'] == 'calibration' and row['call_eligible']]
    packet.require(type(requests) is list and type(observations) is list
                   and [row['id'] for row in requests] == [row['id'] for row in rows] == registered['eligible_ids']
                   and [row['request_hash'] for row in requests] == registered['request_hashes'],
                   'original registered ordered binary requests required')
    cases = []
    for row, request in zip(rows, requests, strict=True):
        packet.require(all(budget.same(request.get(key), row[key]) for key in
                           (*collection.history_fields(), 'label', 'candidate_code_sha256', 'oracle_sha256')),
                       'original independent label/request identity changed')
        cases.append({**row, 'candidate_sha256': row['candidate_code_sha256']})
    by_id = {case['id']: case for case in cases}
    seen = set()
    for row in observations:
        packet.require(type(row) is dict and row.get('id') in by_id and row['id'] not in seen
                       and protocol.terminal_status(row.get('call_status')),
                       'one original terminal observation per registered binary group required')
        seen.add(row['id'])
        case = by_id[row['id']]
        packet.require(row.get('group_id') == case['group_id']
                       and row.get('candidate_sha256') == case['candidate_sha256']
                       and row.get('oracle_sha256') == case['oracle_sha256'],
                       'terminal observation differs from original independent candidate')
    complete = len(seen) == len(cases)
    # Scores, including a seemingly perfect partial prefix, do not authorize
    # early stopping, threshold choice or opening the holdout.
    numeric = protocol.calibrate_numbers(cases, observations, spec) if complete else None
    met = numeric is not None and numeric['calibration_numeric_gate_met'] is True
    return {
        'status': 'qualified_calibration' if met else 'calibration_gate_not_met' if complete else 'formal_jev_incomplete',
        'calibration_groups': 400, 'label_counts': labels['label_counts'],
        'binary_groups': len(cases), 'terminal_binary_groups': len(seen),
        'unresolved_binary_groups': len(cases) - len(seen),
        'all_registered_binary_terminal': complete, 'calibration_gate_met': met,
        'qualified_calibration_threshold': numeric['qualified_numeric_threshold'] if numeric else None,
        'numeric_results': numeric, 'jev_status_counts': dict(Counter(row['call_status'] for row in observations)),
        'failed_jev_attempts': sum(row['call_status'] != 'ok' for row in observations),
        'failed_jev_attempts_retained_in_denominators': True,
        'development_groups_counted_in_formal_quotas': 0, 'holdout_groups_unopened': 400,
        'unknown_is_not_negative': True, 'automatic_retries': 0, 'verified_pass_signals': 0,
        'certified_threshold': None, **{key: False for key in FALSE_FLAGS}}


def _close(joined, bindings):
    cohort.check(bindings)
    observer.close(joined)
    loaded_check()


def proof(registration_file, directories, executable):
    """One fresh original source/raw observer proof; no calls, Oracles or writes."""
    loaded_check()
    owned = budget.raw_bindings([__file__, executable])
    joined = observer.proof(registration_file, directories, executable)
    pack = joined['pack']
    decision = _decision(pack['cohort'], pack['dataset'], pack['registration'],
                         pack['requests'], joined['observations'], protocol.specification())
    bindings = cohort.source_authority._merge_bindings(owned, joined['bindings'], pack['proof_bindings'])
    image = {'snapshot': pack['current'], 'finished': pack['finished'],
             'sql_image': pack['sql_image'], 'raw_budget_bindings': pack['budget_bindings']}
    body = {'schema': SCHEMA, **decision,
        'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
        'registration_hash': pack['registration']['artifact_hash'],
        'cohort_hash': pack['cohort']['artifact_hash'],
        'sender_executable_file': str(Path(executable).resolve(strict=True)),
        'sender_executable_sha256': cohort.sha(executable),
        'prepared_directories': [str(Path(path).resolve(strict=True)) for path in joined['directories']],
        'original_observations_hash': budget.digest(joined['observations']),
        'original_sender_evidence': joined['evidence'], 'original_source_bindings': bindings,
        'whole_original_accounting_digest': budget.digest(image),
        'actual_main_count': len(pack['current']['main_started']),
        'actual_phase_jev_count': len(pack['current']['jev_reservations']),
        'historical_pending_main': sum(value is None for value in pack['finished'].values()),
        'protocol_hash': budget.digest(protocol.specification()), 'decision_owner_sha256': cohort.sha(__file__),
        'report_model_calls': 0, 'report_jev_calls': 0, 'report_oracle_executions': 0,
        'report_reservations_or_phase_markers': 0, 'overall_goal_complete': False,
        'scope': 'Qualified calibration for restricted original STDIN pairs; fresh once-only holdout remains required'}
    _close(joined, bindings)
    return {'proof': joined, 'report': cohort.seal(body), 'bindings': bindings, 'image': image}


def value(registration_file, directories, executable):
    return proof(registration_file, directories, executable)['report']


def revalidate(report_file):
    """Saving or modifying a report cannot create a qualified calibration."""
    path = Path(report_file).resolve(strict=True)
    raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'original calibration decision report required')
    joined = proof(saved['registration_file'], saved['prepared_directories'], saved['sender_executable_file'])
    packet.require(budget.same(saved, joined['report']) and cohort.sha(path) == raw,
                   'saved calibration decision differs from fresh original evidence')
    return joined['report']
