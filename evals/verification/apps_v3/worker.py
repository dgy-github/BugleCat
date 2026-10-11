"""Credential-free, ordered APPS free-function acceptance worker.

The frozen AST policy is reused unchanged. This is a language review gate,
not an OS security sandbox. Unsupported official prelude dependencies are
Unknown; no class, STDIN, remote evaluator or dependency installation is used.
"""

import ast
import builtins
import importlib.util
import json
import sys
from pathlib import Path


def local_module(name, path):
    specification = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


original = local_module("frozen_apps_ast", Path(__file__).resolve().parents[1] / "benchmark_oracle_worker.py")
semantics = local_module("apps_comparison", Path(__file__).with_name("semantics.py"))
UNAVAILABLE_PRELUDE = {"sys", "time", "np", "random"}


class ScopeError(ValueError):
    """Host policy refusal, distinct from exceptions thrown by artifact code."""


def prelude():
    """Supported subset of the official prelude; missing parts are explicit."""
    import collections
    import fractions
    import functools
    import heapq
    import itertools
    import math
    from typing import List, Tuple

    scope = {"itertools": itertools, "collections": collections, "math": math,
             "fractions": fractions, "heapq": heapq, "List": List, "Tuple": Tuple,
             "lru_cache": functools.lru_cache}
    for module, names in (
        (itertools, "accumulate product permutations combinations"),
        (collections, "Counter OrderedDict deque defaultdict ChainMap"),
        (math, "sqrt sin cos tan ceil fabs floor gcd exp log log2"),
        (heapq, "heappush heappop heappushpop heapreplace heapify nlargest nsmallest merge"),
    ):
        scope.update({name: getattr(module, name) for name in names.split()})
    return scope


def validate_io(io):
    if type(io) is not dict or not semantics.public_identifier(io.get("fn_name")):
        raise ValueError("unsupported_entry_point")
    inputs, outputs = io.get("inputs"), io.get("outputs")
    if type(inputs) is not list or type(outputs) is not list or not inputs or len(inputs) != len(outputs):
        raise ValueError("invalid_or_empty_pairs")
    if any(type(arguments) is not list for arguments in inputs):
        raise ValueError("arguments_not_list")
    semantics.require_builtin_value(inputs)
    semantics.require_builtin_value(outputs)
    return inputs, outputs


def reviewed_code(code, function_name=None):
    tree = original.reviewed_tree(code)
    if "class Solution" in code:
        raise ValueError("official_solution_branch_outside_scope")
    if function_name is not None and not any(
        isinstance(node, ast.FunctionDef) and node.name == function_name for node in tree.body
    ):
        raise ValueError("unsupported_entry_point_form")
    return tree


def environment(tree, function_name):
    safe = {name: getattr(builtins, name) for name in original.BUILTINS}
    safe["__import__"] = original.importer
    scope = {"__builtins__": safe, **prelude()}
    exec(compile(tree, "apps-artifact.py", "exec"), scope)
    if any(name in scope and scope[name] is not value for name, value in safe.items()):
        raise ScopeError("oracle_builtin_shadowed")
    # Host lookup is outside the candidate's restricted builtin namespace.
    method = scope.get(function_name)
    if not callable(method):
        raise LookupError("entry_point_not_callable")
    return method


def unknown_error(error):
    if isinstance(error, (MemoryError, RecursionError)):
        return "worker_resource_limit"
    if isinstance(error, NameError):
        if error.name in UNAVAILABLE_PRELUDE:
            return "unsupported_implicit_dependency"
        if error.name not in original.BUILTINS and hasattr(builtins, error.name or ""):
            return "unsupported_implicit_builtin"
    return None


def execute(payload):
    """Run all real pairs; runtime failures continue in the same namespace."""
    try:
        inputs, outputs = validate_io(payload["input_output"])
        tree = reviewed_code(payload["code"], payload["input_output"]["fn_name"])
    except (ValueError, KeyError, TypeError) as error:
        return {"status": "unknown", "reason": str(error)}
    except SyntaxError:
        return {"status": "failed", "error_kind": "SyntaxError", "pair_results": [],
                "pairs_executed": 0, "failure_phase": "compile_or_lookup"}
    try:
        method = environment(tree, payload["input_output"]["fn_name"])
    except ScopeError as error:
        return {"status": "unknown", "reason": str(error)}
    except Exception as error:
        reason = unknown_error(error)
        return {"status": "unknown", "reason": reason} if reason else {
            "status": "failed", "error_kind": type(error).__name__, "pair_results": [],
            "pairs_executed": 0, "failure_phase": "compile_or_lookup"}
    results = []
    for index, (arguments, expected) in enumerate(zip(inputs, outputs, strict=True)):
        arguments, expected = semantics.normalize_pair(arguments, expected)
        try:
            output = method(*arguments)
        except Exception as error:
            reason = unknown_error(error)
            if reason:
                return {"status": "unknown", "reason": reason, "pair_results": results,
                        "pairs_executed": index + 1}
            results.append({"index": index, "passed": False, "error_kind": type(error).__name__})
            continue
        try:
            results.append({"index": index, "passed": semantics.matches(output, expected)})
        except (ValueError, TypeError, RecursionError, MemoryError) as error:
            return {"status": "unknown", "reason": str(error), "pair_results": results,
                    "pairs_executed": index + 1}
    return {"status": "passed" if all(item["passed"] for item in results) else "failed",
            "pair_results": results, "pairs_executed": len(results)}


if __name__ == "__main__":
    sys.stdout.write(json.dumps(execute(json.load(sys.stdin)), sort_keys=True, allow_nan=False))
