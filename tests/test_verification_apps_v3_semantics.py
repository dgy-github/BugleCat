"""Explicit counterexamples for the pinned call-based comparison contract."""

from copy import deepcopy

import pytest

from evals.verification.apps_v3.semantics import matches, normalize_pair


def test_numeric_dict_coercion_drops_extra_arguments_without_mutating_source():
    arguments, expected = [{"1": [2]}, 99], [{"3": 4}, "discarded"]
    original = deepcopy((arguments, expected))
    assert normalize_pair(arguments, expected) == ([{1: [2]}], [{3: 4}])
    assert (arguments, expected) == original


def test_failed_dict_coercion_retains_all_arguments_and_expected():
    assert normalize_pair([{"x": 1}, 99], {"x": 2}) == ([{"x": 1}, 99], {"x": 2})


def test_empty_numeric_dict_also_discards_extra_arguments():
    assert normalize_pair([{}, 99], {}) == ([{}], [{}])


@pytest.mark.parametrize("output,expected,accepted", [
    (7, [7, 8], True), (8, [7, 8], False),
    ((1, 2), [1, 2], True), ((1, 2), [[1, 2]], True),
    ([(1, 2), (3, 4)], [[[1, 2], [3, 4]]], True),
    ([(1, 2), (3, 4)], [[1, 2], [3, 4]], False),
    ([[(1, 2)]], [[[[1, 2]]]], False),
    (0.30000000000000004, 0.3, False), ([], [], True),
    ({"x": 1}, {"x": 1}, True), (set([1, 2]), [[1, 2]], False),
    (1, True, True), ([], [[], "extra"], True),
])
def test_exact_return_alternatives(output, expected, accepted):
    assert matches(output, expected) is accepted


def test_comparison_does_not_execute_custom_equality():
    class Custom:
        def __eq__(self, other):
            raise AssertionError("custom equality must not run")
    with pytest.raises(ValueError, match="unsupported_comparison_value"):
        matches(Custom(), [1])


def test_comparison_depth_is_explicitly_unknown():
    output = 1
    for _ in range(101):
        output = [output]
    with pytest.raises(ValueError, match="comparison_resource_limit"):
        matches(output, output)
