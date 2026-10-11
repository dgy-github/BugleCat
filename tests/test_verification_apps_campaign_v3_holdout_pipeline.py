"""Synthetic controlled proof boundaries, real temporary once/slot/raw joins.

This never proves a real APPS calibration or executes an APPS Oracle. Source
and independent labels are explicit test fixtures; financial files and original
request/response/event joins use the real new owners in isolated directories.
"""
import copy
import json
import runpy
import sqlite3
from pathlib import Path

import pytest

from evals.verification.apps_campaign_v3 import (
    formal_holdout,
    holdout_collection,
    holdout_evaluation,
    holdout_intake,
    holdout_jev,
    holdout_registration,
    packet,
    prospective_protocol,
)
from evals.verification.calibration import create, digest, load
from evals.verification.collection_manifest import file_hash


def write(path, value):
    Path(path).write_bytes(packet.canonical_bytes(value))


def seal(value):
    return {**value, 'artifact_hash': digest(value)}


def test_controlled_qualified_source_main_blind_labels_slots_raw_joins_final_once(tmp_path, monkeypatch):
    fixture = runpy.run_path(str(Path(__file__).with_name('test_verification_apps_campaign_v3_formal_holdout.py')))
    source = fixture['source'].__wrapped__(tmp_path, monkeypatch)
    cohort, before, finished = (source['state'][k] for k in ('cohort', 'before', 'finished'))
    auth, protocol = tmp_path / 'auth.json', tmp_path / 'protocol.json'
    write(auth, {'synthetic approved authority': True})
    write(protocol, prospective_protocol.specification())
    runtime = packet.runtime_info()
    # Fixture proofs already declare immutable source/runtime. Avoid hashing
    # the same real interpreter/primitive files 40,000 times in this synthetic
    # test; actual binaries separately prove the entire compiled/current graph.
    dependency = packet.dependency_sha256(packet.FREE)
    monkeypatch.setattr(packet, 'runtime_info', lambda: copy.deepcopy(runtime))
    monkeypatch.setattr(packet, 'dependency_sha256', lambda kind: copy.deepcopy(dependency))
    cases = [dict(id=f'hold-{n}', group_id=f'group-{n}', task_id=n, task_sha256=digest(n),
                  source_kind=packet.FREE, split='holdout') for n in range(201)]
    cohort.update(cases=[cohort['cases'][0]] + cases, funded_allocation={'holdout_main_earmark': 201},
                  bindings=[dict(role='authorization', file=str(auth), sha256=file_hash(auth))],
                  protocol_file=str(protocol), protocol_sha256=file_hash(protocol), oracle_runtime_binding=runtime)
    write(source['cohortfile'], cohort)
    source['state']['cal']['cohort_sha256'] = file_hash(source['cohortfile'])
    cal = source['state']['cal']
    cal['artifact_hash'] = digest({k: v for k, v in cal.items() if k != 'artifact_hash'})
    write(source['calfile'], cal)
    marker = formal_holdout.open_source(source['calfile'])
    markerfile = Path(marker['marker_file'])
    main = tmp_path / 'main'
    main.mkdir()
    tasks = {n: {'question': f'Original fixture {n}', 'starter_code': '', 'source_kind': packet.FREE,
                 'input_output': {'fn_name': 'f', 'inputs': [[1]], 'outputs': [1]}} for n in range(201)}
    prepared = []
    # The main/label side is a controlled isolated fixture. Actual compiled
    # main raw/session/SQLite behaviour is covered by holdout_binary tests.
    for n, case in enumerate(cases):
        directory = main / case['id']
        directory.mkdir()
        receipt = {'result': {'candidate': '```python\ndef f(x):\n    return x\n```'}}
        write(directory / 'receipt.json', receipt)
        write(directory / 'apps-holdout-oracle.json', {'fixture_original_label': 'unknown' if n == 200 else 'supported' if n < 150 else 'unsupported'})
        start = {**case, 'artifact_cohort_hash': cohort['artifact_hash'], 'holdout_source_file': str(markerfile),
                 'holdout_source_sha256': file_hash(markerfile), 'holdout_source_hash': marker['artifact_hash']}
        before['main_started'].append(dict(ordinal=n + 2, group_id=case['group_id'], started=start))
        before['remaining_main_attempts'] -= 1
        finished[case['group_id']] = {'call_status': 'ok'}
        with sqlite3.connect(source['ledger']) as db:
            db.execute('INSERT INTO attempts VALUES (?,?,?,?)', (n + 2, case['group_id'], json.dumps(start), json.dumps(finished[case['group_id']])))
        prepared.append((case, {}, receipt, 'ok', directory, True))
    before['authorization_chain']['current_sha256'] = file_hash(auth)
    before['authorization_chain']['authorization']['jev']['max_attempts'] = 861
    def current():
        before['jev_reservations'] = [load(p) for p in sorted((source['root'] / 'reservations').glob('*.json'))]
        return copy.deepcopy((before, finished))
    # Replaces only the expensive synthetic source/qualified-calibration proof.
    # All downstream financial and original transport joins remain real.
    def verified(path):
        snapshot, terminal = current()
        return copy.deepcopy(marker), (copy.deepcopy(cohort), tasks, 'test train', snapshot, terminal)
    monkeypatch.setattr(holdout_collection, 'verified_pack', verified)
    monkeypatch.setattr(holdout_collection, 'budget_snapshot', lambda p: current())
    monkeypatch.setattr(holdout_intake, 'build', lambda p: {'executable_sha256': file_hash(p)})
    monkeypatch.setattr(holdout_intake, '_prepared', lambda *a: (prepared, {'directory': str(main)}))
    def label(task, answer, record=None):
        value = record['fixture_original_label'] if record else 'unknown'
        return {'label': value, 'reason': 'synthetic original label', 'call_eligible': value != 'unknown'}
    monkeypatch.setattr(holdout_intake.intake, 'label', label)
    dataset = holdout_intake.collect(markerfile, [main], auth)
    assert dataset['label_counts'] == {'supported': 150, 'unsupported': 50, 'unknown': 1}
    assert dataset['collected_groups'] == dataset['allocated_groups'] == 201
    datasetfile = tmp_path / 'dataset.json'
    create(datasetfile, dataset)
    registration = holdout_registration.register(datasetfile)
    regfile = Path(registration['registration_file'])
    assert registration['independent_label_counts']['unknown'] == 1
    assert len(registration['eligible_ids']) == 200
    allocationfile = tmp_path / 'allocation.json'
    create(allocationfile, holdout_jev.prepare(regfile))
    rebuilt = holdout_jev.prepare(regfile)
    assert [k for k in rebuilt if rebuilt[k] != load(allocationfile)[k]] == []
    values = holdout_jev.prepared_batch(allocationfile)
    assert len(values) == 200
    monkeypatch.setattr(holdout_jev.approved_budget, 'resolve', lambda p: copy.deepcopy(before['authorization_chain']))
    monkeypatch.setattr(holdout_jev, 'reservations', lambda *a: (current()[0]['jev_reservations'], []))
    exe = tmp_path / 'fake.exe'
    exe.write_bytes(b'isolated fixture binary, not a real provider')
    compiled = {'runner_exe_sha256': file_hash(exe), **{k: file_hash(p) for k, p in holdout_jev.runner_sources().items()}}
    monkeypatch.setattr(holdout_jev, 'build_info', lambda p: compiled)
    rows = []
    for n, value in enumerate(values):
        preparedfile = tmp_path / (value['id'] + '.json')
        create(preparedfile, value)
        reservation = holdout_jev.reserve(preparedfile)
        reservationfile = Path(reservation['path'])
        directory = source['root'] / 'attempts' / value['id']
        directory.mkdir(parents=True)
        (directory / 'request.json').write_bytes(value['request_json'].encode())
        raw = {'model': packet.MODEL, 'answers': {'acceptance_violation': {'type': 'noul', 'noul': .01 if n < 150 else .99}},
               'usage': {'input_tokens': 0, 'output_tokens': 0}}
        write(directory / 'raw-response.json', raw)
        common = {'id': value['id'], 'group_id': value['group_id'], 'request_hash': value['request_hash'],
                  'prepared_sha256': file_hash(preparedfile), 'template_version': packet.TEMPLATE,
                  **{k: value[k] for k in ('allocation_hash', 'allocation_sha256', 'artifact_cohort_hash', 'protocol_sha256', 'source_kind',
                                         'registration_file', 'registration_sha256', 'registration_hash',
                                         'holdout_source_file', 'holdout_source_sha256', 'holdout_source_hash', 'fixed_threshold')},
                  'automatic_retries': 0, 'formal_certification': False, 'offline': False,
                  'attempt_directory': str(directory), 'raw_response_file': str(directory / 'raw-response.json'),
                  'reservation_file': str(reservationfile), 'reservation_sha256': file_hash(reservationfile), **compiled}
        events = [{**common, 'status': 'attempt_started'}, {**common, 'status': 'request_submitted', 'actual_request_hash': value['request_hash']},
                  {**common, 'status': 'attempt_finished', 'submitted': True, 'call_status': 'ok', 'response_sha256': file_hash(directory / 'raw-response.json')}]
        write(reservationfile.with_suffix('.started'), events[0])
        write(directory / 'receipt.json', events[-1])
        (directory / 'events.jsonl').write_bytes(b''.join(packet.canonical_bytes(e) + b'\n' for e in events))
        # The original byte join is real, with typed in-transaction packet.
        rows.append(holdout_jev._observe(value, preparedfile, directory, exe, _compiled=compiled))
    observations = tmp_path / 'observations.jsonl'
    observations.write_bytes(b''.join(packet.canonical_bytes(r) + b'\n' for r in rows))
    value = holdout_evaluation.evaluate(regfile, allocationfile, observations, exe)
    target = source['root'] / holdout_evaluation.NAMESPACE / (cohort['artifact_hash'] + '.json')
    assert value == holdout_evaluation.revalidate_evaluation(target)
    assert value['holdout_status'] == 'passed' and value['original_observations'] == 200
    assert value['label_counts']['unknown'] == 1 and value['allocated_groups'] == 201
    assert value['formal_certification'] is value['production_authorized'] is value['overall_goal_complete'] is False
    with pytest.raises(FileExistsError):
        holdout_evaluation.evaluate(regfile, allocationfile, observations, exe)
    with pytest.raises(ValueError, match='once_group'):
        holdout_jev.reserve(tmp_path / (values[0]['id'] + '.json'))
    assert len(list((source['root'] / 'reservations').glob('*.json'))) == 200
