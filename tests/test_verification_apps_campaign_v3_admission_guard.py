"""Resealed source evidence cannot assert calls, thresholds or holdout access."""
import json

import pytest

from evals.verification.apps_campaign_v3 import admission_guard as guard
from evals.verification.apps_v3 import oracle


def value():
    return {'schema': guard.source_authority.SCHEMA, 'source_only': True,
            'new_transaction_before_and_after_hashes_match': True,
            'known_duplicate_policy_rebuild_verified': True, 'bindings': [], 'owner_bindings': [],
            **dict.fromkeys(guard.FALSE_FIELDS, False), **dict.fromkeys(guard.ZERO_FIELDS, 0)}


def seal(body):
    return {**body, 'artifact_hash': oracle.digest(body)}


@pytest.mark.parametrize('field', guard.FALSE_FIELDS)
@pytest.mark.parametrize('replacement', [True, 0, None])
def test_false_metadata_cannot_be_promoted_even_after_reseal(field, replacement):
    body = value()
    body[field] = replacement
    with pytest.raises(ValueError, match='scope_promotion_rejected'):
        guard.validate(seal(body))


@pytest.mark.parametrize('field', guard.ZERO_FIELDS)
@pytest.mark.parametrize('replacement', [1, False, None])
def test_zero_counts_must_be_exact_integer_zero(field, replacement):
    body = value()
    body[field] = replacement
    with pytest.raises(ValueError, match='nonzero_or_boolean_count'):
        guard.validate(seal(body))


def test_exact_source_only_values_allowed_and_original_raw_pin_enforced(tmp_path):
    path = tmp_path / 'source.json'
    path.write_text(json.dumps(seal(value())), encoding='utf-8')
    sha = guard.source_authority.source.file_sha(path)
    assert guard.pinned(path, sha)['threshold_certified'] is False
    path.write_text(json.dumps(seal(value())) + '\n', encoding='utf-8')
    with pytest.raises(ValueError, match='raw_pin_changed'):
        guard.pinned(path, sha)
