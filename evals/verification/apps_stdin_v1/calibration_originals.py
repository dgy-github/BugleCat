"""Fresh development archive plus genuine calibration-prefix proof.

Historical accounting is authenticated against actual current rows and raw
files. It is never installed as a replacement current ledger or passed to a
pre-Jev public validator. One whole C1 source proof per transaction.
"""
import sys
from collections import Counter
from pathlib import Path

from . import (
    budget,
    cohort,
    collection,
    development_acceptance,
    development_jev,
    development_observations,
    development_originals,
    development_registration,
    development_review,
    history,
    intake,
    packet,
    protocol,
)
from . import calibration_opening as opening

SCHEMA = 'buglecat.verification.stdin-calibration-originals.v1'
OPENING_FIELDS = ('calibration_opening_file', 'calibration_opening_sha256', 'calibration_opening_hash')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'OPENING_FIELDS'))
    opening.loaded_check()
    development_originals.loaded_check()


def _prefix(marker, current, finished, sql_image):
    image = marker['original_accounting_image']
    packet.require(set(image) == {'snapshot', 'finished', 'raw_budget_bindings', 'sql_image'},
                   'original accounting image fields changed')
    baseline, old_finished = image['snapshot'], image['finished']
    _, new = history._original(baseline, old_finished, current, finished)
    cases = marker['cases']
    packet.require(type(cases) is list and len(cases) == 400
                   and all(case['split'] == 'calibration' for case in cases), 'fixed400 calibration allocation required')
    history._new_main(new, finished, cases, marker['cohort_hash'], baseline['authorization_chain']['current_sha256'])
    packet.require(budget.same(current['jev_reservations'], baseline['jev_reservations']),
                   'new calibration Jev reservations require a future genuine registration owner')
    packet.require(budget.same(sql_image[0], image['sql_image'][0])
                   and budget.same(sql_image[1][:len(image['sql_image'][1])], image['sql_image'][1])
                   and budget.same(image['sql_image'][2], [[len(baseline['main_started'])]]),
                   'original raw SQL history including pending/failure bytes changed')
    for row in new:
        packet.require(all(row['started'].get(key) == marker['_identity'][key] for key in OPENING_FIELDS),
                       'new calibration start lacks actual opening byte identity')
    return new


def _development_rows(value, source, image, registered):
    baseline, finished = image['snapshot'], image['finished']
    _, dev = history._original(value['ledger_snapshot'], value['original_finished'], baseline, finished)
    history._new_main(dev, finished, history._cases(value['cases'], ('development',)),
                      value['artifact_hash'], value['authorization_sha256'])
    packet.require(len(dev) == 8 and all(finished[row['group_id']] is not None for row in dev),
                   'original eight development main attempts must remain terminal')
    executable = registered['main_executable']
    compiled = intake.build(executable)
    packet.require(budget.same(compiled, value['collector_build_info'])
                   and cohort.sha(executable) == registered['main_executable_sha256'], 'original main compiled image changed')
    own, actual = {row['group_id']: row for row in dev}, {}
    # This is the original historical image named in the opening. It is used
    # solely to associate old receipts, not to authorize a new request.
    local = {'cohort': value, 'source_pack': source}
    for path in registered['prepared_directories']:
        directory = Path(path).resolve(strict=True)
        manifest, _ = collection.validate_manifest(directory / 'collection-manifest.json', pack=local)
        rows = intake.lines(directory / 'prepared.jsonl')
        events = intake.lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
        packet.require(len(rows) == len(manifest['cases'])
                       and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'original development preparation scope changed')
        for row, case in zip(rows, manifest['cases'], strict=True):
            task, case_dir = source['tasks'][case['task_id']], directory / case['id']
            intake._prepared(row, case, task, manifest, directory, compiled)
            start = own.get(case['group_id'])
            matching = start is not None and intake.same_path(start['started']['case_directory'], case_dir)
            original, status = intake.terminal(row, start if matching else None,
                finished[case['group_id']] if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate original development candidate')
                if status == 'ok':
                    intake.verify(row, case_dir / 'receipt.json', events, executable)
                actual[case['group_id']] = (original, status, case_dir, task)
    packet.require(set(actual) == set(own), 'original development started candidate omitted')
    outcomes = []
    for case in value['cases']:
        original, status, directory, task = actual.get(case['group_id'], (None, 'main_not_started', None, None))
        row = {**{key: case[key] for key in collection.history_fields()}, 'main_status': status,
               'label': 'unlabeled' if status == 'main_not_started' else 'unknown', 'reason': status, 'call_eligible': False}
        if original:
            row.update(main_receipt_file=str(directory / 'receipt.json'), main_receipt_sha256=cohort.sha(directory / 'receipt.json'))
        if status == 'ok':
            row.update(intake._oracle(task, original['result']['candidate'], directory, False))
            packet.require(row['reason'] not in ('independent_oracle_not_recorded', 'oracle_attempt_incomplete'),
                           'original development Oracle missing/stranded; never rerun')
        outcomes.append(row)
    evidence = development_review.evidence(registered['prepared_directories'])
    bindings = [row for scope in evidence for row in scope['files']]
    return outcomes, compiled, evidence, bindings


def _archive(marker, value, source, chain, current, finished):
    """Reconstruct actual original main, blind labels, Jev and saved review."""
    reviewed_path = Path(marker['accepted_review_file']).resolve(strict=True)
    reviewed = cohort.sealed(budget.load(reviewed_path))
    registered_path = Path(reviewed['registration_file']).resolve(strict=True)
    registered = cohort.sealed(budget.load(registered_path))
    data_path = Path(registered['dataset_file']).resolve(strict=True)
    data = cohort.sealed(budget.load(data_path))
    image = marker['original_accounting_image']
    # Historical accounting cannot introduce a false cap, reservation, finish,
    # metadata or source exclusion. Actual current prefix checks close SQL too.
    baseline = image['snapshot']
    packet.require(budget.same(baseline['authorization_chain'], chain)
                   and budget.same(baseline['jev_reservations'], current['jev_reservations'])
                   and budget.same(baseline['main_reservations'], current['main_reservations'])
                   and budget.same(baseline['main_metadata'], current['main_metadata']), 'opening accounting lineage changed')
    groups = {row['group_id'] for row in baseline['main_started']} | {row['group_id'] for row in baseline['main_reservations']}
    packet.require(baseline['remaining_main_attempts'] == chain['authorization']['main_model']['max_attempts'] - len(groups)
                   and baseline['excluded_groups'] == sorted(groups | {row['group_id'] for row in baseline['jev_reservations']})
                   and min(baseline['remaining_main_attempts'], chain['authorization']['jev']['max_attempts'] - len(baseline['jev_reservations'])) >= 800,
                   'opening original unspent allocation changed')
    packet.require(budget.same(development_originals.binding_images(chain), image['raw_budget_bindings']),
                   'opening original reservation/marker bytes changed or appended')
    cohort.check(registered['original_reservation_bindings'])
    rows, compiled, evidence, collection_bindings = _development_rows(value, source, image, registered)
    rebuilt = cohort.seal({'schema': intake.SCHEMA, 'cohort_file': registered['cohort_file'], 'cohort_hash': value['artifact_hash'],
        'cohort_sha256': cohort.sha(registered['cohort_file']), 'build_info': compiled,
        'collector_executable_file': str(Path(registered['main_executable']).resolve()), 'cases': rows,
        'allocated_groups': 808, 'collected_groups': 8, 'pending_groups': 0, 'uncollected_groups': 800,
        'label_counts': dict(Counter(row['label'] for row in rows)), 'collection_bindings': collection_bindings,
        'scores_hidden': True, 'jev_probabilities_read': False, 'formal_registration': False, 'holdout_opened': False,
        'production_authorized': False, 'formal_certification': False, 'certified_threshold': None, 'model_calls': 0, 'jev_calls': 0})
    packet.require(budget.same(data, rebuilt), 'original blind development dataset/labels changed')
    requests = []
    for row in rows[:8]:
        if not row['call_eligible']:
            continue
        task = source['tasks'][row['task_id']]
        body = packet.request(task['question'], task['starter_code'], row['candidate_code_span']['code'], task['input_output'], value['oracle_runtime_binding'])
        requests.append({**{key: row[key] for key in development_review.IDENTITY}, 'label': row['label'],
            'candidate_code_sha256': row['candidate_code_sha256'], 'oracle_sha256': row['oracle_sha256'],
            'request': body, 'request_hash': budget.digest(body), 'request_json': packet.canonical_bytes(body).decode()})
    original_bindings = cohort.source_authority._merge_bindings(value['bindings'],
        budget.raw_bindings([data_path, registered['cohort_file'], registered['main_executable'], development_originals.__file__]),
        *[scope['files'] for scope in evidence])
    archive = {'cohort': value, 'dataset': rebuilt, 'dataset_file': str(data_path), 'source': source, 'chain': chain,
        'directories': [Path(path) for path in registered['prepared_directories']], 'bindings': original_bindings,
        'evidence': evidence, 'requests': requests, 'main_executable': str(Path(registered['main_executable']).resolve()),
        'registration': registered, 'registration_file': str(registered_path), 'registration_sha256': cohort.sha(registered_path)}
    packet.require(registered_path == development_registration.marker_path(archive)
                   and budget.same(registered, development_registration.value(archive, registered['ledger_snapshot'],
                                  registered['original_finished'], registered['original_reservation_bindings'])), 'actual original blind registration changed')
    history.validate_prefix(value['ledger_snapshot'], value['original_finished'], baseline, image['finished'],
                            value['cases'], value['artifact_hash'], jev_cases=[row for row in rows[:8] if row['call_eligible']])
    packet.require(budget.same(registered['ledger_snapshot']['main_started'], baseline['main_started'])
                   and budget.same(registered['original_finished'], image['finished'])
                   and budget.same(registered['ledger_snapshot']['jev_reservations'], value['ledger_snapshot']['jev_reservations']),
                   'original blind pre-Jev baseline changed')
    before_jev = registered['ledger_snapshot']
    stable = ('main_ledger_file', 'main_metadata', 'main_reservations', 'authorization_chain', 'remaining_main_attempts')
    original_jev_groups = {row['group_id'] for row in before_jev['jev_reservations']}
    packet.require(set(before_jev) == set(baseline)
                   and all(budget.same(before_jev[key], baseline[key]) for key in stable)
                   and before_jev['excluded_groups'] == sorted(groups | original_jev_groups),
                   'original blind pre-Jev accounting fields changed')
    observer_info = development_jev.build_info(reviewed['sender_executable_file'])
    observations, directories = [], []
    new = history._reservations(baseline['jev_reservations'])[len(value['ledger_snapshot']['jev_reservations']):]
    packet.require(len(new) == len(requests), 'complete development binary prefix required before calibration')
    root = Path(chain['authorization']['jev']['ledger_dir'])
    for request, reserved in zip(requests, new, strict=True):
        row = development_jev._validate(reserved['prepared_file'], archive, observer_info)
        path = root / 'reservations' / (budget.digest(request['group_id']) + '.json')
        packet.require(reserved['request_hash'] == request['request_hash']
                       and reserved['registration_file'] == str(registered_path)
                       and reserved['registration_sha256'] == cohort.sha(registered_path)
                       and reserved['registration_hash'] == registered['artifact_hash']
                       and cohort.sha(reserved['prepared_file']) == reserved['prepared_sha256'], 'original registered Jev slot changed')
        directories.append(Path(row['output_directory']).resolve(strict=True))
        observations.append(development_observations.join(row, Path(reserved['prepared_file']),
            {**reserved, 'reservation_file': str(path)}, directories[-1], reviewed['sender_executable_file']))
    observed_cases = [{**case, 'jev_status': next((row['call_status'] for row in observations if row['group_id'] == case['group_id']), 'jev_not_eligible')}
                      for case in registered['cases']]
    observed_evidence = development_review.evidence(directories)
    observed = cohort.seal({'schema': development_observations.SCHEMA, 'registration_file': str(registered_path),
        'registration_sha256': cohort.sha(registered_path), 'registration_hash': registered['artifact_hash'],
        'cohort_hash': value['artifact_hash'], 'sender_build_info': observer_info,
        'prepared_directories': [str(path) for path in directories], 'cases': observed_cases,
        'eligible_groups': len(requests), 'reserved_groups': len(new), 'terminal_groups': len(observations), 'pending_groups': 0,
        'status_counts': dict(Counter(row['jev_status'] for row in observed_cases)), 'observations': observations,
        'evidence_bindings': observed_evidence, 'budget_snapshot_digest': budget.digest((baseline, image['finished'])),
        'formal_registration': False, 'holdout_opened': False, 'production_authorized': False,
        'formal_certification': False, 'certified_threshold': None, 'verified_pass_signals': 0})
    decision = development_acceptance._decision(registered, observed, protocol.specification())
    expected_review = cohort.seal({'schema': development_acceptance.SCHEMA, 'registration_file': str(registered_path),
        'registration_sha256': cohort.sha(registered_path), 'registration_hash': registered['artifact_hash'],
        'cohort_hash': value['artifact_hash'], 'sender_executable_file': reviewed['sender_executable_file'],
        'sender_executable_sha256': cohort.sha(reviewed['sender_executable_file']), 'sender_build_info': observer_info,
        'prepared_directories': [str(path) for path in directories], 'original_observations_hash': observed['artifact_hash'],
        'original_directory_evidence': observed_evidence, 'budget_snapshot_digest': budget.digest(image),
        'original_registration_bindings': registered['bindings'], 'protocol_hash': budget.digest(protocol.specification()),
        'review_owner_sha256': cohort.sha(development_acceptance.__file__), 'whole_original_allocation': 808,
        'development_groups': 8, 'unopened_groups': 800, **decision,
        **{key: False for key in development_acceptance.NO_AUTHORITY}, 'certified_threshold': None,
        'new_main_calls': 0, 'new_jev_calls': 0, 'new_oracle_executions': 0,
        'scope': 'Original development pipeline only; fresh calibration opening remains required'})
    packet.require(budget.same(reviewed, expected_review) and decision['pipeline_verified'] is True,
                   'original review differs from full actual development archive')
    opening_pack = {'review': reviewed, 'review_file': str(reviewed_path), 'review_sha256': cohort.sha(reviewed_path),
        'campaign': value, 'cohort_file': registered['cohort_file'], 'chain': chain, 'image': image,
        'cases': [case for case in value['cases'] if case['split'] == 'calibration'],
        'bindings': cohort.source_authority._merge_bindings(registered['bindings'],
            budget.raw_bindings([reviewed_path, registered_path, registered['cohort_file'], opening.__file__, development_acceptance.__file__])),
        'original_main_directories': registered['prepared_directories'], 'original_main_evidence': evidence}
    plain = {key: child for key, child in marker.items() if key != '_identity'}
    packet.require(budget.same(plain, opening._body(opening_pack)), 'actual opening is not reconstructed original archive authority')
    return {'registration': registered, 'dataset': data, 'main_directories': registered['prepared_directories'],
            'jev_directories': directories, 'main_evidence': evidence, 'jev_evidence': observed_evidence}


def close(pack):
    cohort.check(pack['bindings'])
    cohort.check(pack.get('calibration_bindings', []))
    packet.require(budget.same(budget._snapshot_chain(pack['chain']), (pack['current'], pack['finished']))
                   and budget.same(development_originals.binding_images(pack['chain']), pack['budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(pack['current']['main_ledger_file'])), pack['sql_image'])
                   and budget.same(development_review.evidence(pack['archive']['main_directories']), pack['archive']['main_evidence'])
                   and budget.same(development_review.evidence(pack['archive']['jev_directories']), pack['archive']['jev_evidence']),
                   'calibration source/archive/current accounting closure changed')
    loaded_check()


def verify_main_prefix(pack):
    """Each actual prior finish needs original prepared/receipt/events/EXE.

    Association of a non-null SQL finish alone is insufficient to allow the next
    group. No candidate Oracle is executed here; even a HTTP failure is joined.
    """
    from . import calibration_collection as collector
    from . import calibration_intake as reader
    directories = sorted({Path(row['started']['manifest_file']).resolve(strict=True).parent for row in pack['new_main']})
    evidence = development_review.evidence(directories) if directories else []
    by_group = {row['group_id']: row for row in pack['new_main']}
    actual = set()
    for directory in directories:
        manifest_path = directory / 'collection-manifest.json'
        if 'source_pack' in pack:
            manifest, _ = collector.validate_manifest(manifest_path, pack=pack)
        else:
            # Between batch-entry full proofs, fixed original public fields and
            # raw captured source graph close the same immutable manifest. No
            # source/member reselection and no hidden task needs projection.
            manifest = cohort.sealed(budget.load(manifest_path))
            packet.require(manifest['schema'] == collector.SCHEMA and manifest['split'] == 'calibration'
                           and 1 <= len(manifest['cases']) <= 8
                           and all(budget.same(manifest[key], pack[key]) for key in OPENING_FIELDS), 'original calibration manifest identity changed')
            fixed = pack['opening']['cases']
            positions = [fixed.index(case) for case in manifest['cases']]
            packet.require(positions == list(range(positions[0], positions[0] + len(positions))), 'original calibration contiguous scope changed')
            public = [{'task_id': case['task_id'], 'source_kind': case['source_kind'],
                       'question': case['public_interface']['question'], 'starter_code': case['public_interface']['starter_code']}
                      for case in manifest['cases']]
            packet.require(manifest['public_tasks'] == [collection.public_task(task) for task in public], 'original public-only prompt changed')
            cohort.check(manifest['phase_proof_bindings'])
        compiled = collector.build(manifest['collector_executable_file'])
        rows = intake.lines(directory / 'prepared.jsonl')
        events = intake.lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
        packet.require(len(rows) == len(manifest['cases']) and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'full calibration prefix preparation scope required')
        for row, case in zip(rows, manifest['cases'], strict=True):
            case_dir = directory / case['id']
            task = pack['source_pack']['tasks'][case['task_id']] if 'source_pack' in pack else {
                'task_id': case['task_id'], 'source_kind': case['source_kind'], **case['public_interface']}
            reader._prepared(row, case, task, manifest, directory, compiled)
            start = by_group.get(case['group_id'])
            matching = start is not None and intake.same_path(start['started']['case_directory'], case_dir)
            _, status = reader.terminal(row, start if matching else None,
                pack['finished'][case['group_id']] if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate actual calibration prefix candidate')
                if status == 'ok':
                    intake.verify(row, case_dir / 'receipt.json', events, manifest['collector_executable_file'])
                actual.add(case['group_id'])
    packet.require(actual == set(by_group) and budget.same(evidence, development_review.evidence(directories) if directories else []),
                   'calibration prior original directory set/bytes changed')
    return evidence


def proof(opening_file):
    loaded_check()
    path = Path(opening_file).absolute()
    marker = cohort.sealed(budget.load(path))
    packet.require(marker.get('schema') == opening.SCHEMA and '_identity' not in marker, 'actual calibration opening required')
    value = cohort.sealed(budget.load(marker['cohort_file']))
    # Capture all raw proof bytes before source/transport reconstruction.
    registered = cohort.sealed(budget.load(cohort.sealed(budget.load(marker['accepted_review_file']))['registration_file']))
    reviewed = cohort.sealed(budget.load(marker['accepted_review_file']))
    evidence = development_review.evidence(registered['prepared_directories'])
    jev_evidence = development_review.evidence(reviewed['prepared_directories'])
    bindings = cohort.source_authority._merge_bindings(marker['bindings'], value['bindings'],
        budget.raw_bindings([path, marker['accepted_review_file'], reviewed['registration_file'], registered['dataset_file'],
                            registered['main_executable'], reviewed['sender_executable_file'], __file__]),
        *[scope['files'] for scope in [*evidence, *jev_evidence]])
    source = cohort._source(value['source_proposal_file'], value['source_snapshot_file'])
    chain = development_originals._facts(value, source)
    packet.require(path == opening.marker_path(chain, value['artifact_hash']), 'opening must be original shared fixed marker')
    current, finished = budget._snapshot_chain(chain)
    sql_image = budget._ledger_rows(Path(current['main_ledger_file']))
    identity = {'calibration_opening_file': str(path), 'calibration_opening_sha256': cohort.sha(path), 'calibration_opening_hash': marker['artifact_hash']}
    marker['_identity'] = identity
    new = _prefix(marker, current, finished, sql_image)
    archive = _archive(marker, value, source, chain, current, finished)
    result = {'schema': SCHEMA, 'opening': {key: child for key, child in marker.items() if key != '_identity'},
        **identity, 'cohort': value, 'source_pack': source, 'chain': chain, 'current': current, 'finished': finished,
        'new_main': new, 'archive': archive, 'bindings': bindings, 'sql_image': sql_image,
        'budget_bindings': development_originals.binding_images(chain)}
    result['calibration_evidence'] = verify_main_prefix(result)
    result['calibration_bindings'] = [row for scope in result['calibration_evidence'] for row in scope['files']]
    close(result)
    return result
