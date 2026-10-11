"""Self-made artifacts only; no APPS executions or genuine account writes."""
import pytest

from evals.verification.apps_stdin_v1 import budget, collection, intake
from tests.stdin_collection_fixtures import append, campaign, write


@pytest.mark.parametrize('prefix,suffix', [('', ''), ('Implementation:\n', '\nI did not execute tests.'),
                                          ('説明\r\n', '\r\n不確実です。')])
def test_exact_original_span_with_assessment(prefix, suffix):
    code = 'print(input())\r\n'
    answer = prefix + '```python\r\n' + code + '```' + suffix
    span = intake.original_code(answer)
    assert span['code'] == code
    assert answer.encode()[span['start']:span['end']] == code.encode()


@pytest.mark.parametrize('answer', ['print(1)', '```python\nprint(1)\n```\n```python\nprint(2)\n```',
                                  'inline ```python\nprint(1)\n```', '```text\nprint(1)\n```',
                                  '```python\nprint("```")\n```', '```python\nprint(1)\n```fake'])
def test_ambiguous_artifact_rejected(answer):
    assert intake.original_code(answer) is None


def task():
    return {'question': 'Echo one line', 'starter_code': '', 'input_output': {'inputs': ['7\n'], 'outputs': ['7']}}


@pytest.mark.parametrize('code,label', [('print(input())\n', 'supported'), ('print("wrong")\n', 'unsupported'),
                                      ('print(missing_name)\n', 'unknown'), ('print(\n', 'unsupported')])
def test_actual_temp_oracle_durable_json(tmp_path, code, label):
    answer = '```python\n' + code + '```\nI have not executed it.'
    result = intake._oracle(task(), answer, tmp_path, True)
    assert result['label'] == label
    marker, receipt = tmp_path / 'stdin-candidate-oracle.started', tmp_path / 'stdin-candidate-oracle.json'
    assert budget.load(marker)['candidate_code_sha256'] == result['candidate_code_sha256']
    original = receipt.read_bytes(), marker.read_bytes()
    assert intake._oracle(task(), answer, tmp_path, False) == result
    assert original == (receipt.read_bytes(), marker.read_bytes())


def test_crashed_marker_is_never_retried(tmp_path):
    marker = tmp_path / 'stdin-candidate-oracle.started'
    intake.durable_new(marker, {'fixture_crashed': True})
    answer = '```python\nprint(input())\n```'
    assert intake._oracle(task(), answer, tmp_path, False)['reason'] == 'oracle_attempt_incomplete'
    with pytest.raises(FileExistsError):
        intake._oracle(task(), answer, tmp_path, True)
    assert budget.load(marker) == {'fixture_crashed': True}
    assert not (tmp_path / 'stdin-candidate-oracle.json').exists()


@pytest.mark.parametrize('status', ['ok', 'transport_error', 'response_content_invalid', 'http_401', 'http_429',
                                  'cancelled', 'candidate_literal_resource_budget', 'packet_read'])
def test_real_main_terminal_status(status):
    assert intake.main_terminal_status(status)


@pytest.mark.parametrize('status', ['pending', 'main_not_started', 'main_not_finished', 'cancelled_before_attempt',
                                  'http_200', 'http_999', 'http_４０１', None, True])
def test_nonterminal_or_invalid_status(status):
    assert not intake.main_terminal_status(status)


def test_directory_all_files_and_symlink_rejection(tmp_path):
    (tmp_path / 'evidence.db').write_bytes(b'fixture')
    assert intake.directory_bindings(tmp_path)[0]['file'].endswith('evidence.db')
    target = tmp_path / 'target'
    target.write_bytes(b'private')
    try:
        (tmp_path / 'linked').symlink_to(target)
    except OSError:
        pytest.skip('Windows symlink privilege unavailable')
    with pytest.raises(ValueError, match='link or escape'):
        intake.directory_bindings(tmp_path)


def prepared_campaign(tmp_path, monkeypatch):
    fixture = campaign(tmp_path, monkeypatch)
    value, path = fixture['value'], fixture['path']
    manifest = collection.manifest_value(path, 1)
    directory = tmp_path / 'prepared'
    directory.mkdir()
    write(directory / 'collection-manifest.json', manifest)
    case = value['cases'][0]
    case_dir = directory / case['id']
    case_dir.mkdir()
    write(case_dir / 'receipt.json', {'fixture_only': True})
    row = {**case, 'artifact_sha256': 'fixture-not-genuine'}
    (directory / 'prepared.jsonl').write_bytes(intake.packet.canonical_bytes(row) + b'\n')
    append(fixture, {**case, 'case_directory': str(case_dir)})
    monkeypatch.setattr(intake, 'loaded_check', lambda: intake.packet.runtime_info())
    monkeypatch.setattr(intake, 'build', lambda _: {'fixture_only': True})
    monkeypatch.setattr(intake, '_prepared', lambda *args: None)
    monkeypatch.setattr(intake, 'verify', lambda *args: None)
    monkeypatch.setattr(intake, 'terminal', lambda *args: ({'result': {'candidate': '```python\nprint(input())\n```'}}, 'ok'))
    return fixture, path, directory, case_dir


def test_allocation_preserved_and_new_oracle_bound(tmp_path, monkeypatch):
    _, path, directory, case_dir = prepared_campaign(tmp_path, monkeypatch)
    value = intake.collect(path, [directory], budget.load(path)['collector_executable_file'], execute_new_oracles=True)
    assert value['allocated_groups'] == 808 and value['collected_groups'] == 1 and value['uncollected_groups'] == 807
    assert value['label_counts'] == {'supported': 1, 'unlabeled': 807}
    files = {row['file'] for row in value['collection_bindings']}
    assert str(case_dir / 'stdin-candidate-oracle.json') in files
    assert str(case_dir / 'stdin-candidate-oracle.started') in files
    assert not value['formal_registration'] and not value['holdout_opened']


@pytest.mark.parametrize('action', ['add', 'delete', 'mutate'])
def test_readonly_intake_rejects_bidirectional_directory_drift(tmp_path, monkeypatch, action):
    _, path, directory, case_dir = prepared_campaign(tmp_path, monkeypatch)
    proof = directory / 'original-proof'
    proof.write_bytes(b'original')
    def changed(*args):
        if action == 'add':
            (case_dir / 'unexpected').write_bytes(b'new')
        elif action == 'delete':
            proof.unlink()
        else:
            proof.write_bytes(b'changed')
        return {'label': 'unknown', 'reason': 'fixture', 'call_eligible': False}
    monkeypatch.setattr(intake, '_oracle', changed)
    with pytest.raises((ValueError, FileNotFoundError)):
        intake.collect(path, [directory], budget.load(path)['collector_executable_file'])
