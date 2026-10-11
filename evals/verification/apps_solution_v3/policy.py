"""Narrow instance-receiver extension of frozen AST/typing review.

Only the validation projection changes. The worker compiles the original AST.
This language gate is not an operating-system sandbox.
"""
import ast
import copy
import importlib.util
from pathlib import Path


def local_module(name, path):
    specification = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


typing_policy = local_module("solution_frozen_typing", Path(__file__).resolve().parents[1] / "humaneval_oracle_worker.py")
PUBLIC_METHOD_EXCLUSIONS = {"self", "Solution", "type"}


def method_body_nodes(solution):
    """The only trusted receiver bindings are declared instance-method self."""
    body_nodes, receiver_args = set(), set()
    for method in solution.body:
        if isinstance(method, ast.Expr) and isinstance(method.value, ast.Constant) and isinstance(method.value.value, str):
            continue
        if not isinstance(method, ast.FunctionDef):
            raise ValueError("unsupported_solution_class_body")
        if method.decorator_list or (method.name.startswith("_") and method.name != "__init__") or method.name in PUBLIC_METHOD_EXCLUSIONS:
            raise ValueError("unsupported_solution_method")
        arguments = [*method.args.posonlyargs, *method.args.args]
        if not arguments or arguments[0].arg != "self":
            raise ValueError("unsupported_solution_receiver")
        receiver_args.add(arguments[0])
        for statement in method.body:
            body_nodes.update(ast.walk(statement))
    return body_nodes, receiver_args


def review_receiver(tree, body_nodes, receiver_args):
    """No aliases/class handles can turn the self-write exception into a module write."""
    for node in ast.walk(tree):
        if isinstance(node, ast.Name):
            if node.id in {"Solution", "type"}:
                raise ValueError("solution_class_object_access")
            if node.id == "self" and (node not in body_nodes or isinstance(node.ctx, (ast.Store, ast.Del))):
                raise ValueError("solution_receiver_rebinding")
        elif isinstance(node, ast.arg) and node.arg in {"self", "Solution", "type"} and node not in receiver_args:
            raise ValueError("solution_receiver_or_class_shadow")
        elif isinstance(node, ast.alias) and (node.asname or node.name.split('.')[0]) in {"self", "Solution", "type"}:
            raise ValueError("solution_receiver_or_class_shadow")
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name in PUBLIC_METHOD_EXCLUSIONS:
            raise ValueError("solution_receiver_or_class_shadow")
        elif isinstance(node, ast.ExceptHandler) and node.name in PUBLIC_METHOD_EXCLUSIONS:
            raise ValueError("solution_receiver_or_class_shadow")
        elif isinstance(node, (ast.MatchAs, ast.MatchStar)) and node.name in PUBLIC_METHOD_EXCLUSIONS:
            raise ValueError("solution_receiver_or_class_shadow")
        elif isinstance(node, ast.MatchMapping) and node.rest in PUBLIC_METHOD_EXCLUSIONS:
            raise ValueError("solution_receiver_or_class_shadow")
        elif isinstance(node, ast.Attribute) and isinstance(node.ctx, (ast.Store, ast.Del)):
            if not (isinstance(node.ctx, ast.Store) and node in body_nodes and
                    isinstance(node.value, ast.Name) and node.value.id == "self" and
                    not node.attr.startswith("_") and
                    node.attr not in {"attrgetter", "methodcaller", "get_field", "get_value"}):
                raise ValueError("oracle_dependency_mutation")


class ReviewProjection(ast.NodeTransformer):
    """Project only reviewed instance writes/classes for existing policy reuse."""
    def visit_ClassDef(self, node):
        return [self.visit(item) for item in node.body]

    def visit_Attribute(self, node):
        if isinstance(node.ctx, ast.Store):
            return ast.copy_location(ast.Name(id="solution_state_slot", ctx=ast.Store()), node)
        return self.generic_visit(node)


def reviewed_code(code, function_name=None):
    if not isinstance(code, str) or len(code.encode()) > 65536:
        raise ValueError("code_size")
    tree = ast.parse(code)
    classes = [node for node in ast.walk(tree) if isinstance(node, ast.ClassDef)]
    if len(classes) != 1 or classes[0] not in tree.body or classes[0].name != "Solution":
        raise ValueError("unsupported_solution_class_form")
    solution = classes[0]
    if solution.bases or solution.keywords or solution.decorator_list:
        raise ValueError("unsupported_solution_inheritance_or_decorator")
    if "class Solution" not in code:
        raise ValueError("official_solution_branch_not_selected")
    body_nodes, receiver_args = method_body_nodes(solution)
    review_receiver(tree, body_nodes, receiver_args)
    projected = ReviewProjection().visit(copy.deepcopy(tree))
    typing_policy.reviewed_tree(ast.unparse(ast.fix_missing_locations(projected)))
    if function_name is not None and not any(isinstance(node, ast.FunctionDef) and node.name == function_name
                                             for node in solution.body):
        raise ValueError("unsupported_entry_point_form")
    return tree
