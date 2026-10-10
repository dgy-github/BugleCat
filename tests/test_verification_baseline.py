import json
from pathlib import Path

import pytest
from pydantic import ValidationError

from evals.verification.baseline import evaluate, run
from evals.verification.contract import VerificationInput, VerificationResult

CASES = Path(__file__).resolve().parents[1] / "evals/verification/cases.json"
FIXTURES = json.loads(CASES.read_text(encoding="utf-8"))


@pytest.mark.parametrize("case", FIXTURES, ids=lambda c: c["id"])
def test_labeled_cases(case):
    assert evaluate(VerificationInput.model_validate(case["input"])).verdict == case["expected"]


def test_split_integrity_and_report_honesty():
    ids = [c["id"] for c in FIXTURES]
    assert len(ids) == len(set(ids))
    groups = {split: {c["group"] for c in FIXTURES if c["split"] == split}
              for split in ("calibration", "holdout")}
    assert groups["calibration"].isdisjoint(groups["holdout"])
    report = run(CASES)
    assert report["runtime_baseline"]["status"] == "not_run"
    assert report["jev_baseline"]["status"] == "not_run"


def test_contract_rejects_fabricated_claim_and_duplicate_evidence():
    data = json.loads(json.dumps(FIXTURES[0]["input"]))
    data["claims"][0]["text"] = "not in candidate"
    with pytest.raises(ValidationError):
        VerificationInput.model_validate(data)
    data = json.loads(json.dumps(FIXTURES[0]["input"]))
    data["evidence"].append(data["evidence"][0])
    with pytest.raises(ValidationError):
        VerificationInput.model_validate(data)


def test_jev_timeout_cannot_pass():
    result = evaluate(VerificationInput.model_validate(FIXTURES[0]["input"])).model_dump()
    result.update(method="jev", call_status="timeout", model="test-model")
    with pytest.raises(ValidationError):
        VerificationResult.model_validate(result)


def test_unknown_reference_does_not_pass():
    data = json.loads(json.dumps(FIXTURES[0]["input"]))
    data["claims"][0]["evidence_ids"] = ["invented"]
    assert evaluate(VerificationInput.model_validate(data)).verdict == "need_evidence"


def test_incomplete_coverage_does_not_weaken_missing_evidence():
    data = json.loads(json.dumps(FIXTURES[0]["input"]))
    data["evidence"] = []
    data["coverage"] = "incomplete"
    assert evaluate(VerificationInput.model_validate(data)).verdict == "need_evidence"
