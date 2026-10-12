"""Explicit self-made original archive continuation; never APPS certification."""
import json
import os
import time
from pathlib import Path

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    cohort,
    holdout_evaluation,
    holdout_intake,
    holdout_observations,
    holdout_registration,
    holdout_terminal,
)
from tests.stdin_collection_fixtures import write
from tests.stdin_holdout_fixtures import (
    JEV_EXE,
    MAIN_EXE,
    complete_scores,
    prepare_whole_holdout,
    restore,
)


def test_complete_self_made_continuation_once_fixed_holdout(tmp_path, monkeypatch):
    selected = os.environ.get('STDIN_C4D_SYNTHETIC_ARCHIVE')
    if not selected:
        pytest.skip('explicit previously archived self-made input is required; never rerun400 factory')
    root = Path(selected).resolve(strict=True)
    private = Path(__file__).resolve().parents[1] / 'reports/.nanocodex/artifact-v3-apps-stdin-c4d-implementation-20261012'
    original_archive = budget.load(private / 'original-c4c-temp-archive/manifest.json')
    assert Path(original_archive['source_root']) == root and original_archive['fee_authority'] is False
    assert all(cohort.sha(row['file']) == row['sha256'] for row in original_archive['files'])
    fixture = restore(root, monkeypatch)
    sql_before = budget._ledger_rows(fixture['database'])
    image_before = budget._snapshot_chain(fixture['chain'])
    timings = {}

    def action(name, function):
        fixture['calls'].clear()
        start = time.perf_counter()
        result = function()
        timings[name] = time.perf_counter() - start
        assert fixture['calls'] == ['full_source_proof'], (name, fixture['calls'])
        print(json.dumps({'action': name, 'seconds': timings[name], 'fresh_source_proofs': 1, 'new_paid_calls': 0}), flush=True)
        return result

    directories = action('prepare_synthetic_holdout400', lambda: prepare_whole_holdout(fixture))
    # Root3 loaded guards remain genuine and unmocked throughout this chain.
    joined = action('original_holdout_intake', lambda: holdout_intake.proof(fixture['holdout_opening'], directories, MAIN_EXE))
    data = joined['dataset']
    assert len(data['cases']) == 808 and data['label_counts'] == {'unknown': 200, 'supported': 150, 'unsupported': 50}
    assert data['holdout_terminal_groups'] == data['holdout_collected_groups'] == 400
    assert data['cases'][408]['main_status'] == 'http_400'
    assert data['cases'][:408] == joined['pack']['calibration_dataset']['cases'][:408]
    assert data['native_claim_certification'] is data['overall_agent_benefit_certification'] is False
    data_file = root / 'synthetic-whole400-holdout-dataset.json'
    write(data_file, data)
    terminal = action('root_holdout_terminal', lambda: holdout_terminal.proof(data_file))
    assert terminal['report']['independent_quotas_met'] is True
    registered = action('blind_holdout_registration', lambda: holdout_registration.register(data_file))
    with pytest.raises(FileExistsError):
        holdout_registration.register(data_file)
    # complete_scores invokes three independent public actions (prepare,
    # validate/reserve first, validate/claim first), each one fresh C1.
    fixture['calls'].clear()
    start = time.perf_counter()
    score_directories = complete_scores(fixture, registered['file'])
    timings['synthetic_score200'] = time.perf_counter() - start
    assert fixture['calls'] == ['full_source_proof'] * 3
    print(json.dumps({'action': 'synthetic_score200', 'seconds': timings['synthetic_score200'],
                      'source_proofs_per_action': 1, 'new_paid_calls': 0}), flush=True)
    observed = action('full_original_holdout_observer', lambda: holdout_observations.proof(registered['file'], score_directories, JEV_EXE))
    assert len(observed['observations']) == 200 and observed['observations'][0]['call_status'] == 'http_400'
    assert observed['pack']['current']['main_started'][:410] == image_before[0]['main_started']
    assert len(observed['pack']['current']['jev_reservations']) == 408
    assert sorted(observed['pack']['current']['jev_reservations'], key=lambda r: r['reservation_index'])[:208] == sorted(image_before[0]['jev_reservations'], key=lambda r: r['reservation_index'])
    result = action('once_fixed_final_evaluation', lambda: holdout_evaluation.evaluate(registered['file'], score_directories, JEV_EXE))
    final = result['evaluation']
    assert final['all_registered_binary_terminal'] is True and final['failed_jev_attempts'] == 1
    assert final['fixed_threshold'] == joined['pack']['opening']['qualified_calibration_threshold']
    assert final['threshold_tuning'] is False and final['certified_threshold'] is None
    assert final['production_authorized'] is False
    rechecked = action('saved_once_evaluation_revalidate', lambda: holdout_evaluation.revalidate(result['file']))
    assert budget.same(rechecked, final)
    with pytest.raises(FileExistsError):
        holdout_evaluation.evaluate(registered['file'], score_directories, JEV_EXE)
    # Unchanged historical400/200 inputs and markers; SQL only appends400 rows.
    old_files = [row for row in original_archive['files'] if not row['file'].endswith('.sqlite3')]
    assert all(cohort.sha(row['file']) == row['sha256'] for row in old_files)
    sql_after = budget._ledger_rows(fixture['database'])
    assert sql_after[0] == sql_before[0] and sql_after[1][:410] == sql_before[1]
    assert budget.same(sql_after[2], [(810,)])
    summary = {'explicit_synthetic': True, 'scope': 'self-made fixtures only; no APPS natural labels or actual certification',
        'timings': timings, 'whole_allocation': 808, 'original_main_groups': 410, 'original_jev_groups': 208,
        'new_holdout_main_groups': 400, 'holdout_binary_groups': 200, 'holdout_unknown_groups': 200,
        'holdout_main_http400_retained': 1, 'holdout_jev_http400_retained': 1,
        'historical_files_preserved': len(old_files), 'source_proofs_per_action': 1,
        'dataset': {'file': str(data_file), 'sha256': cohort.sha(data_file), 'hash': data['artifact_hash']},
        'registration': {'file': registered['file'], 'sha256': registered['sha256'], 'hash': registered['registration']['artifact_hash']},
        'evaluation': {'file': result['file'], 'sha256': result['sha256'], 'hash': final['artifact_hash']},
        'new_paid_calls': 0, 'real_Oracle_executions': 0, 'production_authorized': False}
    write(private / 'synthetic-full-pipeline.json', summary)
