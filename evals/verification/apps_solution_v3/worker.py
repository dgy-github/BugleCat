"""One genuine Solution instance and all original ordered pairs.

Reuse the frozen free-function comparison/loop; do not rewrite artifact code.
All module substitutions below are on privately loaded module instances.
"""
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


FOLDER = Path(__file__).resolve().parent
free = local_module("solution_frozen_pairs", FOLDER.parent / "apps_v3/worker.py")
policy = local_module("solution_receiver_review", FOLDER / "policy.py")


def environment(tree, function_name):
    safe = {name: getattr(builtins, name) for name in free.original.BUILTINS if name != "type"}
    safe["__import__"] = policy.typing_policy.importer
    safe["__build_class__"] = builtins.__build_class__
    scope = {"__builtins__": safe, "__name__": "apps_solution_artifact", **free.prelude()}
    exec(compile(tree, "apps-solution-artifact.py", "exec"), scope)
    if any(name in scope and scope[name] is not value for name, value in safe.items()):
        raise free.ScopeError("oracle_builtin_shadowed")
    solution = scope.get("Solution")
    if type(solution) is not type:
        raise free.ScopeError("unsupported_solution_class_form")
    instance = solution()
    method = getattr(instance, function_name, None)
    if not callable(method):
        raise LookupError("entry_point_not_callable")
    return method


free.reviewed_code = policy.reviewed_code
free.environment = environment
execute = free.execute


if __name__ == "__main__":
    sys.stdout.write(json.dumps(execute(json.load(sys.stdin)), sort_keys=True, allow_nan=False))
