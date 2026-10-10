"""Credential-free child process for bounded MBPP acceptance assertions.

The restricted language is a review gate, not a general Python sandbox. No
candidate ever receives tests, filesystem builtins, or ambient environment.
Unsupported constructs produce Unknown in the parent rather than a negative.
"""

import ast
import builtins
import importlib
import json
import sys

MODULES = {"math", "cmath", "re", "heapq", "itertools", "collections",
           "functools", "bisect", "string", "operator", "statistics", "fractions", "decimal", "copy"}
BUILTINS = ("abs all any bin bool bytes chr complex dict divmod enumerate filter float frozenset "
            "int isinstance issubclass iter len list map max min next ord pow range repr reversed "
            "round set slice sorted str sum tuple type zip Exception ValueError TypeError IndexError "
            "KeyError ZeroDivisionError StopIteration AssertionError").split()
FORBIDDEN = {"exec", "eval", "compile", "open", "input", "globals", "locals", "vars",
             "getattr", "setattr", "delattr", "hasattr", "breakpoint", "help", "dir",
             "exit", "quit", "memoryview", "print"}


def reviewed_tree(code):
    if not isinstance(code, str) or len(code.encode()) > 65536:
        raise ValueError("code_size")
    tree = ast.parse(code)
    for node in ast.walk(tree):
        if isinstance(node, (ast.ClassDef, ast.AsyncFunctionDef, ast.Await, ast.Global, ast.Nonlocal)):
            raise ValueError("unsupported_construct")
        if isinstance(node, ast.Name) and (node.id.startswith("__") or node.id in FORBIDDEN):
            raise ValueError("restricted_name")
        if isinstance(node, ast.Attribute) and (node.attr.startswith("_") or node.attr in {"attrgetter", "methodcaller", "get_field", "get_value"}):
            raise ValueError("restricted_attribute")
        if isinstance(node, ast.Attribute) and isinstance(node.ctx, (ast.Store, ast.Del)):
            raise ValueError("oracle_dependency_mutation")
        if isinstance(node, ast.Constant) and isinstance(node.value, str) and "__" in node.value:
            raise ValueError("restricted_reflection_string")
        if isinstance(node, ast.Import):
            if any(alias.name not in MODULES for alias in node.names):
                raise ValueError("restricted_import")
        if isinstance(node, ast.ImportFrom):
            if node.level or node.module not in MODULES or any(a.name.startswith("_") or a.name in {"*", "attrgetter", "methodcaller", "Formatter"} for a in node.names):
                raise ValueError("restricted_import")
    return tree


def importer(name, globals=None, locals=None, fromlist=(), level=0):
    if name not in MODULES or level:
        raise ValueError("restricted_import")
    return importlib.import_module(name)


def acceptance(code, imports, tests):
    reviewed_tree(code)
    for item in [*imports, *tests]:
        reviewed_tree(item)
    safe = {name: getattr(builtins, name) for name in BUILTINS}
    safe["__import__"] = importer
    scope = {"__builtins__": safe}
    try:
        exec(compile(code, "candidate.py", "exec"), scope)
        if any(name in scope and scope[name] is not value for name, value in safe.items()):
            raise ValueError("oracle_builtin_shadowed")
        for item in imports:
            exec(compile(item, "oracle-import", "exec"), scope)
        for index, item in enumerate(tests):
            exec(compile(item, f"oracle-test-{index}", "exec"), scope)
    except ValueError as error:
        if str(error) == "oracle_builtin_shadowed":
            raise
        return {"status": "failed", "error_kind": type(error).__name__,
                "tests_passed_before_failure": index if "index" in locals() else 0}
    except Exception as error:
        return {"status": "failed", "error_kind": type(error).__name__,
                "tests_passed_before_failure": index if "index" in locals() else 0}
    return {"status": "passed", "tests_passed": len(tests)}


def run(payload):
    try:
        reference = acceptance(payload["reference"], payload["imports"], payload["tests"])
        if reference["status"] != "passed":
            return {"status": "unknown", "reason": "frozen_reference_does_not_pass", "reference": reference}
        try:
            reviewed_tree(payload["candidate"])
        except SyntaxError:
            return {"status": "failed", "error_kind": "SyntaxError", "reference": reference}
        candidate = acceptance(payload["candidate"], payload["imports"], payload["tests"])
        return {**candidate, "reference": reference}
    except (ValueError, SyntaxError, KeyError, TypeError):
        return {"status": "unknown", "reason": "unsupported_or_invalid_oracle_input"}


if __name__ == "__main__":
    result = run(json.load(sys.stdin))
    sys.stdout.write(json.dumps(result, sort_keys=True))
