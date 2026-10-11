"""Authenticate the original STDIN proposal after append-only collection.

The frozen source transaction reads no current ledger, scores or candidates.
Current accounting and collection permission belong to budget/cohort owners.
Only the exact pre-candidate revision2 proposal and original snapshot are valid.
"""
import hashlib
import sys
import types
from copy import deepcopy
from pathlib import Path

from .. import artifact_profile_cohort, artifact_v2_development_cohort, collection_manifest
from ..apps_campaign_v3 import approved_budget
from . import packet, protocol, sampling, source_authority

RAW_PINS = {
    'proposal': '7429e274a44472b4b3f6a2262cde8e45646f86cd02982515641fe5352a2f603e',
    'snapshot': '9cdf3c6b460f17e17817051a0f1f1e2501c537a6f879ed489e74972d6601d7a1',
    'authority': 'f3f757e568980a2d59aacbfd28360b22110753cc7d50bd7e13f69ef4ff6e731d',
    'authorization': 'd25cf26ab40a3548e16bd8195b19aec1ae73dc1d28922b9ff8c44226fbfd1422',
    'profile': 'e0044b9de9e61ca55358a5081eba27a85c352f17ef3d74afa8ba028bc9064427',
}
FALSE_FLAGS = ('candidate_labels_read', 'jev_probabilities_read', 'collection_authorized',
               'formal_cohort_created', 'formal_registration', 'holdout_opened', 'production_authorized',
               'natural_negative_sufficiency_proven')
ZERO_COUNTS = ('model_calls', 'jev_calls', 'oracle_executions', 'reservations')
EXPECTED_SETTINGS = {'model': 'deepseek-flash', 'reasoning_effort': 'high', 'max_tokens': 16384,
                     'timeout_s': 120, 'max_retries': 0, 'provider_protocol': 'openai'}


def same(left, right):
    return packet.canonical_bytes(left) == packet.canonical_bytes(right)


def prompt_declaration():
    # Other declarations in this legacy module are decorated transaction
    # wrappers. This owner invokes only its plain system_prompt declaration.
    module = artifact_v2_development_cohort
    actual = module.system_prompt
    packet.require(type(actual) is types.FunctionType, 'loaded original prompt function differs')
    image = compile(Path(module.__file__).read_bytes(), actual.__code__.co_filename, 'exec')
    declared = {code.co_name: code for code in image.co_consts if isinstance(code, types.CodeType)}
    packet.require(actual.__code__ == declared.get('system_prompt') and actual.__globals__ is vars(module),
                   'loaded original prompt bytecode differs')


def loaded_check():
    modules = (sys.modules[__name__], approved_budget, artifact_profile_cohort,
               artifact_v2_development_cohort, collection_manifest)
    for module in modules:
        if module is artifact_v2_development_cohort:
            continue
        packet.loaded_functions(module, Path(module.__file__))
    prompt_declaration()
    packet.loaded_constants(sys.modules[__name__], Path(__file__),
                            ('RAW_PINS', 'FALSE_FLAGS', 'ZERO_COUNTS', 'EXPECTED_SETTINGS'))
    packet.loaded_constants(artifact_profile_cohort, Path(artifact_profile_cohort.__file__), ('SETTINGS',))
    packet.require(same(artifact_profile_cohort.SETTINGS, EXPECTED_SETTINGS), 'typed original main settings changed')
    return {str(Path(module.__file__).resolve()): collection_manifest.file_hash(module.__file__)
            for module in modules}


def input_binding(path, role):
    result = source_authority.binding(path)
    packet.require(result['sha256'] == RAW_PINS[role], 'pinned original ' + role + ' bytes changed')
    return result


def proposal_gate(value):
    source_authority.sealed(value)
    packet.require(value.get('schema') == sampling.SCHEMA and value.get('seed') == sampling.SEED
                   and all(value.get(k) is False for k in FALSE_FLAGS)
                   and all(type(value.get(k)) is int and value[k] == 0 for k in ZERO_COUNTS)
                   and value.get('certified_threshold') is None,
                   'original source-only proposal cannot grant collection or certification')
    packet.require(value.get('whole_development_allocation_excluded_from_formal') is True
                   and value.get('membership_changes_after_candidates_or_probabilities_allowed') is False
                   and same(value.get('strata'), sampling.STRATA)
                   and same(value.get('main_collection_settings'), EXPECTED_SETTINGS),
                   'frozen allocation or original main settings changed')
    packet.require(all(type(value.get(k)) is int and value[k] == expected for k, expected in (
        ('original_main_started', 834), ('original_phase_jev_reservations', 229),
        ('maximum_new_main_requests', 808), ('maximum_new_jev_requests', 808),
        ('additional_main_requests_needed', 492), ('additional_jev_requests_needed', 176))),
        'original pre-candidate accounting and complete proposed allocation required')
    protocol.validate(value['protocol'])
    packet.require(same(value['packet_dependency_sha256'], packet.dependencies())
                   and same(value['oracle_runtime_binding'], packet.runtime_info()),
                   'original packet or runtime changed')


def snapshot_gate(proposal, record, chain):
    """Pure original-snapshot association; never accepts a current ledger here."""
    packet.require(type(record) is dict and set(record) == {'snapshot', 'finished'},
                   'original accounting snapshot shape required')
    baseline, finished = record['snapshot'], record['finished']
    packet.require(source_authority.digest(baseline) == proposal['current_ledger_snapshot_sha256']
                   and source_authority.digest(finished) == proposal['current_completion_snapshot_sha256'],
                   'original source snapshot digest changed')
    packet.require(same(baseline['authorization_chain'], chain)
                   and len(chain['bindings']) == len(chain['allowed_sha256']) == 4
                   and chain['current_sha256'] == RAW_PINS['authorization']
                   and Path(baseline['main_ledger_file']).resolve() == Path(proposal['main_ledger_file']).resolve(),
                   'original shared ledger or four-link authorization lineage changed')
    starts, reservations = baseline['main_started'], baseline['jev_reservations']
    packet.require(type(starts) is list and len(starts) == 834
                   and type(reservations) is list and len(reservations) == 229
                   and type(baseline['remaining_main_attempts']) is int and baseline['remaining_main_attempts'] == 316,
                   'original spent-prefix counts or remaining main budget changed')
    groups = [row['group_id'] for row in starts]
    packet.require(type(finished) is dict and set(finished) == set(groups) and len(set(groups)) == len(starts)
                   and all(type(group) is str and bool(group) for group in groups)
                   and all(type(row['ordinal']) is int for row in starts)
                   and [row['ordinal'] for row in starts] == list(range(1, 835)),
                   'complete original main starts and completions required')
    metadata = baseline['main_metadata']
    packet.require(len(dict(metadata)) == len(metadata)
                   and dict(metadata).get('authorization_sha256') == chain['ledger_authorization_sha256'],
                   'original shared metadata origin changed')
    return baseline, finished


def allocation_gate(proposal, authority, baseline, legacy):
    """Reproduce selection from the original baseline, never from new starts."""
    excluded = legacy | set(baseline['excluded_groups'])
    packet.require(proposal['excluded_started_reserved_or_complete_old_allocation_groups'] == sorted(excluded),
                   'whole original history and old allocation exclusion changed')
    selected = sampling.choose(authority['cases'], excluded)
    packet.require(same(selected, proposal['cases']), 'frozen whole allocation membership/order changed')
    return [deepcopy(case) for stage in sampling.STAGES for case in selected[stage]]


def load_pack(proposal_file, snapshot_file, *, with_tasks=False):
    """Read-only batch proof, valid after new starts without reselecting sources.

    Return the original proposal, authority, baseline/finished, complete raw
    bindings, and optionally authenticated original tasks/train. No authority
    for paid calls or holdout consumption is created. No Oracle is executed.
    """
    packet.require(type(with_tasks) is bool, 'with_tasks must be bool')
    owner_graph = loaded_check()
    locked = [input_binding(proposal_file, 'proposal'), input_binding(snapshot_file, 'snapshot')]
    proposal = source_authority.load(proposal_file)
    proposal_gate(proposal)
    locked += [input_binding(proposal['source_authority_file'], 'authority'),
               input_binding(proposal['authorization_file'], 'authorization'),
               input_binding(proposal['main_resource_profile_file'], 'profile')]
    packet.require(proposal['source_authority_sha256'] == RAW_PINS['authority']
                   and proposal['authorization_sha256'] == RAW_PINS['authorization']
                   and proposal['main_resource_profile_sha256'] == RAW_PINS['profile'],
                   'original input role association changed')
    source_authority.check(locked)
    chain = approved_budget.resolve(proposal['authorization_file'])
    baseline, finished = snapshot_gate(proposal, source_authority.load(snapshot_file), chain)
    source_authority.check(locked)
    authority = source_authority.validate(proposal['source_authority_file'])
    packet.require(authority['artifact_hash'] == proposal['source_authority_hash'], 'original authority hash changed')
    source_authority.check(locked)
    profile, profile_bindings = artifact_profile_cohort.validate_profile(proposal['main_resource_profile_file'])
    packet.require(profile['artifact_hash'] == proposal['main_resource_profile_hash'], 'original profile changed')
    source_authority.check(locked)
    legacy, legacy_bindings = sampling.legacy_exclusions()
    cases = allocation_gate(proposal, authority, baseline, legacy)
    prompt = artifact_v2_development_cohort.system_prompt()
    packet.require(hashlib.sha256(prompt.encode()).hexdigest() == proposal['public_system_prompt_sha256'],
                   'original public system prompt changed')
    owner_bindings = [{'file': path, 'sha256': sha} for path, sha in owner_graph.items()]
    required = [*locked, *authority['bindings'], *profile_bindings, *legacy_bindings, *chain['bindings']]
    recorded = {item['file']: item['sha256'] for item in proposal['bindings']}
    packet.require(len(recorded) == len(proposal['bindings']), 'duplicate proposal binding')
    # The snapshot and frozen owner are subsequent proof material. All other
    # original role bytes must already belong to the pre-candidate proposal.
    packet.require(all(recorded.get(item['file']) == item['sha256'] for item in required
                       if item['file'] not in {locked[0]['file'], locked[1]['file']}),
                   'complete original proposal role graph required')
    all_bindings = source_authority._merge_bindings(proposal['bindings'], required, owner_bindings)
    tasks, train = {}, Path(authority['train_file'])
    if with_tasks:
        # This helper consumes only this transaction's fresh complete C1 proof.
        # No caller-sealed object or prior transaction cache reaches this point.
        by_group = {case['group_id']: case for case in authority['cases']}
        tasks, train = source_authority._read_original_tasks(authority, [by_group[c['group_id']] for c in cases])
    source_authority.check(all_bindings)
    packet.require(loaded_check() == owner_graph, 'frozen source owner graph changed during proof')
    packet.require(same(proposal['packet_dependency_sha256'], source_authority.loaded_check()),
                   'loaded source graph changed during frozen proof')
    return {'proposal': proposal, 'authority': authority, 'baseline': baseline, 'finished': finished,
            'bindings': all_bindings, 'tasks': tasks, 'train': train}
