"""Initial pure kernel fixtures, not genuine original authority or paid cases."""
from copy import deepcopy

import pytest

from evals.verification.apps_stdin_v1 import budget, formal_archive, formal_originals
from tests.stdin_collection_fixtures import campaign


def facts(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    baseline, finished = budget._snapshot_chain(fixture['chain'])
    requests = [{'id': f'binary-{i}', 'group_id': f'binary-group-{i}', 'request_hash': budget.digest(i)} for i in range(200)]
    reg_file = tmp_path / 'synthetic-registration.json'
    registered = {'ledger_snapshot': baseline, 'original_finished': deepcopy(finished),
        'original_sql_image': budget._ledger_rows(fixture['database']), 'artifact_hash': 'b' * 64,
        'eligible_ids': [row['id'] for row in requests], 'request_hashes': [row['request_hash'] for row in requests]}
    current = deepcopy(baseline)
    slot = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev', 'group_id': requests[0]['group_id'],
        'request_hash': requests[0]['request_hash'], 'reservation_index': 1,
        'registration_file': str(reg_file), 'registration_sha256': 'c' * 64, 'registration_hash': registered['artifact_hash'],
        'authorization_sha256': current['authorization_chain']['current_sha256'],
        'automatic_retries': 0, 'status': 'reserved_before_transport'}
    current['jev_reservations'] = [slot]
    current['excluded_groups'] = sorted([*baseline['excluded_groups'], slot['group_id']])
    return registered, current, finished, requests, reg_file, slot


def test_real_current_original_pending_failure_not_truncated(tmp_path, monkeypatch):
    registered, current, finished, requests, path, slot = facts(tmp_path, monkeypatch)
    actual = deepcopy(current)
    assert formal_originals.current_prefix(registered, current, finished,
        registered['original_sql_image'], requests, path, 'c' * 64) == [slot]
    assert current == actual and finished['old-pending'] is None
    assert finished['old-failed']['call_status'] == 'transport_error'


@pytest.mark.parametrize('mutation', ['original_finish', 'raw_sql', 'new_main', 'skip', 'request', 'registration', 'retries', 'holdout_funds', 'exclusion'])
def test_current_whole_image_and_fixed_request_drift_fail_closed(tmp_path, monkeypatch, mutation):
    registered, current, finished, requests, path, slot = facts(tmp_path, monkeypatch)
    sql = deepcopy(registered['original_sql_image'])
    if mutation == 'original_finish':
        finished['old-pending'] = {'call_status': 'ok'}
    elif mutation == 'raw_sql':
        sql[1][0] = (*sql[1][0][:3], sql[1][0][3] + ' ', sql[1][0][4])
    elif mutation == 'new_main':
        current['main_started'].append({'ordinal': 3, 'group_id': 'forbidden-new-main', 'started': {}})
        finished['forbidden-new-main'] = None
    elif mutation == 'skip':
        slot['group_id'] = requests[1]['group_id']
    elif mutation == 'request':
        slot['request_hash'] = 'd' * 64
    elif mutation == 'registration':
        slot['registration_sha256'] = 'd' * 64
    elif mutation == 'retries':
        slot['automatic_retries'] = False
    elif mutation == 'holdout_funds':
        current['authorization_chain']['authorization']['jev']['max_attempts'] = 400
    else:
        current['excluded_groups'] = []
    with pytest.raises(ValueError):
        formal_originals.current_prefix(registered, current, finished, sql, requests, path, 'c' * 64)


def test_historical_raw_images_checked_as_prefix_not_replacement_ledger(tmp_path):
    old, added = tmp_path / 'old-failure.started', tmp_path / 'new-slot.json'
    old.write_bytes(b'original-failure')
    added.write_bytes(b'new-synthetic-slot')
    before, current = budget.raw_bindings([old]), budget.raw_bindings([old, added])
    formal_archive.raw_prefix(before, current)
    assert len(current) == 2
    old.write_bytes(b'changed-original')
    with pytest.raises(ValueError):
        formal_archive.raw_prefix(before, current)


def test_new_literal_loaded_drift_rejected(monkeypatch):
    monkeypatch.setattr(formal_originals, 'SCHEMA', 'forged')
    with pytest.raises(ValueError, match='constants'):
        formal_originals.loaded_check()


@pytest.mark.parametrize('io', [
    {'inputs': ['7\r\n', ['α', 'β']], 'outputs': ['7', ['α', 'β']]},
    {'inputs': [''], 'outputs': [''], 'fn_name': None},
])
def test_same_action_packet_and_public_prompt_exact_frozen_algorithm(io):
    from evals.verification.apps_stdin_v1 import packet
    context = formal_archive.packet_context(None)
    question, starter, code = 'Synthetic public task α\n', '# 原 starter\r\n', 'print(input())\r\n'
    assert formal_archive._prompt(question, starter, context) == packet.main_prompt(question, starter)
    assert packet.canonical_bytes(formal_archive._packet(question, starter, code, io, context)) == packet.canonical_bytes(
        packet.request(question, starter, code, io))
    formal_archive.close_context(context)


def test_same_action_context_closes_loaded_parameter_drift(monkeypatch):
    from evals.verification.apps_stdin_v1 import packet
    context = formal_archive.packet_context(None)
    monkeypatch.setattr(packet, 'MAX_CANDIDATE_JSON_LITERAL_BYTES', 16001)
    with pytest.raises(ValueError):
        formal_archive.close_context(context)


@pytest.mark.parametrize('code,io', [
    ('x' * 16001, {'inputs': [''], 'outputs': ['']}),
    ('print(1)', {'inputs': ['1'], 'outputs': []}),
    ('print(1)', {'inputs': ['1'], 'outputs': ['1'], 'hidden_label': True}),
])
def test_same_action_projection_retains_resource_and_full_io_gate(code, io):
    context = formal_archive.packet_context(None)
    with pytest.raises(ValueError):
        formal_archive._packet('Synthetic task', '', code, io, context)


def test_slot_graph_merges_once_retaining_all_original_prepared_bindings(tmp_path, monkeypatch):
    from evals.verification.apps_stdin_v1 import formal_jev, formal_originals
    from tests.stdin_collection_fixtures import write
    executable = tmp_path / 'explicit-synthetic.exe'
    executable.write_bytes(b'not-a-production-tool')
    inherited = tmp_path / 'inherited-source.txt'
    inherited.write_bytes(b'original-bound-bytes')
    original_merge = formal_originals.cohort.source_authority._merge_bindings
    calls, builds = [], []

    def merge(*groups):
        calls.append('complete_graph_merge')
        return original_merge(*groups)

    def build(_):
        builds.append('actual_image')
        return {'runner_exe_sha256': formal_originals.cohort.sha(executable),
                'fixture_source_sha256': formal_originals.cohort.sha(inherited)}

    monkeypatch.setattr(formal_originals.cohort.source_authority, '_merge_bindings', merge)
    monkeypatch.setattr(formal_jev, 'build_info', build)
    monkeypatch.setattr(formal_jev, 'runner_sources', lambda: {'fixture_source_sha256': inherited})
    monkeypatch.setattr(formal_jev, '_validate', lambda *args: None)
    pack = {'chain': {'authorization': {'jev': {'ledger_dir': str(tmp_path)}}},
            'current': {}, 'new_reservations': [], 'proof_bindings': [
                {'file': str(inherited), 'sha256': formal_originals.cohort.sha(inherited)}] * 1000,
            'registration_file': str(tmp_path / 'synthetic-register.json'),
            'registration_sha256': 'a' * 64, 'registration': {'artifact_hash': 'b' * 64},
            'directory_images': []}
    expected_files = {str(inherited.resolve()), str(executable.resolve())}
    for index in range(20):
        path = tmp_path / f'synthetic-{index}.prepared.json'
        row = {'sender_executable_file': str(executable), 'group_id': f'synthetic-{index}',
               'request_hash': 'c' * 64, 'output_directory': str(tmp_path / f'unclaimed-{index}'),
               'endpoint_sha256': 'd' * 64}
        write(path, formal_originals.cohort.seal(row))
        pack['new_reservations'].append({'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
            'group_id': row['group_id'], 'request_hash': row['request_hash'], 'authorization_sha256': 'e' * 64,
            'reservation_index': index + 1, 'status': 'reserved_before_transport', 'automatic_retries': 0,
            'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
            'registration_hash': pack['registration']['artifact_hash'], 'prepared_file': str(path),
            'prepared_sha256': formal_originals.cohort.sha(path), 'sender_executable_sha256': build(None)['runner_exe_sha256'],
            'output_directory': row['output_directory'], 'endpoint_sha256': row['endpoint_sha256']})
        expected_files.add(str(path.resolve()))
    pack['chain']['current_sha256'] = 'e' * 64
    builds.clear()
    formal_originals._slots(pack)
    assert calls == ['complete_graph_merge'] and builds == ['actual_image']
    assert {row['file'] for row in pack['proof_bindings']} == expected_files
    formal_originals.cohort.check(pack['proof_bindings'])
