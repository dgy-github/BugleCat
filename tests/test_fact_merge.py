"""Layer 5: fact_merge dedup + conflict detection."""

from __future__ import annotations

from nanocodex.agent.fact_merge import merge_facts
from nanocodex.agent.state import Fact


def test_exact_duplicates_collapse():
    facts = [Fact("repo uses pytest", "r1"), Fact("repo uses pytest", "r2")]
    out = merge_facts(facts)
    assert len(out.merged) == 1
    assert not out.conflicts


def test_normalized_duplicates_collapse():
    facts = [Fact("Repo uses pytest.", "r1"), Fact("repo  uses pytest", "r2")]
    out = merge_facts(facts)
    assert len(out.merged) == 1


def test_conflicting_values_flagged_disputed():
    facts = [
        Fact("the test runner is pytest", "r1"),
        Fact("the test runner is unittest", "r2"),
    ]
    out = merge_facts(facts)
    assert "the test runner" in out.conflicts
    assert all(f.disputed for f in out.merged)


def test_non_conflicting_facts_kept_clean():
    facts = [
        Fact("the test runner is pytest", "r1"),
        Fact("the build tool is hatch", "r2"),
    ]
    out = merge_facts(facts)
    assert not out.conflicts
    assert all(not f.disputed for f in out.merged)
