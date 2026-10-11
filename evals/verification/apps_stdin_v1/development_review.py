"""Read-only original development evidence review, without phase authority.

Reconstruct the saved intake from actual bytes. A completed main/Oracle check
is one input to a future opening; it does not approve Jev, calibration or fees.
"""
import sys
from collections import Counter
from pathlib import Path

from . import budget, cohort, intake, packet

SCHEMA = 'buglecat.verification.stdin-original-development-review.v1'
IDENTITY = ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')
NO_AUTHORITY = ('collection_authorized', 'calibration_opened', 'formal_registration',
                'holdout_opened', 'formal_certification', 'production_authorized')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__),
                            ('SCHEMA', 'IDENTITY', 'NO_AUTHORITY'))
    packet.loaded_functions(intake, Path(intake.__file__))
    intake.loaded_check()
    budget.loaded_check()
    packet.require(budget.same is cohort.budget.same and cohort.sha is intake.cohort.sha,
                   'development review dependency aliases changed')


def evidence(directories):
    """Capture the complete original directory set; no suffix exclusions."""
    paths = [Path(path).absolute() for path in directories]
    packet.require(paths and all(path.is_dir() and not path.is_symlink() and not path.is_junction() for path in paths),
                   'actual original preparation directories required')
    roots = [path.resolve(strict=True) for path in paths]
    packet.require(len(set(roots)) == len(roots)
                   and not any(left != right and left.is_relative_to(right)
                               for left in roots for right in roots),
                   'duplicate or overlapping original preparation directories')
    records = []
    for root in roots:
        children = sorted(root.rglob('*'))
        packet.require(all(not child.is_symlink() and not child.is_junction() and child.resolve(strict=True).is_relative_to(root)
                           for child in children), 'original evidence path escapes preparation directory')
        packet.require(all(child.is_file() or child.is_dir() for child in children),
                       'unsupported original evidence node')
        records.append({'directory': str(root), 'directories': [str(child) for child in children if child.is_dir()],
                        'files': budget.raw_bindings([child for child in children if child.is_file()])})
    return records


def main_terminal(status):
    return intake.main_terminal_status(status)


def allocation(data, fixed):
    """All allocated groups remain, and unopened groups cannot acquire labels."""
    rows = data.get('cases')
    packet.require(type(rows) is list and len(rows) == len(fixed) == 808,
                   'complete immutable 808 group allocation required')
    stages = Counter()
    development = []
    for row, case in zip(rows, fixed, strict=True):
        packet.require(type(row) is dict and all(budget.same(row.get(key), case[key]) for key in IDENTITY),
                       'original allocation identity or order changed')
        stages[row['split']] += 1
        packet.require(row.get('label') in ('supported', 'unsupported', 'unknown', 'unlabeled')
                       and type(row.get('call_eligible')) is bool,
                       'typed independent original label required')
        if row['split'] != 'development':
            packet.require(row['main_status'] == 'main_not_started' and row['label'] == 'unlabeled'
                           and row['call_eligible'] is False
                           and not any(key in row for key in ('main_receipt_file', 'oracle_file',
                                                             'candidate_code_span', 'candidate_code_sha256')),
                           'unopened calibration or holdout acquired original evidence')
            continue
        status = row['main_status']
        packet.require(status in ('main_not_started', 'main_not_finished') or main_terminal(status),
                       'original main status is not a recognized outcome')
        if status in ('main_not_started', 'main_not_finished'):
            packet.require(row['label'] == 'unlabeled' and row['call_eligible'] is False,
                           'pending or unstarted main cannot be independently labeled')
        else:
            packet.require(row['label'] != 'unlabeled', 'terminal main label omitted')
        packet.require(row['call_eligible'] is (row['label'] in ('supported', 'unsupported')),
                       'Unknown cannot become a binary or Jev eligible outcome')
        if row['label'] in ('supported', 'unsupported'):
            packet.require(status == 'ok' and all(row.get(key) for key in
                ('main_receipt_file', 'main_receipt_sha256', 'oracle_file', 'oracle_sha256',
                 'candidate_code_sha256', 'candidate_code_span')), 'binary label lacks original candidate/Oracle proof')
        development.append(row)
    packet.require(dict(stages) == {'development': 8, 'calibration': 400, 'holdout': 400},
                   'fixed allocation phase sizes changed')
    counts = dict(Counter(row['label'] for row in rows))
    started = sum(row['main_status'] != 'main_not_started' for row in rows)
    packet.require(type(data.get('allocated_groups')) is int and data['allocated_groups'] == 808
                   and type(data.get('collected_groups')) is int and data['collected_groups'] == started
                   and type(data.get('uncollected_groups')) is int and data['uncollected_groups'] == 808 - started
                   and budget.same(data.get('label_counts'), counts), 'saved coverage or independent label counts changed')
    return development


def review(dataset_file, prepared_directories, executable):
    """No Oracle execution, reservation, HTTP or durable opening is performed."""
    loaded_check()
    dataset_file = Path(dataset_file).resolve(strict=True)
    dataset_sha = cohort.sha(dataset_file)
    data = cohort.sealed(budget.load(dataset_file))
    packet.require(data.get('schema') == intake.SCHEMA and data.get('scores_hidden') is True
                   and data.get('jev_probabilities_read') is False
                   and all(data.get(key) is False for key in
                           ('formal_registration', 'holdout_opened', 'formal_certification', 'production_authorized'))
                   and data.get('certified_threshold') is None
                   and type(data.get('model_calls')) is int and data['model_calls'] == 0
                   and type(data.get('jev_calls')) is int and data['jev_calls'] == 0,
                   'prediction blind genuine original intake required')
    cohort_file = Path(data['cohort_file']).resolve(strict=True)
    value = cohort.sealed(budget.load(cohort_file))
    packet.require(cohort.sha(cohort_file) == data['cohort_sha256']
                   and value['artifact_hash'] == data['cohort_hash']
                   and Path(executable).resolve(strict=True) == Path(data['collector_executable_file']).resolve(strict=True),
                   'actual original cohort or collector changed')
    initial = evidence(prepared_directories)
    bindings = budget.raw_bindings([dataset_file, cohort_file, executable, Path(__file__)])
    chain = value['ledger_snapshot']['authorization_chain']
    before = budget._snapshot_chain(chain)
    # Intake owns source, funded cohort, actual compiled image and original
    # transport/Oracle authenticity. Never accept caller supplied labels alone.
    rebuilt = intake.collect(cohort_file, prepared_directories, executable, execute_new_oracles=False)
    packet.require(budget.same(data, rebuilt), 'saved independent dataset differs from actual original intake')
    development = allocation(rebuilt, value['cases'])
    packet.require(budget.same(evidence(prepared_directories), initial),
                   'original evidence directory set or bytes changed during review')
    cohort.check(bindings)
    packet.require(cohort.sha(dataset_file) == dataset_sha
                   and budget.same(budget._snapshot_chain(chain), before), 'dataset or shared accounting changed during review')
    loaded_check()
    complete = all(main_terminal(row['main_status']) for row in development)
    unresolved = [row['id'] for row in development if row.get('reason') in
                  ('independent_oracle_not_recorded', 'oracle_attempt_incomplete')]
    return cohort.seal({'schema': SCHEMA, 'dataset_file': str(dataset_file), 'dataset_sha256': dataset_sha,
        'dataset_hash': data['artifact_hash'], 'cohort_file': str(cohort_file), 'cohort_sha256': data['cohort_sha256'],
        'cohort_hash': value['artifact_hash'], 'original_evidence': initial, 'review_bindings': bindings,
        'budget_snapshot_hash': budget.digest(before[0]), 'completion_snapshot_hash': budget.digest(before[1]),
        'allocated_groups': 808, 'development_groups': 8, 'unopened_groups': 800,
        'development_main_status_counts': dict(Counter(row['main_status'] for row in development)),
        'development_independent_label_counts': dict(Counter(row['label'] for row in development)),
        'all_development_main_terminal': complete, 'unrecorded_oracle_ids': unresolved,
        'development_main_oracle_evidence_complete': complete and not unresolved,
        'dev_jev_pipeline_verified': False, 'scores_hidden': True, 'jev_probabilities_read': False,
        **{key: False for key in NO_AUTHORITY}, 'certified_threshold': None,
        'new_model_calls': 0, 'new_jev_calls': 0, 'oracle_executions': 0, 'reservations': 0,
        'scope': 'Original development main and Oracle evidence only; no calibration opening or certification'})
