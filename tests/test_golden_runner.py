"""Offline integrity guards for the golden regression suite.

These tests keep the eval set itself honest: manifest well-formed, referenced
targets exist, bench tasks valid (non-vacuous grader + seeded defect fails +
reference passes), self-repair anchors unique. They never invoke the agent.
"""

from __future__ import annotations

import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))

from evals.golden import runner  # noqa: E402


def test_golden_manifest_wellformed() -> None:
    manifest = runner.load_manifest()
    cases = manifest["cases"]
    assert manifest["schema"] == "buglecat.golden.v1"
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    for case in cases:
        assert case["tier"] in manifest["tiers"], case["id"]
        assert case["capability"] in manifest["capabilities"], case["id"]
        assert case["priority"] in {"critical", "high", "medium"}, case["id"]
        # All critical cases must be offline-runnable (unit tier) so the
        # default gate can never pass on skipped criticals alone.
        if case["priority"] == "critical":
            assert case["tier"] == "unit", f"{case['id']} is critical but not unit-tier"


def test_unit_pytest_targets_exist() -> None:
    manifest = runner.load_manifest()
    for case in manifest["cases"]:
        if case["kind"] == "pytest":
            assert (REPO / case["target"]).exists(), f"{case['id']} target missing: {case['target']}"


def test_bench_tasks_all_valid() -> None:
    """Full bench library validity: grader non-vacuous, seeded defects fail,
    committed references pass. This is the G-BENCH-001 case body."""
    ok, records = runner.validate_bench_tasks()
    invalid = [r for r in records if not r.get("valid")]
    assert ok, f"invalid bench tasks: {invalid}"
    # The previously-broken t2_median must now have a real seeded defect.
    t2 = next(r for r in records if r["task"] == "t2_median")
    assert t2["seeded_fails"] is True, "t2_median seed must fail check (bug was reintroduced)"
    assert t2["reference_passes"] is True
    # New coverage tasks must carry valid seeds and references.
    for name in ("t19_multi_file_stats", "t20_nav_fix"):
        row = next(r for r in records if r["task"] == name)
        assert row["seeded_fails"] is True, f"{name} seed must fail check"
        assert row["reference_passes"] is True, f"{name} reference must pass check"


def test_self_repair_anchors_unique() -> None:
    ok, records = runner.validate_self_repair_anchors()
    broken = [r for r in records if r["issues"]]
    assert ok, f"broken anchors: {broken}"


def test_reference_files_are_pure_data() -> None:
    """Reference solutions must be JSON data, never executed Python."""
    tasks_dir = REPO / "bench" / "tasks"
    for solution in tasks_dir.glob("*/reference/*.py"):
        raise AssertionError(f"reference must be data-only, found executable file: {solution}")


def test_judge_endpoint_policy_rejects_private_and_metadata() -> None:
    from evals.golden.judge import resolve_endpoint

    assert resolve_endpoint("https://api.deepseek.com") is not None
    assert resolve_endpoint("http://127.0.0.1:8080/v1") is not None  # loopback http allowed
    assert resolve_endpoint("http://169.254.169.254/latest/meta-data") is None
    assert resolve_endpoint("http://10.0.0.5/v1") is None  # private http rejected
    assert resolve_endpoint("ftp://example.com") is None
