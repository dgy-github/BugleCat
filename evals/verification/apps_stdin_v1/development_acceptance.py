"""Fresh development transport review; a saved review cannot open a phase.

Labels come from the original blind marker, observations from the actual sender
observer. No threshold is selected on development data. A future calibration
opening must consume this review again under its own durable transaction.
"""
import sys
from collections import Counter
from pathlib import Path

from . import (
    budget,
    cohort,
    development_review,
    packet,
    protocol,
)
from . import (
    development_observations as observations,
)
from . import (
    development_originals as originals,
)
from . import (
    development_registration as registration,
)

SCHEMA = 'buglecat.verification.stdin-development-pipeline-review.v1'
NO_AUTHORITY = ('collection_authorized', 'calibration_opened', 'formal_registration',
                'holdout_opened', 'formal_certification', 'production_authorized')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NO_AUTHORITY'))
    packet.loaded_functions(observations, Path(observations.__file__))
    packet.loaded_functions(registration, Path(registration.__file__))
    originals.loaded_check()
    protocol.dependencies()
    packet.require(observations.registration is registration,
                   'development observer registration alias changed')


def _binary_cases(registered):
    cases = registered['cases']
    packet.require(type(cases) is list and len(cases) == 8
                   and all(case['split'] == 'development' for case in cases),
                   'complete original eight development outcomes required')
    binary = []
    for case in cases:
        if case['label'] not in ('supported', 'unsupported'):
            packet.require(case['label'] == 'unknown' and case['call_eligible'] is False,
                           'nonbinary development outcome must remain Unknown')
            continue
        packet.require(case['call_eligible'] is True, 'original binary eligibility changed')
        binary.append({key: case[key] for key in ('id', 'group_id', 'source_kind', 'label', 'oracle_sha256')}
                      | {'candidate_sha256': case['candidate_code_sha256']})
    packet.require([case['id'] for case in binary] == registered['eligible_ids'],
                   'original blind binary order changed')
    return binary


def _decision(registered, observed, spec):
    """Internal outcome policy; caller numbers alone carry no authority."""
    binary = _binary_cases(registered)
    rows = observed['observations']
    outcomes = observed['cases']
    packet.require(type(outcomes) is list and len(outcomes) == 8
                   and all(budget.same({key: value for key, value in outcome.items() if key != 'jev_status'}, case)
                           for outcome, case in zip(outcomes, registered['cases'], strict=True)),
                   'complete original development labels or outcomes changed')
    selected = [case for case in outcomes if case['call_eligible']]
    for case in outcomes:
        if case['call_eligible']:
            packet.require(protocol.terminal_status(case['jev_status'])
                           or case['jev_status'] in ('jev_not_started', 'jev_pending'),
                           'unrecognized development attempt state')
        else:
            packet.require(case['jev_status'] == 'jev_not_eligible', 'Unknown acquired Jev attempt')
    terminal = [case for case in selected if protocol.terminal_status(case['jev_status'])]
    packet.require(type(rows) is list and [row['id'] for row in rows] == [case['id'] for case in terminal]
                   and all(row['call_status'] == case['jev_status']
                           for row, case in zip(rows, terminal, strict=True)),
                   'complete fixed ordered terminal observations required')
    statuses = Counter(case['jev_status'] for case in selected)
    complete = bool(binary) and len(terminal) == len(binary)
    if complete:
        protocol._verified(binary, rows, spec)
    successful = statuses.get('ok', 0)
    reason = ('no_original_binary_development_cases' if not binary else
              'development_jev_incomplete' if not complete else
              'no_valid_original_jev_response' if not successful else
              'development_pipeline_verified')
    return {'pipeline_verified': complete and successful > 0, 'reason': reason,
            'development_label_counts': dict(Counter(case['label'] for case in registered['cases'])),
            'eligible_groups': len(binary), 'jev_status_counts': dict(statuses),
            'terminal_jev_groups': len(terminal),
            'valid_original_jev_responses': successful,
            'development_data_used_for_threshold_selection': False,
            'development_groups_counted_in_formal_quotas': 0}


def _image(chain):
    current, finished = budget._snapshot_chain(chain)
    return {'snapshot': current, 'finished': finished,
            'raw_budget_bindings': originals.binding_images(chain),
            'sql_image': budget._ledger_rows(Path(current['main_ledger_file']))}


def review(registration_file, prepared_directories, sender_executable):
    """Read-only fresh original proof, no Oracle execution or phase marker.

    Directory and budget sets are closed before and after the original observer;
    this includes unsuccessful and pending attempts. The returned seal is an
    integrity checksum, never a caller-generated permission or certification.
    """
    loaded_check()
    registered_path = Path(registration_file).resolve(strict=True)
    executable = Path(sender_executable).resolve(strict=True)
    registered = cohort.sealed(budget.load(registered_path))
    locked = budget.raw_bindings([registered_path, executable, __file__])
    hashes = {row['file']: row['sha256'] for row in locked}
    directories = [Path(path).absolute() for path in prepared_directories]
    evidence = development_review.evidence(directories) if directories else []
    chain = registered['ledger_snapshot']['authorization_chain']
    before = _image(chain)
    actual = observations.collect(registered_path, directories, executable)
    packet.require(actual['schema'] == observations.SCHEMA
                   and actual['registration_file'] == str(registered_path)
                   and actual['registration_sha256'] == hashes[str(registered_path)]
                   and actual['registration_hash'] == registered['artifact_hash']
                   and actual['cohort_hash'] == registered['cohort_hash']
                   and actual['budget_snapshot_digest'] == budget.digest((before['snapshot'], before['finished']))
                   and actual['certified_threshold'] is None
                   and all(actual[key] is False for key in ('formal_registration', 'holdout_opened',
                                                           'formal_certification', 'production_authorized')),
                   'actual observer differs from locked blind registration')
    spec = protocol.specification()
    decision = _decision(registered, actual, spec)
    body = {'schema': SCHEMA, 'registration_file': str(registered_path),
            'registration_sha256': hashes[str(registered_path)], 'registration_hash': registered['artifact_hash'],
            'cohort_hash': registered['cohort_hash'], 'sender_executable_file': str(executable),
            'sender_executable_sha256': hashes[str(executable)], 'sender_build_info': actual['sender_build_info'],
            'prepared_directories': [str(path.resolve(strict=True)) for path in directories],
            'original_observations_hash': actual['artifact_hash'],
            'original_directory_evidence': evidence, 'budget_snapshot_digest': budget.digest(before),
            'original_registration_bindings': registered['bindings'],
            'protocol_hash': budget.digest(spec), 'review_owner_sha256': hashes[str(Path(__file__).resolve())],
            'whole_original_allocation': 808, 'development_groups': 8, 'unopened_groups': 800,
            **decision, **{key: False for key in NO_AUTHORITY}, 'certified_threshold': None,
            'new_main_calls': 0, 'new_jev_calls': 0, 'new_oracle_executions': 0,
            'scope': 'Original development pipeline only; fresh calibration opening remains required'}
    cohort.check(cohort.source_authority._merge_bindings(locked, registered['bindings']))
    packet.require(budget.same(development_review.evidence(directories) if directories else [], evidence)
                   and budget.same(_image(chain), before),
                   'development review directory or accounting changed')
    loaded_check()
    return cohort.seal(body)


def revalidate(review_file):
    """Rebuild a saved review from current original bytes; never trust its flags."""
    path = Path(review_file).resolve(strict=True)
    raw_sha = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'actual development pipeline review required')
    rebuilt = review(saved['registration_file'], saved['prepared_directories'], saved['sender_executable_file'])
    packet.require(budget.same(saved, rebuilt) and cohort.sha(path) == raw_sha,
                   'saved development review differs from fresh original evidence')
    return rebuilt
