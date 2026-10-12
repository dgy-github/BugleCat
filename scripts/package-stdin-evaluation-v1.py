"""Package the audited runtime and tested STDIN development tools.

No live evaluation, authority creation or installation. The package states
which pipeline stages are implemented; offline fixtures never certify model
accuracy. The compiled evaluation tools still require their original workspace.
"""
import argparse
import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from evals.verification.apps_stdin_v1 import (  # noqa: E402
    budget,
    calibration_collection,
    cohort,
    development_acceptance,
    development_jev,
    formal_jev,
    formal_observations,
    holdout_intake,
    holdout_jev,
    holdout_observations,
    intake,
)

ACCEPTANCE = ROOT / 'reports/artifact-v3-apps-stdin-formal-offline-20261012.json'
HOLDOUT_ACCEPTANCE = ROOT / 'reports/artifact-v3-apps-stdin-holdout-offline-20261012.json'
AUTHORIZATION = ROOT / 'evals/verification/reports/jev_budget_extension_20261011_apps316.json'
HELPERS = ('package-stdin-evaluation-v1.py', 'package-artifact-evaluation-v2.py',
           'package-artifact-evaluation.py')
SCHEMA = 'buglecat.verification.stdin-development-package.v1'


def support():
    spec = importlib.util.spec_from_file_location('stdin_package_support', ROOT / 'scripts/package-artifact-evaluation-v2.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def accepted_report(path, schema):
    value = cohort.sealed(budget.load(path))
    require = formal_jev.packet.require
    require(value.get('schema') == schema
            and value.get('formal_certification') is False
            and value.get('production_authorized') is False
            and value.get('certified_threshold') is None
            and value.get('real_new_paid_calls') == 0,
            'actual offline development acceptance without certification required')
    for name, digest in value['source_sha256'].items():
        path = support().safe_file(ROOT / name)
        require(cohort.sha(path) == digest, 'accepted public source changed')
    return value


def acceptance():
    return {'formal': accepted_report(ACCEPTANCE, 'buglecat.verification.stdin-formal-offline-acceptance.v1'),
            'holdout': accepted_report(HOLDOUT_ACCEPTANCE, 'buglecat.verification.stdin-holdout-offline-acceptance.v1')}


def tools():
    directory = ROOT / 'rust/target/debug/examples'
    rows = []
    for name, reader in (
        ('verification_apps_stdin_v1_collect', intake.build),
        ('verification_apps_stdin_v1_jev_development', development_jev.build_info),
        ('verification_apps_stdin_v1_calibration_collect', calibration_collection.build),
        ('verification_apps_stdin_v1_jev_formal', formal_jev.build_info),
        ('verification_apps_stdin_v1_holdout_collect', holdout_intake.build),
        ('verification_apps_stdin_v1_jev_holdout', holdout_jev.build_info),
    ):
        path = directory / (name + '.exe')
        rows.append((name, path, reader(path), reader))
    return rows


def public_sources():
    """Exactly compiled owner paths plus named public packaging owners."""
    paths = {Path(path).resolve(strict=True) for path in holdout_jev.runner_sources().values()}
    paths.update(ROOT / 'scripts' / name for name in HELPERS)
    paths.update(ROOT / 'evals/verification/apps_stdin_v1' / (name + '.py')
                 for name in ('formal_decision', 'holdout_opening'))
    paths = sorted(path for path in paths if path.suffix in {'.py', '.rs', '.toml', '.lock', '.md'})
    helper = support()
    for path in paths:
        helper.safe_file(path)
        formal_jev.packet.require(not helper.old_packager().SECRET.search(path.read_text(encoding='utf-8-sig')),
                                  'credential-like public source refused')
    return paths


def attest(image, name, compiled, reader, private):
    require = formal_jev.packet.require
    require(reader(image) == compiled, 'copied actual compiled tool differs')
    env = {key: value for key, value in os.environ.items()
           if not any(word in key.upper() for word in ('KEY', 'TOKEN', 'SECRET'))}
    if name.endswith(('jev_formal', 'jev_holdout')):
        args = ['offline', '--output', str(private), '--case', 'success']
    elif name.endswith('jev_development'):
        args = ['offline', '--output', str(private), '--case', 'success']
    else:
        args = ['offline-test', str(private), 'success']
    done = subprocess.run([str(image), *args], cwd=ROOT, env=env, capture_output=True, timeout=40)
    require(done.returncode == 0, 'actual copied STDIN offline entrypoint failed')
    value = budget.parse(done.stdout)
    require(value.get('paid_calls', value.get('new_paid_calls')) == 0,
            'offline tool cannot make a paid call')
    if name.endswith(('jev_formal', 'jev_holdout')):
        prepared_file = private / 'fixture-prepared.json'
        observer = holdout_observations if name.endswith('jev_holdout') else formal_observations
        observation = observer.join(
            budget.load(prepared_file), prepared_file, budget.load(private / 'fixture-reservation.json'),
            private / 'attempt', image, fixture=True)
        require(observation['call_status'] == 'ok'
                and observation['request_hash'] == observation['actual_request_hash'],
                'actual copied formal raw request/response join required')
    require(reader(image) == compiled, 'copied tool changed during local fixture')
    return {'name': name, 'sha256': cohort.sha(image), 'bytes': image.stat().st_size,
            'compiled_build_info': compiled, 'actual_copied_entrypoint': True,
            'local_fixture_only': True, 'paid_calls': 0}


def stage(parent, output):
    helper = support()
    require = formal_jev.packet.require
    output = Path(output).absolute()
    require(output.parent == ROOT / 'releases' and not output.exists()
            and not output.with_suffix('.zip').exists(), 'new fixed release location required')
    require(output.parent.resolve(strict=True) == ROOT / 'releases'
            and not output.parent.is_junction() and not output.parent.is_symlink(),
            'staging root cannot escape the workspace')
    parent_manifest = helper.verify_parent(parent)
    public = acceptance()
    binaries, sources = tools(), public_sources()
    hashes = budget.raw_bindings([*sources, ACCEPTANCE, HOLDOUT_ACCEPTANCE])
    chain = budget.previous(AUTHORIZATION)
    before = development_acceptance._image(chain)
    output.mkdir()
    for name in sorted(helper.NATIVE):
        shutil.copyfile(Path(parent) / name, output / name)
    helper.write_json(output / 'parent-manifest.json', parent_manifest)
    shutil.copyfile(Path(parent) / parent_manifest['source_snapshot'], output / 'parent-source-snapshot.json')
    for path in [*sources, ACCEPTANCE, HOLDOUT_ACCEPTANCE]:
        destination = output / path.relative_to(ROOT)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
    evidence = output / 'evidence'
    evidence.mkdir()
    attestations = []
    with tempfile.TemporaryDirectory(prefix='buglecat-stdin-package-local-') as temporary:
        for name, path, compiled, reader in binaries:
            image = output / path.relative_to(ROOT)
            image.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, image)
            attestations.append(attest(image, name, compiled, reader, Path(temporary) / name))
    helper.write_json(evidence / 'stdin-tool-attestations.json', attestations)
    runtime = helper.isolated_runtime_smoke(helper.old_packager(), output, parent_manifest)
    # The old localhost smoke also produces a WebView cache. Keep only its
    # named aggregate evidence, never browser storage in the release archive.
    keep = {'runtime-smoke.json', 'lifecycle.json', 'stdin-tool-attestations.json'}
    browser_cache = output / 'BugleCat.exe.WebView2'
    if browser_cache.exists():
        require(browser_cache.resolve(strict=True).parent == output.resolve(strict=True)
                and not browser_cache.is_symlink() and not browser_cache.is_junction(),
                'browser cache cleanup escaped fresh staging')
        shutil.rmtree(browser_cache)
    for path in list(evidence.iterdir()):
        if path.name not in keep:
            require(path.resolve(strict=True).is_relative_to(output.resolve(strict=True))
                    and path.absolute().parent == evidence, 'evidence cleanup escaped staging')
            if path.is_dir():
                require(not path.is_symlink() and not path.is_junction(), 'unsafe smoke directory')
                shutil.rmtree(path)
            else:
                path.unlink()
    (output / 'README.md').write_text(
        '# BugleCat：v3.3 运行时与 STDIN 开发评测工具\n\n'
        '双击 BugleCat.exe，或运行 ncx.exe --help。安装包已附带，本次未执行安装。\n\n'
        'STDIN 来源筛选、开发采集、400组校准采集、盲登记、正式 Jev 与阈值裁决、一次留出开放、'
        '独立400组留出采集与标签、固定阈值一次最终评测已完成离线开发验收。'
        '合成夹具用于检查工具和记录一致性；没有新的自然模型样本或正式效果认证。\n\n'
        '新留出原始收据验证使用真实读回函数；旧开发/校准夹具保留显式替身。'
        '全仓格式检查仍被四个既有冻结文件阻挡，新增Rust文件的限定格式检查通过。\n\n'
        '生产 Jev 默认关闭，生产阈值为空；低分不会显示为已验证通过。\n\n'
        '评测工具依赖原编译仓库、冻结来源和私有原始证据；复制工具的本地测试不证明可独立迁移运行。'
        '本包不含密钥、真实收费账本、原题、候选代码、原始供应商响应或浏览器缓存。\n', encoding='utf-8')
    cohort.check(hashes)
    require(acceptance() == public and tools() == binaries, 'accepted source or tools drifted')
    require(budget.same(before, development_acceptance._image(chain)), 'real accounting changed while packaging')
    helper.verify_parent(parent)
    files = [{'path': path.relative_to(output).as_posix(), 'sha256': cohort.sha(path), 'bytes': path.stat().st_size}
             for path in sorted(output.rglob('*')) if path.is_file()]
    for item in files:
        path = output / item['path']
        if path.suffix in {'.py', '.rs', '.toml', '.lock', '.json', '.md'}:
            require(not helper.old_packager().SECRET.search(path.read_text(encoding='utf-8-sig')),
                    'credential-like package text refused')
    manifest = {'schema': SCHEMA, 'policy_version': parent_manifest['policy_version'],
        'parent_manifest_sha256': helper.PARENT_SHA, 'stdin_acceptance_sha256': cohort.sha(ACCEPTANCE),
        'stdin_holdout_acceptance_sha256': cohort.sha(HOLDOUT_ACCEPTANCE),
        'components': attestations, 'runtime_validation': runtime, 'files': files,
        'production_authorized': False, 'production_jev_threshold': None, 'production_enforce_categories': [],
        'formal_certification': False, 'certified_threshold': None, 'overall_goal_complete': False,
        'installer_executed': False, 'paid_package_calls': 0, 'fee_snapshot_unchanged': True,
        'staging_script_sha256': cohort.sha(__file__)}
    helper.write_json(output / 'manifest.json', manifest)
    helper.checked_zip(output, output.with_suffix('.zip'), files)
    cohort.check(hashes)
    require(budget.same(before, development_acceptance._image(chain)), 'real accounting changed during ZIP creation')
    return {'package': str(output), 'zip': str(output.with_suffix('.zip')),
            'zip_bytes': output.with_suffix('.zip').stat().st_size,
            'zip_sha256': cohort.sha(output.with_suffix('.zip')), 'manifest_sha256': cohort.sha(output / 'manifest.json'),
            'paid_calls': 0, 'formal_certification': False}


if __name__ == '__main__':
    import json
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--parent', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--stage', action='store_true')
    args = parser.parse_args()
    if args.stage:
        result = stage(args.parent, args.output)
    else:
        support().verify_parent(args.parent)
        result = {'status': 'read_only_preflight', 'acceptance_sha256': cohort.sha(ACCEPTANCE),
                  'holdout_acceptance_sha256': cohort.sha(HOLDOUT_ACCEPTANCE),
                  'tool_names': [row[0] for row in tools()], 'public_sources': len(public_sources()), 'paid_calls': 0}
        acceptance()
    print(json.dumps(result))
