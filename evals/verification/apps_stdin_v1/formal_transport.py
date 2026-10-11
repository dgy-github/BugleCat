"""Same global reservation lock/ordinal and permanent pre-HTTP started marker.

The registered pack is internal to the freshly validated action. This module
does not authenticate arbitrary caller packs and is not a public fee API.
"""
import os
import sqlite3
import sys
from pathlib import Path

from .. import paid_budget
from ..paid_budget import durable_new
from . import budget, cohort, development_registration, packet, protocol
from . import formal_originals as originals
from .development_registration import safe_directory

SCHEMA = 'buglecat.verification.stdin-formal-calibration-jev-attempt-start.v1'


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA',))
    packet.loaded_functions(paid_budget, Path(paid_budget.__file__))
    packet.require(durable_new is paid_budget.durable_new and safe_directory is development_registration.safe_directory, 'actual durability/namespace alias changed')


def _prior(pack, rows=None):
    # Each previous slot needs original terminal bytes, never merely a claim in
    # the reservation. Pending/stranded slots prevent later spending; terminal
    # failures remain in the denominator and never permit that group to retry.
    from . import formal_jev, formal_observations
    root = Path(pack['chain']['authorization']['jev']['ledger_dir'])
    images = {}
    for item in pack['new_reservations'] if rows is None else rows:
        row = budget.load(item['prepared_file'])
        packet.require((Path(row['output_directory']) / 'receipt.json').is_file(),
                       'prior unresolved Jev slot prevents further spending')
        prepared_sha = cohort.sha(item['prepared_file'])
        packet.require(item['prepared_sha256'] == prepared_sha, 'prior prepared original bytes changed')
        reservation_file = root / 'reservations' / (budget.digest(item['group_id']) + '.json')
        executable = row['sender_executable_file']
        if executable not in images:
            images[executable] = formal_observations.sender.build_info(executable)
        formal_jev._validate(item['prepared_file'], pack, images[executable])
        finished = formal_observations._join(row, Path(item['prepared_file']), item,
            Path(row['output_directory']), images[executable], reservation_file=reservation_file)
        packet.require(protocol.terminal_status(finished['call_status']), 'prior unresolved Jev slot prevents further spending')


def _locked_close(pack, connection):
    # No ordinary snapshot under our own shared reservation lock.
    cohort.check(pack['proof_bindings'])
    packet.require(connection.in_transaction
                   and budget.same(originals.development_originals.binding_images(pack['chain']), pack['budget_bindings'])
                   and budget.same(budget._ledger_rows(Path(pack['current']['main_ledger_file'])), pack['sql_image']),
                   'original formal bytes/accounting changed under short shared lock')
    root = Path(pack['chain']['authorization']['jev']['ledger_dir'])
    packet.require(budget.same(budget.load(root / 'authorization.json'),
        {'authorization_sha256': pack['chain']['ledger_authorization_sha256'], 'kind': 'jev'}), 'actual shared metadata changed')
    for directories, image in pack['directory_images']:
        packet.require(budget.same(originals.development_review.evidence(directories), image), 'original archive topology changed')
    loaded_check()


def _lock(pack, action):
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    descriptor = os.open(root / 'reservation.lock', os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.fsync(descriptor)
        with sqlite3.connect(pack['current']['main_ledger_file']) as connection:
            connection.execute('BEGIN IMMEDIATE')
            _locked_close(pack, connection)
            cohort.check([{'file': pack['registration_file'], 'sha256': pack['registration_sha256']}])
            loaded_check()
            return action(root)
    finally:
        os.close(descriptor)
        (root / 'reservation.lock').unlink()


def _reserve(prepared_file, row, pack):
    loaded_check()
    index = len(pack['new_reservations'])
    packet.require(index < len(pack['requests']) and row['id'] == pack['requests'][index]['id'],
                   'cannot skip/resend a registered group')
    _prior(pack)
    packet.require(len(pack['current']['jev_reservations']) < pack['chain']['authorization']['jev']['max_attempts'],
                   'actual cumulative Jev budget exhausted')
    raw_sha = cohort.sha(prepared_file)

    def create(root):
        cohort.check([{'file': str(Path(prepared_file).resolve()), 'sha256': raw_sha}])
        safe_directory(root, 'reservations', create=True)
        target = root / 'reservations' / (budget.digest(row['group_id']) + '.json')
        value = {'schema': 'buglecat.verification.paid-reservation.v1', 'kind': 'jev',
            'group_id': row['group_id'], 'request_hash': row['request_hash'],
            'authorization_sha256': pack['chain']['current_sha256'],
            'reservation_index': len(pack['current']['jev_reservations']) + 1,
            'status': 'reserved_before_transport', 'automatic_retries': 0,
            'registration_file': pack['registration_file'], 'registration_sha256': pack['registration_sha256'],
            'registration_hash': pack['registration']['artifact_hash'], 'prepared_file': str(Path(prepared_file).resolve()),
            'prepared_sha256': raw_sha, 'sender_executable_sha256': row['sender_build_info']['runner_exe_sha256'],
            'output_directory': row['output_directory'], 'endpoint_sha256': row['endpoint_sha256']}
        durable_new(target, value)
        return {**value, 'reservation_file': str(target), 'reservation_sha256': cohort.sha(target)}

    return _lock(pack, create)


def _claim(prepared_file, row, pack, reservation_file):
    loaded_check()
    path = Path(reservation_file).resolve(strict=True)
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve(strict=True)
    packet.require(path == root / 'reservations' / (budget.digest(row['group_id']) + '.json'), 'reservation path changed')
    reserved = budget.load(path)
    packet.require(pack['new_reservations'] and budget.same(reserved, pack['new_reservations'][-1])
                   and reserved['group_id'] == row['group_id'] and reserved['request_hash'] == row['request_hash']
                   and reserved['prepared_file'] == str(Path(prepared_file).resolve())
                   and reserved['prepared_sha256'] == cohort.sha(prepared_file)
                   and reserved['output_directory'] == row['output_directory']
                   and reserved['sender_executable_sha256'] == row['sender_build_info']['runner_exe_sha256']
                   and reserved['endpoint_sha256'] == row['endpoint_sha256'], 'original unique reservation/actual compiled body changed')
    _prior(pack, pack['new_reservations'][:-1])
    raw_sha = cohort.sha(path)

    def create(_):
        cohort.check([{'file': str(path), 'sha256': raw_sha}])
        cohort.check([{'file': str(Path(prepared_file).resolve()), 'sha256': reserved['prepared_sha256']}])
        output = Path(row['output_directory'])
        root = Path(pack['chain']['authorization']['jev']['ledger_dir']).absolute()
        packet.require(output.parent.resolve().is_relative_to(root.resolve(strict=True)), 'attempt output escapes actual root')
        relative = output.relative_to(root)
        safe_directory(root, *relative.parts)
        output.mkdir(exist_ok=False)
        start = {'schema': SCHEMA, 'group_id': row['group_id'], 'request_hash': row['request_hash'],
                 'reservation_file': str(path), 'reservation_sha256': raw_sha, 'reservation_index': reserved['reservation_index'],
                 'prepared_file': str(Path(prepared_file).resolve()), 'prepared_sha256': cohort.sha(prepared_file),
                 'output_directory': str(output), 'new_paid_calls': 0}
        # A crash here leaves a permanent directory/slot; never clean/retry it.
        durable_new(path.with_suffix('.started'), start)
        return start

    return _lock(pack, create)


def reserve(prepared_file):
    """Public boundary: genuine original proof, not a caller supplied pack."""
    from . import formal_jev
    row, pack = formal_jev.validate(prepared_file)
    return _reserve(prepared_file, row, pack)


def claim(prepared_file, reservation_file):
    """Public boundary: authenticate original shared slot before claiming."""
    from . import formal_jev
    row, pack = formal_jev.validate(prepared_file)
    return _claim(prepared_file, row, pack, reservation_file)
