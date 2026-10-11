"""Read-only original main terminal report before Jev registration.

Replays actual full intake without executing an Oracle. Insufficient independent
labels are reported as insufficient, with the entire fixed allocation retained.
This report creates no registration, source-opening marker or fee authority.
"""
from collections import Counter
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from . import (
    approved_budget,
    formal_collection,
    formal_intake,
    formal_registration,
    packet,
    prospective_protocol,
)

SCHEMA = 'buglecat.verification.apps-main-terminal.formal.v3'


def value(dataset_file):
    dataset_sha = file_hash(dataset_file)
    data = sealed(load(dataset_file))
    require(data['schema'] == formal_intake.SCHEMA and data['split'] == 'calibration'
            and data['scores_hidden'] is True and data['jev_probabilities_read'] is False
            and data['certified_threshold'] is None
            and all(data[k] is False for k in ('formal_registration', 'formal_certification',
                                              'production_authorized', 'holdout_opened')),
            'original_prediction_blind_calibration_required')
    before_cohort = sealed(load(data['cohort_file']))
    evidence = formal_registration._evidence_bindings(data, before_cohort)
    authorization = next(b['file'] for b in before_cohort['bindings'] if b['role'] == 'authorization')
    before_budget = approved_budget.snapshot(authorization)
    actual = formal_intake.collect(data['cohort_file'], [b['directory'] for b in data['collection_bindings']],
                                  data['collector_executable_file'])
    require(packet.canonical_bytes(actual) == packet.canonical_bytes(data), 'original_full_dataset_changed')
    require(data['allocated_groups'] == data['collected_groups'] == len(data['cases'])
            and data['uncollected_groups'] == data['pending_groups'] == 0
            and all(c['main_status'] not in {'main_not_started', 'main_not_finished'} for c in data['cases']),
            'complete_terminal_fixed_allocation_required')
    cohort = sealed(load(data['cohort_file']))
    require(file_hash(data['cohort_file']) == data['cohort_sha256']
            and cohort['artifact_hash'] == data['cohort_hash'], 'original_terminal_cohort_changed')
    auth = next(b['file'] for b in cohort['bindings'] if b['role'] == 'authorization')
    snapshot, finished = approved_budget.snapshot(auth)
    require(not {c['group_id'] for c in cohort['cases']} & {r['group_id'] for r in snapshot['jev_reservations']},
            'pre_registration_report_cannot_follow_scope_Jev_reservation')
    require(all(finished.get(c['group_id']) is not None for c in data['cases'])
            and all(c['call_eligible'] is (c['label'] in {'supported', 'unsupported'}) for c in data['cases']),
            'original_terminal_receipts_and_eligibility_required')
    spec = prospective_protocol.validate(load(data['protocol_file']))
    counts = {label: sum(c['label'] == label for c in data['cases'])
              for label in ('supported', 'unsupported', 'unknown', 'unlabeled')}
    quota_met = counts['supported'] >= spec['min_supported'] and counts['unsupported'] >= spec['min_unsupported']
    paths = (__file__, formal_intake.__file__, formal_collection.__file__, approved_budget.__file__,
             prospective_protocol.__file__, formal_registration.__file__)
    bindings = [{'file': str(Path(p).resolve(strict=True)), 'sha256': file_hash(p)} for p in paths]
    body = dict(schema=SCHEMA, split='calibration', status='ready_for_original_registration' if quota_met else 'independent_quotas_not_met',
        dataset_file=str(Path(dataset_file).resolve(strict=True)), dataset_sha256=dataset_sha, dataset_hash=data['artifact_hash'],
        cohort_file=data['cohort_file'], cohort_sha256=data['cohort_sha256'], cohort_hash=cohort['artifact_hash'],
        protocol_file=data['protocol_file'], protocol_sha256=data['protocol_sha256'],
        allocated_groups=data['allocated_groups'], collected_groups=data['collected_groups'], label_counts=counts,
        main_status_counts=dict(Counter(c['main_status'] for c in data['cases'])),
        independent_quotas_met=quota_met, min_supported=spec['min_supported'], min_unsupported=spec['min_unsupported'],
        missing_supported=max(0, spec['min_supported'] - counts['supported']),
        missing_unsupported=max(0, spec['min_unsupported'] - counts['unsupported']),
        current_main_count=len(snapshot['main_started']), current_phase_jev_count=len(snapshot['jev_reservations']),
        historical_main_pending=sum(f is None for f in finished.values()), whole_allocation_retained=True,
        scores_hidden=True, jev_probabilities_read=False, genuine_original_intake_revalidated=True,
        source_bindings=bindings, original_evidence_bindings=evidence,
        formal_registration=False, formal_certification=False, certified_threshold=None,
        holdout_opened=False, production_authorized=False, overall_goal_complete=False,
        native_claim_certification=False, overall_agent_benefit_certification=False,
        report_model_calls=0, report_jev_calls=0, report_oracle_executions=0, report_reservations=0)
    require(file_hash(dataset_file) == dataset_sha
            and packet.canonical_bytes(before_cohort) == packet.canonical_bytes(cohort)
            and packet.canonical_bytes(evidence) == packet.canonical_bytes(formal_registration._evidence_bindings(data, cohort))
            and all(file_hash(b['file']) == b['sha256'] for b in bindings)
            and packet.canonical_bytes(before_budget) == packet.canonical_bytes((snapshot, finished))
            and packet.canonical_bytes(approved_budget.snapshot(auth)) == packet.canonical_bytes((snapshot, finished)),
            'original_terminal_evidence_owner_or_budget_changed')
    return {**body, 'artifact_hash': digest(body)}


def revalidate(path):
    original = sealed(load(path))
    require(original['schema'] == SCHEMA, 'original_main_terminal_report_required')
    actual = value(original['dataset_file'])
    require(packet.canonical_bytes(actual) == packet.canonical_bytes(original), 'original_main_terminal_report_changed')
    return actual


def public_summary(original):
    """Aggregate from an already revalidated transaction, no original content."""
    fields = ('schema', 'status', 'allocated_groups', 'collected_groups', 'label_counts', 'main_status_counts',
              'independent_quotas_met', 'min_supported', 'min_unsupported', 'missing_supported', 'missing_unsupported',
              'current_main_count', 'current_phase_jev_count', 'historical_main_pending', 'whole_allocation_retained',
              'formal_registration', 'formal_certification', 'certified_threshold', 'holdout_opened',
              'production_authorized', 'overall_goal_complete', 'native_claim_certification', 'overall_agent_benefit_certification')
    return {**{k: original[k] for k in fields}, 'original_result_hash': original['artifact_hash'],
            'original_dataset_sha256': original['dataset_sha256'], 'original_cohort_hash': original['cohort_hash'],
            'unknown_is_not_negative': True, 'paid_retries': 0,
            'scope': 'APPS official train host split, restricted original ordered-call assertions'}
