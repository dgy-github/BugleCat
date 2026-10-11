"""Conservative source grouping before candidates, labels or probabilities.

Reuse both existing reference fingerprints. Matches exclude whole connected
groups; unmatched tasks are not proof of semantic uniqueness. No execution,
reference-pass selection, split, paid authority or admission occurs here.
"""

import ast
import copy
import unicodedata
from collections import defaultdict
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit

from ..humaneval_source import implementation_fingerprint
from ..public_benchmark import solution_fingerprint
from .oracle import digest, raw_sha

POLICY = "apps-original-source-connected-dedup-v3"


def normalized_question(value):
    if not isinstance(value, str) or not value.strip():
        raise ValueError("original question required")
    return " ".join(unicodedata.normalize("NFKC", value).casefold().split())


def url_identity(value):
    if value is None or value == "":
        return None
    parsed = urlsplit(value)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username or parsed.password:
        raise ValueError("public problem URL required")
    host = parsed.hostname.casefold().removeprefix("www.")
    # Preserve the query's problem identifiers and meaningful path case.
    if parsed.port:
        host += f":{parsed.port}"
    return urlunsplit(("https", host, parsed.path.rstrip("/") or "/",
                      urlencode(sorted(parse_qsl(parsed.query, keep_blank_values=True))), ""))


def stateless_method_fingerprint(code):
    """Exclusion-only bridge for a class method that never uses its receiver."""
    tree = ast.parse(code)
    classes = [node for node in tree.body if isinstance(node, ast.ClassDef) and node.name == "Solution"]
    if len(classes) != 1:
        return None
    methods = [node for node in classes[0].body if isinstance(node, ast.FunctionDef) and not node.name.startswith("_")]
    if len(methods) != 1 or classes[0].bases or classes[0].keywords or classes[0].decorator_list:
        return None
    method = copy.deepcopy(methods[0])
    arguments = method.args.posonlyargs + method.args.args
    if not arguments or arguments[0].arg != "self" or method.decorator_list:
        return None
    if any(isinstance(node, ast.Name) and node.id == "self" for statement in method.body for node in ast.walk(statement)):
        return None
    if method.args.posonlyargs:
        method.args.posonlyargs = method.args.posonlyargs[1:]
    else:
        method.args.args = method.args.args[1:]
    # This AST is never executed; additional matches can only exclude groups.
    return implementation_fingerprint(ast.unparse(ast.Module(body=[method], type_ignores=[])))


def features(record):
    keys = {"question:" + digest(normalized_question(record["question"]))}
    url = url_identity(record.get("url"))
    if url:
        keys.add("url:" + digest(url))
    references = record["references"]
    if type(references) is not list or not references or any(type(code) is not str or not code.strip() for code in references):
        raise ValueError("complete original references required")
    unparsed = 0
    for code in references:
        keys.add("raw-reference:" + raw_sha(code.encode()))
        try:
            keys.add("solution:" + solution_fingerprint(code))
            keys.add("implementation:" + implementation_fingerprint(code))
            bridge = stateless_method_fingerprint(code)
            if bridge:
                keys.add("implementation:" + bridge)
        except (SyntaxError, ValueError, TypeError, RecursionError, MemoryError):
            unparsed += 1
    return keys, unparsed


def groups(records, historical_records):
    """Whole-source components, fixed earliest representative, history exclusion."""
    ids = [record["source_id"] for record in records]
    if len(ids) != len(set(ids)) or any(not isinstance(identity, str) or not identity for identity in ids):
        raise ValueError("distinct original source identities required")
    positions = [record["original_line_number"] for record in records]
    if any(type(position) is not int or position < 1 for position in positions) or len(positions) != len(set(positions)):
        raise ValueError("distinct original source line positions required")
    parents = {identity: identity for identity in ids}

    def root(identity):
        while parents[identity] != identity:
            parents[identity] = parents[parents[identity]]
            identity = parents[identity]
        return identity

    owners, exposures = {}, defaultdict(set)
    unparsed = 0
    for record in historical_records:
        keys, skipped = features(record)
        unparsed += skipped
        for key in keys:
            exposures[key].add(record["source_id"])
    historical_matches = {}
    for record in records:
        identity = record["source_id"]
        keys, skipped = features(record)
        unparsed += skipped
        historical_matches[identity] = set().union(*(exposures[key] for key in keys))
        for key in keys:
            if key in owners:
                parents[root(identity)] = root(owners[key])
            else:
                owners[key] = identity
    components = defaultdict(list)
    by_id = {record["source_id"]: record for record in records}
    for identity in ids:
        components[root(identity)].append(identity)
    result = []
    for members in components.values():
        members.sort(key=lambda identity: by_id[identity]["original_line_number"])
        matched = sorted(set().union(*(historical_matches[identity] for identity in members)))
        result.append({"group_id": "apps-source-v3-" + digest(sorted(members))[:24],
                       "members": members, "representative": members[0],
                       "historical_source_matches": matched, "excluded_by_history": bool(matched),
                       "source_admitted": False, "evaluation_split": None})
    result.sort(key=lambda row: by_id[row["representative"]]["original_line_number"])
    return {"policy": POLICY, "source_rows": len(records), "historical_source_rows": len(historical_records),
            "source_group_count": len(result), "groups": result,
            "excluded_groups": sum(row["excluded_by_history"] for row in result),
            "unparsed_reference_count": unparsed, "reference_pass_results_used_for_grouping": False,
            "source_admitted_groups": 0, "semantic_near_duplicate_absence_proven": False,
            "paid_calls": 0}
