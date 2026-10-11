"""Create one fixed new holdout opening after a genuine qualified calibration.

No threshold is certified here. The marker freezes the qualified threshold and
all 400 fresh source identities before the holdout can collect any candidate.
"""
import os
import sqlite3
import sys
from pathlib import Path

from .. import paid_budget
from . import budget, cohort, development_originals, development_review, packet, protocol
from . import formal_decision as decision

SCHEMA = 'buglecat.verification.stdin-once-holdout-opening.v1'
NAMESPACE = 'stdin-holdout-openings'
FALSE_FLAGS = ('formal_registration', 'formal_certification', 'production_authorized',
               'native_claim_certification', 'overall_agent_benefit_certification')


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'NAMESPACE', 'FALSE_FLAGS'))
    decision.loaded_check()
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))


def marker_path(chain, campaign_hash):
    root = Path(chain['authorization']['main_model']['ledger_dir']).absolute()
    packet.require(protocol._hash(campaign_hash) and root.is_dir()
                   and not root.is_symlink() and not root.is_junction(), 'original shared main root required')
    root = root.resolve(strict=True)
    namespace = root / NAMESPACE
    packet.require(not namespace.exists() or (namespace.is_dir() and not namespace.is_symlink()
                   and not namespace.is_junction() and namespace.resolve(strict=True).parent == root),
                   'once holdout namespace cannot escape original shared main root')
    return namespace / (campaign_hash + '.json')


def _proof(report_file):
    path = Path(report_file).resolve(strict=True)
    raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == decision.SCHEMA, 'genuine original calibration decision required')
    owned = budget.raw_bindings([path, __file__])
    joined = decision.proof(saved['registration_file'], saved['prepared_directories'], saved['sender_executable_file'])
    report = joined['report']
    packet.require(budget.same(saved, report) and cohort.sha(path) == raw
                   and report['all_registered_binary_terminal'] is True
                   and report['calibration_gate_met'] is True and report['status'] == 'qualified_calibration'
                   and type(report['qualified_calibration_threshold']) in (int, float)
                   and report['qualified_calibration_threshold'] in protocol.GRID
                   and report['certified_threshold'] is None
                   and all(report[key] is False for key in decision.FALSE_FLAGS),
                   'fresh qualified calibration required before once holdout opening')
    pack = joined['proof']['pack']
    cases = [case for case in pack['cohort']['cases'] if case['split'] == 'holdout']
    excluded = set(pack['current']['excluded_groups'])
    remaining_jev = pack['chain']['authorization']['jev']['max_attempts'] - len(pack['current']['jev_reservations'])
    packet.require(len(cases) == 400 and len({case['group_id'] for case in cases}) == 400
                   and all(case['group_id'] not in excluded for case in cases)
                   and type(pack['current']['remaining_main_attempts']) is int
                   and min(pack['current']['remaining_main_attempts'], remaining_jev) >= 400,
                   'complete fixed400 unopened and funded holdout required')
    bindings = cohort.source_authority._merge_bindings(owned, joined['bindings'])
    decision._close(joined['proof'], bindings)
    return {'decision': joined, 'report_file': str(path), 'report_sha256': raw,
            'cases': cases, 'bindings': bindings}


def _body(opened):
    joined = opened['decision']
    pack, report = joined['proof']['pack'], joined['report']
    return cohort.seal({'schema': SCHEMA, 'accepted_decision_file': opened['report_file'],
        'accepted_decision_sha256': opened['report_sha256'], 'accepted_decision_hash': report['artifact_hash'],
        'cohort_file': pack['registration']['cohort_file'],
        'cohort_sha256': pack['registration']['cohort_sha256'], 'cohort_hash': pack['cohort']['artifact_hash'],
        'authorization_chain': pack['chain'], 'original_accounting_image': joined['image'],
        'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
        'registration_hash': pack['registration']['artifact_hash'],
        'original_calibration_archive': pack['directory_images'],
        'original_formal_sender_directories': report['prepared_directories'],
        'original_formal_sender_evidence': joined['proof']['evidence'],
        'bindings': opened['bindings'], 'cases': opened['cases'],
        'allocation_groups': 400, 'opened_split': 'holdout', 'holdout_opened': True,
        'collection_authorized': True, 'before_holdout_requests': True,
        'qualified_calibration_threshold': report['qualified_calibration_threshold'],
        'threshold_frozen_before_holdout': True, 'holdout_max_consumptions': 1,
        'all_fixed_groups_must_be_terminal_before_registration': True,
        'source_replacements_allowed': False, 'automatic_retries': 0,
        'protocol_hash': report['protocol_hash'], 'opening_owner_sha256': cohort.sha(__file__),
        'certified_threshold': None, 'new_main_calls': 0, 'new_jev_calls': 0, 'new_oracle_executions': 0,
        **{key: False for key in FALSE_FLAGS}})


def _locked_close(opened, connection):
    """Close authenticated bytes under this call's real Jev lock and SQL tx."""
    joined = opened['decision']
    proof, image = joined['proof'], joined['image']
    pack = proof['pack']
    cohort.check(opened['bindings'])
    packet.require(connection.in_transaction
                   and budget.same(development_originals.binding_images(pack['chain']), image['raw_budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(image['snapshot']['main_ledger_file'])), image['sql_image']),
                   'real full accounting changed before once holdout opening')
    for directories, evidence in pack['directory_images']:
        packet.require(budget.same(development_review.evidence(directories), evidence),
                       'original calibration archive changed under holdout lock')
    packet.require(budget.same(development_review.evidence(proof['directories']), proof['evidence']),
                   'original formal sender files changed under holdout lock')
    loaded_check()


def open_holdout(report_file, output_file=None):
    """Create-new once; existing or stranded markers block any repetition."""
    loaded_check()
    opened = _proof(report_file)
    pack = opened['decision']['proof']['pack']
    target = marker_path(pack['chain'], pack['cohort']['artifact_hash'])
    packet.require(output_file is None or Path(output_file).absolute() == target,
                   'holdout marker must use fixed original shared namespace')
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    lock = root / 'reservation.lock'
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.fsync(descriptor)
        with sqlite3.connect(pack['current']['main_ledger_file']) as connection:
            connection.execute('BEGIN IMMEDIATE')
            _locked_close(opened, connection)
            target.parent.mkdir(exist_ok=True)
            packet.require(marker_path(pack['chain'], pack['cohort']['artifact_hash']) == target,
                           'holdout namespace changed under accounting lock')
            body = _body(opened)
            paid_budget.durable_new(target, body)
            _locked_close(opened, connection)
    finally:
        os.close(descriptor)
        lock.unlink()
    return {'file': str(target), 'sha256': cohort.sha(target), 'opening': body}


def before_first_request(opening_file):
    """Deliberately pre-holdout only; later prefixes require a new owner."""
    loaded_check()
    path = Path(opening_file).absolute()
    raw = cohort.sha(path)
    saved = cohort.sealed(budget.load(path))
    packet.require(saved.get('schema') == SCHEMA, 'genuine once holdout opening required')
    opened = _proof(saved['accepted_decision_file'])
    pack = opened['decision']['proof']['pack']
    packet.require(path == marker_path(pack['chain'], pack['cohort']['artifact_hash'])
                   and budget.same(saved, _body(opened)) and cohort.sha(path) == raw,
                   'holdout opening differs from fresh original calibration and fixed source identities')
    decision._close(opened['decision']['proof'], opened['bindings'])
    return {'opening': saved, 'proof': opened}
