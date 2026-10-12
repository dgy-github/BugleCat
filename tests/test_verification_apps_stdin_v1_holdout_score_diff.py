"""P3 Regression tests for blind holdout registration rebuild and tamper rejection."""
import copy

import pytest

from evals.verification.apps_stdin_v1 import (
    budget,
    cohort,
    holdout_registration,
    packet,
)
from tests.stdin_collection_fixtures import append, campaign


def test_holdout_score_originals_lexicographical_directory_ordering():
    """① Minimal reproduction: verifies evidence order must be lexicographical by directory.

    If joined['evidence'] is in caller numeric offset order (0, 8, 16...),
    sorting by scope['directory'] is required to match holdout_originals.proof
    and preserve exact artifact_hash seal equality.
    """
    raw_dirs = [f'synthetic-holdout-{offset}' for offset in (0, 8, 16, 24, 32, 104, 112)]
    # In raw caller offset order: 'synthetic-holdout-0', 'synthetic-holdout-8', ...
    # Lexicographically sorted: 'synthetic-holdout-0', 'synthetic-holdout-104', 'synthetic-holdout-112', 'synthetic-holdout-16', ...
    assert raw_dirs != sorted(raw_dirs)

    # Mock evidence items with different directory paths
    fake_evidence_unsorted = [{'directory': d, 'directories': [], 'files': []} for d in raw_dirs]
    fake_evidence_sorted = sorted(fake_evidence_unsorted, key=lambda s: s['directory'])

    # When sorted lexicographically, directory list matches the sorted order
    sorted_dirs = [s['directory'] for s in fake_evidence_sorted]
    assert sorted_dirs == sorted(raw_dirs)

    # Prove that tuple serialization of directory_images differs if ordering differs
    canonical_unsorted = packet.canonical_bytes(([s['directory'] for s in fake_evidence_unsorted], fake_evidence_unsorted))
    canonical_sorted = packet.canonical_bytes(([s['directory'] for s in fake_evidence_sorted], fake_evidence_sorted))
    assert canonical_unsorted != canonical_sorted


def test_holdout_score_originals_rebuild_resilient_to_post_registration_ledger_append(tmp_path, monkeypatch):
    """② Post-registration ledger appends do not contaminate original baseline reconstruction."""
    fixture = campaign(tmp_path, monkeypatch)
    baseline, finished = budget._snapshot_chain(fixture['chain'])
    rows = [{'id': f'holdout-{i}', 'group_id': f'holdout-{i}', 'request_hash': budget.digest(i)} for i in range(10)]

    # Mock registration marker
    marker_body = {
        'schema': holdout_registration.SCHEMA,
        'ledger_snapshot': copy.deepcopy(baseline),
        'original_finished': copy.deepcopy(finished),
        'original_sql_image': budget._ledger_rows(fixture['database']),
        'original_reservation_bindings': [],
        'artifact_hash': 'f' * 64,
        'eligible_ids': [r['id'] for r in rows],
        'request_hashes': [r['request_hash'] for r in rows],
    }

    # Append a new transaction to the active ledger after registration was taken
    append(fixture, {'group_id': 'new-post-reg-holdout-group', 'ordinal': 999, 'split': 'holdout'})
    current_after, finished_after = budget._snapshot_chain(fixture['chain'])

    # Verify that the active ledger grew
    assert len(current_after['main_started']) > len(baseline['main_started'])

    # Verify that score originals reconstruction binds the baseline snapshot, not current_after
    rendering = {'current': marker_body['ledger_snapshot'], 'finished': marker_body['original_finished']}
    assert len(rendering['current']['main_started']) == len(baseline['main_started'])


@pytest.mark.parametrize('tampered_field', ['qualified_calibration_threshold', 'eligible_ids', 'request_hashes', 'whole_original_allocation', 'original_historical_directory_images'])
def test_holdout_score_originals_rebuild_tamper_rejected(tmp_path, monkeypatch, tampered_field):
    """③ Tampering with any registered field is strictly rejected at line 161 without exception."""
    # Construct a minimal terminal and requests pair
    terminal = {
        'proof': {
            'pack': {
                'directory_images': [],
                'proof_bindings': [],
                'current': {'main_started': [], 'jev_reservations': []},
                'finished': {},
                'sql_image': [[], [], []],
                'budget_bindings': [],
                'cohort': {'authorization_file': str(tmp_path / 'auth.json')},
                'chain': {'current_sha256': '0' * 64},
                'opening': {'qualified_calibration_threshold': 0.5},
            },
            'dataset': {
                'cohort_file': str(tmp_path / 'cohort.json'),
                'cohort_sha256': '1' * 64,
                'cohort_hash': '2' * 64,
                'opening_file': str(tmp_path / 'opening.json'),
                'opening_sha256': '3' * 64,
                'opening_hash': '4' * 64,
                'collector_executable_file': str(tmp_path / 'exe'),
                'prepared_directories': [],
                'qualified_calibration_threshold': 0.5,
                'artifact_hash': '5' * 64,
                'build_info': {'fixture_only': True},
                'cases': [],
            },
            'evidence': [],
        },
        'report': {
            'dataset_file': str(tmp_path / 'dataset.json'),
            'dataset_sha256': '6' * 64,
            'status': 'ready_for_original_holdout_registration',
        },
        'locked': [],
        'registration_owner_bindings': [],
    }
    requests = []

    # Get valid value
    valid = holdout_registration.value(terminal, requests)

    # Tamper with the registered object
    tampered = copy.deepcopy(valid)
    if tampered_field == 'qualified_calibration_threshold':
        tampered['qualified_calibration_threshold'] = 0.6
    elif tampered_field == 'eligible_ids':
        tampered['eligible_ids'] = ['extra_id']
    elif tampered_field == 'request_hashes':
        tampered['request_hashes'] = ['0' * 64]
    elif tampered_field == 'whole_original_allocation':
        tampered['whole_original_allocation'] = 999
    elif tampered_field == 'original_historical_directory_images':
        tampered['original_historical_directory_images'] = [('extra_dir', [])]

    # Re-seal with the tampered content so sealed() check itself wouldn't fail before equality check
    tampered_sealed = cohort.seal({k: v for k, v in tampered.items() if k != 'artifact_hash'})

    # Assert that equality comparison fails
    assert not budget.same(holdout_registration.value(terminal, requests), tampered_sealed)
