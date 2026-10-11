"""Independent bounded HumanEval worker; frozen MBPP worker is unchanged.

Complete official check(candidate) bodies run in order. Typing annotations are
supported by a narrowly reviewed import extension; unsupported code is Unknown.
This restricted language is a review gate, not a general Python sandbox.
"""
import ast
import builtins
import copy
import importlib
import importlib.util
import json
import sys
from pathlib import Path

try:
    from . import benchmark_oracle_worker as original
except ImportError:  # Isolated child invocation uses an explicit sibling path.
    specification = importlib.util.spec_from_file_location(
        "frozen_mbpp_oracle", Path(__file__).with_name("benchmark_oracle_worker.py"))
    original = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(original)

TYPING_NAMES = {"Any", "Callable", "Dict", "Iterable", "Iterator", "List", "Optional",
                "Sequence", "Set", "Tuple", "Union"}


def reviewed_tree(code):
    if not isinstance(code, str) or len(code.encode()) > 65536:
        raise ValueError("code_size")
    tree = ast.parse(code)
    # Review the original AST except the explicit, narrow typing import addition.
    # Execution always uses original bytes; annotations/assertions are not stripped.
    reviewed = copy.deepcopy(tree)
    for node in ast.walk(reviewed):
        if isinstance(node, ast.ImportFrom) and node.module == "typing":
            if node.level or any(alias.name not in TYPING_NAMES for alias in node.names):
                raise ValueError("restricted_typing_import")
            node.module = "collections"  # safe module for the existing import review
        elif isinstance(node, ast.Import) and any(alias.name == "typing" for alias in node.names):
            # Module-wide access would expose reflection helpers outside the
            # explicit annotation imports. Preserve such tasks as Unknown.
            raise ValueError("restricted_typing_import")
    original.reviewed_tree(ast.unparse(reviewed))
    return tree


def importer(name, globals=None, locals=None, fromlist=(), level=0):
    if name == "typing" and not level:
        if not fromlist or any(item not in TYPING_NAMES for item in fromlist):
            raise ValueError("restricted_typing_import")
        return importlib.import_module("typing")
    return original.importer(name, globals, locals, fromlist, level)


def acceptance(code, imports, tests):
    for text in [code, *imports, *tests]:
        reviewed_tree(text)
    safe = {name: getattr(builtins, name) for name in original.BUILTINS}
    safe["__import__"] = importer
    scope = {"__builtins__": safe}
    index = 0
    try:
        exec(compile(code, "candidate.py", "exec"), scope)
        if any(name in scope and scope[name] is not value for name, value in safe.items()):
            raise ValueError("oracle_builtin_shadowed")
        for text in imports:
            exec(compile(text, "oracle-import", "exec"), scope)
        for index, text in enumerate(tests):
            exec(compile(text, f"oracle-test-{index}", "exec"), scope)
    except ValueError as error:
        if str(error) == "oracle_builtin_shadowed":
            raise
        return {"status": "failed", "error_kind": type(error).__name__, "test_blocks_passed_before_failure": index}
    except Exception as error:
        return {"status": "failed", "error_kind": type(error).__name__, "test_blocks_passed_before_failure": index}
    return {"status": "passed", "test_blocks_passed": len(tests)}


def run(payload):
    try:
        reference = acceptance(payload["reference"], payload["imports"], payload["tests"])
        if reference["status"] != "passed":
            return {"status": "unknown", "reason": "frozen_reference_does_not_pass", "reference": reference}
        try:
            reviewed_tree(payload["candidate"])
        except SyntaxError:
            return {"status": "failed", "error_kind": "SyntaxError", "reference": reference}
        return {**acceptance(payload["candidate"], payload["imports"], payload["tests"]), "reference": reference}
    except (ValueError, SyntaxError, KeyError, TypeError):
        return {"status": "unknown", "reason": "unsupported_or_invalid_oracle_input"}


if __name__ == "__main__":
    sys.stdout.write(json.dumps(run(json.load(sys.stdin)), sort_keys=True))
