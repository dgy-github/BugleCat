"""Genuine once-opened holdout archive and actual main append prefix.

The full current ledger is never replaced with its historical calibration
image. Original rendering happens only after raw/source/main/Oracle joins.
This main owner deliberately rejects all added holdout Jev reservations.
"""
import sys
from pathlib import Path

from . import (
    budget,
    cohort,
    development_originals,
    development_review,
    formal_archive,
    history,
    holdout_archive,
    holdout_opening,
    packet,
)

opening = holdout_opening

SCHEMA = 'buglecat.verification.stdin-holdout-originals.v1'
OPENING_FIELDS = ('holdout_opening_file', 'holdout_opening_sha256', 'holdout_opening_hash')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'OPENING_FIELDS'))
    holdout_archive.loaded_check()
    holdout_opening.loaded_check()


def prefix(marker, identity, current, finished, sql):
    image = marker['original_accounting_image']
    baseline = image['snapshot']
    _, new = history._original(baseline, image['finished'], current, finished)
    cases = marker['cases']
    packet.require(type(cases) is list and len(cases) == 400 and all(case['split'] == 'holdout' for case in cases),
                   'fixed original whole400 holdout required')
    history._new_main(new, finished, cases, marker['cohort_hash'], baseline['authorization_chain']['current_sha256'])
    packet.require(budget.same(sql[0], image['sql_image'][0])
                   and budget.same(sql[1][:len(image['sql_image'][1])], image['sql_image'][1])
                   and budget.same(image['sql_image'][2], [[len(baseline['main_started'])]]),
                   'original development/calibration raw SQL history changed')
    packet.require(all(all(budget.same(row['started'].get(key), identity[key]) for key in OPENING_FIELDS) for row in new),
                   'holdout main lacks actual once-opening raw identity')
    return new


def close(pack):
    cohort.check(pack['proof_bindings'])
    packet.require(cohort.sha(pack['holdout_opening_file']) == pack['holdout_opening_sha256']
                   and budget.same(budget._snapshot_chain(pack['chain']), (pack['current'], pack['finished']))
                   and budget.same(budget._ledger_rows(Path(pack['current']['main_ledger_file'])), pack['sql_image'])
                   and budget.same(development_originals.binding_images(pack['chain']), pack['budget_bindings']),
                   'whole holdout current accounting/opening changed during proof')
    for directories, evidence in pack['directory_images']:
        packet.require(budget.same(development_review.evidence(directories), evidence), 'original holdout archive topology/bytes changed')
    loaded_check()


def archive(opening_file):
    """Internal reconstruction; extra Jev authority belongs to score originals."""
    loaded_check()
    path = Path(opening_file).absolute()
    raw = cohort.sha(path)
    marker = cohort.sealed(budget.load(path))
    packet.require(marker.get('schema') == holdout_opening.SCHEMA and marker.get('opened_split') == 'holdout'
                   and marker.get('certified_threshold') is None, 'actual once holdout opening required')
    # Bind every saved source byte and new owner before any original proof work.
    bindings = cohort.source_authority._merge_bindings(marker['bindings'], budget.raw_bindings([path, __file__, holdout_archive.__file__]))
    chain = marker['authorization_chain']
    packet.require(path == holdout_opening.marker_path(chain, marker['cohort_hash']), 'actual shared fixed opening path required')
    current, finished = budget._snapshot_chain(chain)
    sql = budget._ledger_rows(Path(current['main_ledger_file']))
    identity = dict(zip(OPENING_FIELDS, (str(path), raw, marker['artifact_hash']), strict=True))
    new = prefix(marker, identity, current, finished, sql)
    cal = holdout_archive.calibration(marker['registration_file'], current, finished, sql)
    packet.require(budget.same(cal['chain'], chain) and cal['cohort']['artifact_hash'] == marker['cohort_hash'],
                   'once opening cannot change original funded source chain')
    accepted = holdout_archive.accepted(marker, cal)
    result = {'schema': SCHEMA, 'opening': marker, **identity, 'cohort': cal['cohort'], 'source_pack': cal['source_pack'],
        'chain': chain, 'current': current, 'finished': finished, 'sql_image': sql,
        'budget_bindings': cal['budget_bindings'], 'new_main': new,
        'archive': cal['archive'], 'calibration_dataset': accepted['calibration_dataset'],
        'calibration_original_pack': cal,
        'directory_images': list(cal['directory_images']), 'historical_directory_images': list(cal['directory_images']),
        'bindings': cohort.source_authority._merge_bindings(bindings, cal['proof_bindings'], accepted['bindings']),
        'proof_bindings': cohort.source_authority._merge_bindings(bindings, cal['proof_bindings'], accepted['bindings'])}
    cohort.check(result['proof_bindings'])
    return result


def proof(opening_file):
    pack = archive(opening_file)
    packet.require(budget.same(pack['current']['jev_reservations'], pack['opening']['original_accounting_image']['snapshot']['jev_reservations'])
                   and budget.same(pack['budget_bindings'], pack['opening']['original_accounting_image']['raw_budget_bindings']),
                   'holdout scoring requires future genuine blind registration owner')
    evidence = verify_main_prefix(pack)
    if evidence:
        directories = [scope['directory'] for scope in evidence]
        pack['directory_images'].append((directories, evidence))
        pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'],
            *[scope['files'] for scope in evidence])
    close(pack)
    return pack


def verify_main_prefix(pack):
    """Every actual earlier finish is joined before a subsequent group starts."""
    from . import holdout_collection as collector
    from . import holdout_intake as reader
    from . import intake

    directories = sorted({Path(row['started']['manifest_file']).resolve(strict=True).parent for row in pack['new_main']})
    evidence = development_review.evidence(directories) if directories else []
    own = {row['group_id']: row for row in pack['new_main']}
    context = formal_archive.packet_context(pack.get('cohort', {}).get('oracle_runtime_binding'))
    image = manifest_images(directories)
    context['manifest_sha256'] = image['sha256']
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack.get('proof_bindings', []), image['bindings'])
    actual, compiled_images = set(), {}
    for directory in directories:
        manifest = image['manifests'][str(directory)]
        fixed = pack['opening']['cases']
        positions = [fixed.index(case) for case in manifest['cases']]
        packet.require(manifest['schema'] == collector.SCHEMA and manifest['split'] == 'holdout'
                       and 1 <= len(positions) <= 8 and positions == list(range(positions[0], positions[0] + len(positions)))
                       and all(budget.same(manifest[key], pack[key]) for key in OPENING_FIELDS),
                       'original holdout manifest/prefix/opening changed')
        executable = manifest['collector_executable_file']
        if executable not in compiled_images:
            compiled_images[executable] = collector.build(executable)
        compiled = compiled_images[executable]
        rows = intake.lines(directory / 'prepared.jsonl')
        events = intake.lines(directory / 'attempt-events.jsonl') if (directory / 'attempt-events.jsonl').exists() else []
        packet.require(len(rows) == len(manifest['cases']) and [row['id'] for row in rows] == [case['id'] for case in manifest['cases']]
                       and all(event['id'] in {row['id'] for row in rows} for event in events), 'complete holdout preparation scope required')
        for row, case in zip(rows, manifest['cases'], strict=True):
            case_dir = directory / case['id']
            task = pack['source_pack']['tasks'][case['task_id']] if 'source_pack' in pack else {
                'task_id': case['task_id'], 'source_kind': case['source_kind'], **case['public_interface']}
            reader._prepared(row, case, task, manifest, directory, compiled, context=context)
            start = own.get(case['group_id'])
            matching = start is not None and intake.same_path(start['started']['case_directory'], case_dir)
            _, status = reader.terminal(row, start if matching else None,
                pack['finished'][case['group_id']] if matching else None, events, case_dir)
            if matching:
                packet.require(case['group_id'] not in actual, 'duplicate actual holdout candidate')
                if status == 'ok':
                    intake.verify(row, case_dir / 'receipt.json', events, executable)
                actual.add(case['group_id'])
    packet.require(actual == set(own) and budget.same(evidence, development_review.evidence(directories) if directories else []),
                   'holdout actual started scope or raw topology/bytes changed')
    close_manifest_images(image)
    pack['holdout_manifest_images'] = image
    formal_archive.close_context(context)
    return evidence


def manifest_images(directories):
    """One ephemeral complete graph check, with each raw manifest read once.

    This is an internal original-proof projection, never a persisted authority
    or a caller option to skip source/accounting proof. All rows still undergo
    original raw/session/terminal joins, and closing rehashes the full graph.
    """
    manifests, hashes, bindings = {}, {}, []
    for directory in directories:
        path = Path(directory) / 'collection-manifest.json'
        raw = path.read_bytes()
        value = cohort.sealed(budget.parse(raw))
        packet.require(intake_sha(path) == packet_sha(raw), 'manifest changed while capturing original bytes')
        manifests[str(Path(directory).resolve(strict=True))] = value
        hashes[str(path.resolve(strict=True))] = packet_sha(raw)
        bindings.extend(value['phase_proof_bindings'])
    bindings = cohort.source_authority._merge_bindings(bindings)
    cohort.check(bindings)
    return {'manifests': manifests, 'sha256': hashes, 'bindings': bindings}


def packet_sha(raw):
    import hashlib
    return hashlib.sha256(raw).hexdigest()


def intake_sha(path):
    return cohort.sha(path)


def close_manifest_images(image):
    packet.require(all(cohort.sha(path) == raw for path, raw in image['sha256'].items()),
                   'captured original manifest bytes changed during proof')
