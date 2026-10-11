"""Strict source-only metadata guard for the next, separately frozen stage.

The original complete rebuild owner and its actual receipts stay byte-frozen.
This guard cannot grant paid collection or statistical certification.
"""
from pathlib import Path

from . import source_authority

FALSE_FIELDS = ('collection_authorized', 'formal_registration', 'production_authorized',
                'candidate_labels_assigned', 'threshold_certified', 'holdout_opened',
                'protocol_or_resource_or_cohort_frozen',
                'original_grouping_before_load_graph_retroactively_complete')
ZERO_FIELDS = ('oracle_executions', 'model_calls', 'jev_calls', 'paid_calls')


def validate(value):
    """Strict metadata and original bindings; raw artifact pin remains required."""
    for field in FALSE_FIELDS:
        source_authority.require(value.get(field) is False, 'source_only_scope_promotion_rejected')
    for field in ZERO_FIELDS:
        source_authority.require(type(value.get(field)) is int and value[field] == 0,
                                 'source_only_nonzero_or_boolean_count_rejected')
    source_authority.require(value.get('source_only') is True
                             and value.get('new_transaction_before_and_after_hashes_match') is True
                             and value.get('known_duplicate_policy_rebuild_verified') is True,
                             'complete_source_transaction_required')
    return source_authority.validate(value)


def pinned(path, expected_raw_sha256):
    """Reject changed raw evidence before any downstream cohort construction."""
    source_authority.require(type(expected_raw_sha256) is str and len(expected_raw_sha256) == 64
                             and all(char in '0123456789abcdef' for char in expected_raw_sha256),
                             'original_source_authority_raw_pin_required')
    path = Path(path).resolve(strict=True)
    source_authority.require(source_authority.source.file_sha(path) == expected_raw_sha256,
                             'source_authority_raw_pin_changed')
    value = validate(source_authority.load(path))
    source_authority.require(source_authority.source.file_sha(path) == expected_raw_sha256,
                             'source_authority_changed_during_validation')
    return value
