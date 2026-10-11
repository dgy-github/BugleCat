"""Funded fixed STDIN campaign; only its development prefix can collect.

Construction is pure. Root persists returned bytes only after real approval.
The original 808-member proposal remains immutable after collection begins.
"""
import hashlib
import sys
from copy import deepcopy
from pathlib import Path

from .. import (
    artifact_profile_cohort,
    artifact_v2_development_cohort,
    collection_receipts,
    paid_budget,
)
from . import budget, compiled_sources, frozen, history, packet, protocol, source_authority

SCHEMA = 'buglecat.verification.stdin-funded-source-cohort.v1'
FALSE_FLAGS = ('formal_registration', 'holdout_opened', 'production_authorized',
               'formal_certification', 'candidate_labels_assigned', 'jev_probabilities_read')
ROOT = Path(__file__).resolve().parents[3]
FOLDER = Path(__file__).resolve().parent
EXAMPLES = ROOT / 'rust/crates/ncx-core/examples'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def seal(body):
    return {**body, 'artifact_hash': budget.digest(body)}


def sealed(value):
    packet.require(type(value) is dict, 'sealed cohort object required')
    body = {key: child for key, child in value.items() if key != 'artifact_hash'}
    packet.require(value.get('artifact_hash') == budget.digest(body), 'cohort seal changed')
    return value


def owners():
    """Explicit immutable dependencies; no directory glob or future owners."""
    files = {f'stdin_{name}_source_sha256': FOLDER / (name + '.py') for name in
             ('cohort', 'collection', 'intake', 'budget', 'frozen', 'history', 'source',
              'source_authority', 'sampling', 'protocol', 'packet', 'contract', 'worker', 'oracle')}
    files.update({
        'runner_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect.rs',
        'collection_engine_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect/engine.rs',
        'support_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect/support.rs',
        'ledger_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect/ledger.rs',
        'provenance_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect/provenance.rs',
        'transport_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect/transport.rs',
        'fixture_source_sha256': EXAMPLES / 'verification_apps_stdin_v1_collect/fixture.rs',
        'request_builder_source_sha256': ROOT / 'rust/crates/ncx-provider/src/request.rs',
        'response_parser_source_sha256': ROOT / 'rust/crates/ncx-provider/src/response.rs',
        'legacy_support_source_sha256': EXAMPLES / 'verification_collect/support.rs',
        'shared_development_owner_source_sha256': Path(artifact_v2_development_cohort.__file__),
        'profile_owner_source_sha256': Path(artifact_profile_cohort.__file__),
        'receipt_owner_source_sha256': Path(collection_receipts.__file__),
        'durable_owner_source_sha256': Path(paid_budget.__file__),
    })
    files['stdin_compiled_sources_source_sha256'] = Path(compiled_sources.__file__)
    files.update(compiled_sources.paths(ROOT))
    return files


def loaded_check():
    packet.dependencies()
    budget.loaded_check()
    packet.loaded_functions(compiled_sources, Path(compiled_sources.__file__))
    packet.loaded_constants(compiled_sources, Path(compiled_sources.__file__), ('FILES',))
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'FALSE_FLAGS'))


def check(bindings):
    source_authority.check(bindings)


def _chain(authorization_file, pack):
    """Reuse frozen budget semantics with this transaction's original proof.

    budget.resolve would call frozen.load_pack again. The exact same frozen
    extension validator consumes the already freshly authenticated source pack.
    Nothing from a caller-supplied boolean or mutable global cache is trusted.
    """
    path = Path(authorization_file).resolve(strict=True)
    raw = path.read_bytes()
    value = budget.parse(raw)
    old = budget.previous(value['previous_authorization_file']) if value.get('schema') == budget.SCHEMA else budget.previous(path)
    packet.require(budget.same(old, pack['baseline']['authorization_chain']), 'source budget lineage differs')
    if value.get('schema') != budget.SCHEMA:
        return old
    budget._extension(value, path, old)
    packet.require(Path(value['source_proposal_file']).resolve() == Path(pack['proposal_file'])
                   and Path(value['source_snapshot_file']).resolve() == Path(pack['snapshot_file'])
                   and value['source_proposal_sha256'] == frozen.RAW_PINS['proposal']
                   and value['source_snapshot_sha256'] == frozen.RAW_PINS['snapshot']
                   and value['source_proposal_hash'] == pack['proposal']['artifact_hash'],
                   'approved extension bound to another fixed source pack')
    current = hashlib.sha256(raw).hexdigest()
    check([*pack['bindings'], {'file': str(path), 'sha256': current}])
    return {'authorization': value, 'current_sha256': current,
            'allowed_sha256': [*old['allowed_sha256'], current],
            'ledger_authorization_sha256': old['ledger_authorization_sha256'],
            'bindings': [*old['bindings'], {'file': str(path), 'sha256': current}]}


def _source(proposal_file, snapshot_file):
    result = frozen.load_pack(proposal_file, snapshot_file, with_tasks=True)
    result['proposal_file'] = str(Path(proposal_file).resolve(strict=True))
    result['snapshot_file'] = str(Path(snapshot_file).resolve(strict=True))
    return result


def _cases(pack):
    result = []
    for split in ('development', 'calibration', 'holdout'):
        for original in pack['proposal']['cases'][split]:
            task = pack['tasks'][original['task_id']]
            public = {'question': task['question'], 'starter_code': task['starter_code'],
                      'source_kind': task['source_kind']}
            result.append({**deepcopy(original), 'public_interface': public})
    packet.require(len(result) == 808 and len({row['group_id'] for row in result}) == 808,
                   'entire fixed development/calibration/holdout allocation required')
    return result


def _funding(snapshot, cases):
    main = snapshot['remaining_main_attempts']
    jev = snapshot['authorization_chain']['authorization']['jev']['max_attempts'] - len(snapshot['jev_reservations'])
    packet.require(type(main) is int and type(jev) is int and min(main, jev) >= len(cases),
                   'actual approved budget cannot fund all 808 fixed groups')
    return {'main_at_freeze': main, 'jev_at_freeze': jev, 'maximum_main_attempts': len(cases),
            'maximum_jev_attempts': len(cases), 'earmark_is_not_reservation': True}


def _bridge(pack, chain, current, finished):
    """Caps-only bridge on this transaction's already verified typed chain."""
    source, before = pack['baseline'], pack['finished']
    stable = ('main_ledger_file', 'main_metadata', 'main_started', 'main_reservations', 'jev_reservations', 'excluded_groups')
    packet.require(all(budget.same(source[key], current[key]) for key in stable) and budget.same(before, finished)
                   and budget.same(current['authorization_chain'], chain)
                   and len(chain['bindings']) == 5
                   and budget.same(chain['bindings'][:4], source['authorization_chain']['bindings'])
                   and type(current['remaining_main_attempts']) is int
                   and current['remaining_main_attempts'] == source['remaining_main_attempts'] + 492,
                   'whole original history changed before caps-only bridge')
    return {'kind': 'approved_caps_only_extension', 'source_snapshot_hash': budget.digest(source),
            'budget_snapshot_hash': budget.digest(current), 'source_completion_hash': budget.digest(before),
            'source_authorization_sha256': source['authorization_chain']['current_sha256'],
            'approved_authorization_sha256': chain['current_sha256']}


def collector_build():
    """Anchor the actual compiled entry point before a genuine cohort exists."""
    from . import intake
    path = ROOT / 'rust/target/debug/examples/verification_apps_stdin_v1_collect.exe'
    info = intake.build(path)
    return {'collector_executable_file': str(path.resolve(strict=True)),
            'collector_executable_sha256': info['executable_sha256'], 'collector_build_info': info}


def preregister(proposal_file, snapshot_file, authorization_file, output_directory):
    """Pure build; no directories, authorization, fees or phase markers written."""
    loaded_check()
    pack = _source(proposal_file, snapshot_file)
    chain = _chain(authorization_file, pack)
    current, finished = budget._snapshot_chain(chain)
    packet.require(len(chain['bindings']) == 5, 'genuine approved STDIN extension required')
    bridge = _bridge(pack, chain, current, finished)
    cases = _cases(pack)
    compiled = collector_build()
    all_bindings = source_authority._merge_bindings(pack['bindings'], chain['bindings'],
        [{'file': str(path.resolve()), 'sha256': sha(path)} for path in owners().values()],
        [{'file': compiled['collector_executable_file'], 'sha256': compiled['collector_executable_sha256']}])
    body = {'schema': SCHEMA, 'cases': cases, 'bindings': all_bindings,
            'source_proposal_file': pack['proposal_file'], 'source_proposal_sha256': sha(proposal_file),
            'source_proposal_hash': pack['proposal']['artifact_hash'],
            'source_snapshot_file': pack['snapshot_file'], 'source_snapshot_sha256': sha(snapshot_file),
            'authorization_file': str(Path(authorization_file).resolve()), 'authorization_sha256': chain['current_sha256'],
            'source_authority_file': pack['proposal']['source_authority_file'],
            'source_authority_sha256': pack['proposal']['source_authority_sha256'],
            'main_resource_profile_file': pack['proposal']['main_resource_profile_file'],
            'main_resource_profile_sha256': pack['proposal']['main_resource_profile_sha256'],
            'main_resource_profile_hash': pack['proposal']['main_resource_profile_hash'],
            'protocol': deepcopy(pack['proposal']['protocol']), 'protocol_sha256': budget.digest(pack['proposal']['protocol']),
            'main_collection_settings': deepcopy(frozen.EXPECTED_SETTINGS),
            'main_system_prompt_sha256': pack['proposal']['public_system_prompt_sha256'],
            'oracle_runtime_binding': deepcopy(pack['proposal']['oracle_runtime_binding']),
            'ledger_snapshot': current, 'original_finished': finished, 'approved_budget_bridge': bridge,
            'funded_allocation': _funding(current, cases), 'output_directory': str(Path(output_directory).resolve()),
            'collection_authorized': True, 'collection_authorized_split': 'development',
            'before_main_requests': True, 'certified_threshold': None, 'new_paid_calls': 0,
            **compiled, **{key: False for key in FALSE_FLAGS}}
    check(all_bindings)
    packet.require(budget.same(budget._snapshot_chain(chain), (current, finished)), 'budget changed during preregistration')
    loaded_check()
    return seal(body)


def verified_pack(cohort_file):
    """One whole source proof per batch; no reselecting from current spending."""
    loaded_check()
    locked = {'file': str(Path(cohort_file).resolve(strict=True)), 'sha256': sha(cohort_file)}
    value = sealed(budget.load(cohort_file))
    packet.require(value.get('schema') == SCHEMA and value.get('collection_authorized') is True
                   and value.get('collection_authorized_split') == 'development'
                   and value.get('before_main_requests') is True and value.get('certified_threshold') is None
                   and type(value.get('new_paid_calls')) is int and value['new_paid_calls'] == 0
                   and all(value.get(key) is False for key in FALSE_FLAGS), 'development-only genuine cohort required')
    check([locked, *value['bindings']])
    packet.require(all(budget.same(value.get(key), child) for key, child in collector_build().items()),
                   'original actual compiled collector anchor changed')
    expected = {str(path.resolve()): sha(path) for path in owners().values()}
    recorded = {row['file']: row['sha256'] for row in value['bindings']}
    packet.require(all(recorded.get(path) == digest for path, digest in expected.items()), 'complete current collector owner graph required')
    pack = _source(value['source_proposal_file'], value['source_snapshot_file'])
    packet.require(budget.same(_cases(pack), value['cases'])
                   and value['source_proposal_hash'] == pack['proposal']['artifact_hash']
                   and value['source_proposal_sha256'] == sha(value['source_proposal_file'])
                   and value['source_snapshot_sha256'] == sha(value['source_snapshot_file'])
                   and all(budget.same(value[key], pack['proposal'][key]) for key in (
                       'source_authority_file', 'source_authority_sha256', 'main_resource_profile_file',
                       'main_resource_profile_sha256', 'main_resource_profile_hash'))
                   and budget.same(value['protocol'], pack['proposal']['protocol'])
                   and value['protocol_sha256'] == budget.digest(value['protocol'])
                   and budget.same(value['main_collection_settings'], frozen.EXPECTED_SETTINGS)
                   and budget.same(value['oracle_runtime_binding'], packet.runtime_info())
                   and value['main_system_prompt_sha256'] == pack['proposal']['public_system_prompt_sha256'],
                   'fixed source/protocol/runtime/public prompt changed')
    protocol.validate(value['protocol'])
    chain = _chain(value['authorization_file'], pack)
    packet.require(len(chain['bindings']) == 5 and value['authorization_sha256'] == chain['current_sha256'],
                   'funded authorization changed')
    baseline = value['ledger_snapshot']
    packet.require(budget.same(baseline['authorization_chain'], chain)
                   and budget.same(_bridge(pack, chain, baseline, value['original_finished']), value['approved_budget_bridge'])
                   and budget.same(_funding(baseline, value['cases']), value['funded_allocation']), 'original funded budget bridge changed')
    current, finished = budget._snapshot_chain(chain)
    history.validate_prefix(baseline, value['original_finished'], current, finished, value['cases'], value['artifact_hash'])
    own_started = {row['group_id'] for row in current['main_started']} & {case['group_id'] for case in value['cases']}
    remaining_jev = chain['authorization']['jev']['max_attempts'] - len(current['jev_reservations'])
    packet.require(current['remaining_main_attempts'] >= 808 - len(own_started) and remaining_jev >= 808,
                   'remaining shared budget cannot fund complete immutable allocation')
    check([locked, *value['bindings']])
    loaded_check()
    return {'cohort': value, 'source_pack': pack, 'current': current, 'finished': finished,
            'bindings': [locked, *value['bindings']]}


def validate(path):
    return verified_pack(path)['cohort']
