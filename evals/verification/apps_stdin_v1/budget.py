"""Read-only STDIN budget lineage, original shared-ledger snapshot and bridge.

The quote below is a supported future contract, not evidence of approval. This
module creates no authorization, reservation, marker, directory or SQLite row.
"""
import ast
import json
import sqlite3
import sys
from pathlib import Path

from .. import artifact_cohort, collection_manifest
from ..apps_campaign_v3 import approved_budget as original
from ..artifact_cohort import reservations
from ..collection_manifest import file_hash
from . import packet

SCHEMA = 'buglecat.verification.stdin-approved-budget-extension.v1'
PLAN = 'stdin-808-groups'
QUOTE = '批准 STDIN 808 组方案：主模型追加最多 492 次、Jev 追加最多 176 次'
PREVIOUS_SHA = 'd25cf26ab40a3548e16bd8195b19aec1ae73dc1d28922b9ff8c44226fbfd1422'
PROPOSAL_SHA = '7429e274a44472b4b3f6a2262cde8e45646f86cd02982515641fe5352a2f603e'
SNAPSHOT_SHA = '9cdf3c6b460f17e17817051a0f1f1e2501c537a6f879ed489e74972d6601d7a1'
OLD_CAPS = ((400, 400), (500, 500), (750, 750), (1150, 861))
EXTRAS = {'main_model': 492, 'jev': 176}
LOADED_SHA = file_hash(__file__)
LOADED_ORIGINAL_SHA = file_hash(original.__file__)
LOADED_RESERVATION_SHA = file_hash(artifact_cohort.__file__)
LOADED_FILE_HASH_SHA = file_hash(collection_manifest.__file__)


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


def same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def digest(value):
    import hashlib
    return hashlib.sha256(packet.canonical_bytes(value)).hexdigest()


def parse(raw):
    def invalid(_):
        raise ValueError('nonfinite_budget_json')
    def pairs(items):
        require(len({key for key, _ in items}) == len(items), 'duplicate budget JSON key')
        return dict(items)
    value = json.loads(raw, parse_constant=invalid, object_pairs_hook=pairs)
    packet.canonical_bytes(value)
    return value


def load(path):
    return parse(Path(path).read_bytes())


def typed_same(left, right):
    """Compare every nested literal type, including integer versus float."""
    if type(left) is not type(right):
        return False
    if type(left) in (tuple, list):
        return len(left) == len(right) and all(typed_same(a, b) for a, b in zip(left, right, strict=True))
    if type(left) is dict:
        return len(left) == len(right) and all(
            any(typed_same(key, other) and typed_same(value, right[other]) for other in right)
            for key, value in left.items())
    return left == right


def loaded_check():
    """Reject stale owners, changed literal caps/options, or import aliases."""
    require(file_hash(__file__) == LOADED_SHA and file_hash(original.__file__) == LOADED_ORIGINAL_SHA and
            file_hash(artifact_cohort.__file__) == LOADED_RESERVATION_SHA and
            file_hash(collection_manifest.__file__) == LOADED_FILE_HASH_SHA, 'loaded budget owner differs from disk')
    for module in (sys.modules[__name__], original, artifact_cohort, collection_manifest):
        packet.loaded_functions(module, Path(module.__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__),
        ('SCHEMA', 'PLAN', 'QUOTE', 'PREVIOUS_SHA', 'PROPOSAL_SHA', 'SNAPSHOT_SHA', 'OLD_CAPS', 'EXTRAS'))
    declarations = {target.id: ast.literal_eval(node.value) for node in ast.parse(Path(__file__).read_bytes()).body
                    if isinstance(node, ast.Assign) for target in node.targets
                    if isinstance(target, ast.Name) and target.id in ('OLD_CAPS', 'EXTRAS')}
    require(all(typed_same(globals()[name], value) for name, value in declarations.items()) and
            set(declarations) == {'OLD_CAPS', 'EXTRAS'}, 'loaded nested budget constants differ')
    require(reservations is artifact_cohort.reservations and file_hash is collection_manifest.file_hash,
            'loaded budget import aliases differ')
    return packet.dependencies()


def raw_bindings(paths):
    rows = {str(Path(path).resolve(strict=True)): file_hash(path) for path in paths}
    return [{'file': path, 'sha256': value} for path, value in sorted(rows.items())]


def close(bindings):
    require(all(file_hash(row['file']) == row['sha256'] for row in bindings), 'budget raw bytes drifted')


def previous(path):
    """Authenticate the unchanged 400 -> 500 -> 750 -> 1150/861 lineage."""
    require(file_hash(path) == PREVIOUS_SHA, 'pinned previous authorization required')
    chain = original.resolve(path)
    require(len(chain['bindings']) == len(chain['allowed_sha256']) == 4 and chain['current_sha256'] == PREVIOUS_SHA,
            'original four authorization bindings required')
    require(chain['allowed_sha256'] == [row['sha256'] for row in chain['bindings']] and
            chain['ledger_authorization_sha256'] == chain['allowed_sha256'][0], 'original lineage metadata differs')
    require(same(chain['authorization'], load(chain['bindings'][-1]['file'])), 'original authorization projection differs')
    for row, caps in zip(chain['bindings'], OLD_CAPS, strict=True):
        value = load(row['file'])
        require(file_hash(row['file']) == row['sha256'] and type(value.get('automatic_retries')) is int and
                value['automatic_retries'] == 0, 'original authorization bytes or retries changed')
        for kind, cap in zip(('main_model', 'jev'), caps, strict=True):
            require(type(value[kind]['max_attempts']) is int and value[kind]['max_attempts'] == cap and
                    Path(value[kind]['ledger_dir']).resolve() == Path(chain['authorization'][kind]['ledger_dir']).resolve(),
                    'original cap or ledger roots changed')
    close(chain['bindings'])
    return chain


def _pack(value, previous_chain):
    """Authenticate frozen source selection without reading the current ledger."""
    from .frozen import load_pack
    paths = [Path(value[k]).resolve(strict=True) for k in ('source_proposal_file', 'source_snapshot_file')]
    locked = raw_bindings(paths)
    require(file_hash(paths[0]) == value['source_proposal_sha256'] == PROPOSAL_SHA and
            file_hash(paths[1]) == value['source_snapshot_sha256'] == SNAPSHOT_SHA, 'pinned proposal or snapshot raw bytes differ')
    pack = load_pack(paths[0], paths[1], with_tasks=False)
    proposal, baseline = pack['proposal'], pack['baseline']
    require(proposal['artifact_hash'] == value['source_proposal_hash'] and
            same(baseline['authorization_chain'], previous_chain) and bool(baseline['main_started']) and
            digest(baseline) == proposal['current_ledger_snapshot_sha256'] and
            digest(pack['finished']) == proposal['current_completion_snapshot_sha256'], 'frozen source budget baseline differs')
    require(set(proposal['cases']) == {'development', 'calibration', 'holdout'} and
            [len(proposal['cases'][split]) for split in ('development', 'calibration', 'holdout')] == [8, 400, 400],
            'complete fixed 808 group allocation required')
    groups = [case['group_id'] for split in ('development', 'calibration', 'holdout') for case in proposal['cases'][split]]
    require(len(set(groups)) == 808, 'source allocation cannot reuse groups')
    require(Path(baseline['main_ledger_file']).resolve() ==
            (Path(previous_chain['authorization']['main_model']['ledger_dir']) / 'main-attempts.sqlite3').resolve(),
            'frozen source original SQLite root changed')
    close([*locked, *pack['bindings']])
    return pack


def _extension(value, path, previous_chain):
    require(value.get('schema') == SCHEMA and value.get('approved_plan') == PLAN and
            value.get('approval_status') == 'approved' and value.get('approved') is True and
            type(value.get('user_authorization')) is str and value['user_authorization'] == QUOTE,
            'exact explicit STDIN approval required')
    integers = {'additional_main_attempt_cap': 492, 'additional_jev_attempt_cap': 176, 'automatic_retries': 0,
                'per_group_max_main_attempts': 1, 'per_group_max_jev_attempts': 1}
    require(all(type(value.get(k)) is int and value[k] == count for k, count in integers.items()) and
            value.get('old_attempts_and_failures_count') is True and value.get('old_pending_attempts_count') is True and
            value.get('production_authorized') is False, 'extension cannot reset spent slots or permit retry/production')
    require(value.get('previous_authorization_sha256') == PREVIOUS_SHA and
            Path(value['previous_authorization_file']).resolve() != path and
            previous_chain['current_sha256'] == PREVIOUS_SHA, 'only one pinned extension allowed')
    for kind, extra in EXTRAS.items():
        original_cap = previous_chain['authorization'][kind]['max_attempts']
        require(type(value[kind]['max_attempts']) is int and value[kind]['max_attempts'] == original_cap + extra and
                Path(value[kind]['ledger_dir']).resolve() ==
                Path(previous_chain['authorization'][kind]['ledger_dir']).resolve(), 'approved cap or ledger root changed')


def resolve(path):
    """Default original four-chain, or one exactly quoted pinned STDIN extension."""
    graph = loaded_check()
    path = Path(path).resolve(strict=True)
    raw = path.read_bytes()
    import hashlib
    locked = [{'file': str(path), 'sha256': hashlib.sha256(raw).hexdigest()}]
    value = parse(raw)
    if value.get('schema') != SCHEMA:
        chain = previous(path)
    else:
        prior = previous(value['previous_authorization_file'])
        _extension(value, path, prior)
        _pack(value, prior)
        current = locked[0]['sha256']
        chain = {'authorization': value, 'current_sha256': current,
            'allowed_sha256': [*prior['allowed_sha256'], current],
            'ledger_authorization_sha256': prior['ledger_authorization_sha256'],
            'bindings': [*prior['bindings'], {'file': str(path), 'sha256': current}]}
    close([*locked, *chain['bindings']])
    require(loaded_check() == graph, 'loaded budget graph changed during resolve')
    return chain


def _ledger_rows(ledger):
    with sqlite3.connect(ledger.as_uri() + '?mode=ro', uri=True) as db:
        db.execute('BEGIN')
        metadata = list(db.execute('SELECT key,value FROM metadata ORDER BY key'))
        rows = list(db.execute('SELECT ordinal,group_id,split,started_json,finished_json FROM attempts ORDER BY ordinal'))
        sequence = list(db.execute("SELECT seq FROM sqlite_sequence WHERE name='attempts'"))
    return metadata, rows, sequence


def _reservation_files(roots):
    """Include metadata and all reservation bytes, including spent .started markers."""
    result = []
    for root in roots:
        root = Path(root).resolve(strict=True)
        require(not (root / 'reservation.lock').exists(), 'reservation transaction in progress')
        metadata = root / 'authorization.json'
        if metadata.exists():
            result.append(metadata)
        folder = root / 'reservations'
        if folder.exists():
            files = [path for path in folder.iterdir() if path.is_file()]
            for marker in (path for path in files if path.name.endswith('.started')):
                stem = marker.name[:-len('.started')]
                expected = folder / (stem if stem.endswith('.json') else stem + '.json')
                require(expected.is_file(), 'spent start marker has no original reservation')
            result.extend(files)
    return raw_bindings(result)


def _snapshot_chain(chain):
    """Actual read transaction; fixture chains do not confer real authority."""
    auth = chain['authorization']
    roots = [Path(auth[kind]['ledger_dir']).resolve(strict=True) for kind in ('main_model', 'jev')]
    ledger = (roots[0] / 'main-attempts.sqlite3').resolve(strict=True)
    close(chain['bindings'])
    raw_reservations = _reservation_files(roots)
    images = {row['file']: Path(row['file']).read_bytes() for row in raw_reservations}
    import hashlib
    require(all(hashlib.sha256(images[row['file']]).hexdigest() == row['sha256'] for row in raw_reservations),
            'reservation image changed before snapshot')
    metadata, raw, sequence = _ledger_rows(ledger)
    require(bool(raw), 'historical shared ledger cannot be empty')
    require(len(dict(metadata)) == len(metadata) and
            dict(metadata).get('authorization_sha256') == chain['ledger_authorization_sha256'],
            'original SQLite metadata lineage changed')
    caps = {}
    for row in chain['bindings']:
        value = load(row['file'])
        require(all(type(value[kind]['max_attempts']) is int and value[kind]['max_attempts'] > 0
                    for kind in ('main_model', 'jev')), 'typed authorization caps required')
        caps[row['sha256']] = value['main_model']['max_attempts']
    starts, finished, groups = [], {}, set()
    for ordinal, group, split, encoded, complete in raw:
        start = parse(encoded)
        require(type(group) is str and bool(group) and group not in groups and type(ordinal) is int and
                ordinal == len(starts) + 1 and type(start) is dict and start.get('group_id') == group and
                start.get('authorization_sha256') in caps and ordinal <= caps[start['authorization_sha256']] and
                (start.get('split') is None or start['split'] == split), 'invalid authorization or ordinal SQLite start')
        terminal = parse(complete) if complete is not None else None
        require(terminal is None or (type(terminal) is dict and
                ('group_id' not in terminal or terminal['group_id'] == group) and
                ('global_main_attempt' not in terminal or type(terminal['global_main_attempt']) is int and
                 terminal['global_main_attempt'] == ordinal)), 'historical finish identity changed')
        groups.add(group)
        starts.append({'ordinal': ordinal, 'group_id': group, 'started': start})
        finished[group] = terminal
    require(sequence == [(len(raw),)], 'spent SQLite ordinal deleted or reset')
    main_reserved, _ = reservations(roots[0], 'main_model', chain)
    jev_reserved, _ = reservations(roots[1], 'jev', chain)
    for root, kind, rows in zip(roots, ('main_model', 'jev'), (main_reserved, jev_reserved), strict=True):
        metadata_file = str(root / 'authorization.json')
        if metadata_file in images:
            require(parse(images[metadata_file]) == {'authorization_sha256': chain['ledger_authorization_sha256'], 'kind': kind},
                    'original reservation metadata image differs')
        paths = {path for path in images if Path(path).parent == root / 'reservations' and path.endswith('.json')}
        expected = {str(root / 'reservations' / (digest(row['group_id']) + '.json')) for row in rows}
        require(paths == expected and all(same(parse(images[str(root / 'reservations' / (digest(row['group_id']) + '.json'))]), row)
                                         for row in rows), 'reservation parsed projection differs from original bytes')
        require(sorted(row['reservation_index'] for row in rows) == list(range(1, len(rows) + 1)) and
                all(type(row['automatic_retries']) is int and row['automatic_retries'] == 0 for row in rows),
                'reservation prefix or retries changed')
    main_groups = groups | {row['group_id'] for row in main_reserved}
    require(len(main_groups) <= auth['main_model']['max_attempts'], 'shared main cap exceeded')
    body = {'main_ledger_file': str(ledger), 'main_metadata': metadata, 'main_started': starts,
        'main_reservations': main_reserved, 'jev_reservations': jev_reserved, 'authorization_chain': chain,
        'excluded_groups': sorted(main_groups | {row['group_id'] for row in jev_reserved}),
        'remaining_main_attempts': auth['main_model']['max_attempts'] - len(main_groups)}
    require(_ledger_rows(ledger) == (metadata, raw, sequence), 'SQL rows changed during snapshot')
    require(_reservation_files(roots) == raw_reservations, 'reservation raw file set changed during snapshot')
    close([*chain['bindings'], *raw_reservations])
    return body, finished


def snapshot(path):
    graph = loaded_check()
    chain = resolve(path)
    result = _snapshot_chain(chain)
    require(loaded_check() == graph, 'loaded budget graph changed during snapshot')
    return result


def bridge(source, source_finished, current, finished):
    """Before any new collection, only approved caps/chain may change."""
    graph = loaded_check()
    stable = ('main_ledger_file', 'main_metadata', 'main_started', 'main_reservations', 'jev_reservations', 'excluded_groups')
    require(all(same(source[k], current[k]) for k in stable) and same(source_finished, finished),
            'whole original history changed before budget bridge')
    old, new = source['authorization_chain'], current['authorization_chain']
    require(same(previous(old['bindings'][-1]['file']), old), 'actual original budget lineage differs')
    if same(old, new):
        require(same(source, current), 'unchanged budget snapshot drifted')
        kind = 'unchanged_original_authorization'
    else:
        require(len(old['bindings']) == 4 and len(new['bindings']) == 5 and
                same(new['bindings'][:4], old['bindings']) and new['allowed_sha256'][:4] == old['allowed_sha256'] and
                new['ledger_authorization_sha256'] == old['ledger_authorization_sha256'] and
                new['authorization']['schema'] == SCHEMA, 'only one original-four-to-five extension allowed')
        require(same(resolve(new['bindings'][-1]['file']), new), 'actual approved lineage changed')
        require(type(source['remaining_main_attempts']) is int and type(current['remaining_main_attempts']) is int and
                current['remaining_main_attempts'] == source['remaining_main_attempts'] + EXTRAS['main_model'],
                'approved remaining budget drifted')
        kind = 'approved_caps_only_extension'
    require(loaded_check() == graph, 'loaded budget graph changed during bridge')
    return {'kind': kind, 'source_snapshot_hash': digest(source), 'budget_snapshot_hash': digest(current),
        'source_completion_hash': digest(source_finished), 'source_authorization_sha256': old['current_sha256'],
        'approved_authorization_sha256': new['current_sha256']}
