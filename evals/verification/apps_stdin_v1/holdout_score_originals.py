"""Original full holdout labels/blind registration and actual score append.

No old main/pre-score validator receives a trimmed ledger. Every action reads
the actual complete SQL/books, reconstructs the once-qualified archive and
all raw holdout candidates/Oracle receipts before joining new score slots.
"""
import sys
from pathlib import Path

from . import (
    budget,
    cohort,
    development_review,
    formal_archive,
    history,
    holdout_collection,
    holdout_intake,
    holdout_originals,
    holdout_registration,
    holdout_terminal,
    packet,
    protocol,
)
from . import calibration_originals as calibration_originals
from . import development_originals as development_originals

SCHEMA = 'buglecat.verification.stdin-holdout-score-originals.v1'


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    holdout_intake.loaded_check()
    holdout_registration.loaded_check()


def close(pack):
    holdout_originals.close(pack)
    packet.require(cohort.sha(pack['registration_file']) == pack['registration_sha256'], 'original blind holdout registration bytes changed')
    loaded_check()


def _report(joined, registered):
    """Historical pure rendering after original intake/labels are authenticated."""
    pack, data = joined['pack'], joined['dataset']
    fixed = [{key: row[key] for key in holdout_collection.collection.history_fields()} for row in pack['cohort']['cases']]
    decision = holdout_terminal._decision(fixed, data['cases'], pack['calibration_dataset']['cases'], protocol.specification())
    holdout_terminal._dataset_counts(data, decision)
    packet.require(decision['independent_quotas_met'] is True, 'whole400 original holdout quotas required')
    image = {'snapshot': registered['ledger_snapshot'], 'finished': registered['original_finished'],
             'sql_image': registered['original_sql_image'], 'raw_budget_bindings': registered['original_reservation_bindings']}
    body = {'schema': holdout_terminal.SCHEMA, **decision,
        **{key: data[key] for key in ('cohort_file', 'cohort_sha256', 'cohort_hash', 'opening_file', 'opening_sha256',
            'opening_hash', 'collector_executable_file', 'prepared_directories', 'qualified_calibration_threshold')},
        'dataset_file': registered['dataset_file'], 'dataset_sha256': cohort.sha(registered['dataset_file']),
        'dataset_hash': data['artifact_hash'], 'collector_build_info': data['build_info'],
        'original_holdout_evidence': joined['evidence'], 'original_source_bindings': pack['proof_bindings'],
        'whole_original_accounting_digest': budget.digest(image),
        'current_main_count': len(image['snapshot']['main_started']), 'current_phase_jev_count': len(image['snapshot']['jev_reservations']),
        'historical_main_pending': sum(value is None for value in image['finished'].values()),
        'protocol_hash': budget.digest(protocol.specification()), 'terminal_owner_sha256': cohort.sha(holdout_terminal.__file__),
        'holdout_scores_hidden': True, 'holdout_jev_probabilities_read': False,
        'report_model_calls': 0, 'report_jev_calls': 0, 'report_oracle_executions': 0,
        'report_reservations_or_phase_markers': 0, 'overall_goal_complete': False,
        'scope': 'Original restricted STDIN holdout labels; fixed calibration threshold is not yet certified'}
    return cohort.seal(body)


def requests(joined):
    """Exact frozen packet projection, one actual local policy context per action."""
    pack, data = joined['proof']['pack'], joined['proof']['dataset']
    threshold = pack['opening']['qualified_calibration_threshold']
    rows = [row for row in data['cases'] if row['split'] == 'holdout' and row['call_eligible']]
    packet.require(joined['report']['independent_quotas_met'] is True and 200 <= len(rows) <= 400
                   and type(threshold) in (int, float) and threshold in protocol.GRID
                   and budget.same(data['qualified_calibration_threshold'], threshold), 'original fixed qualified threshold required')
    # The rendering pack names the actual authenticated registration-time image.
    # Current remaining funds are checked independently by snapshot/reservation.
    packet.require(pack['chain']['authorization']['jev']['max_attempts'] - len(pack['current']['jev_reservations']) >= len(rows),
                   'entire original binary registration was not funded')
    context = formal_archive.packet_context(pack['cohort']['oracle_runtime_binding'])
    result = []
    for row in rows:
        task = pack['source_pack']['tasks'][row['task_id']]
        code = row['candidate_code_span']['code']
        packet.require(type(code) is str and holdout_intake.intake.oracle.raw_sha(code.encode()) == row['candidate_code_sha256'],
                       'original complete holdout UTF8 code span changed')
        body = formal_archive._packet(task['question'], task['starter_code'], code, task['input_output'], context)
        result.append({**{key: row[key] for key in development_review.IDENTITY}, 'label': row['label'],
            'candidate_code_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
            'request': body, 'request_hash': budget.digest(body), 'request_json': packet.canonical_bytes(body).decode()})
    formal_archive.close_context(context)
    return result


def current_prefix(registered, current, finished, sql, rows, path, raw):
    baseline = registered['ledger_snapshot']
    _, new = history._original(baseline, registered['original_finished'], current, finished)
    packet.require(not new and budget.same(sql, registered['original_sql_image'])
                   and budget.same(finished, registered['original_finished']), 'blind holdout main history cannot append or change')
    old, actual = history._reservations(baseline['jev_reservations']), history._reservations(current['jev_reservations'])
    packet.require(budget.same(actual[:len(old)], old), 'original dev/cal Jev history changed')
    appended = actual[len(old):]
    packet.require(200 <= len(rows) <= 400 and len(appended) <= len(rows)
                   and registered['eligible_ids'] == [row['id'] for row in rows]
                   and registered['request_hashes'] == [row['request_hash'] for row in rows], 'fixed genuine ordered holdout binary requests required')
    for item, row in zip(appended, rows, strict=False):
        packet.require(item.get('schema') == 'buglecat.verification.paid-reservation.v1' and item.get('kind') == 'jev'
                       and item.get('group_id') == row['group_id'] and item.get('request_hash') == row['request_hash']
                       and item.get('registration_file') == str(path) and item.get('registration_sha256') == raw
                       and item.get('registration_hash') == registered['artifact_hash']
                       and item.get('authorization_sha256') == current['authorization_chain']['current_sha256']
                       and item.get('status') == 'reserved_before_transport'
                       and type(item.get('automatic_retries')) is int and item['automatic_retries'] == 0,
                       'actual new holdout Jev slot skips/changes original registration')
    groups = {row['group_id'] for row in current['main_started']} | {row['group_id'] for row in current['main_reservations']}
    packet.require(current['excluded_groups'] == sorted(groups | {row['group_id'] for row in actual}),
                   'current whole-source exclusion image changed')
    return appended


def proof(registration_file):
    loaded_check()
    path = Path(registration_file).absolute()
    raw = cohort.sha(path)
    registered = cohort.sealed(budget.load(path))
    packet.require(registered.get('schema') == holdout_registration.SCHEMA, 'actual original blind holdout registration required')
    before = development_review.evidence(registered['prepared_directories'])
    locked = budget.raw_bindings([path, registered['dataset_file'], holdout_terminal.__file__,
                                 registered['collector_executable_file'], *holdout_collection.runner_sources().values()])
    pack = holdout_originals.archive(registered['opening_file'])
    packet.require(path == holdout_registration.marker_path(pack), 'actual shared fixed blind holdout marker path required')
    baseline = registered['ledger_snapshot']
    history._original(baseline, registered['original_finished'], pack['current'], pack['finished'])
    packet.require(budget.same(pack['sql_image'], registered['original_sql_image'])
                   and budget.same(pack['finished'], registered['original_finished'])
                   and budget.same(baseline['jev_reservations'], pack['opening']['original_accounting_image']['snapshot']['jev_reservations'])
                   and budget.same(registered['original_reservation_bindings'], pack['opening']['original_accounting_image']['raw_budget_bindings']),
                   'genuine blind whole holdout baseline/main history changed')
    own = pack['new_main']
    packet.require(len(own) == 400 and all(pack['finished'][row['group_id']] is not None for row in own),
                   'all fixed400 holdout main must be terminal')
    joined = holdout_intake._proof(registered['opening_file'], registered['prepared_directories'],
                                  registered['collector_executable_file'], execute_new_oracles=False, pack=pack)
    original = cohort.sealed(budget.load(registered['dataset_file']))
    packet.require(budget.same(joined['dataset'], original) and budget.same(joined['evidence'], before),
                   'original complete holdout outcomes/Oracle/directory bytes changed')
    # Match the original main proof's full evidence graph, without admitting a
    # saved boolean, filtered current ledger or old pre-score validator.
    sorted_evidence = sorted(joined['evidence'], key=lambda scope: scope['directory'])
    pack['directory_images'].append(([scope['directory'] for scope in sorted_evidence], sorted_evidence))
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'],
        *[scope['files'] for scope in sorted_evidence])
    report = _report(joined, registered)
    packet.require(budget.same(report, registered['terminal_report']), 'actual original holdout terminal policy/report changed')
    rendering = {**pack, 'current': baseline, 'finished': registered['original_finished'],
        'sql_image': registered['original_sql_image'], 'budget_bindings': registered['original_reservation_bindings']}
    terminal = {'proof': {**joined, 'pack': rendering}, 'report': report,
                'locked': [row for row in locked if row['file'] != str(path)],
                'registration_owner_bindings': budget.raw_bindings([holdout_registration.__file__])}
    rows = requests(terminal)
    packet.require(budget.same(holdout_registration.value(terminal, rows), registered),
                   'original blind holdout marker is not authentic full raw archive projection')
    cohort.check(locked)
    formal_archive.raw_prefix(registered['original_reservation_bindings'], pack['budget_bindings'])
    pack.update(registration=registered, registration_file=str(path), registration_sha256=raw,
                dataset=original, requests=rows, directories=joined['directories'], evidence=joined['evidence'])
    pack['new_reservations'] = current_prefix(registered, pack['current'], pack['finished'], pack['sql_image'], rows, path, raw)
    # Raw history is an exact set plus these new slot/claim files, not merely a
    # subset that could conceal an omitted original file in a self-signed image.
    original_files = {item['file'] for item in registered['original_reservation_bindings']}
    added_files = set()
    reservation_root = Path(pack['chain']['authorization']['jev']['ledger_dir']) / 'reservations'
    for reserved in pack['new_reservations']:
        target = reservation_root / (budget.digest(reserved['group_id']) + '.json')
        added_files.add(str(target.resolve(strict=True)))
        if target.with_suffix('.started').exists():
            added_files.add(str(target.with_suffix('.started').resolve(strict=True)))
    packet.require({item['file'] for item in pack['budget_bindings']} == original_files | added_files,
                   'actual holdout raw budget set differs from original plus genuine slots')
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], locked,
        budget.raw_bindings([__file__]))
    _slots(pack)
    close(pack)
    return pack


def _slots(pack):
    from . import holdout_jev, holdout_observations
    images, added = {}, []
    root = Path(pack['chain']['authorization']['jev']['ledger_dir'])
    for reserved in pack['new_reservations']:
        prepared = Path(reserved['prepared_file'])
        cohort.check([{'file': str(prepared.resolve()), 'sha256': reserved['prepared_sha256']}])
        row = cohort.sealed(budget.load(prepared))
        executable = row['sender_executable_file']
        if executable not in images:
            images[executable] = holdout_jev.build_info(executable)
            added.extend({'file': str(child.resolve()), 'sha256': images[executable][role]} for role, child in holdout_jev.runner_sources().items())
            added.append({'file': str(Path(executable).resolve()), 'sha256': images[executable]['runner_exe_sha256']})
        compiled = images[executable]
        holdout_jev._validate(prepared, pack, compiled)
        expected = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
            'group_id': row['group_id'], 'request_hash': row['request_hash'], 'authorization_sha256': pack['chain']['current_sha256'],
            'reservation_index': reserved['reservation_index'], 'status': 'reserved_before_transport', 'automatic_retries': 0,
            'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
            'registration_hash': pack['registration']['artifact_hash'], 'prepared_file': str(prepared.resolve()),
            'prepared_sha256': cohort.sha(prepared), 'sender_executable_sha256': compiled['runner_exe_sha256'],
            'output_directory': row['output_directory'], 'endpoint_sha256': row['endpoint_sha256']}
        packet.require(budget.same(expected, reserved), 'actual holdout compiled/prepared/reserved body changed')
        added.append({'file': str(prepared.resolve()), 'sha256': reserved['prepared_sha256']})
        output = Path(row['output_directory'])
        reservation = root / 'reservations' / (budget.digest(row['group_id']) + '.json')
        if output.exists():
            image = development_review.evidence([output])
            pack['directory_images'].append(([str(output)], image))
            added.extend(image[0]['files'])
            if (output / 'receipt.json').exists():
                holdout_observations._join(row, prepared, reserved, output, compiled, reservation_file=reservation)
        elif reservation.with_suffix('.started').exists():
            raise ValueError('original claimed holdout output directory deleted')
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], added)
