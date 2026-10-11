"""Whole original holdout, locked threshold, one durable evaluation consumption."""
from collections import Counter
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..collection_receipts import read_lines
from ..paid_budget import durable_new
from . import (
    holdout_collection,
    holdout_intake,
    holdout_jev,
    holdout_registration,
    prospective_protocol,
)

SCHEMA = 'buglecat.verification.apps-original-evaluation.holdout.v3'
START = 'buglecat.verification.apps-once-evaluation-start.holdout.v3'
NAMESPACE = 'formal-apps-holdout-evaluations-v3'
TERMINAL = 'buglecat.verification.apps-original-terminal.holdout.v3'


def terminal_value(source_file, prepared_directories, executable):
    """Read-only whole allocation. In-progress is not a deliverable terminal.

    Original Oracle bytes are reused; this never executes an Oracle, registers
    labels, opens a source, reserves a slot, or reads Jev probabilities.
    """
    marker, pack = holdout_collection.verified_pack(source_file)
    directories = [str(Path(p).resolve(strict=True)) for p in prepared_directories]
    evidence_data = {'collector_executable_file': str(Path(executable).resolve(strict=True)),
                     'collection_bindings': [{'directory': p} for p in directories]}
    captured = holdout_registration.formal_registration._evidence_bindings(evidence_data, pack[0]) + marker['source_bindings']
    source_sha = file_hash(source_file)
    data = holdout_intake.collect(source_file, directories, executable, _source_pack=(marker, pack))
    require(file_hash(source_file) == source_sha
            and all(file_hash(b['file']) == b['sha256'] for b in captured)
            and holdout_jev.same(holdout_registration.formal_registration._evidence_bindings(evidence_data, pack[0]) + marker['source_bindings'], captured),
            'original_holdout_terminal_graph_changed')
    groups = {c['group_id'] for c in data['cases']}
    require(not groups & {r['group_id'] for r in pack[3]['jev_reservations']}, 'holdout_jev_before_label_terminal')
    counts = dict(Counter(c['label'] for c in data['cases']))
    spec = prospective_protocol.validate(load(data['protocol_file']))
    complete = data['collected_groups'] == data['allocated_groups'] and data['uncollected_groups'] == data['pending_groups'] == 0
    quota = counts.get('supported', 0) >= spec['min_supported'] and counts.get('unsupported', 0) >= spec['min_unsupported']
    body = {'schema': TERMINAL, 'split': 'holdout', 'dataset': data, 'dataset_hash': data['artifact_hash'],
            'holdout_source_file': str(Path(source_file).resolve(strict=True)), 'holdout_source_sha256': source_sha,
            'holdout_source_hash': marker['artifact_hash'], 'fixed_threshold': marker['fixed_threshold'],
            'cohort_file': data['cohort_file'], 'cohort_sha256': data['cohort_sha256'], 'cohort_hash': data['cohort_hash'],
            'protocol_file': data['protocol_file'], 'protocol_sha256': data['protocol_sha256'],
            'prepared_directories': directories, 'collector_executable_file': str(Path(executable).resolve(strict=True)),
            'collector_executable_sha256': file_hash(executable), 'source_bindings': captured,
            'allocated_groups': data['allocated_groups'], 'collected_groups': data['collected_groups'],
            'uncollected_groups': data['uncollected_groups'], 'pending_groups': data['pending_groups'], 'label_counts': counts,
            'whole_allocation_terminal': complete, 'independent_label_quota_met': quota,
            'status': 'in_progress' if not complete else 'ready_for_original_registration' if quota else 'incomplete_labels',
            'holdout_status': 'not_evaluated', 'whole_allocation_retained': True, 'new_paid_calls': 0,
            'holdout_registration': False, 'formal_certification': False, 'certified_threshold': None,
            'APPS_ordered_pair_artifact_threshold_certified': False, 'certified_APPS_ordered_pair_artifact_threshold': None,
            'production_authorized': False, 'native_claim_certification': False, 'overall_agent_benefit_certification': False,
            'overall_goal_complete': False, 'oracle_executions': 0, 'reservations': 0,
            'scope': 'APPS original ordered-pair restricted Python; no numerical threshold certificate or Verified promotion'}
    require(holdout_jev.same(holdout_collection.budget_snapshot(
        next(b['file'] for b in pack[0]['bindings'] if b['role'] == 'authorization')), (pack[3], pack[4])),
        'original_holdout_budget_changed_during_terminal')
    return {**body, 'artifact_hash': digest(body)}


def revalidate_terminal(path):
    value = sealed(load(path))
    require(value['schema'] == TERMINAL and value['split'] == 'holdout', 'genuine_original_holdout_terminal_required')
    actual = terminal_value(value['holdout_source_file'], value['prepared_directories'], value['collector_executable_file'])
    require(holdout_jev.same(actual, value), 'original_holdout_terminal_changed')
    return actual


def evaluation_value(registration_file, allocation_file, observations_file, executable, *, _original_registration=None, _verified_pack=None):
    if _original_registration is None:
        registered, pack = holdout_registration.proof_transaction(registration_file)
    else:
        registered, pack = _original_registration, _verified_pack
    reg, data, binary = registered
    allocation = holdout_jev.validate_pack(allocation_file, _registered=registered, _verified_pack=pack)
    before, reg_sha = file_hash(observations_file), file_hash(registration_file)
    supplied = read_lines(observations_file)
    require(len(supplied) == len(binary) and [r['id'] for r in supplied] == reg['eligible_ids'], 'whole_original_holdout_observations_required')
    compiled = holdout_jev.build_info(executable)
    originals = []
    for row in supplied:
        require(Path(row['runner_executable_file']).resolve(strict=True) == Path(executable).resolve(strict=True), 'one_original_holdout_runner_required')
        prepared = holdout_jev.validate(row['prepared_file'], _allocation_pack=allocation)
        actual = holdout_jev._observe(prepared, row['prepared_file'], row['attempt_directory'], executable, _compiled=compiled)
        require(holdout_jev.same(actual, row), 'original_holdout_observation_changed')
        originals.append(actual)
    spec = prospective_protocol.validate(load(data['protocol_file']))
    threshold = reg['fixed_threshold']
    numeric = prospective_protocol.numbers(binary, originals, threshold, spec)
    require(file_hash(observations_file) == before and file_hash(registration_file) == reg_sha
            and file_hash(reg['dataset_file']) == reg['dataset_sha256']
            and file_hash(data['holdout_source_file']) == data['holdout_source_sha256']
            and all(file_hash(b['file']) == b['sha256'] for b in reg['source_bindings'] + reg['original_evidence_bindings'])
            and holdout_jev.build_info(executable) == compiled
            and holdout_jev.same(holdout_collection.budget_snapshot(reg['authorization_file']), (pack[3], pack[4])),
            'original_holdout_evidence_or_budget_changed_during_evaluation')
    body = {'schema': SCHEMA, 'split': 'holdout', 'registration_file': str(Path(registration_file).resolve(strict=True)),
            'registration_sha256': reg_sha, 'registration_hash': reg['artifact_hash'],
            'allocation_file': str(Path(allocation_file).resolve(strict=True)), 'allocation_sha256': file_hash(allocation_file),
            'allocation_hash': allocation['artifact_hash'], 'observations_file': str(Path(observations_file).resolve(strict=True)),
            'observations_sha256': before, 'observations_hash': digest(originals),
            'runner_executable_file': str(Path(executable).resolve(strict=True)), 'runner_executable_sha256': file_hash(executable),
            'runner_bindings': [{'file': str(p.resolve()), 'sha256': file_hash(p)} for p in holdout_jev.runner_sources().values()],
            **{k: data[k] for k in ('cohort_file', 'cohort_sha256', 'cohort_hash', 'protocol_file', 'protocol_sha256',
                                   'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash')},
            'allocated_groups': data['allocated_groups'], 'collected_groups': data['collected_groups'],
            'label_counts': dict(Counter(c['label'] for c in data['cases'])), 'eligible_groups': len(binary),
            'original_observations': len(originals), 'fixed_threshold': threshold, **numeric,
            'holdout_status': 'passed' if numeric['gate_met'] else 'failed', 'genuine_original_proof_verified': True,
            'threshold_retuning_allowed': False, 'whole_allocation_retained': True,
            'APPS_ordered_pair_artifact_threshold_certified': numeric['gate_met'],
            'certified_APPS_ordered_pair_artifact_threshold': threshold if numeric['gate_met'] else None,
            'scope': 'original APPS frozen ordered-pair restricted Python assertions only; reject signals, no Verified promotion',
            'formal_certification': False, 'certified_threshold': None, 'production_authorized': False,
            'native_claim_certification': False, 'overall_agent_benefit_certification': False, 'overall_goal_complete': False,
            'new_paid_calls': 0, 'source_bindings': [{'file': str(Path(__file__).resolve()), 'sha256': file_hash(__file__)}]}
    return {**body, 'artifact_hash': digest(body)}


def _target(registration):
    chain = holdout_jev.approved_budget.resolve(registration['authorization_file'])
    root = Path(chain['authorization']['jev']['ledger_dir']).resolve(strict=True)
    namespace = root / NAMESPACE
    require(not namespace.is_symlink(), 'evaluation_namespace_symlink')
    return namespace / (registration['cohort_hash'] + '.json')


def evaluate(registration_file, allocation_file, observations_file, executable):
    """Root only. Real qualified source/labels first; failed consumption stays."""
    registered, pack = holdout_registration.proof_transaction(registration_file)
    target = _target(registered[0])
    target.parent.mkdir(exist_ok=True)
    start = {'schema': START, 'registration_file': str(Path(registration_file).resolve(strict=True)),
             'registration_sha256': file_hash(registration_file), 'allocation_file': str(Path(allocation_file).resolve(strict=True)),
             'allocation_sha256': file_hash(allocation_file), 'observations_file': str(Path(observations_file).resolve(strict=True)),
             'observations_sha256': file_hash(observations_file), 'executable_file': str(Path(executable).resolve(strict=True)),
             'executable_sha256': file_hash(executable), 'fixed_threshold': registered[0]['fixed_threshold'],
             'cohort_hash': registered[0]['cohort_hash'], 'result_file': str(target), 'new_paid_calls': 0}
    start['artifact_hash'] = digest(start)
    durable_new(target.with_suffix('.started'), start)
    try:
        actual = evaluation_value(registration_file, allocation_file, observations_file, executable,
                                  _original_registration=registered, _verified_pack=pack)
        durable_new(target, actual)
    except Exception:
        failure = {'schema': START, 'start_hash': start['artifact_hash'], 'status': 'original_evaluation_failed_closed',
                   'certified_threshold': None, 'formal_certification': False, 'production_authorized': False, 'new_paid_calls': 0}
        durable_new(target.with_suffix('.failed'), failure)
        raise
    return actual


def revalidate_evaluation(path):
    value = sealed(load(path))
    require(value['schema'] == SCHEMA and value['split'] == 'holdout', 'genuine_original_holdout_result_required')
    actual = evaluation_value(value['registration_file'], value['allocation_file'], value['observations_file'], value['runner_executable_file'])
    require(holdout_jev.same(actual, value), 'original_once_result_or_threshold_changed')
    reg = load(value['registration_file'])
    target = _target(reg)
    require(Path(path).resolve(strict=True) == target.resolve(strict=True), 'genuine_shared_once_result_required')
    start = sealed(load(target.with_suffix('.started')))
    require(start['schema'] == START and start['result_file'] == str(target) and start['fixed_threshold'] == value['fixed_threshold']
            and start['registration_file'] == value['registration_file'] and start['registration_sha256'] == value['registration_sha256']
            and start['allocation_file'] == value['allocation_file'] and start['allocation_sha256'] == value['allocation_sha256']
            and start['observations_file'] == value['observations_file'] and start['observations_sha256'] == value['observations_sha256']
            and start['executable_file'] == value['runner_executable_file'] and start['executable_sha256'] == value['runner_executable_sha256']
            and start['cohort_hash'] == value['cohort_hash'] and type(start['new_paid_calls']) is int and start['new_paid_calls'] == 0
            and not target.with_suffix('.failed').exists(), 'original_shared_evaluation_consumption_changed')
    return actual
