"""Different result paths cannot reopen the fixed holdout evaluation."""
import json

import pytest

from evals.verification import artifact_v2_formal as owner
from evals.verification import budget_authorization_v2


def test_fixed_shared_marker_blocks_second_output(tmp_path, monkeypatch):
    observations = tmp_path / 'observations.jsonl'
    observations.write_text('{}\n')
    registration = {'artifact_hash': 'b' * 64, 'calibration_hash': 'c' * 64}
    data = {'authorization_file': 'auth', 'cohort_hash': 'a' * 64}
    result = {'artifact_hash': 'd' * 64, 'certified_threshold': .9}
    monkeypatch.setattr(owner, 'holdout_value', lambda *args: result)
    monkeypatch.setattr(owner, 'revalidate', lambda *args: (registration, data, [], []))
    monkeypatch.setattr(budget_authorization_v2, 'resolve', lambda *args: {
        'authorization': {'jev': {'ledger_dir': str(tmp_path / 'ledger')}}})
    first, second = tmp_path / 'one.json', tmp_path / 'two.json'
    assert owner.holdout('registered', observations, first) == result
    with pytest.raises(FileExistsError):
        owner.holdout('registered', observations, second)
    assert not second.exists()
    marker = tmp_path / 'ledger/formal-v2-holdout-evaluations' / ('a' * 64 + '.json')
    assert json.loads(marker.read_text())['result_hash'] == result['artifact_hash']


def test_invalid_calibration_cannot_consume_evaluation(tmp_path, monkeypatch):
    def invalid(*args):
        raise ValueError('calibration failed')
    monkeypatch.setattr(owner, 'holdout_value', invalid)
    with pytest.raises(ValueError, match='calibration failed'):
        owner.holdout('registered', 'observations', tmp_path / 'result.json')
    assert list(tmp_path.iterdir()) == []
