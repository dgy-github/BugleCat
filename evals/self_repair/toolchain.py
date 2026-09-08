"""Frozen visible test entrypoint surviving the agent's minimal shell environment."""
import json
import os
import shutil
import sys
from pathlib import Path

# No credentials or arbitrary process environment is serialized into a candidate.
BUILD_KEYS = {"PATH", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "CARGO_TARGET_DIR",
              "PROGRAMFILES", "PROGRAMFILES(X86)", "PROGRAMW6432", "APPDATA", "LOCALAPPDATA",
              "ALLUSERSPROFILE", "PROGRAMDATA", "INCLUDE", "LIB", "LIBPATH", "VSINSTALLDIR",
              "VCINSTALLDIR", "VCTOOLSINSTALLDIR", "WINDOWSSDKDIR", "WINDOWSSDKVERSION"}


def minimal_environment(source=None):
    """Mirror PolicyExecutor's Windows environment for an offline preflight."""
    source = os.environ if source is None else source
    keys = {"SYSTEMROOT", "COMSPEC", "PATH", "PATHEXT", "USERPROFILE", "TEMP", "TMP"}
    result = {k: v for k, v in source.items() if k.upper() in keys}
    result.update(PYTHONUNBUFFERED="1", PYTHONIOENCODING="utf-8", PYTHONUTF8="1", LANG="C.UTF-8")
    return result


def install_wrapper(workspace: Path, command: list[str], environment: dict):
    build = {k: v for k, v in environment.items() if k.upper() in BUILD_KEYS}
    source = ("# Frozen visible test command, not a reference solution.\n"
              "import json, os, subprocess, sys\nfrom pathlib import Path\n"
              f"settings = json.loads({json.dumps(dict(command=command, env=build))!r})\n"
              "environment = dict(os.environ)\nenvironment.update(settings['env'])\n"
              "result = subprocess.run(settings['command'], cwd=Path(__file__).parent, env=environment)\n"
              "sys.exit(result.returncode)\n")
    (workspace / "self_repair_check.py").write_text(source, encoding="utf-8")


def install_case_wrapper(case, workspace, cache):
    command = ([shutil.which("cargo") or "cargo", "test", "--offline", "--locked",
                "--manifest-path", "rust/Cargo.toml", "-p", case.crate, case.test_filter,
                "--", "--test-threads=1"] if case.crate else
               [sys.executable, "-B", "-m", "pytest", "-q", "-p", "no:cacheprovider",
                "tests/test_bench_run.py", "tests/test_eval_harness.py"])
    environment = dict(os.environ, CARGO_TARGET_DIR=str(cache))
    install_wrapper(workspace, command, environment)
