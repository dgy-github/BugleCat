"""Full fixed-calibration label policy; genuine intake replay is required.

The pure decision grants no registration or transport permission. Development
labels and unopened holdout groups never count toward the calibration quotas.
"""
import sys
from collections import Counter
from pathlib import Path

from . import budget, cohort, collection, development_review, intake, packet, protocol
from . import calibration_intake as calibration

SCHEMA = 'buglecat.verification.stdin-calibration-main-terminal.v1'
LABELS = ('supported', 'unsupported', 'unknown', 'unlabeled')
INCOMPLETE = ('main_not_started', 'main_not_finished')
ORACLE_PENDING = ('independent_oracle_not_recorded', 'oracle_attempt_incomplete')
UNKNOWN_WITHOUT_ORACLE = ('single_unmodified_python_fence_required', 'complete_packet_resource_or_scope_rejected')
NO_AUTHORITY = ('formal_registration', 'holdout_opened', 'formal_certification',
                'production_authorized', 'native_claim_certification', 'overall_agent_benefit_certification')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__),
                            ('SCHEMA', 'LABELS', 'INCOMPLETE', 'ORACLE_PENDING', 'UNKNOWN_WITHOUT_ORACLE', 'NO_AUTHORITY'))
    intake.loaded_check()
    calibration.loaded_check()
    protocol.dependencies()


def _case(original, row):
    """A caller cannot move a development outcome into the fixed calibration."""
    packet.require(type(row) is dict and all(key in row for key in collection.history_fields())
                   and budget.same({key: row[key] for key in collection.history_fields()}, original),
                   'complete original ordered calibration identities required')
    status, label = row.get('main_status'), row.get('label')
    packet.require(label in LABELS and type(row.get('call_eligible')) is bool,
                   'typed independent label eligibility required')
    if status in INCOMPLETE:
        packet.require(label == 'unlabeled' and row['call_eligible'] is False and row.get('reason') == status,
                       'unstarted or pending main cannot have a label')
        return
    packet.require(intake.main_terminal_status(status), 'genuine terminal main status required')
    packet.require(label != 'unlabeled' and row['call_eligible'] is (label in ('supported', 'unsupported')),
                   'terminal labels and exact binary eligibility required')
    if status != 'ok':
        packet.require(label == 'unknown' and row.get('reason') == status,
                       'failed main transport is Unknown, never a negative')
        return
    if label == 'unknown' and row.get('reason') in (*ORACLE_PENDING, *UNKNOWN_WITHOUT_ORACLE):
        packet.require(row['call_eligible'] is False, 'missing or scope-rejected Oracle cannot be binary')
        return
    packet.require(row.get('reason') == {'supported': 'passed', 'unsupported': 'failed', 'unknown': 'unknown'}[label]
                   and protocol._hash(row.get('candidate_code_sha256'))
                   and protocol._hash(row.get('oracle_sha256')),
                   'successful main needs its original candidate and independent Oracle outcome')


def _decision(original_cases, outcomes, spec):
    """Pure completeness and quota summary; no saved result is authority."""
    protocol.validate(spec)
    packet.require(type(original_cases) is list and type(outcomes) is list
                   and len(original_cases) == len(outcomes) == 808,
                   'whole fixed 808 allocation must remain present')
    packet.require(all(type(case) is dict and set(case) == set(collection.history_fields())
                       and case['source_kind'] == protocol.SOURCE_KIND
                       and type(case['task_id']) is int and case['task_id'] >= 0
                       and all(type(case[key]) is str and bool(case[key])
                               for key in collection.history_fields() if key != 'task_id')
                       for case in original_cases)
                   and len({case['id'] for case in original_cases}) == 808
                   and len({case['group_id'] for case in original_cases}) == 808
                   and dict(Counter(case['split'] for case in original_cases))
                       == {'development': 8, 'calibration': 400, 'holdout': 400},
                   'unique original phase allocations required')
    selected = []
    for original, row in zip(original_cases, outcomes, strict=True):
        _case(original, row)
        if original['split'] == 'development':
            packet.require(row['main_status'] not in INCOMPLETE and row['reason'] not in ORACLE_PENDING,
                           'original development must remain complete')
        elif original['split'] == 'holdout':
            packet.require(row['main_status'] == 'main_not_started', 'holdout must remain unopened')
        else:
            selected.append(row)
    counts = {label: sum(row['label'] == label for row in selected) for label in LABELS}
    pending = sum(row['main_status'] == 'main_not_finished' for row in selected)
    unstarted = sum(row['main_status'] == 'main_not_started' for row in selected)
    terminal = 400 - pending - unstarted
    whole_terminal = terminal == 400
    unresolved = sum(row['reason'] in ORACLE_PENDING for row in selected)
    labels_ready = whole_terminal and unresolved == 0
    quota = labels_ready and counts['supported'] >= spec['min_supported'] and counts['unsupported'] >= spec['min_unsupported']
    reason = ('calibration_main_incomplete' if not whole_terminal else
              'independent_labels_incomplete' if not labels_ready else
              'ready_for_original_registration' if quota else 'independent_quotas_not_met')
    return {'status': reason, 'allocated_groups': 400, 'terminal_groups': terminal,
            'pending_groups': pending, 'uncollected_groups': unstarted, 'label_counts': counts,
            'main_status_counts': dict(Counter(row['main_status'] for row in selected)),
            'whole_allocation_terminal': whole_terminal, 'unresolved_oracle_groups': unresolved,
            'labels_ready': labels_ready, 'independent_quotas_met': quota,
            'min_supported': spec['min_supported'], 'min_unsupported': spec['min_unsupported'],
            'missing_supported': max(0, spec['min_supported'] - counts['supported']),
            'missing_unsupported': max(0, spec['min_unsupported'] - counts['unsupported']),
            'development_groups_counted_in_formal_quotas': 0, 'holdout_groups_unopened': 400,
            'whole_allocation_retained': True, 'unknown_is_not_negative': True,
            'automatic_retries': 0, 'certified_threshold': None, 'verified_pass_signals': 0,
            **{key: False for key in NO_AUTHORITY}}


def _dataset_counts(data, decision):
    """Intake projections must match independently counted complete outcomes."""
    expected = {
        'allocated_groups': 808, 'calibration_allocated_groups': 400,
        'calibration_terminal_groups': decision['terminal_groups'],
        'calibration_pending_groups': decision['pending_groups'],
        'calibration_uncollected_groups': decision['uncollected_groups'],
        'calibration_collected_groups': 400 - decision['uncollected_groups'],
        'unresolved_oracle_groups': decision['unresolved_oracle_groups'],
        'labels_ready': decision['labels_ready'],
        'label_counts': {label: count for label, count in decision['label_counts'].items() if count},
    }
    packet.require(all(budget.same(data.get(key), value) for key, value in expected.items()),
                   'saved calibration counts differ from original complete outcomes')


def _close(joined, locked):
    """Close original source/archive/current accounting and all calibration files."""
    cohort.check(locked)
    calibration.originals.close(joined['pack'])
    evidence = development_review.evidence(joined['directories']) if joined['directories'] else []
    packet.require(budget.same(evidence, joined['evidence']),
                   'original calibration file set or bytes changed during terminal review')
    loaded_check()


def proof(dataset_file):
    """Replay genuine intake once, read-only; return ephemeral proof and report.

    Missing/stranded candidate Oracles are reported as incomplete without being
    executed. The returned pack is valid only inside this transaction; it cannot
    serve as a saved permission or source cache for a later request.
    """
    loaded_check()
    path = Path(dataset_file).resolve(strict=True)
    data_sha = cohort.sha(path)
    data = cohort.sealed(budget.load(path))
    packet.require(data.get('schema') == calibration.SCHEMA and data.get('scores_hidden') is True
                   and data.get('jev_probabilities_read') is False and data.get('certified_threshold') is None
                   and all(data.get(key) is False for key in ('formal_registration', 'holdout_opened',
                                                            'formal_certification', 'production_authorized')),
                   'original prediction-blind calibration intake required')
    locked = budget.raw_bindings([path, __file__, data['collector_executable_file'],
                                 *calibration.runner_sources().values()])
    packet.require(any(row['file'] == str(path) and row['sha256'] == data_sha for row in locked),
                   'dataset changed before fresh original intake')
    joined = calibration.proof(data['opening_file'], data['prepared_directories'],
                               data['collector_executable_file'], execute_new_oracles=False)
    packet.require(budget.same(joined['dataset'], data), 'saved dataset differs from fresh original calibration intake')
    pack = joined['pack']
    cases = [{key: case[key] for key in collection.history_fields()} for case in pack['cohort']['cases']]
    decision = _decision(cases, data['cases'], protocol.specification())
    _dataset_counts(data, decision)
    packet.require(data['cohort_hash'] == pack['cohort']['artifact_hash']
                   and data['opening_sha256'] == pack['calibration_opening_sha256']
                   and data['opening_hash'] == pack['calibration_opening_hash'],
                   'original source or opening differs from reconstructed labels')
    body = {'schema': SCHEMA, **decision, 'dataset_file': str(path), 'dataset_sha256': data_sha,
            'dataset_hash': data['artifact_hash'], 'opening_file': data['opening_file'],
            'opening_sha256': data['opening_sha256'], 'opening_hash': data['opening_hash'],
            'cohort_file': data['cohort_file'], 'cohort_sha256': data['cohort_sha256'],
            'cohort_hash': data['cohort_hash'], 'collector_executable_file': data['collector_executable_file'],
            'collector_build_info': data['build_info'], 'prepared_directories': data['prepared_directories'],
            'original_calibration_evidence': joined['evidence'], 'original_source_bindings': pack['bindings'],
            'current_main_count': len(pack['current']['main_started']),
            'current_phase_jev_count': len(pack['current']['jev_reservations']),
            'historical_main_pending': sum(value is None for value in pack['finished'].values()),
            'original_accounting_digest': budget.digest({'snapshot': pack['current'], 'finished': pack['finished'],
                'sql_image': pack['sql_image'], 'raw_budget_bindings': pack['budget_bindings']}),
            'protocol_hash': budget.digest(protocol.specification()), 'terminal_owner_sha256': cohort.sha(__file__),
            'scores_hidden': True, 'jev_probabilities_read': False,
            'report_model_calls': 0, 'report_jev_calls': 0, 'report_oracle_executions': 0,
            'report_reservations': 0, 'overall_goal_complete': False,
            'scope': 'Original restricted STDIN calibration labels only; no threshold or transport authority'}
    _close(joined, locked)
    return {'proof': joined, 'report': cohort.seal(body), 'locked': locked}


def value(dataset_file):
    """Read-only genuine report; saving it grants no phase permission."""
    return proof(dataset_file)['report']


def revalidate(report_file):
    path = Path(report_file).resolve(strict=True)
    raw_sha = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'genuine calibration terminal report required')
    actual = value(saved['dataset_file'])
    packet.require(budget.same(saved, actual) and cohort.sha(path) == raw_sha,
                   'saved report differs from fresh original calibration')
    return actual
