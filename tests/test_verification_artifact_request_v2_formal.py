import copy
import hashlib
import sys
from pathlib import Path

import pytest

from evals.verification.artifact_request_v2 import signal as development_signal
from evals.verification.artifact_request_v2_formal import request, signal, validate_request

ROOT = Path(__file__).resolve().parents[1]


def packet(kind="mbpp"):
    task = {"prompt": "public", "code": "REFERENCE_SECRET", "label": "LABEL_SECRET",
            "test_list": ["def check(candidate):\n    assert candidate(2) == 4", "check(f)"],
            "test_imports": []}
    return request(task, ["def f(n):"], "def f(n): return 2 * n", kind)


def test_workers_and_allowed_typing_are_explicit_and_complete_tests_preserved():
    mbpp, human = packet(), packet("humaneval")
    assert "allowed_typing_from_import_names" not in mbpp["state"]["execution_contract"]
    assert "List" in human["state"]["execution_contract"]["allowed_typing_from_import_names"]
    assert human["state"]["execution_contract"]["worker_sha256"] == hashlib.sha256(
        (ROOT / "evals/verification/humaneval_oracle_worker.py").read_bytes()).hexdigest()
    assert human["state"]["acceptance_assertions"] == mbpp["state"]["acceptance_assertions"]
    assert list(human["questions"]) == ["setup_violation", "assertion_0_violation", "assertion_1_violation"]
    assert "function definitions do not run their bodies" in human["state"]["execution_contract"]["test_blocks"]
    assert "REFERENCE_SECRET" not in str(human) and "LABEL_SECRET" not in str(human)


@pytest.mark.parametrize("kind", ["mbpp", "humaneval"])
def test_valid_numeric_signal_cannot_promote_to_pass_or_formal_certification(kind):
    body = packet(kind)
    validate_request(body)
    response = {"model": "jev-1.13.0", "usage": {"input_tokens": 1, "output_tokens": 1},
                "answers": {name: {"type": "noul", "noul": .8} for name in body["questions"]}}
    result = signal(body, response)
    assert result["acceptance_violation"] == .8
    assert result["formal_certification"] is result["production_authorized"] is False
    assert result["certified_threshold"] is None and result["verified_pass_signals"] == 0
    with pytest.raises(ValueError):
        development_signal(body, response)


def test_foreign_source_kind_and_runtime_contract_edits_rejected():
    with pytest.raises(ValueError, match="source kind"):
        packet("unspecified")
    changed = copy.deepcopy(packet("humaneval"))
    changed["state"]["execution_contract"]["allowed_typing_from_import_names"].append("get_type_hints")
    with pytest.raises(ValueError, match="contract changed"):
        validate_request(changed)


def runtime():
    return {"executable_file": "LOCAL_PRIVATE_EXECUTABLE", "executable_sha256": "a"*64,
            "version": "LOCAL_BUILD_DETAIL", "version_info": list(sys.version_info[:3]),
            "implementation": "cpython", "platform": sys.platform, "platform_detail": "LOCAL_MACHINE_DETAIL"}


def test_actual_runtime_proof_retains_hash_version_without_sending_machine_path():
    body = request({"prompt": "public", "test_list": ["assert f(2) == 4"], "test_imports": []},
                   ["def f(n):"], "def f(n): return 2*n", "mbpp", runtime())
    validate_request(body)
    public = body["state"]["execution_contract"]["oracle_runtime_binding"]
    assert public["executable_sha256"] == "a"*64 and public["version_info"] == list(sys.version_info[:3])
    assert "LOCAL_" not in str(body)


@pytest.mark.parametrize("field,value", [("version_info", [0, 0, 0]), ("executable_sha256", "wrong"),
                                          ("oracle_label", "supported")])
def test_runtime_mismatch_or_outcome_injection_rejected(field, value):
    proof = runtime()
    proof[field] = value
    with pytest.raises(ValueError):
        request({"prompt": "public", "test_list": ["assert f(2) == 4"], "test_imports": []},
                ["def f(n):"], "def f(n): return 2*n", "mbpp", proof)
