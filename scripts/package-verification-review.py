"""Package current GUI/CLI and offline review tools; verify copied localhost lifecycle."""
import argparse
import hashlib
import importlib.util
import json
import re
import shutil
import subprocess
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = (
    "scripts/run_jev_certification.py", "scripts/replay_verification.py",
    "scripts/audit_jev_history.py", "scripts/format_claim_drift_report.py",
    "reports/claim_drift_eval_report_v1.json", "reports/claim_drift_eval_report_v1.md",
    "reports/jev-history-audit-20261013.json", "tests/test_verification_review_tools.py",
    "evals/verification/reports/claim_pilot_20261013.json",
)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_support():
    spec = importlib.util.spec_from_file_location("review_packaging_support", ROOT / "scripts/package-artifact-evaluation-v2.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stage(output):
    output = output.resolve()
    if output.parent != ROOT / "releases" or output.exists() or output.with_suffix(".zip").exists():
        raise ValueError("fresh immediate releases directory required")
    gui = ROOT / "rust/gui/src-tauri/target/x86_64-pc-windows-msvc/release"
    binaries = {"BugleCat.exe": gui / "ncx-gui.exe",
                "ncx.exe": ROOT / "rust/target/release/ncx.exe",
                "BugleCat_0.1.0_x64-setup.exe": gui / "bundle/nsis/BugleCat_0.1.0_x64-setup.exe"}
    for path in binaries.values():
        if not path.is_file():
            raise FileNotFoundError(path)
    output.mkdir()
    for name, path in binaries.items():
        shutil.copy2(path, output / name)
    for name in PUBLIC:
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / name, target)
    policy_source = (ROOT / "rust/crates/ncx-core/src/verification/types.rs").read_text(encoding="utf-8")
    policy_match = re.search(r'pub const POLICY_VERSION: &str = "(host-rules-v[0-9.]+)";', policy_source)
    if not policy_match:
        raise ValueError("missing runtime policy version")
    manifest = {"schema": "buglecat.verification.review-runtime-package.v1",
                "policy_version": policy_match[1], "production_jev_threshold": None,
                "production_default_mode": "off", "paid_calls": 0, "installer_executed": False,
                "historical_accounting_reconciled": False, "formal_certification": False,
                "files": {name: {"sha256": sha(output / name), "bytes": (output / name).stat().st_size}
                          for name in binaries}}
    evidence = output / "evidence"
    evidence.mkdir()
    helper = load_support()
    smoke = helper.isolated_runtime_smoke(helper.old_packager(), output, manifest)
    # WebView2 may leave cache next to the copied executable. Package only the
    # explicit deliverable list; never recursively archive runtime profiles.
    readme = """# BugleCat 声明核验修复运行包（2026-10-13）

双击 BugleCat.exe 启动；ncx.exe --help 查看命令行。安装程序已构建，未执行安装。
包含最新声明抽取修复及离线审计工具，生产校验默认 off，未认证生产准确率。
真实本地工具链测试使用固定模型回答，不代表真实模型自然准确率。
scripts 下 Python 工具需要 Python 与 scipy；历史评分和账本未打包。
run_jev_certification.py 仅离线复核，无法发送请求；历史收费仍待服务端对账。
历史冻结 STDIN 评测工具仍在之前的独立开发包中，本包不重新认证它们。
evidence 保存实际复制 GUI/CLI 的本地启动、恢复、取消和三模式验收。
"""
    (output / "README.md").write_text(readme, encoding="utf-8")
    names = [*binaries, *PUBLIC, "README.md",
             "evidence/runtime-smoke.json", "evidence/lifecycle.json", "evidence/runtime-verification-records.json"]
    manifest["runtime_acceptance"] = smoke
    manifest["git_head"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    tracked = subprocess.check_output(["git", "ls-files", "rust/crates", "rust/gui/src", "rust/gui/src-tauri/src",
                                      "rust/Cargo.lock", "rust/gui/src-tauri/Cargo.lock"], cwd=ROOT, text=True).splitlines()
    manifest["source_sha256"] = {name: sha(ROOT / name) for name in tracked if (ROOT / name).is_file()}
    manifest["members"] = {name: {"sha256": sha(output / name), "bytes": (output / name).stat().st_size} for name in names}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    names.append("manifest.json")
    archive = output.with_suffix(".zip")
    with zipfile.ZipFile(archive, "x", zipfile.ZIP_DEFLATED) as stream:
        for name in names:
            stream.write(output / name, name)
    with zipfile.ZipFile(archive) as stream:
        assert stream.testzip() is None and set(stream.namelist()) == set(names)
        for name in names:
            assert hashlib.sha256(stream.read(name)).hexdigest() == sha(output / name)
    return {"schema": manifest["schema"], "package": str(archive.relative_to(ROOT)),
            "sha256": sha(archive), "bytes": archive.stat().st_size, "member_count": len(names),
            "runtime_acceptance": smoke, "installer_executed": False,
            "formal_certification": False, "historical_accounting_reconciled": False,
            "source_manifest_sha256": sha(output / "manifest.json"), "paid_calls": 0}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        raise FileExistsError(args.report)
    result = stage(args.output)
    with args.report.open("x", encoding="utf-8") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print(json.dumps(result, indent=2))
