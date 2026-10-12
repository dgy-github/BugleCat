"""Original byte joins only: numerical observations never establish Pass.

Batch proof reconstructs registration once, then authenticates each submitted
request/response/event/reservation and closes all original files and budgets.
"""
import hashlib
import json
import math
import sys
from collections import Counter
from pathlib import Path

from . import budget, cohort, packet, protocol
from . import holdout_jev as sender
from . import holdout_score_originals as registration

SCHEMA = 'buglecat.verification.stdin-formal-holdout-original-observations.v1'
TERMINAL = ('ok', 'cancelled', 'missing_key', 'timeout', 'transport_error', 'response_budget',
            'invalid_json', 'actual_model_mismatch', 'exact_answers_required', 'invalid_noul', 'invalid_usage', 'client_error')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'TERMINAL'))
    sender.loaded_check()


def _finite(value, depth=0):
    packet.require(depth < 128, 'invalid JSON nesting')
    if isinstance(value, float):
        packet.require(math.isfinite(value), 'nonfinite JSON value')
    if type(value) is int and not -(2 ** 63) <= value < 2 ** 64:
        packet.require(math.isfinite(float(value)), 'nonfinite JSON integer projection')
    if type(value) in (dict, list):
        for child in value.values() if type(value) is dict else value:
            _finite(child, depth + 1)


def _constant(_):
    raise ValueError('nonfinite JSON constant')


def _object(pairs):
    result = {}
    for key, value in pairs:
        packet.require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def parsed_status(raw):
    try:
        value = json.loads(raw, parse_constant=_constant, object_pairs_hook=_object)
        _finite(value)
    except (ValueError, TypeError, RecursionError, UnicodeError, OverflowError):
        return 'invalid_json', None
    if type(value) is not dict or value.get('model') != 'jev-1.13.0':
        return 'actual_model_mismatch', None
    answers = value.get('answers')
    if type(answers) is not dict or set(answers) != {'acceptance_violation'}:
        return 'exact_answers_required', None
    answer = answers['acceptance_violation']
    if type(answer) is not dict or answer.get('type') != 'noul' or type(answer.get('noul')) not in (int, float) or not 0 <= answer['noul'] <= 1:
        return 'invalid_noul', None
    usage = value.get('usage')
    if type(usage) is not dict or any(type(usage.get(key)) is not int or not 0 <= usage[key] < 2 ** 64 for key in ('input_tokens', 'output_tokens')):
        return 'invalid_usage', None
    return 'ok', value


def identity(row, prepared_file, reservation, reservation_file):
    return {key: row[key] for key in ('id', 'group_id', 'request_hash', 'endpoint_sha256', 'registration_file',
        'registration_sha256', 'registration_hash', 'cohort_hash', 'candidate_code_sha256', 'oracle_sha256',
        'holdout_opening_file', 'holdout_opening_sha256', 'holdout_opening_hash', 'qualified_calibration_threshold')} | {
        'prepared_file': str(Path(prepared_file).resolve()), 'prepared_sha256': cohort.sha(prepared_file),
        'reservation_file': str(Path(reservation_file).resolve()), 'reservation_sha256': cohort.sha(reservation_file),
        'reservation_index': reservation['reservation_index'], 'build_info': row['sender_build_info']}


def join(row, prepared_file, reservation, directory, executable, *, fixture=False, reservation_file=None):
    """Strict local join after the batch has authenticated original membership."""
    compiled = sender.build_info(executable)
    return _join(row, prepared_file, reservation, directory, compiled, fixture=fixture, reservation_file=reservation_file)


def _join(row, prepared_file, reservation, directory, compiled, *, fixture=False, reservation_file=None):
    """Private same-transaction image, never a caller proof/permission API."""
    packet.require(budget.same(compiled, row['sender_build_info']), 'actual sender image changed')
    reservation_file = Path(directory).parent / 'fixture-reservation.json' if fixture else Path(reservation_file or reservation['reservation_file'])
    expected = identity(row, prepared_file, reservation, reservation_file)
    marker = budget.load(reservation_file.with_suffix('.started'))
    packet.require(marker.get('schema') == 'buglecat.verification.stdin-formal-holdout-jev-attempt-start.v1'
                   if not fixture else marker.get('synthetic_provenance') is True, 'original started marker kind changed')
    if not fixture:
        expected_marker = {'schema': 'buglecat.verification.stdin-formal-holdout-jev-attempt-start.v1',
            'group_id': row['group_id'], 'request_hash': row['request_hash'],
            'reservation_file': str(reservation_file.resolve()), 'reservation_sha256': cohort.sha(reservation_file),
            'reservation_index': reservation['reservation_index'], 'prepared_file': str(Path(prepared_file).resolve()),
            'prepared_sha256': cohort.sha(prepared_file), 'output_directory': str(Path(directory)), 'new_paid_calls': 0}
        packet.require(budget.same(marker, expected_marker), 'actual started marker/prepared/group/slot differs')
    packet.require(marker['request_hash'] == row['request_hash'] and marker['prepared_sha256'] == cohort.sha(prepared_file)
                   and marker['reservation_sha256'] == cohort.sha(reservation_file)
                   and registration.calibration_originals.intake.same_path(marker['output_directory'], directory), 'actual pre-HTTP marker changed')
    event_lines = (Path(directory) / 'events.jsonl').read_text(encoding='utf-8').splitlines()
    packet.require(all(line and line.strip() == line for line in event_lines), 'original event framing changed')
    events = [budget.parse(line) for line in event_lines]
    packet.require(len(events) in (2, 3) and events[0]['event'] == 'attempt_started' and events[-1]['event'] == 'attempt_finished',
                   'original attempt not terminal or events missing')
    packet.require(all(all(registration.calibration_originals.intake.same_path(event.get(k), v) if k in ('prepared_file', 'reservation_file')
                           else budget.same(event.get(k), v) for k, v in expected.items()) for event in events),
                   'original request/reservation/source/compiled events differ')
    request = (Path(directory) / 'request.json').read_bytes()
    actual_hash = hashlib.sha256(request).hexdigest()
    packet.require(request == row['request_json'].encode() and actual_hash == row['request_hash'], 'actual submitted original UTF8 body changed')
    finished = events[-1]
    status = finished['call_status']
    packet.require(status in TERMINAL or protocol.terminal_status(status), 'unknown/pending status cannot become terminal')
    if len(events) == 2:
        packet.require(status in ('cancelled', 'missing_key') and finished['http_status'] is None, 'unsubmitted failure misrepresented')
    else:
        packet.require(events[1]['event'] == 'request_submitted' and events[1]['actual_request_hash'] == actual_hash,
                       'actual transport hash missing')
    receipt_file = Path(directory) / 'receipt.json'
    receipt = budget.load(receipt_file)
    packet.require(finished['receipt_sha256'] == cohort.sha(receipt_file)
                   and budget.same(receipt, {k: v for k, v in finished.items() if k not in ('event', 'receipt_sha256')}),
                   'immutable terminal receipt/event differs')
    response = None
    if finished['raw_response_file'] is not None:
        raw_file = Path(directory) / 'raw-response.json'
        packet.require(registration.calibration_originals.intake.same_path(finished['raw_response_file'], raw_file)
                       and raw_file.is_file() and len(raw_file.read_bytes()) <= 32000
                       and finished['response_sha256'] == cohort.sha(raw_file), 'original raw response/path/SHA changed')
        raw = raw_file.read_bytes()
        http = finished['http_status']
        packet.require(type(http) is int and 100 <= http <= 599, 'original actual HTTP status missing')
        if 200 <= http < 300:
            inferred, response = parsed_status(raw)
            packet.require(status == inferred and budget.same(finished['response'], response), 'raw numeric status/usage/body differs')
        else:
            packet.require(status == 'http_' + str(http) and finished['response'] is None, 'original non2xx body/status differs')
    else:
        packet.require(status in ('cancelled', 'missing_key', 'timeout', 'transport_error', 'response_budget', 'client_error')
                       and finished['response_sha256'] is None and finished['response'] is None,
                       'complete response omitted or fabricated failure response')
    return {'id': row['id'], 'group_id': row['group_id'], 'call_status': status,
        'candidate_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
        'acceptance_violation': response['answers']['acceptance_violation']['noul'] if response else None,
        'request_hash': row['request_hash'], 'actual_request_hash': actual_hash if len(events) == 3 else None,
        'response_sha256': finished['response_sha256'], 'model': response['model'] if response else None,
        'transport_binding_status': 'original_transport_bytes_verified' if response else 'recorded_terminal_failure',
        'usage': response['usage'] if response else None, 'events_file': str(Path(directory) / 'events.jsonl')}


def proof(registration_file, directories, executable):
    loaded_check()
    packet.require(type(directories) in (list, tuple), 'original directories required')
    paths = [Path(path).absolute() for path in directories]
    packet.require(len({str(path.resolve()) for path in paths}) == len(paths), 'duplicate sender directory')
    evidence = registration.development_review.evidence(paths) if paths else []
    pack = registration.proof(registration_file)
    compiled = sender.build_info(executable)
    by_group = {row['group_id']: row for row in pack['new_reservations']}
    observations = []
    for request in pack['requests']:
        reserved = by_group.get(request['group_id'])
        if not reserved:
            continue
        row = sender._validate(reserved['prepared_file'], pack, compiled)
        packet.require(cohort.sha(reserved['prepared_file']) == reserved['prepared_sha256'], 'actual reserved prepared bytes changed')
        directory = Path(row['output_directory']).resolve()
        if not directory.exists():
            # An immutable reservation with no claimed directory is unresolved;
            # it still occupies its original slot and cannot be resubmitted.
            continue
        packet.require(directory in paths, 'actual allocated attempt directory omitted')
        if not (directory / 'receipt.json').exists():
            continue  # explicitly unresolved; never resubmit this slot
        reservation_file = Path(pack['chain']['authorization']['jev']['ledger_dir']) / 'reservations' / (budget.digest(row['group_id']) + '.json')
        observations.append(_join(row, Path(reserved['prepared_file']), {**reserved, 'reservation_file': str(reservation_file)}, directory, compiled))
    expected_dirs = {Path(row['output_directory']).resolve() for row in pack['new_reservations'] if Path(row['output_directory']).exists()}
    packet.require(set(paths) == expected_dirs, 'unexpected/omitted original attempt scope')
    packet.require(budget.same(evidence, registration.development_review.evidence(paths) if paths else []), 'sender directory set/bytes changed')
    registration.close(pack)
    packet.require(budget.same(sender.build_info(executable), compiled), 'compiled sender changed during original join')
    bindings = registration.cohort.source_authority._merge_bindings(pack['proof_bindings'],
        [{'file': str(path.resolve()), 'sha256': compiled[role]} for role, path in sender.runner_sources().items()],
        [{'file': str(Path(executable).resolve()), 'sha256': compiled['runner_exe_sha256']}],
        *[scope['files'] for scope in evidence])
    result = {'pack': pack, 'observations': observations, 'build_info': compiled, 'build_info_executable': str(Path(executable).resolve()),
              'directories': paths, 'evidence': evidence, 'bindings': bindings}
    close(result)
    return result


def collect(registration_file, prepared_directories, executable):
    original = proof(registration_file, prepared_directories, executable)
    pack, observed = original['pack'], {row['group_id']: row for row in original['observations']}
    reserved = {row['group_id'] for row in pack['new_reservations']}
    cases = []
    for row in pack['dataset']['cases'][408:]:
        status = observed.get(row['group_id'], {}).get('call_status')
        if status is None:
            status = 'jev_pending' if row['group_id'] in reserved else ('jev_not_started' if row['call_eligible'] else 'jev_not_eligible')
        cases.append({**row, 'jev_status': status})
    return cohort.seal({'schema': SCHEMA, 'registration_file': pack['registration_file'],
        'registration_sha256': pack['registration_sha256'], 'registration_hash': pack['registration']['artifact_hash'],
        'cohort_hash': pack['cohort']['artifact_hash'],
        'qualified_calibration_threshold': pack['opening']['qualified_calibration_threshold'], 'sender_build_info': original['build_info'],
        'prepared_directories': [str(path) for path in original['directories']], 'cases': cases,
        'eligible_groups': len(pack['requests']), 'reserved_groups': len(reserved), 'terminal_groups': len(observed),
        'pending_groups': len(reserved) - len(observed), 'status_counts': dict(Counter(row['jev_status'] for row in cases)),
        'observations': original['observations'], 'evidence_bindings': original['evidence'],
        'budget_snapshot_digest': budget.digest((pack['current'], pack['finished'])),
        'formal_registration': True, 'holdout_only': True, 'holdout_opened': True, 'production_authorized': False,
        'formal_certification': False, 'certified_threshold': None, 'verified_pass_signals': 0})


def close(joined):
    """Fresh transaction closure used by the independent decision owner."""
    registration.close(joined['pack'])
    cohort.check(joined['bindings'])
    packet.require(budget.same(registration.development_review.evidence(joined['directories']) if joined['directories'] else [], joined['evidence']),
                   'formal original sender directory changed during decision')
    packet.require(budget.same(sender.build_info(joined['build_info_executable']), joined['build_info']), 'formal compiled sender changed')
    loaded_check()
