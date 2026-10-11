"""Whole-source leakage propagation and outcome-independent representation."""

import pytest

from evals.verification.apps_v3.dedup import groups, stateless_method_fingerprint, url_identity
from evals.verification.humaneval_source import implementation_fingerprint


def record(identity, position, question, code, url=None):
    return {"source_id": identity, "original_line_number": position, "question": question,
            "references": [code], "url": url}


def test_transitive_connected_group_and_fixed_earliest_representative():
    rows = [record("a", 3, "Question A", "def f(x): return x+1"),
            record("b", 1, "Question A", "def f(x): return x-1"),
            record("c", 2, "Question C", "def renamed(y): return y-1")]
    result = groups(rows, [])
    assert result["source_group_count"] == 1
    assert result["groups"][0]["members"] == ["b", "c", "a"]
    assert result["groups"][0]["representative"] == "b"
    assert result["reference_pass_results_used_for_grouping"] is False


def test_any_historical_variant_excludes_the_whole_component():
    rows = [record("a", 1, "Question", "def f(x): return x+1"),
            record("b", 2, "Question", "def f(x): return x-1")]
    history = [record("historical-full", 1, "Other", "def old(z): return z-1")]
    result = groups(rows, history)
    assert result["groups"][0]["historical_source_matches"] == ["historical-full"]
    assert result["excluded_groups"] == 1
    assert result["groups"][0]["source_admitted"] is False


def test_all_original_references_participate_before_any_outcomes():
    task = record("a", 1, "Question A", "def f(x): return x+1")
    task["references"].append("def f(x): return x*7")
    historical = record("old", 1, "Other", "def named(y): return y*7")
    assert groups([task], [historical])["excluded_groups"] == 1


def test_outcome_fields_never_select_a_passing_representative():
    rows = [record("first", 1, "Question", "def f(x): return x+1"),
            record("later", 2, "Question", "def f(x): return x-1")]
    rows[0]["reference_passed"] = False
    rows[1]["reference_passed"] = True
    assert groups(rows, [])["groups"][0]["representative"] == "first"


def test_url_identity_preserves_problem_query_and_path_case():
    assert url_identity("http://www.Example.org/Task/?id=2&lang=py#fragment") == "https://example.org/Task?id=2&lang=py"
    assert url_identity("https://example.org/Task?id=1") != url_identity("https://example.org/Task?id=2")
    assert url_identity(None) is None
    with pytest.raises(ValueError):
        url_identity("https://secret:password@example.org/problem")


def test_stateless_class_method_bridge_is_exclusion_only():
    bridge = stateless_method_fingerprint("class Solution:\n def f(self,x): return x+1")
    assert bridge == implementation_fingerprint("def renamed(y): return y+1")
    assert stateless_method_fingerprint("class Solution:\n def f(self,x): return self.a+x") is None


def test_parse_failure_keeps_exact_source_exposure():
    rows = [record("new", 1, "Question", "invalid Python !")]
    history = [record("old", 1, "Other", "invalid Python !")]
    result = groups(rows, history)
    assert result["excluded_groups"] == 1 and result["unparsed_reference_count"] == 2


def test_duplicate_source_identity_is_rejected():
    task = record("new", 1, "Question", "def f(x): return x")
    with pytest.raises(ValueError, match="distinct original"):
        groups([task, task], [])
