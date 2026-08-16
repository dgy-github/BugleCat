"""Check changed production files for project code-structure limits."""

from __future__ import annotations

import argparse
import ast
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PRODUCTION_ROOTS = ("nanocodex/", "rust/crates/", "rust/gui/src/")
SOURCE_SUFFIXES = {".py", ".rs", ".ts", ".js", ".svelte"}
GENERATED_MARKERS = ("generated", "@generated", "do not edit")


def logical_lines(lines: list[str]) -> int:
    return sum(1 for line in lines if line.strip() and not line.lstrip().startswith(("#", "//")))


def python_functions(path: Path) -> list[tuple[str, int, int]]:
    try:
        tree = ast.parse(path.read_text(encoding="utf-8"))
    except (OSError, SyntaxError, UnicodeError):
        return []
    result = []
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.end_lineno:
            result.append((node.name, node.lineno, node.end_lineno))
    return result


RUST_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)[^;]*\{")


def brace_functions(lines: list[str]) -> list[tuple[str, int, int]]:
    result: list[tuple[str, int, int]] = []
    for index, line in enumerate(lines):
        match = RUST_FN.match(line)
        if not match:
            continue
        depth = 0
        started = False
        for end in range(index, len(lines)):
            for char in lines[end]:
                if char == "{":
                    depth += 1
                    started = True
                elif char == "}":
                    depth -= 1
            if started and depth == 0:
                result.append((match.group(1), index + 1, end + 1))
                break
    return result


def git_changed(base: str) -> list[str]:
    command = ["git", "diff", "--name-only", "--diff-filter=ACMR", base, "--"]
    command.extend(["*.py", "*.rs", "*.ts", "*.js", "*.svelte"])
    completed = subprocess.run(command, cwd=ROOT, check=True, capture_output=True, text=True)
    return completed.stdout.splitlines()


def selected_files(args: argparse.Namespace) -> list[Path]:
    names = args.files or (git_changed(args.git_diff) if args.git_diff else [])
    result = []
    for name in names:
        normalized = name.replace("\\", "/")
        path = (ROOT / normalized).resolve()
        if path.exists() and path.suffix in SOURCE_SUFFIXES and normalized.startswith(PRODUCTION_ROOTS):
            result.append(path)
    return sorted(set(result))


def check_file(path: Path) -> list[str]:
    relative = path.relative_to(ROOT).as_posix()
    text = path.read_text(encoding="utf-8")
    lowered = text[:500].lower()
    if any(marker in lowered for marker in GENERATED_MARKERS):
        return []
    lines = text.splitlines()
    errors = []
    file_limit = 300 if path.suffix == ".svelte" else 700
    if len(lines) > file_limit:
        errors.append(f"{relative}:1 file has {len(lines)} lines; hard limit is {file_limit}")
    functions = python_functions(path) if path.suffix == ".py" else brace_functions(lines)
    for name, start, end in functions:
        count = logical_lines(lines[start - 1 : end])
        if count > 80:
            errors.append(f"{relative}:{start} {name} has {count} logical lines; hard limit is 80")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--git-diff", help="Git base revision, for example origin/main...HEAD")
    parser.add_argument("--files", nargs="*", default=[])
    args = parser.parse_args()
    files = selected_files(args)
    errors = [error for path in files for error in check_file(path)]
    if errors:
        print("\n".join(errors))
        return 1
    print(f"Code structure check passed for {len(files)} changed production files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
