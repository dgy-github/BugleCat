"""Package original APPS terminal evidence and copied, offline-tested tools.

Default preflight reads only. --stage copies the audited GUI/CLI, checks actual
copied entrypoints against local fake transports, and verifies every ZIP byte.
No real credentials, ledgers, questions, candidates or provider responses ship.
"""
import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from evals.verification.apps_campaign_v3 import (  # noqa: E402
    formal_collection,
    formal_evaluation,
    formal_intake,
    formal_jev,
    formal_terminal,
    holdout_evaluation,
    holdout_intake,
    holdout_jev,
)
from evals.verification.calibration import load, require  # noqa: E402
from evals.verification.collection_manifest import file_hash  # noqa: E402

SCOPE = 'APPS official train host split, restricted original ordered-call assertions'
MAIN = 'verification_apps_v3_formal_collect'
JEV = 'verification_apps_v3_jev_formal'
HOLDOUT_MAIN = 'verification_apps_v3_holdout_collect'
HOLDOUT_JEV = 'verification_apps_v3_jev_holdout'
HELPERS = ('package-apps-evaluation-v3.py', 'package-artifact-evaluation-v2.py',
           'package-artifact-evaluation.py')


def support():
    spec = importlib.util.spec_from_file_location('audited_packaging_helpers', ROOT / 'scripts/package-artifact-evaluation-v2.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def summary(result_file):
    """Only actual completed terminal paths may reach package staging."""
    original = load(result_file)
    holdout_opened = False
    apps_certified = False
    apps_threshold = None
    if original['schema'] == formal_terminal.SCHEMA:
        actual = formal_terminal.revalidate(result_file)
        require(actual['independent_quotas_met'] is False, 'ready_calibration_requires_actual_Jev_evaluation')
        public = formal_terminal.public_summary(actual)
    elif original['schema'] == formal_evaluation.SCHEMA:
        actual = formal_evaluation.revalidate_calibration(result_file)
        require(actual['qualified_for_once_holdout'] is False, 'qualified_calibration_requires_actual_once_holdout')
        helper = support()
        public = dict(status='calibration_gate_failed_holdout_not_opened', allocated_groups=actual['allocated_groups'],
            label_counts=actual['label_counts'], original_observations=actual['original_observations'],
            grid_results=[helper.public_metrics(row) for row in actual['grid_results']],
            original_result_hash=actual['artifact_hash'], formal_certification=False, certified_threshold=None)
    elif original['schema'] == holdout_evaluation.TERMINAL:
        actual = holdout_evaluation.revalidate_terminal(result_file)
        require(actual['whole_allocation_terminal'] is True and actual['independent_label_quota_met'] is False
                and actual['status'] == 'incomplete_labels', 'whole_terminal_holdout_labels_required_for_incomplete_package')
        holdout_opened = True
        public = dict(status='holdout_incomplete_labels', allocated_groups=actual['allocated_groups'],
            collected_groups=actual['collected_groups'], label_counts=actual['label_counts'],
            original_result_hash=actual['artifact_hash'], formal_certification=False, certified_threshold=None)
    elif original['schema'] == holdout_evaluation.SCHEMA:
        actual = holdout_evaluation.revalidate_evaluation(result_file)
        holdout_opened = True
        apps_certified = actual['APPS_ordered_pair_artifact_threshold_certified']
        apps_threshold = actual['certified_APPS_ordered_pair_artifact_threshold']
        require(type(apps_certified) is bool and apps_certified == (actual['holdout_status'] == 'passed')
                and apps_threshold == (actual['fixed_threshold'] if apps_certified else None),
                'original_APPS_scope_certificate_changed')
        public = dict(status='holdout_' + actual['holdout_status'], allocated_groups=actual['allocated_groups'],
            label_counts=actual['label_counts'], original_observations=actual['original_observations'],
            metrics=support().public_metrics(actual), original_result_hash=actual['artifact_hash'],
            formal_certification=False, certified_threshold=None)
    else:
        raise ValueError('original_APPS_terminal_result_required')
    require(actual['production_authorized'] is False and actual['certified_threshold'] is None,
            'unqualified_package_cannot_enable_production')
    return {**public, 'original_result_sha256': file_hash(result_file), 'scope': SCOPE,
            'production_authorized': False, 'holdout_opened': holdout_opened,
            'APPS_ordered_pair_artifact_threshold_certified': apps_certified,
            'certified_APPS_ordered_pair_artifact_threshold': apps_threshold, 'native_claim_certification': False,
            'overall_agent_benefit_certification': False, 'overall_goal_complete': False}


def sources():
    paths = {Path(row['file']).resolve(strict=True) for row in formal_collection.owners()}
    paths.update(Path(p).resolve(strict=True) for p in formal_intake.runner_sources().values())
    paths.update(Path(p).resolve(strict=True) for p in formal_jev.runner_sources().values())
    paths.update(Path(p).resolve(strict=True) for p in holdout_intake.runner_sources().values())
    paths.update(Path(p).resolve(strict=True) for p in holdout_jev.runner_sources().values())
    paths.add(Path(formal_terminal.__file__).resolve())
    paths.update(ROOT / 'scripts' / name for name in HELPERS)
    paths.update((ROOT / 'evals/verification/apps_campaign_v3').glob('*.py'))
    allowed = []
    for path in sorted(paths):
        require(path.is_relative_to(ROOT), 'public_source_outside_project')
        if path.suffix in {'.py', '.rs'} and (path.is_relative_to(ROOT / 'evals/verification')
                                            or path.is_relative_to(ROOT / 'rust/crates')
                                            or path in {ROOT / 'scripts' / name for name in HELPERS}):
            allowed.append(path)
    return allowed


def tools():
    main = ROOT / f'rust/target/debug/examples/{MAIN}.exe'
    jev = ROOT / f'rust/target/debug/examples/{JEV}.exe'
    holdout_main = ROOT / f'rust/target/debug/examples/{HOLDOUT_MAIN}.exe'
    holdout_jev_exe = ROOT / f'rust/target/debug/examples/{HOLDOUT_JEV}.exe'
    return ((MAIN, main, formal_intake.build(main)), (JEV, jev, formal_jev.build_info(jev)),
            (HOLDOUT_MAIN, holdout_main, holdout_intake.build(holdout_main)),
            (HOLDOUT_JEV, holdout_jev_exe, holdout_jev.build_info(holdout_jev_exe)))


def offline_copy(binary, name, compiled, directory):
    """Actual copied compiled source proof and fake transport, never live mode."""
    env = {k: v for k, v in os.environ.items() if not any(t in k.upper() for t in ('KEY', 'TOKEN', 'SECRET'))}
    readers = {MAIN: formal_intake.build, JEV: formal_jev.build_info,
               HOLDOUT_MAIN: holdout_intake.build, HOLDOUT_JEV: holdout_jev.build_info}
    require(name in readers, 'known_actual_APPS_tool_required')
    info = readers[name](binary)
    require(info == compiled, 'copied_compiled_source_or_executable_changed')
    original_sha = file_hash(binary)
    action = 'offline-test' if name in {MAIN, HOLDOUT_MAIN} else 'offline-transport-test'
    result = subprocess.run([str(binary), action, str(directory), 'success'], cwd=ROOT, env=env,
                            capture_output=True, timeout=90)
    require(result.returncode == 0, 'copied_APPS_offline_entrypoint_failed')
    value = json.loads(result.stdout)
    require(value.get('paid_calls', value.get('new_paid_calls')) == 0
            and value.get('http_calls', value.get('network_calls')) == 0, 'offline_fixture_cannot_make_network_calls')
    require(file_hash(binary) == original_sha and readers[name](binary) == compiled,
            'copied_APPS_executable_or_sources_changed_during_fixture')
    return dict(name=name, sha256=file_hash(binary), bytes=binary.stat().st_size,
                actual_copied_entrypoint=True, fake_transport_only=True, paid_calls=0,
                compiled_source_bindings={k: v for k, v in compiled.items() if k.endswith('_sha256')},
                source_proof_is_actual_byte_join=True)


def stage(parent, result_file, output):
    helper = support()
    output = Path(output).resolve()
    require(output.is_relative_to(ROOT / 'releases') and output != ROOT / 'releases'
            and not output.exists() and not output.with_suffix('.zip').exists(), 'new_release_directory_and_zip_required')
    parent_manifest = helper.verify_parent(parent)
    public = summary(result_file)
    binaries = tools()
    source_files = sources()
    hashes = {path: file_hash(path) for path in source_files}
    before_snapshot = formal_collection.budget_snapshot(
        next(b['file'] for b in load(load(result_file)['cohort_file'])['bindings'] if b['role'] == 'authorization'))
    output.mkdir()
    for name in helper.NATIVE:
        shutil.copy2(Path(parent) / name, output / name)
    helper.write_json(output / 'parent-manifest.json', parent_manifest)
    shutil.copy2(Path(parent) / parent_manifest['source_snapshot'], output / 'parent-source-snapshot.json')
    secret = helper.old_packager().SECRET
    for path in source_files:
        original = helper.safe_file(path)
        require(not secret.search(original.read_text(encoding='utf-8-sig')), 'credential_like_source_refused')
        destination = output / path.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
    evidence = output / 'evidence'
    evidence.mkdir()
    helper.write_json(evidence / 'evaluation-summary.json', public)
    attestations = []
    with tempfile.TemporaryDirectory(prefix='buglecat-apps-package-fake-') as private:
        for name, path, compiled in binaries:
            destination = output / path.relative_to(ROOT)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
            attestations.append(offline_copy(destination, name, compiled, Path(private) / name))
    helper.write_json(evidence / 'sidecar-attestations.json', attestations)
    runtime = helper.isolated_runtime_smoke(helper.old_packager(), output, parent_manifest)
    (evidence / 'runtime-verification-records.json').unlink(missing_ok=True)
    (output / 'README.md').write_text(
        '# BugleCat：v3.3策略运行时与最新APPS评测工具\n\n'
        '运行 BugleCat.exe，或 ncx.exe --help。安装包已附带，本次打包不执行安装。\n\n'
        f'本次评测状态：{public["status"]}。范围：{SCOPE}。\n'
        f'APPS限定断言范围阈值认证：{public["APPS_ordered_pair_artifact_threshold_certified"]}；'
        f'限定范围阈值：{public["certified_APPS_ordered_pair_artifact_threshold"]}。\n'
        '通用声明阈值尚未认证；生产Jev默认关闭，阈值为空。低分不等于已验证通过。\n'
        '评测工具需要原仓库的冻结来源与私有证据，不能独立给任意代码认证。\n'
        '包中不包含密钥、真实收费账本、原题、候选或原始响应。\n', encoding='utf-8')
    require(summary(result_file) == public and tools() == binaries, 'original_result_or_compiled_tools_changed')
    require(all(file_hash(path) == value == file_hash(output / path.relative_to(ROOT)) for path, value in hashes.items()),
            'actual_copied_public_source_changed')
    authorization = next(b['file'] for b in load(load(result_file)['cohort_file'])['bindings'] if b['role'] == 'authorization')
    require(formal_collection._same(formal_collection.budget_snapshot(authorization), before_snapshot),
            'real_fee_or_reservation_changed_during_packaging')
    helper.verify_parent(parent)
    files = [dict(path=p.relative_to(output).as_posix(), sha256=file_hash(p), bytes=p.stat().st_size)
             for p in sorted(output.rglob('*')) if p.is_file()]
    for row in files:
        path = output / row['path']
        if path.suffix in {'.py', '.rs', '.json', '.jsonl', '.md', '.txt'}:
            require(not secret.search(path.read_text(encoding='utf-8-sig')), 'credential_like_package_text_refused')
    manifest = dict(schema='buglecat.artifact-evaluation-package.apps.v3',
        parent_manifest_sha256=helper.PARENT_SHA, policy_version=parent_manifest['policy_version'],
        evaluation_summary=public, components=attestations, runtime_validation=runtime,
        production_authorized=False, production_jev_threshold=None, production_enforce_categories=[],
        formal_certification=False, overall_goal_complete=False, installer_executed=False,
        APPS_ordered_pair_artifact_threshold_certified=public['APPS_ordered_pair_artifact_threshold_certified'],
        certified_APPS_ordered_pair_artifact_threshold=public['certified_APPS_ordered_pair_artifact_threshold'],
        paid_package_calls=0, fee_snapshot_unchanged=True, staging_script_sha256=file_hash(__file__), files=files)
    helper.write_json(output / 'manifest.json', manifest)
    helper.checked_zip(output, output.with_suffix('.zip'), files)
    require(formal_collection._same(formal_collection.budget_snapshot(authorization), before_snapshot),
            'real_fee_or_reservation_changed_during_zip_creation')
    require(summary(result_file) == public and tools() == binaries
            and all(file_hash(path) == value == file_hash(output / path.relative_to(ROOT)) for path, value in hashes.items()),
            'original_evidence_or_copied_sources_changed_during_zip_creation')
    return dict(package=str(output), zip=str(output.with_suffix('.zip')), zip_sha256=file_hash(output.with_suffix('.zip')),
                zip_bytes=output.with_suffix('.zip').stat().st_size, manifest_sha256=file_hash(output / 'manifest.json'),
                status=public['status'], paid_calls=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent', type=Path, required=True)
    parser.add_argument('--result', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--stage', action='store_true')
    args = parser.parse_args()
    if args.stage:
        value = stage(args.parent, args.result, args.output)
    else:
        support().verify_parent(args.parent)
        value = dict(status='read_only_preflight', evaluation=summary(args.result), tools=[t[0] for t in tools()], paid_calls=0)
    print(json.dumps(value, ensure_ascii=False))


if __name__ == '__main__':
    main()
