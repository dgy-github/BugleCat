"""Filesystem isolation, provenance and protected-test checks for self-repair."""
import hashlib
import os
import re
import shutil
import subprocess
from pathlib import Path

from train.process_control import run_owned

SKIP_DIRS = {".git", ".nanocodex", ".ncx", "target", "__pycache__", ".pytest_cache",
             ".ruff_cache", ".venv", "node_modules"}
EXCLUDE = ("evals/self_repair/", "tests/test_self_repair_suite.py", "HANDOFF.md",
           "docs/reports/self_repair", ".github/", "local-plugins/")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def safe_child(root: Path, name: str) -> Path:
    if not re.fullmatch(r"[a-zA-Z0-9_-]+", name):
        raise ValueError("Only a simple child directory name is allowed")
    child = (root.resolve() / name).resolve()
    if child.parent != root.resolve():
        raise ValueError("Destination escaped the dedicated run directory")
    if child.exists():
        raise FileExistsError(child)
    return child


def git(root: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(root), *args], text=True, encoding="utf-8")


def export_workspace(source: Path, destination: Path) -> None:
    """Export current source, not its answer-bearing git history or evaluator."""
    if destination.exists():
        raise FileExistsError(destination)
    source = source.resolve()
    names = git(source, "ls-files", "--cached", "--others", "--exclude-standard", "-z")
    destination.mkdir(parents=True)
    for name in sorted(set(names.split("\0")) - {""}):
        if name.startswith(EXCLUDE) or any(p in SKIP_DIRS for p in Path(name).parts):
            continue
        src = source / name
        if src.is_symlink() or not src.resolve().is_relative_to(source):
            raise ValueError(f"Refusing link/outside source: {name}")
        if not src.is_file():
            continue
        dst = destination / name
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)


def fingerprint(workspace: Path) -> dict:
    result = {}
    for directory, dirs, files in os.walk(workspace, followlinks=False):
        for name in dirs + files:
            if (Path(directory) / name).is_symlink():
                raise ValueError(f"Symlink in candidate: {name}")
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for name in files:
            path = Path(directory) / name
            data = path.read_bytes()
            normalized = data.replace(b"\r\n", b"\n")
            marker = normalized.find(b"#[cfg(test)]") if path.suffix == ".rs" else -1
            result[path.relative_to(workspace).as_posix()] = {
                "sha256": digest(data),
                "inline_tests": digest(normalized[marker:]) if marker >= 0 else None,
            }
    return result


def audit(workspace: Path, baseline: dict, target: str) -> list[str]:
    try:
        current = fingerprint(workspace)
    except ValueError as error:
        return [str(error)]
    issues = []
    for name in sorted(baseline.keys() | current.keys()):
        if baseline.get(name) == current.get(name):
            continue
        if name == target and name in baseline and name in current:
            if baseline[name]["inline_tests"] != current[name]["inline_tests"]:
                issues.append(f"protected inline tests changed: {name}")
            continue
        new_test = (name not in baseline and name.startswith("tests/test_self_repair_")
                    and name.endswith(".py"))
        new_rust_test = (name not in baseline and name.startswith("rust/crates/")
                         and "/tests/self_repair_" in name and name.endswith(".rs"))
        if not (new_test or new_rust_test):
            issues.append(f"out-of-scope change: {name}")
    return issues


def initialize_seed(workspace: Path) -> str:
    git(workspace, "init", "-q")
    git(workspace, "add", "--all")
    git(workspace, "-c", "user.name=Self Repair Fixture", "-c", "user.email=fixture@localhost",
        "-c", "commit.gpgsign=false", "commit", "-qm", "Seeded self-repair task (no reference history)")
    return git(workspace, "rev-parse", "HEAD").strip()


def grading_env() -> dict:
    return {k: v for k, v in os.environ.items()
            if not re.search(r"API.?KEY|TOKEN|SECRET|PASSWORD", k, re.I)
            and not k.startswith(("NANOCODEX_", "DEEPSEEK_", "NCX_"))}


def owned_command(command: list[str], workspace: Path, timeout: int, env: dict) -> dict:
    import time
    started = time.monotonic()
    try:
        result = run_owned(command, cwd=workspace, env=env, timeout=timeout,
                           capture_output=True, text=True, encoding="utf-8", errors="replace")
        return {"returncode": result.returncode, "timed_out": False,
                "elapsed_s": round(time.monotonic() - started, 2),
                "stdout": result.stdout or "", "stderr": result.stderr or ""}
    except subprocess.TimeoutExpired as error:
        return {"returncode": None, "timed_out": True,
                "elapsed_s": round(time.monotonic() - started, 2),
                "stdout": error.stdout or "", "stderr": error.stderr or ""}


def sanitized(text: str, credentials: tuple[str, ...] = ()) -> str:
    for credential in credentials:
        if credential:
            text = text.replace(credential, "[REDACTED]")
    text = re.sub(r"(?i)(bearer\s+)\S+", r"\1[REDACTED]", text)
    text = re.sub(r"\bsk-[A-Za-z0-9_-]{10,}", "[REDACTED]", text)
    return re.sub(r"(?i)((?:api[_-]?key|token|password)\s*[=:]\s*)[^\s,;]+",
                  r"\1[REDACTED]", text)


def sanitize_value(value, credentials: tuple[str, ...] = ()):
    """Redact values before JSON serialization so regexes cannot eat its syntax."""
    if isinstance(value, str):
        return sanitized(value, credentials)
    if isinstance(value, dict):
        return {k: sanitize_value(v, credentials) for k, v in value.items()}
    if isinstance(value, list):
        return [sanitize_value(v, credentials) for v in value]
    return value
