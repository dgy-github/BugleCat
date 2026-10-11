"""Exclusive opening of the fixed calibration allocation after development.

This marker opens only the existing 400 calibration sources. It neither starts
HTTP nor registers binary labels. The development collector cannot consume it;
the separate calibration collector must verify this owner and its fixed prefix.
"""
import os
import sqlite3
import sys
from pathlib import Path

from .. import paid_budget
from . import budget, cohort, development_review, packet
from . import development_acceptance as acceptance

SCHEMA = 'buglecat.verification.stdin-calibration-opening.v1'
NAMESPACE = 'stdin-calibration-openings'
NO_AUTHORITY = ('formal_registration', 'holdout_opened', 'formal_certification', 'production_authorized')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NAMESPACE', 'NO_AUTHORITY'))
    acceptance.loaded_check()
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))


def marker_path(chain, campaign_hash):
    """The actual original shared main root, never a caller output namespace."""
    root = Path(chain['authorization']['main_model']['ledger_dir']).absolute()
    packet.require(root.is_dir() and not root.is_symlink() and not root.is_junction(),
                   'original shared main budget root required')
    root = root.resolve(strict=True)
    namespace = root / NAMESPACE
    packet.require(not namespace.exists() or (namespace.is_dir() and not namespace.is_symlink()
                   and not namespace.is_junction() and namespace.resolve(strict=True).parent == root),
                   'calibration opening namespace cannot escape original main root')
    packet.require(protocol_hash(campaign_hash), 'original campaign hash required')
    return namespace / (campaign_hash + '.json')


def protocol_hash(value):
    return type(value) is str and len(value) == 64 and all(char in '0123456789abcdef' for char in value)


def _proof(review_file):
    path = Path(review_file).resolve(strict=True)
    raw_sha = cohort.sha(path)
    reviewed = acceptance.revalidate(path)
    packet.require(reviewed['pipeline_verified'] is True and reviewed['reason'] == 'development_pipeline_verified',
                   'complete verified original development pipeline required')
    registered_path = Path(reviewed['registration_file']).resolve(strict=True)
    registered = cohort.sealed(budget.load(registered_path))
    dataset = cohort.sealed(budget.load(registered['dataset_file']))
    campaign_path = Path(dataset['cohort_file']).resolve(strict=True)
    campaign = cohort.sealed(budget.load(campaign_path))
    packet.require(campaign['artifact_hash'] == reviewed['cohort_hash'] == registered['cohort_hash']
                   and cohort.sha(registered_path) == reviewed['registration_sha256']
                   and campaign['artifact_hash'] == dataset['cohort_hash'],
                   'accepted review differs from original campaign')
    chain = registered['ledger_snapshot']['authorization_chain']
    image = acceptance._image(chain)
    packet.require(budget.digest(image) == reviewed['budget_snapshot_digest'],
                   'accounting changed after fresh development review')
    cases = [case for case in campaign['cases'] if case['split'] == 'calibration']
    packet.require(len(cases) == 400 and len({case['group_id'] for case in cases}) == 400
                   and not any(case['group_id'] in set(image['snapshot']['excluded_groups']) for case in cases),
                   'all fixed original calibration groups must remain unstarted and unreserved')
    # The holdout earmark remains intact even if some development groups were
    # Unknown; missing calibration labels cannot be filled from that earmark.
    remaining_main = image['snapshot']['remaining_main_attempts']
    remaining_jev = chain['authorization']['jev']['max_attempts'] - len(image['snapshot']['jev_reservations'])
    packet.require(type(remaining_main) is int and type(remaining_jev) is int
                   and min(remaining_main, remaining_jev) >= 800, 'both unopened stages must remain funded')
    bindings = cohort.source_authority._merge_bindings(registered['bindings'],
        budget.raw_bindings([path, registered_path, campaign_path, __file__, acceptance.__file__]))
    packet.require(cohort.sha(path) == raw_sha, 'accepted review bytes changed during opening proof')
    return {'review': reviewed, 'review_file': str(path), 'review_sha256': raw_sha,
            'campaign': campaign, 'cohort_file': str(campaign_path), 'chain': chain,
            'image': image, 'cases': cases, 'bindings': bindings,
            'original_main_directories': registered['prepared_directories'],
            'original_main_evidence': registered['original_evidence']}


def _body(pack):
    return cohort.seal({'schema': SCHEMA, 'accepted_review_file': pack['review_file'],
        'accepted_review_sha256': pack['review_sha256'], 'accepted_review_hash': pack['review']['artifact_hash'],
        'cohort_file': pack['cohort_file'], 'cohort_sha256': cohort.sha(pack['cohort_file']),
        'cohort_hash': pack['campaign']['artifact_hash'], 'authorization_chain': pack['chain'],
        'original_accounting_image': pack['image'], 'bindings': pack['bindings'], 'cases': pack['cases'],
        'original_main_directories': pack['original_main_directories'],
        'original_main_evidence': pack['original_main_evidence'],
        'allocation_groups': 400, 'opened_split': 'calibration', 'collection_authorized': True,
        'before_calibration_requests': True, 'holdout_groups_unopened': 400,
        'protocol_sha256': pack['campaign']['protocol_sha256'], 'opening_owner_sha256': cohort.sha(__file__),
        'all_fixed_groups_must_be_terminal_before_registration': True,
        'source_replacements_allowed': False, 'automatic_retries': 0,
        'new_main_calls': 0, 'new_jev_calls': 0, 'new_oracle_executions': 0,
        'certified_threshold': None, **{key: False for key in NO_AUTHORITY}})


def _close(pack):
    """Close the exact pre-proof image while this owner holds the Jev lock.

    The normal budget snapshot correctly rejects any active reservation lock.
    Under our own lock compare its already authenticated raw files and SQL image
    directly; do not remove the lock or fabricate an unlocked snapshot.
    """
    cohort.check(pack['bindings'])
    expected = pack['image']
    packet.require(budget.same(acceptance.originals.binding_images(pack['chain']), expected['raw_budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(expected['snapshot']['main_ledger_file'])),
                                   expected['sql_image']),
                   'shared accounting changed before calibration opening')
    reviewed = pack['review']
    evidence = development_review.evidence(reviewed['prepared_directories']) if reviewed['prepared_directories'] else []
    packet.require(budget.same(evidence, reviewed['original_directory_evidence']),
                   'original development sender directory changed before opening')
    main_evidence = development_review.evidence(pack['original_main_directories']) if pack['original_main_directories'] else []
    packet.require(budget.same(main_evidence, pack['original_main_evidence']),
                   'original development main directory changed before opening')
    loaded_check()


def open_calibration(review_file, output_file=None):
    """Create the one real marker under shared Jev and main accounting locks.

    A repeated or stranded marker is retained and blocks another opening. There
    is no retry, deletion, alternative namespace, phase selection or HTTP here.
    """
    loaded_check()
    pack = _proof(review_file)
    target = marker_path(pack['chain'], pack['campaign']['artifact_hash'])
    packet.require(output_file is None or Path(output_file).absolute() == target,
                   'calibration marker must use original fixed shared namespace')
    jev_root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    lock = jev_root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.fsync(descriptor)
        with sqlite3.connect(pack['image']['snapshot']['main_ledger_file']) as connection:
            connection.execute('BEGIN IMMEDIATE')
            _close(pack)
            target.parent.mkdir(exist_ok=True)
            packet.require(marker_path(pack['chain'], pack['campaign']['artifact_hash']) == target,
                           'opening namespace changed under accounting lock')
            body = _body(pack)
            paid_budget.durable_new(target, body)
            _close(pack)
    finally:
        os.close(descriptor)
        lock.unlink()
    return {'file': str(target), 'sha256': cohort.sha(target), 'opening': body}


def before_first_request(opening_file):
    """Fresh proof before any calibration attempt; later prefixes need their owner.

    This deliberately fails after calibration starts. It cannot reselect sources
    or certify a later append-only phase by trimming the actual current ledger.
    """
    loaded_check()
    path = Path(opening_file).absolute()
    raw_sha = cohort.sha(path)
    value = cohort.sealed(budget.load(path))
    packet.require(value.get('schema') == SCHEMA, 'genuine calibration opening required')
    pack = _proof(value['accepted_review_file'])
    packet.require(path == marker_path(pack['chain'], pack['campaign']['artifact_hash'])
                   and budget.same(value, _body(pack)) and cohort.sha(path) == raw_sha,
                   'opening differs from fresh original development and fixed allocation')
    _close(pack)
    return {'opening': value, 'proof': pack}
