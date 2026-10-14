import importlib.util
import json
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "audit_claim_source_pool", ROOT / "scripts/audit_claim_source_pool.py"
)
pool_audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(pool_audit)


def test_audit_source_pool_matches_runtime_policy_and_excludes_pilot():
    report = pool_audit.audit_source_pool(ROOT)
    assert report["policy_version"] == "host-rules-v3.4"
    assert report["exclusions"]["pilot_exposed_task_ids"] == [25, 28, 44, 76, 163, 207, 213, 221]
    assert report["exclusions"]["pilot_exposed_groups"] == 8
    assert report["mbpp_expanded_v2"]["pilot_overlap_with_unexposed"] == 0
    assert report["mbpp_expanded_v2"]["unexposed_calibration_groups"] == 29
    assert report["mbpp_expanded_v2"]["unexposed_holdout_groups"] == 473
    assert report["readiness"]["calibration_pool_sufficient_from_mbpp_alone"] is False
    assert report["readiness"]["calibration_group_deficit_min"] == 171
    assert report["readiness"]["new_independent_source_required"] is True
    assert report["readiness"]["holdout_opened"] is False
    assert report["readiness"]["production_authorized"] is False
    assert report["readiness"]["paid_calls"] == 0


def test_audit_source_pool_rejects_policy_mismatch(monkeypatch):
    monkeypatch.setattr(pool_audit, "runtime_policy_version", lambda root=ROOT: "host-rules-v9.9")
    with pytest.raises(ValueError, match="policy_version"):
        pool_audit.audit_source_pool(ROOT)


def test_audit_source_pool_rejects_unmarked_pilot_exposure(tmp_path, monkeypatch):
    protocol = json.loads((ROOT / "evals/verification/claim_production_protocol_v1.json").read_bytes())
    pilot = json.loads((ROOT / "evals/verification/reports/claim_pilot_20261013.json").read_bytes())
    pilot["cases"][0]["author_exposed"] = False

    (tmp_path / "evals/verification/reports").mkdir(parents=True)
    (tmp_path / "rust/crates/ncx-core/src/verification").mkdir(parents=True)
    (tmp_path / "rust/crates/ncx-core/src/verification/types.rs").write_text(
        'pub const POLICY_VERSION: &str = "host-rules-v3.4";\n',
        encoding="utf-8",
    )
    (tmp_path / "evals/verification/claim_production_protocol_v1.json").write_text(
        json.dumps(protocol), encoding="utf-8"
    )
    (tmp_path / "evals/verification/reports/claim_pilot_20261013.json").write_text(
        json.dumps(pilot), encoding="utf-8"
    )
    with pytest.raises(ValueError, match="author_exposed"):
        pool_audit.audit_source_pool(tmp_path)
