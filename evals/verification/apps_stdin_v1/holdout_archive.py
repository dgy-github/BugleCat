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
    formal_originals,
    history,
    packet,
    protocol,
)

SCHEMA = 'buglecat.verification.stdin-holdout-original-archive.v1'


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    formal_originals.loaded_check()


def calibration(registration_file, current, finished, sql):
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
        budget.raw_bindings([path, registered['dataset_file'], formal_originals.__file__, formal_archive.__file__]),
        *[scope['files'] for scope in cal_evidence])
    source = cohort._source(value['source_proposal_file'], value['source_snapshot_file'])
    chain = development_originals._facts(value, source)
    packet.require(Path(registered['opening_file']).absolute() == calibration_originals.opening.marker_path(chain, value['artifact_hash'])
                   and cohort.sha(registered['opening_file']) == registered['opening_sha256']
                   and opening['artifact_hash'] == registered['opening_hash'], 'original calibration source opening changed')
    packet.require(budget.same(registered['ledger_snapshot']['jev_reservations'],
                              opening['original_accounting_image']['snapshot']['jev_reservations'])
                   and budget.same(registered['original_reservation_bindings'],
                                   opening['original_accounting_image']['raw_budget_bindings']),
                   'blind calibration baseline must preserve exactly the genuine pre-score Jev archive')
    # Actual complete main never changes after blind registration. Historical
    # image is independently checked against those real unchanged rows/bytes.
    history._original(registered['ledger_snapshot'], registered['original_finished'], current, finished)
    original_sql = registered['original_sql_image']
    packet.require(budget.same(sql[0], original_sql[0])
                   and budget.same(sql[1][:len(original_sql[1])], original_sql[1])
                   and budget.same(original_sql[2], [[len(registered['ledger_snapshot']['main_started'])]]),
                   'original raw calibration SQL history changed')
    _, new_main = history._original(opening['original_accounting_image']['snapshot'],
        opening['original_accounting_image']['finished'], registered['ledger_snapshot'], registered['original_finished'])
    history._new_main(new_main, finished, opening['cases'], value['artifact_hash'], chain['current_sha256'])
    packet.require(len(new_main) == 400 and all(finished[row['group_id']] is not None for row in new_main),
                   'original whole400 calibration main must remain terminal')
    identity = {'calibration_opening_file': str(Path(registered['opening_file']).absolute()),
        'calibration_opening_sha256': registered['opening_sha256'], 'calibration_opening_hash': opening['artifact_hash']}
    packet.require(all(all(row['started'].get(key) == child for key, child in identity.items()) for row in new_main),
                   'actual original calibration opening identity changed')
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
    report = formal_originals._terminal_report(joined, registered)
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
    old = history._reservations(registered['ledger_snapshot']['jev_reservations'])
    actual = history._reservations(current['jev_reservations'])
    packet.require(budget.same(actual[:len(old)], old), 'original dev Jev prefix changed')
    own = actual[len(old):len(old) + len(requests)]
    packet.require(len(own) == len(requests), 'complete original calibration observations required')
    for request, reserved in zip(requests, own, strict=True):
        packet.require(reserved.get('group_id') == request['group_id']
                       and reserved.get('request_hash') == request['request_hash']
                       and reserved.get('registration_file') == str(path)
                       and reserved.get('registration_sha256') == raw
                       and reserved.get('registration_hash') == registered['artifact_hash']
                       and reserved.get('authorization_sha256') == chain['current_sha256'],
                       'original calibration Jev prefix identity changed')
    pack['new_reservations'] = own
    formal_originals._slots(pack)
    return pack


def accepted(marker, calibration_pack):
    """Rebuild the actual original decision/opening from raw historical joins.

    The current full ledger stays in calibration_pack. The named original
    accounting image is used only to render its already authenticated history.
    No old pre-score/current validator receives a substituted current image.
    """
    from . import formal_decision, formal_jev, formal_observations, holdout_opening

    pack = calibration_pack
    path = Path(marker['accepted_decision_file']).resolve(strict=True)
    report_raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    registered = pack['registration']
    packet.require(marker['accepted_decision_sha256'] == report_raw
                   and marker['accepted_decision_hash'] == saved['artifact_hash']
                   and saved['schema'] == formal_decision.SCHEMA
                   and saved['registration_file'] == pack['registration_file']
                   and saved['registration_sha256'] == pack['registration_sha256'],
                   'accepted original decision/registration raw binding changed')
    executable = saved['sender_executable_file']
    compiled = formal_jev.build_info(executable)
    directories = [Path(value).resolve(strict=True) for value in saved['prepared_directories']]
    observations, expected_dirs = [], []
    root = Path(pack['chain']['authorization']['jev']['ledger_dir'])
    for reserved in pack['new_reservations']:
        row = formal_jev._validate(reserved['prepared_file'], pack, compiled)
        directory = Path(row['output_directory']).resolve(strict=True)
        expected_dirs.append(directory)
        reservation = root / 'reservations' / (budget.digest(row['group_id']) + '.json')
        observations.append(formal_observations._join(row, Path(reserved['prepared_file']), reserved,
                            directory, compiled, reservation_file=reservation))
    packet.require(directories == expected_dirs and len(observations) == len(pack['requests']),
                   'complete original formal sender order required')
    evidence = development_review.evidence(directories)
    sender_bindings = cohort.source_authority._merge_bindings(pack['proof_bindings'],
        [{'file': str(child.resolve()), 'sha256': compiled[role]} for role, child in formal_jev.runner_sources().items()],
        [{'file': str(Path(executable).resolve()), 'sha256': compiled['runner_exe_sha256']}],
        *[scope['files'] for scope in evidence])
    bindings = cohort.source_authority._merge_bindings(budget.raw_bindings([formal_decision.__file__, executable]),
                                                      sender_bindings, pack['proof_bindings'])
    image = marker['original_accounting_image']
    old = registered['ledger_snapshot']
    baseline = image['snapshot']
    history._original(old, registered['original_finished'], baseline, image['finished'])
    packet.require(budget.same(baseline['main_started'], old['main_started'])
                   and budget.same(image['finished'], registered['original_finished'])
                   and budget.same(image['sql_image'], registered['original_sql_image'])
                   and budget.same(history._reservations(baseline['jev_reservations']),
                                   [*history._reservations(old['jev_reservations']), *pack['new_reservations']]),
                   'opening original accounting does not equal authenticated full calibration')
    groups = {row['group_id'] for row in baseline['main_started']} | {row['group_id'] for row in baseline['main_reservations']}
    packet.require(baseline['remaining_main_attempts'] == pack['chain']['authorization']['main_model']['max_attempts'] - len(groups)
                   and baseline['excluded_groups'] == sorted(groups | {row['group_id'] for row in baseline['jev_reservations']}),
                   'original opening counters/exclusion image changed')
    formal_archive.raw_prefix(image['raw_budget_bindings'], pack['budget_bindings'])
    decision = formal_decision._decision(pack['cohort'], pack['dataset'], registered, pack['requests'], observations,
                                         protocol.specification())
    body = {'schema': formal_decision.SCHEMA, **decision,
        'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
        'registration_hash': registered['artifact_hash'], 'cohort_hash': pack['cohort']['artifact_hash'],
        'sender_executable_file': str(Path(executable).resolve(strict=True)), 'sender_executable_sha256': cohort.sha(executable),
        'prepared_directories': [str(path) for path in directories], 'original_observations_hash': budget.digest(observations),
        'original_sender_evidence': evidence, 'original_source_bindings': bindings,
        'whole_original_accounting_digest': budget.digest(image),
        'actual_main_count': len(baseline['main_started']), 'actual_phase_jev_count': len(baseline['jev_reservations']),
        'historical_pending_main': sum(value is None for value in image['finished'].values()),
        'protocol_hash': budget.digest(protocol.specification()), 'decision_owner_sha256': cohort.sha(formal_decision.__file__),
        'report_model_calls': 0, 'report_jev_calls': 0, 'report_oracle_executions': 0,
        'report_reservations_or_phase_markers': 0, 'overall_goal_complete': False,
        'scope': 'Qualified calibration for restricted original STDIN pairs; fresh once-only holdout remains required'}
    report = cohort.seal(body)
    packet.require(budget.same(saved, report) and decision['calibration_gate_met'] is True
                   and decision['all_registered_binary_terminal'] is True
                   and decision['certified_threshold'] is None,
                   'once holdout requires actual complete qualified calibration')
    opening_bindings = cohort.source_authority._merge_bindings(budget.raw_bindings([path, holdout_opening.__file__]), bindings)
    historical_pack = {**pack, 'current': baseline, 'finished': image['finished'], 'sql_image': image['sql_image'],
                       'budget_bindings': image['raw_budget_bindings']}
    joined = {'proof': {'pack': historical_pack, 'evidence': evidence, 'directories': directories},
              'report': report, 'bindings': bindings, 'image': image}
    opened = {'decision': joined, 'report_file': str(path), 'report_sha256': report_raw,
              'cases': [case for case in pack['cohort']['cases'] if case['split'] == 'holdout'], 'bindings': opening_bindings}
    packet.require(budget.same(holdout_opening._body(opened), marker),
                   'actual fixed holdout marker differs from original qualified evidence')
    cohort.check(opening_bindings)
    return {'report': report, 'formal_directories': directories, 'formal_evidence': evidence,
            'calibration_dataset': pack['dataset'], 'bindings': opening_bindings}
