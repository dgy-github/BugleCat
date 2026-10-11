"""Formal calibration current-prefix association kernel.

Every action rebuilds original source/main/Oracle/blind registration once and
associates the actual whole ledger with its original prefix. No historical
snapshot is installed as a current ledger or handed to a pre-score validator.
"""
import sys
from pathlib import Path

from . import (
    budget,
    calibration_collection,
    calibration_originals,
    calibration_registration,
    calibration_terminal,
    cohort,
    development_originals,
    development_review,
    formal_archive,
    history,
    packet,
    protocol,
)

SCHEMA = 'buglecat.verification.stdin-formal-calibration-originals.v1'


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    budget.loaded_check()
    formal_archive.loaded_check()
    calibration_registration.loaded_check()


def close(pack):
    cohort.check(pack['proof_bindings'])
    packet.require(cohort.sha(pack['registration_file']) == pack['registration_sha256'], 'original blind registration raw bytes changed')
    packet.require(budget.same(budget._snapshot_chain(pack['chain']), (pack['current'], pack['finished']))
                   and budget.same(budget._ledger_rows(Path(pack['current']['main_ledger_file'])), pack['sql_image'])
                   and budget.same(development_originals.binding_images(pack['chain']), pack['budget_bindings']),
                   'formal whole actual accounting changed during proof')
    for directories, captured in pack['directory_images']:
        packet.require(budget.same(development_review.evidence(directories), captured), 'original formal archive directory changed')
    loaded_check()


def _terminal_report(joined, registered):
    """Exact historical report rendering, not a current accounting validator."""
    data, pack = joined['dataset'], joined['pack']
    fixed = [{key: row[key] for key in calibration_originals.collection.history_fields()} for row in pack['cohort']['cases']]
    spec = protocol.specification()
    decision = calibration_terminal._decision(fixed, data['cases'], spec)
    calibration_terminal._dataset_counts(data, decision)
    packet.require(decision['independent_quotas_met'] is True, 'genuine full400 independent quotas required')
    image = {'snapshot': registered['ledger_snapshot'], 'finished': registered['original_finished'],
             'sql_image': registered['original_sql_image'], 'raw_budget_bindings': registered['original_reservation_bindings']}
    return cohort.seal({'schema': calibration_terminal.SCHEMA, **decision,
        'dataset_file': registered['dataset_file'], 'dataset_sha256': cohort.sha(registered['dataset_file']),
        'dataset_hash': data['artifact_hash'], 'opening_file': data['opening_file'], 'opening_sha256': data['opening_sha256'],
        'opening_hash': data['opening_hash'], 'cohort_file': data['cohort_file'], 'cohort_sha256': data['cohort_sha256'],
        'cohort_hash': data['cohort_hash'], 'collector_executable_file': data['collector_executable_file'],
        'collector_build_info': data['build_info'], 'prepared_directories': data['prepared_directories'],
        'original_calibration_evidence': joined['evidence'], 'original_source_bindings': pack['bindings'],
        'current_main_count': len(image['snapshot']['main_started']), 'current_phase_jev_count': len(image['snapshot']['jev_reservations']),
        'historical_main_pending': sum(row is None for row in image['finished'].values()),
        'original_accounting_digest': budget.digest(image), 'protocol_hash': budget.digest(spec),
        'terminal_owner_sha256': cohort.sha(calibration_terminal.__file__), 'scores_hidden': True, 'jev_probabilities_read': False,
        'report_model_calls': 0, 'report_jev_calls': 0, 'report_oracle_executions': 0, 'report_reservations': 0,
        'overall_goal_complete': False, 'scope': 'Original restricted STDIN calibration labels only; no threshold or transport authority'})


def proof(registration_file):
    """Fresh real archive once, actual complete SQL and Jev prefix, no writes."""
    loaded_check()
    path = Path(registration_file).absolute()
    raw = cohort.sha(path)
    registered = cohort.sealed(budget.load(path))
    packet.require(registered.get('schema') == calibration_registration.SCHEMA, 'actual formal calibration registration required')
    opening = cohort.sealed(budget.load(registered['opening_file']))
    value = cohort.sealed(budget.load(opening['cohort_file']))
    reviewed = cohort.sealed(budget.load(opening['accepted_review_file']))
    dev_registered = cohort.sealed(budget.load(reviewed['registration_file']))
    dev_evidence = development_review.evidence(dev_registered['prepared_directories'])
    dev_jev_evidence = development_review.evidence(reviewed['prepared_directories'])
    cal_evidence = development_review.evidence(registered['prepared_directories'])
    original_bindings = cohort.source_authority._merge_bindings(opening['bindings'], value['bindings'],
        budget.raw_bindings([registered['opening_file'], opening['accepted_review_file'], reviewed['registration_file'],
            dev_registered['dataset_file'], dev_registered['main_executable'], reviewed['sender_executable_file'], calibration_originals.__file__]),
        *[scope['files'] for scope in [*dev_evidence, *dev_jev_evidence]])
    proof_bindings = cohort.source_authority._merge_bindings(original_bindings, registered['bindings'],
        budget.raw_bindings([path, registered['dataset_file'], __file__, formal_archive.__file__]),
        *[scope['files'] for scope in cal_evidence])
    source = cohort._source(value['source_proposal_file'], value['source_snapshot_file'])
    chain = development_originals._facts(value, source)
    packet.require(Path(registered['opening_file']).absolute() == calibration_originals.opening.marker_path(chain, value['artifact_hash'])
                   and cohort.sha(registered['opening_file']) == registered['opening_sha256']
                   and opening['artifact_hash'] == registered['opening_hash'], 'original calibration source opening changed')
    current, finished = budget._snapshot_chain(chain)
    sql = budget._ledger_rows(Path(current['main_ledger_file']))
    packet.require(budget.same(registered['ledger_snapshot']['jev_reservations'],
                              opening['original_accounting_image']['snapshot']['jev_reservations'])
                   and budget.same(registered['original_reservation_bindings'],
                                   opening['original_accounting_image']['raw_budget_bindings']),
                   'blind calibration baseline must preserve exactly the genuine pre-score Jev archive')
    # Actual complete main never changes after blind registration. Historical
    # image is independently checked against those real unchanged rows/bytes.
    packet.require(budget.same(sql, registered['original_sql_image']) and budget.same(finished, registered['original_finished'])
                   and budget.same(current['main_started'], registered['ledger_snapshot']['main_started']),
                   'original full calibration main cannot append or change during scoring')
    _, new_main = history._original(opening['original_accounting_image']['snapshot'],
        opening['original_accounting_image']['finished'], current, finished)
    history._new_main(new_main, finished, opening['cases'], value['artifact_hash'], chain['current_sha256'])
    packet.require(len(new_main) == 400 and all(finished[row['group_id']] is not None for row in new_main),
                   'original whole400 calibration main must remain terminal')
    identity = {'calibration_opening_file': str(Path(registered['opening_file']).absolute()),
        'calibration_opening_sha256': registered['opening_sha256'], 'calibration_opening_hash': opening['artifact_hash']}
    packet.require(all(all(row['started'].get(key) == child for key, child in identity.items()) for row in new_main),
                   'actual calibration main lacks original opening identity')
    archive = formal_archive.development_archive(opening, value, source, chain, current, finished)
    pack = {'schema': SCHEMA, 'registration': registered, 'registration_file': str(path), 'registration_sha256': raw,
        'opening': opening, **identity, 'cohort': value, 'source_pack': source, 'chain': chain,
        'current': current, 'finished': finished, 'sql_image': sql, 'new_main': new_main,
        'archive': archive, 'bindings': original_bindings, 'proof_bindings': proof_bindings,
        'budget_bindings': development_originals.binding_images(chain),
        'directory_images': [(dev_registered['prepared_directories'], dev_evidence), (reviewed['prepared_directories'], dev_jev_evidence),
                             (registered['prepared_directories'], cal_evidence)]}
    packet.require(path == calibration_registration.marker_path(pack), 'actual shared blind marker path required')
    formal_archive.raw_prefix(registered['original_reservation_bindings'], pack['budget_bindings'])
    joined = formal_archive.calibration_dataset(pack, registered)
    original_data = cohort.sealed(budget.load(registered['dataset_file']))
    packet.require(budget.same(joined['dataset'], original_data), 'original full808 outcome/span/Oracle/source dataset changed')
    report = _terminal_report(joined, registered)
    packet.require(budget.same(report, registered['terminal_report']), 'original blind terminal report changed')
    # These are named historical rendering inputs only. Neither current ledger
    # nor a pre-score validator is replaced, called or monkeypatched.
    rendering = {**pack, 'current': registered['ledger_snapshot'], 'finished': registered['original_finished'],
        'sql_image': registered['original_sql_image'], 'budget_bindings': registered['original_reservation_bindings']}
    locked = budget.raw_bindings([registered['dataset_file'], calibration_terminal.__file__,
        registered['collector_executable_file'], *calibration_collection.runner_sources().values()])
    rendering_joined = {'proof': {**joined, 'pack': rendering}, 'report': report, 'locked': locked,
        'registration_owner_bindings': budget.raw_bindings([calibration_registration.__file__])}
    requests = formal_archive._requests(rendering_joined, joined['packet_context'])
    formal_archive.close_context(joined['packet_context'])
    packet.require(budget.same(calibration_registration.value(rendering_joined, requests), registered),
                   'actual original registration differs from complete original archive projection')
    pack.update(dataset=joined['dataset'], requests=requests, directories=joined['directories'], evidence=joined['evidence'])
    pack['new_reservations'] = current_prefix(registered, current, finished, sql, requests, path, raw)
    _slots(pack)
    close(pack)
    return pack


def _slots(pack):
    """Authenticate each real added slot, including stranded/pending attempts.

    A response is interpreted only to match its immutable recorded terminal
    status; no score is used for source selection or permission. The independent
    observer later returns the original numerical observations to the decision.
    """
    from . import formal_jev, formal_observations
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve()
    images = {}
    added_bindings = []
    for reserved in pack['new_reservations']:
        prepared = Path(reserved['prepared_file'])
        raw = reserved['prepared_sha256']
        cohort.check([{'file': str(prepared.resolve()), 'sha256': raw}])
        row = cohort.sealed(budget.load(prepared))
        executable = str(Path(row['sender_executable_file']).resolve())
        if executable not in images:
            images[executable] = formal_jev.build_info(executable)
            compiled = images[executable]
            added_bindings.extend({'file': str(path.resolve()), 'sha256': compiled[role]}
                                  for role, path in formal_jev.runner_sources().items())
            added_bindings.append({'file': executable, 'sha256': compiled['runner_exe_sha256']})
        compiled = images[executable]
        formal_jev._validate(prepared, pack, compiled)
        expected = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
            'group_id': row['group_id'], 'request_hash': row['request_hash'],
            'authorization_sha256': pack['chain']['current_sha256'],
            'reservation_index': reserved['reservation_index'], 'status': 'reserved_before_transport', 'automatic_retries': 0,
            'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
            'registration_hash': pack['registration']['artifact_hash'], 'prepared_file': str(prepared.resolve()),
            'prepared_sha256': raw, 'sender_executable_sha256': compiled['runner_exe_sha256'],
            'output_directory': row['output_directory'], 'endpoint_sha256': row['endpoint_sha256']}
        packet.require(budget.same(expected, reserved), 'actual added reservation/compiled/prepared identity changed')
        added_bindings.append({'file': str(prepared.resolve()), 'sha256': raw})
        output = Path(row['output_directory'])
        reservation = root / 'reservations' / (budget.digest(row['group_id']) + '.json')
        if output.exists():
            image = development_review.evidence([output])
            pack['directory_images'].append(([str(output)], image))
            added_bindings.extend(image[0]['files'])
            if (output / 'receipt.json').exists():
                formal_observations._join(row, prepared, reserved, output, compiled, reservation_file=reservation)
        elif reservation.with_suffix('.started').exists():
            raise ValueError('original claimed output directory deleted')
    # Keep every row's original raw join, then resolve/merge the full inherited
    # graph once. Re-merging thousands of inherited paths for each slot is O(N²).
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], added_bindings)


def current_prefix(registered, current, finished, sql_image, requests, registration_file, registration_sha256):
    """Associate actual unchanged main and complete genuine Jev append prefix.

    Pure validation is not a substitute for raw source, Oracle or transport
    authentication. No copied/filtered ledger image is returned or installed.
    """
    baseline = registered['ledger_snapshot']
    _, appended_main = history._original(baseline, registered['original_finished'], current, finished)
    packet.require(not appended_main and budget.same(current['main_started'], baseline['main_started'])
                   and budget.same(finished, registered['original_finished'])
                   and budget.same(sql_image, registered['original_sql_image']),
                   'formal calibration cannot add, finish, trim or reinterpret original main history')
    stable = ('main_ledger_file', 'main_metadata', 'main_reservations', 'authorization_chain', 'remaining_main_attempts')
    packet.require(all(budget.same(current[key], baseline[key]) for key in stable), 'original formal main accounting changed')
    original = history._reservations(baseline['jev_reservations'])
    actual = history._reservations(current['jev_reservations'])
    packet.require(len(actual) >= len(original) and budget.same(actual[:len(original)], original),
                   'original development Jev prefix deleted or changed')
    new = actual[len(original):]
    packet.require(type(requests) is list and 200 <= len(requests) <= 400 and len(new) <= len(requests)
                   and [row['id'] for row in requests] == registered['eligible_ids']
                   and [row['request_hash'] for row in requests] == registered['request_hashes']
                   and len({row['group_id'] for row in requests}) == len(requests),
                   'complete genuine registered binary request order required')
    for item, request in zip(new, requests, strict=False):
        packet.require(item.get('schema') == 'buglecat.verification.paid-reservation.v1'
                       and item.get('kind') == 'jev' and item.get('group_id') == request['group_id']
                       and item.get('request_hash') == request['request_hash']
                       and item.get('registration_file') == str(Path(registration_file).absolute())
                       and item.get('registration_sha256') == registration_sha256
                       and item.get('registration_hash') == registered['artifact_hash']
                       and item.get('authorization_sha256') == current['authorization_chain']['current_sha256']
                       and item.get('status') == 'reserved_before_transport'
                       and type(item.get('automatic_retries')) is int and item['automatic_retries'] == 0,
                       'current formal Jev slot skips or changes original registration/request/authorization')
    main_groups = {row['group_id'] for row in current['main_started']} | {row['group_id'] for row in current['main_reservations']}
    packet.require(current['excluded_groups'] == sorted(main_groups | {row['group_id'] for row in actual})
                   and current['remaining_main_attempts'] >= 400
                   and current['authorization_chain']['authorization']['jev']['max_attempts'] - len(actual) >= 400,
                   'actual full exclusion set or unopened holdout funding changed')
    return new
