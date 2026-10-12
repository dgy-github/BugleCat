"""Independent labels for the entire fixed once-opened holdout allocation.

Historical development/calibration outcomes are retained unchanged. The pure
policy cannot register labels; actual authority needs a fresh holdout intake.
"""
import sys
from collections import Counter
from pathlib import Path

from . import budget, calibration_terminal, cohort, collection, development_review, packet, protocol

SCHEMA = 'buglecat.verification.stdin-holdout-main-terminal.v1'
NO_AUTHORITY = ('formal_registration', 'formal_certification', 'production_authorized',
                'native_claim_certification', 'overall_agent_benefit_certification')


def _intake():
    from . import holdout_intake
    return holdout_intake


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NO_AUTHORITY'))
    calibration_terminal.loaded_check()
    _intake().loaded_check()
    protocol.dependencies()


def _decision(original_cases, outcomes, original_calibration, spec):
    """Pure completeness/quota policy; historical counts cannot fill holdout."""
    prior = calibration_terminal._decision(original_cases, original_calibration, spec)
    packet.require(prior['independent_quotas_met'] is True
                   and type(outcomes) is list and len(outcomes) == 808,
                   'complete original calibration and fixed808 allocation required')
    selected = []
    for fixed, row, historical in zip(original_cases, outcomes, original_calibration, strict=True):
        calibration_terminal._case(fixed, row)
        if fixed['split'] == 'holdout':
            selected.append(row)
        else:
            packet.require(budget.same(row, historical), 'original development/calibration outcomes changed')
    counts = {label: sum(row['label'] == label for row in selected) for label in calibration_terminal.LABELS}
    pending = sum(row['main_status'] == 'main_not_finished' for row in selected)
    unstarted = sum(row['main_status'] == 'main_not_started' for row in selected)
    terminal = 400 - pending - unstarted
    unresolved = sum(row['reason'] in calibration_terminal.ORACLE_PENDING for row in selected)
    whole = terminal == 400
    ready = whole and unresolved == 0
    quota = ready and counts['supported'] >= spec['min_supported'] and counts['unsupported'] >= spec['min_unsupported']
    status = ('holdout_main_incomplete' if not whole else 'independent_labels_incomplete' if not ready
              else 'ready_for_original_holdout_registration' if quota else 'independent_quotas_not_met')
    return {'status': status, 'allocated_groups': 400, 'terminal_groups': terminal,
        'pending_groups': pending, 'uncollected_groups': unstarted, 'label_counts': counts,
        'main_status_counts': dict(Counter(row['main_status'] for row in selected)),
        'whole_allocation_terminal': whole, 'unresolved_oracle_groups': unresolved,
        'labels_ready': ready, 'independent_quotas_met': quota,
        'min_supported': spec['min_supported'], 'min_unsupported': spec['min_unsupported'],
        'missing_supported': max(0, spec['min_supported'] - counts['supported']),
        'missing_unsupported': max(0, spec['min_unsupported'] - counts['unsupported']),
        'development_or_calibration_labels_counted_in_holdout_quotas': 0,
        'whole_allocation_retained': True, 'unknown_is_not_negative': True,
        'holdout_opened': True, 'threshold_tuning': False, 'automatic_retries': 0,
        'certified_threshold': None, 'verified_pass_signals': 0,
        **{key: False for key in NO_AUTHORITY}}


def _dataset_counts(data, decision):
    expected = {'allocated_groups': 808, 'holdout_allocated_groups': 400,
        'holdout_terminal_groups': decision['terminal_groups'],
        'holdout_pending_groups': decision['pending_groups'],
        'holdout_uncollected_groups': decision['uncollected_groups'],
        'holdout_collected_groups': 400 - decision['uncollected_groups'],
        'unresolved_oracle_groups': decision['unresolved_oracle_groups'], 'labels_ready': decision['labels_ready'],
        'label_counts': {label: count for label, count in decision['label_counts'].items() if count}}
    packet.require(all(budget.same(data.get(key), value) for key, value in expected.items()),
                   'saved holdout counts differ from complete original outcomes')


def _close(joined, locked):
    cohort.check(locked)
    _intake().originals.close(joined['pack'])
    evidence = development_review.evidence(joined['directories']) if joined['directories'] else []
    packet.require(budget.same(evidence, joined['evidence']), 'original holdout file set or bytes changed')
    loaded_check()


def proof(dataset_file):
    """Fresh original holdout intake; read only, no Oracle execution or slots."""
    loaded_check()
    intake = _intake()
    path = Path(dataset_file).resolve(strict=True)
    raw = cohort.sha(path)
    data = cohort.sealed(budget.load(path))
    packet.require(data.get('schema') == intake.SCHEMA and data.get('holdout_scores_hidden') is True
                   and data.get('holdout_jev_probabilities_read') is False
                   and data.get('holdout_opened') is True and data.get('certified_threshold') is None
                   and all(data.get(key) is False for key in NO_AUTHORITY),
                   'original prediction-blind holdout intake required')
    locked = budget.raw_bindings([path, __file__, data['collector_executable_file'], *intake.runner_sources().values()])
    packet.require(any(row['file'] == str(path) and row['sha256'] == raw for row in locked), 'dataset changed before intake')
    joined = intake.proof(data['opening_file'], data['prepared_directories'], data['collector_executable_file'],
                          execute_new_oracles=False)
    packet.require(budget.same(joined['dataset'], data), 'saved holdout dataset differs from original intake')
    pack = joined['pack']
    fixed = [{key: row[key] for key in collection.history_fields()} for row in pack['cohort']['cases']]
    decision = _decision(fixed, data['cases'], pack['calibration_dataset']['cases'], protocol.specification())
    _dataset_counts(data, decision)
    packet.require(data['cohort_hash'] == pack['cohort']['artifact_hash']
                   and data['opening_sha256'] == pack['holdout_opening_sha256']
                   and data['opening_hash'] == pack['holdout_opening_hash']
                   and budget.same(data['qualified_calibration_threshold'], pack['opening']['qualified_calibration_threshold']),
                   'original fixed threshold, source or once opening changed')
    image = {'snapshot': pack['current'], 'finished': pack['finished'], 'sql_image': pack['sql_image'],
             'raw_budget_bindings': pack['budget_bindings']}
    body = {'schema': SCHEMA, **decision,
        **{key: data[key] for key in ('cohort_file', 'cohort_sha256', 'cohort_hash', 'opening_file',
            'opening_sha256', 'opening_hash', 'collector_executable_file', 'prepared_directories',
            'qualified_calibration_threshold')},
        'dataset_file': str(path), 'dataset_sha256': raw, 'dataset_hash': data['artifact_hash'],
        'collector_build_info': data['build_info'], 'original_holdout_evidence': joined['evidence'],
        'original_source_bindings': pack['proof_bindings'], 'whole_original_accounting_digest': budget.digest(image),
        'current_main_count': len(pack['current']['main_started']),
        'current_phase_jev_count': len(pack['current']['jev_reservations']),
        'historical_main_pending': sum(row is None for row in pack['finished'].values()),
        'protocol_hash': budget.digest(protocol.specification()), 'terminal_owner_sha256': cohort.sha(__file__),
        'holdout_scores_hidden': True, 'holdout_jev_probabilities_read': False,
        'report_model_calls': 0, 'report_jev_calls': 0, 'report_oracle_executions': 0,
        'report_reservations_or_phase_markers': 0, 'overall_goal_complete': False,
        'scope': 'Original restricted STDIN holdout labels; fixed calibration threshold is not yet certified'}
    _close(joined, locked)
    return {'proof': joined, 'report': cohort.seal(body), 'locked': locked, 'image': image}


def value(dataset_file):
    return proof(dataset_file)['report']


def revalidate(report_file):
    path = Path(report_file).resolve(strict=True)
    raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'original holdout terminal report required')
    actual = value(saved['dataset_file'])
    packet.require(budget.same(saved, actual) and cohort.sha(path) == raw, 'saved holdout report changed')
    return actual
