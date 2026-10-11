"""Original calibrated APPS evidence, numeric gates grant no production authority.

Full registration/source proof once per validation transaction; every observation
still rejoins its exact original byte/event/reservation/compiled evidence.
"""
from collections import Counter
from pathlib import Path

from ..artifact_scoring import sealed
from ..calibration import digest, load, require
from ..collection_manifest import file_hash
from ..collection_receipts import read_lines
from . import formal_jev, formal_registration, prospective_protocol

SCHEMA = 'buglecat.verification.apps-original-calibration.formal.v3'


def calibration_value(registration_file, allocation_file, observations_file, executable, *, _original_registration=None, _verified_pack=None):
    if _original_registration is None:
        registered, _verified_pack = formal_registration.proof_transaction(registration_file)
    else:
        registered = _original_registration
    reg, data, binary = registered
    registration_sha = file_hash(registration_file)
    allocation = formal_jev.validate_pack(allocation_file, _registered=registered, _verified_pack=_verified_pack)
    require(allocation['registration_hash'] == reg['artifact_hash']
            and allocation['registration_sha256'] == file_hash(registration_file), 'same_actual_registration_required')
    compiled = formal_jev.build_info(executable)
    before = file_hash(observations_file)
    supplied = read_lines(observations_file)
    require(len(supplied) == len(binary)
            and [r['id'] for r in supplied] == reg['eligible_ids'], 'complete_original_eligible_observations_in_fixed_order_required')
    originals = []
    for row in supplied:
        require(Path(row['runner_executable_file']).resolve(strict=True) == Path(executable).resolve(strict=True),
                'one_actual_compiled_observer_required')
        prepared = formal_jev.validate(row['prepared_file'], _allocation_pack=allocation)
        actual = formal_jev._observe(prepared, row['prepared_file'], row['attempt_directory'], executable, _compiled=compiled)
        require(formal_jev.same(actual, row), 'original_observation_changed')
        originals.append(actual)
    spec = prospective_protocol.validate(load(data['protocol_file']))
    numeric = prospective_protocol.calibrate_numbers(binary, originals, spec)
    require(file_hash(observations_file) == before and formal_jev.build_info(executable) == compiled,
            'original_inputs_changed_during_calibration')
    require(file_hash(registration_file) == registration_sha
            and file_hash(reg['dataset_file']) == reg['dataset_sha256']
            and file_hash(data['cohort_file']) == data['cohort_sha256']
            and file_hash(data['protocol_file']) == data['protocol_sha256']
            and all(file_hash(b['file']) == b['sha256'] for b in reg['source_bindings'] + reg['original_evidence_bindings'])
            and formal_jev.same(formal_registration.formal_collection.budget_snapshot(reg['authorization_file']),
                                (_verified_pack[3], _verified_pack[4])), 'registered_originals_or_budget_changed_during_calibration')
    body = {'schema': SCHEMA, 'split': 'calibration', 'registration_file': str(Path(registration_file).resolve(strict=True)),
            'registration_sha256': file_hash(registration_file), 'registration_hash': reg['artifact_hash'],
            'allocation_file': str(Path(allocation_file).resolve(strict=True)), 'allocation_sha256': file_hash(allocation_file),
            'allocation_hash': allocation['artifact_hash'], 'observations_file': str(Path(observations_file).resolve(strict=True)),
            'observations_sha256': before, 'observations_hash': digest(originals),
            'runner_executable_file': str(Path(executable).resolve(strict=True)), 'runner_executable_sha256': file_hash(executable),
            'runner_bindings': [{'file': str(p.resolve()), 'sha256': file_hash(p)} for p in formal_jev.runner_sources().values()],
            'cohort_file': data['cohort_file'], 'cohort_sha256': data['cohort_sha256'], 'cohort_hash': reg['cohort_hash'],
            'protocol_file': data['protocol_file'], 'protocol_sha256': data['protocol_sha256'],
            'allocated_groups': data['allocated_groups'], 'label_counts': dict(Counter(c['label'] for c in data['cases'])),
            'eligible_groups': len(binary), 'original_observations': len(originals), **numeric,
            'genuine_original_proof_verified': True,
            'qualified_for_once_holdout': numeric['calibration_numeric_gate_met'],
            'qualified_for_holdout_threshold': numeric['qualified_numeric_threshold'],
            'holdout_status': 'not_opened', 'actual_holdout_source_authorization': False,
            'scope': 'APPS original ordered-pair restricted Python benchmark only; reject signal, no Verified promotion',
            'native_claim_certification': False, 'overall_agent_benefit_certification': False,
            'source_bindings': [{'file': str(Path(__file__).resolve()), 'sha256': file_hash(__file__)}]}
    return {**body, 'artifact_hash': digest(body)}


def proof_transaction(path):
    """One real original registration transaction, plus each raw observation."""
    value = sealed(load(path))
    require(value['schema'] == SCHEMA and value['split'] == 'calibration', 'genuine_original_APPS_calibration_required')
    registered, pack = formal_registration.proof_transaction(value['registration_file'])
    actual = calibration_value(value['registration_file'], value['allocation_file'], value['observations_file'],
                               value['runner_executable_file'], _original_registration=registered, _verified_pack=pack)
    require(formal_jev.same(actual, value), 'original_calibration_or_threshold_changed')
    return actual, registered, pack


def revalidate_calibration(path):
    return proof_transaction(path)[0]
