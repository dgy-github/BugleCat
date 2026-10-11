"""Pure association of one frozen campaign with an append-only shared prefix.

Budget authenticates current SQLite/reservation bytes; a cohort authenticates
the original baseline and cases. This helper verifies their association only,
and grants no phase opening, collection, registration or certification.
"""
import sys
from pathlib import Path

from . import packet, sampling, source_authority

IDENTITY = ('id', 'group_id', 'task_id', 'task_sha256', 'source_kind', 'split')
SCHEMA = 'buglecat.verification.stdin-original-history-prefix.v1'


def same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('IDENTITY', 'SCHEMA'))
    return source_authority.binding(__file__)


def _hash(value):
    return type(value) is str and len(value) == 64 and all(c in '0123456789abcdef' for c in value)


def _starts(rows):
    packet.require(type(rows) is list, 'complete starts required')
    result = {}
    for ordinal, row in enumerate(rows, 1):
        packet.require(type(row) is dict and type(row.get('ordinal')) is int and row['ordinal'] == ordinal
                       and type(row.get('group_id')) is str and bool(row['group_id'])
                       and row['group_id'] not in result and type(row.get('started')) is dict,
                       'unique complete original ordinal prefix required')
        result[row['group_id']] = row
    return result


def _original(baseline, original_finished, current, finished):
    packet.require(same(baseline['main_metadata'], current['main_metadata'])
                   and same(baseline['authorization_chain'], current['authorization_chain'])
                   and Path(baseline['main_ledger_file']).resolve() == Path(current['main_ledger_file']).resolve(),
                   'frozen funded authorization, metadata or shared ledger changed')
    original = _starts(baseline['main_started'])
    starts = _starts(current['main_started'])
    packet.require(type(original_finished) is dict and set(original_finished) == set(original)
                   and type(finished) is dict and set(finished) == set(starts)
                   and same(current['main_started'][:len(original)], baseline['main_started'])
                   and all(same(finished[group], value) for group, value in original_finished.items()),
                   'original main history, including pending and failures, changed')
    packet.require(same(current['main_reservations'], baseline['main_reservations']),
                   'unaccounted main reservation appended or original reservation changed')
    return starts, current['main_started'][len(original):]


def _cases(cases, allowed_splits):
    packet.require(type(cases) is list and bool(cases) and type(allowed_splits) is tuple
                   and allowed_splits in (('development',), ('development', 'calibration'), sampling.STAGES),
                   'ordered allowed phase prefix required; phase opening needs its own owner')
    ids, groups, previous = set(), set(), -1
    selected = []
    for row in cases:
        packet.require(type(row) is dict and all(key in row for key in IDENTITY)
                       and type(row['id']) is str and bool(row['id']) and row['id'] not in ids
                       and type(row['group_id']) is str and bool(row['group_id']) and row['group_id'] not in groups
                       and type(row['task_id']) is int and row['task_id'] >= 0 and _hash(row['task_sha256'])
                       and row['source_kind'] == 'apps_stdin_restricted_script' and row['split'] in sampling.STAGES,
                       'unique complete STDIN campaign identity required')
        phase = sampling.STAGES.index(row['split'])
        packet.require(phase >= previous, 'frozen campaign phase order changed')
        previous = phase
        ids.add(row['id'])
        groups.add(row['group_id'])
        if row['split'] in allowed_splits:
            selected.append(row)
    return selected


def _new_main(rows, finished, selected, campaign_hash, authorization_sha):
    packet.require(len(rows) <= len(selected), 'main start without an opened phase')
    for index, row in enumerate(rows):
        case, start = selected[index], row['started']
        packet.require(row['group_id'] == case['group_id']
                       and start.get('artifact_cohort_hash') == campaign_hash
                       and start.get('authorization_sha256') == authorization_sha
                       and all(same(start.get(key), case[key]) for key in IDENTITY),
                       'new main starts must follow exactly the opened frozen source prefix')
        terminal = finished[row['group_id']]
        packet.require(terminal is None or (type(terminal) is dict
                       and terminal.get('group_id') == case['group_id']
                       and type(terminal.get('global_main_attempt')) is int
                       and terminal['global_main_attempt'] == row['ordinal']), 'new main finish identity changed')
        packet.require(terminal is not None or index == len(rows) - 1,
                       'new main pending attempt cannot be skipped or resent')


def _reservations(rows):
    packet.require(type(rows) is list and all(type(row) is dict for row in rows), 'complete reservation list required')
    ordered = sorted(rows, key=lambda row: row.get('reservation_index', -1))
    groups = set()
    for index, row in enumerate(ordered, 1):
        packet.require(type(row) is dict and type(row.get('reservation_index')) is int
                       and row['reservation_index'] == index
                       and type(row.get('group_id')) is str and bool(row['group_id']) and row['group_id'] not in groups,
                       'unique complete Jev reservation ordinal prefix required')
        groups.add(row['group_id'])
    return ordered


def _new_jev(baseline, current, eligible, selected, new_main, finished, authorization_sha):
    original, reservations = _reservations(baseline['jev_reservations']), _reservations(current['jev_reservations'])
    packet.require(same(reservations[:len(original)], original), 'original Jev reservation removed or changed')
    packet.require(type(eligible) is list, 'authenticated eligible case prefix required')
    by_group = {row['group_id']: row for row in selected}
    available = {row['group_id'] for row in new_main}
    ordered = []
    for case in eligible:
        packet.require(type(case) is dict and case.get('group_id') in by_group
                       and case['group_id'] in available
                       and all(same(case.get(k), by_group[case['group_id']][k]) for k in IDENTITY)
                       and finished[case['group_id']] is not None
                       and case.get('label') in ('supported', 'unsupported'),
                       'Jev eligibility requires original completed binary case association')
        ordered.append(case['group_id'])
    expected_order = [case['group_id'] for case in selected if case['group_id'] in set(ordered)]
    packet.require(ordered == expected_order, 'registered Jev source order or uniqueness changed')
    new = reservations[len(original):]
    packet.require(len(new) <= len(eligible), 'Jev reserved before authenticated binary registration')
    for index, row in enumerate(new):
        packet.require(row['group_id'] == eligible[index]['group_id']
                       and row.get('schema') == 'buglecat.verification.paid-reservation.v1'
                       and row.get('kind') == 'jev' and row.get('authorization_sha256') == authorization_sha
                       and row.get('status') == 'reserved_before_transport'
                       and type(row.get('automatic_retries')) is int and row['automatic_retries'] == 0
                       and _hash(row.get('request_hash')), 'new Jev reservation identity, source prefix or retries changed')
    return new


def validate_prefix(baseline, original_finished, current, finished, cases, campaign_hash, *,
                    allowed_splits=('development',), jev_cases=None):
    """Require original history plus permitted main/Jev ordered additions.

    Caller-supplied phase/eligibility values are associations, not authenticated
    openings or labels. Actual cohort/registration owners must establish them.
    This pure result never authorizes transport, funding or holdout consumption.
    """
    before = loaded_check()
    packet.require(_hash(campaign_hash), 'original campaign hash required')
    _, new = _original(baseline, original_finished, current, finished)
    selected = _cases(cases, allowed_splits)
    authorization_sha = baseline['authorization_chain']['current_sha256']
    _new_main(new, finished, selected, campaign_hash, authorization_sha)
    jev = _new_jev(baseline, current, [] if jev_cases is None else jev_cases,
                   selected, new, finished, authorization_sha)
    packet.require(loaded_check() == before, 'history prefix owner changed during validation')
    return {'schema': SCHEMA, 'main_new_starts': len(new), 'jev_new_reservations': len(jev),
            'original_main_starts': len(baseline['main_started']),
            'original_jev_reservations': len(baseline['jev_reservations']),
            'source_order_and_original_history_association_verified': True,
            'original_provenance_authenticated_by_this_helper': False,
            'collection_authorized': False, 'formal_registration': False, 'holdout_opened': False,
            'production_authorized': False, 'certified_threshold': None}
