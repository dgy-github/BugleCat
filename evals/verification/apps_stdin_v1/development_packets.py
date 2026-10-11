"""Prediction-blind original development packet preparation, without spending.

All eight original outcomes remain. Binary packets are reconstructed from
original transport bytes and complete source IO after a fresh read-only review.
This object is not a registration or permission to reserve/send requests.
"""
import sys
from pathlib import Path

from . import budget, cohort, development_review, intake, packet, source_authority

SCHEMA = 'buglecat.verification.stdin-original-development-packets.v1'


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    development_review.loaded_check()
    source_authority.loaded_check()


def prepare(dataset_file, prepared_directories, executable):
    """Only local byte reads/build-info; no new Oracle, reservation or HTTP."""
    loaded_check()
    dataset_file = Path(dataset_file).resolve(strict=True)
    dataset_sha = cohort.sha(dataset_file)
    original_evidence = development_review.evidence(prepared_directories)
    reviewed = development_review.review(dataset_file, prepared_directories, executable)
    packet.require(reviewed['development_main_oracle_evidence_complete'] is True,
                   'complete development main and original Oracle evidence required')
    data = cohort.sealed(budget.load(dataset_file))
    value = cohort.sealed(budget.load(data['cohort_file']))
    packet.require(cohort.sha(dataset_file) == dataset_sha == reviewed['dataset_sha256']
                   and cohort.sha(data['cohort_file']) == reviewed['cohort_sha256']
                   and data['artifact_hash'] == reviewed['dataset_hash']
                   and value['artifact_hash'] == reviewed['cohort_hash'], 'original review transaction changed')
    before = budget._snapshot_chain(value['ledger_snapshot']['authorization_chain'])
    packet.require(budget.digest(before[0]) == reviewed['budget_snapshot_hash']
                   and budget.digest(before[1]) == reviewed['completion_snapshot_hash'],
                   'shared accounting changed after original review')
    # The immediate fresh review proved this actual authority/source graph.
    # Read its original rows within this transaction; never accept a cached
    # caller proof or repeat reference/Oracle execution.
    authority_file = Path(value['source_authority_file']).resolve(strict=True)
    packet.require(cohort.sha(authority_file) == value['source_authority_sha256'], 'original source authority changed')
    authority = source_authority.sealed(budget.load(authority_file))
    train_file = Path(authority['train_file']).resolve(strict=True)
    bindings = budget.raw_bindings([dataset_file, data['cohort_file'], authority_file, train_file,
                                   executable, Path(__file__), Path(development_review.__file__)])
    original_cases = {case['group_id']: case for case in authority['cases']}
    selected = []
    for case in value['cases']:
        original = original_cases.get(case['group_id'])
        packet.require(original is not None and all(budget.same(original[key], case[key]) for key in
            ('group_id', 'task_id', 'task_sha256', 'source_kind')), 'cohort differs from original source case')
        selected.append(original)
    tasks, train = source_authority._read_original_tasks(authority, selected)
    packet.require(Path(train).resolve(strict=True) == train_file, 'original source path changed')
    rows = []
    for original in data['cases'][:8]:
        row = {key: original[key] for key in development_review.IDENTITY}
        row.update(main_status=original['main_status'], label=original['label'],
                   reason=original['reason'], call_eligible=original['call_eligible'],
                   request=None, request_hash=None, candidate_code_sha256=None, oracle_sha256=None)
        if original['call_eligible']:
            receipt = budget.load(original['main_receipt_file'])
            extracted = intake.original_code(receipt['result']['candidate'])
            packet.require(extracted is not None
                           and extracted['code_sha256'] == original['candidate_code_sha256']
                           and budget.same(extracted, original['candidate_code_span']),
                           'original candidate code span changed')
            task = tasks[original['task_id']]
            packet.require(budget.digest(task) == original['task_sha256'], 'original task payload changed')
            request = packet.request(task['question'], task['starter_code'], extracted['code'],
                                     task['input_output'], value['oracle_runtime_binding'])
            row.update(request=request, request_hash=budget.digest(request),
                       candidate_code_sha256=original['candidate_code_sha256'], oracle_sha256=original['oracle_sha256'])
        rows.append(row)
    cohort.check([*value['bindings'], *reviewed['review_bindings']])
    packet.require(cohort.sha(dataset_file) == dataset_sha
                   and budget.same(development_review.evidence(prepared_directories), original_evidence)
                   and budget.same(budget._snapshot_chain(value['ledger_snapshot']['authorization_chain']), before),
                   'original source, evidence or accounting changed during packet preparation')
    cohort.check(bindings)
    loaded_check()
    return cohort.seal({'schema': SCHEMA, 'dataset_file': str(dataset_file), 'dataset_sha256': dataset_sha,
        'dataset_hash': data['artifact_hash'], 'cohort_file': data['cohort_file'],
        'cohort_sha256': data['cohort_sha256'], 'cohort_hash': value['artifact_hash'],
        'review_hash': reviewed['artifact_hash'], 'prepared_directories': [row['directory'] for row in original_evidence],
        'collector_executable_file': str(Path(executable).resolve(strict=True)),
        'bindings': bindings, 'original_evidence': original_evidence, 'cases': rows,
        'development_groups': 8, 'whole_allocation_groups_retained_in_original_dataset': 808,
        'eligible_groups': sum(row['call_eligible'] for row in rows),
        'protocol_sha256': value['protocol_sha256'], 'template_version': packet.TEMPLATE,
        'scores_hidden': True, 'jev_probabilities_read': False,
        'collection_authorized': False, 'reservation_authorized': False,
        'formal_registration': False, 'calibration_opened': False, 'holdout_opened': False,
        'formal_certification': False, 'production_authorized': False, 'certified_threshold': None,
        'new_model_calls': 0, 'new_jev_calls': 0, 'oracle_executions': 0, 'reservations': 0})
