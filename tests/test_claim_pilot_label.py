import hashlib
import json

import pytest

from scripts.label_claim_pilot import freeze


def test_label_binds_exact_claim_and_rejects_score_exposure(tmp_path):
    directory = tmp_path / "one"
    directory.mkdir()
    (tmp_path / "manifest.json").write_text(json.dumps({"cases": [{"id": "one"}]}))
    (directory / "receipt.json").write_text("{}")
    (directory / "assertions.json").write_text("{}")
    prepared = {"receipt_sha256": hashlib.sha256(b"{}").hexdigest(),
                "request_hash": "frozen", "body": {"state": {"claim": "check passed"}}}
    (directory / "jev-prepared.json").write_text(json.dumps(prepared))
    decision = {"index": 0, "claim": "wrong", "label": "supported", "scope_basis": "exit 0"}
    with pytest.raises(ValueError, match="differs"):
        freeze(tmp_path, [decision])
    assert not (directory / "scope-review.json").exists()
    decision["claim"] = "check passed"
    freeze(tmp_path, [decision])
    review = json.loads((directory / "scope-review.json").read_bytes())
    assert review["jev_scores_read"] is False
    assert review["formal_eligible"] is False
    (directory / "jev-response.json").write_text("{}")
    with pytest.raises(ValueError, match="already exposed"):
        freeze(tmp_path, [decision])
