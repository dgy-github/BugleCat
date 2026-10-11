"""Independent implementation of pinned APPS call-based return comparison.

Only built-in JSON values, tuples and sets are within this local comparison
scope. APPS STDIN tolerances and arbitrary custom equality are unsupported.
No official evaluator is imported or executed. Original inputs are preserved.
"""

from copy import deepcopy

OFFICIAL_COMMIT = "362aedc3c71cd7d9bd2fc96a6c80e11dbc38c7a5"
OFFICIAL_EVALUATOR_SHA256 = "9a4e58ff2634ef606c42597457c0733910862e0588ea750d901265bbfe65d36f"
CONTRACT = "apps-call-based-builtins-ordered-v3"


def public_identifier(value):
    """Entry points are public identifiers, not reflection or module paths."""
    return isinstance(value, str) and value.isidentifier() and not value.startswith("_")


def normalize_pair(arguments, expected):
    """Replicate the three original coercion attempts, including argument loss."""
    if type(arguments) is not list:
        raise ValueError("arguments_not_list")
    arguments, expected = deepcopy(arguments), deepcopy(expected)
    try:
        if isinstance(arguments[0], dict):
            arguments = [{int(key): value for key, value in arguments[0].items()}]
    except (IndexError, ValueError, TypeError, OverflowError):
        pass
    try:
        if isinstance(expected, dict):
            expected = [{int(key): value for key, value in expected.items()}]
    except (ValueError, TypeError, OverflowError):
        pass
    try:
        if isinstance(expected[0], dict):
            expected = [{int(key): value for key, value in expected[0].items()}]
    except (IndexError, KeyError, ValueError, TypeError, OverflowError):
        pass
    return arguments, expected


def require_builtin_value(value, *, depth=0, remaining=None):
    """Reject ambiguous custom equality and excessive result structures."""
    if remaining is None:
        remaining = [100000]
    remaining[0] -= 1
    if remaining[0] < 0 or depth > 100:
        raise ValueError("comparison_resource_limit")
    if type(value) in (type(None), bool, int, float, str, bytes):
        return
    if type(value) not in (list, tuple, dict, set, frozenset):
        raise ValueError("unsupported_comparison_value")
    members = value.items() if type(value) is dict else value
    for member in members:
        require_builtin_value(member, depth=depth + 1, remaining=remaining)


def matches(output, expected):
    """Exact official alternatives, without recursive conversion/float tolerance."""
    require_builtin_value(output)
    require_builtin_value(expected)
    if isinstance(output, tuple):
        output = list(output)
    accepted = output == expected
    if isinstance(expected, list) and expected:
        accepted = accepted or output == expected[0]
    try:
        if isinstance(output[0], tuple):
            accepted = accepted or [list(item) for item in output] == expected[0]
    except (IndexError, KeyError, TypeError, ValueError):
        pass
    return bool(accepted)
