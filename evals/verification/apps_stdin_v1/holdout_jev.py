"""Genuine fixed-threshold holdout preparation; validation sends no HTTP.

One original proof per action. The ephemeral pack remains inside this owner;
no caller eligibility list, synthetic source flag or persistent proof cache.
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

from .. import paid_budget
from ..paid_budget import durable_new
from . import budget, cohort, development_registration, packet
from . import holdout_score_originals as originals
from .development_registration import safe_directory

SCHEMA = 'buglecat.verification.stdin-formal-holdout-jev-prepared.v1'
ROOT = Path(__file__).resolve().parents[3]
EXAMPLE = 'verification_apps_stdin_v1_jev_holdout'
ENDPOINT = 'https://api.typesafe.ai/v1/systemone'


def runner_sources():
    from . import holdout_collection
    result = dict(holdout_collection.runner_sources())
    result['holdout_main_runner_source_sha256'] = result.pop('runner_source_sha256')
    for name in ('holdout_terminal', 'holdout_registration', 'holdout_evaluation', 'holdout_score_originals',
                 'holdout_jev', 'holdout_transport', 'holdout_observations'):
        result['stdin_' + name + '_source_sha256'] = Path(__file__).with_name(name + '.py')
    folder = ROOT / 'rust/crates/ncx-core/examples'
    result['runner_source_sha256'] = folder / (EXAMPLE + '.rs')
    for name in ('engine', 'provenance', 'support', 'fixture'):
        result['jev_' + name + '_source_sha256'] = folder / EXAMPLE / (name + '.rs')
    return result


def loaded_check():
    packet.loaded_functions(sys.modules[__name__], Path(__file__))
    packet.loaded_constants(sys.modules[__name__], Path(__file__), ('SCHEMA', 'EXAMPLE', 'ENDPOINT'))
    originals.loaded_check()
    packet.require(durable_new is paid_budget.durable_new and safe_directory is development_registration.safe_directory, 'actual preparation durability/namespace alias changed')


def build_info(executable):
    loaded_check()
    executable = Path(executable).resolve(strict=True)
    env = {key: value for key, value in os.environ.items() if not any(word in key.upper() for word in ('KEY', 'TOKEN', 'SECRET'))}
    done = subprocess.run([str(executable), 'build-info'], capture_output=True, timeout=20, env=env)
    packet.require(done.returncode == 0, 'actual sender build-info unavailable')
    info = budget.parse(done.stdout.decode('utf-8'))
    expected = {role: cohort.sha(path) for role, path in runner_sources().items()}
    packet.require(info.get('schema') == 'buglecat.verification.stdin-formal-holdout-jev-build.v1'
                   and info.get('runner_exe_sha256') == cohort.sha(executable)
                   and all(info.get(key) == value for key, value in expected.items())
                   and info.get('template_version') == packet.TEMPLATE, 'actual compiled sender/source identity changed')
    return info


def prepared_value(pack, request, executable, compiled, directory):
    return cohort.seal({'schema': SCHEMA, **request, 'registration_file': pack['registration_file'],
        'registration_sha256': pack['registration_sha256'], 'registration_hash': pack['registration']['artifact_hash'],
        'cohort_hash': pack['cohort']['artifact_hash'], 'sender_executable_file': str(Path(executable).resolve()),
        **{key: pack[key] for key in ('holdout_opening_file', 'holdout_opening_sha256', 'holdout_opening_hash')},
        'sender_build_info': compiled, 'output_directory': str(Path(directory).resolve()),
        'endpoint_sha256': budget.digest(ENDPOINT), 'template_version': packet.TEMPLATE,
        'qualified_calibration_threshold': pack['opening']['qualified_calibration_threshold'],
        'expected_actual_model': 'jev-1.13.0', 'development_only': False, 'formal_registration': True, 'holdout_only': True,
        'holdout_opened': True, 'production_authorized': False, 'formal_certification': False,
        'certified_threshold': None})


def _image_bindings(compiled, executable):
    return [{'file': str(path.resolve()), 'sha256': compiled[role]} for role, path in runner_sources().items()] + [
        {'file': str(Path(executable).resolve()), 'sha256': compiled['runner_exe_sha256']}]


def _validate(prepared_file, pack, compiled):
    row = cohort.sealed(budget.load(prepared_file))
    request = next((case for case in pack['requests'] if case['id'] == row.get('id')), None)
    packet.require(request is not None, 'caller packet is not an authentic original binary group')
    expected = prepared_value(pack, request, row['sender_executable_file'], compiled, row['output_directory'])
    packet.require(budget.same(expected, row), 'prepared source/body/compiled/executable binding changed')
    # expected is rebuilt from this action's authenticated original packets;
    # exact equality above retains all fields without rerunning full source per slot.
    packet.require(row['request_json'].encode() == packet.canonical_bytes(row['request'])
                   and budget.digest(row['request']) == row['request_hash'], 'exact original submitted UTF8 body changed')
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve()
    safe_directory(root, 'stdin-holdout-attempts', pack['cohort']['artifact_hash'])
    output = root / 'stdin-holdout-attempts' / pack['cohort']['artifact_hash'] / row['id']
    packet.require(Path(row['output_directory']).resolve() == output
                   and Path(prepared_file).resolve() == output.parent / (row['id'] + '.prepared.json'),
                   'prepared/attempt must use original shared fixed namespace')
    return row


def prepare(registration_file, executable):
    pack = originals.proof(registration_file)
    compiled = build_info(executable)
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], _image_bindings(compiled, executable))
    root = Path(pack['chain']['authorization']['jev']['ledger_dir']).resolve()
    folder = safe_directory(root, 'stdin-holdout-attempts', pack['cohort']['artifact_hash'], create=True)
    rows = []
    for request in pack['requests']:
        path = folder / (request['id'] + '.prepared.json')
        row = prepared_value(pack, request, executable, compiled, folder / request['id'])
        if path.exists():
            # Preparation has no fee authority. Recover only identical original
            # bytes; never rewrite a partially prepared or claimed attempt.
            packet.require(budget.same(cohort.sealed(budget.load(path)), row), 'existing original prepared packet changed')
        else:
            durable_new(path, row)
        rows.append({'id': row['id'], 'file': str(path), 'sha256': cohort.sha(path)})
    originals.close(pack)
    return rows


def validate(path):
    loaded_check()
    raw = cohort.sha(path)
    row = cohort.sealed(budget.load(path))
    pack = originals.proof(row['registration_file'])
    compiled = build_info(row['sender_executable_file'])
    result = _validate(path, pack, compiled)
    graph = [{'file': str(file.resolve()), 'sha256': compiled[role]} for role, file in runner_sources().items()]
    pack['proof_bindings'] = cohort.source_authority._merge_bindings(pack['proof_bindings'], graph,
        [{'file': str(Path(path).resolve()), 'sha256': raw},
         {'file': str(Path(row['sender_executable_file']).resolve()), 'sha256': compiled['runner_exe_sha256']}])
    packet.require(cohort.sha(path) == raw, 'prepared bytes changed during proof')
    originals.close(pack)
    return result, pack


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('validate', 'reserve', 'claim', 'prepare'))
    parser.add_argument('--prepared')
    parser.add_argument('--registration')
    parser.add_argument('--executable')
    parser.add_argument('--reservation')
    args = parser.parse_args()
    if args.action == 'prepare':
        result = {'prepared': prepare(args.registration, args.executable), 'new_paid_calls': 0}
    else:
        row, pack = validate(args.prepared)
        if args.action == 'validate':
            result = {'status': 'genuine_original_holdout_verified', 'request_hash': row['request_hash'], 'new_paid_calls': 0}
        else:
            from . import holdout_transport
            result = holdout_transport._reserve(args.prepared, row, pack) if args.action == 'reserve' else holdout_transport._claim(args.prepared, row, pack, args.reservation)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
