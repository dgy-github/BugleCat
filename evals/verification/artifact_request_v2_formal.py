"""Candidate formal V2 packet owner; pure construction, no paid admission.

Source-kind-specific execution contracts distinguish the original restricted
MBPP worker from the narrow typing extension used by the new HumanEval source.
A versioned packet is not registration, threshold qualification or a Pass.
"""
import hashlib
import json
import sys
from pathlib import Path

from .artifact_request_v2 import MAX_INPUT_BYTES, answer_signal
from .artifact_request_v2 import request as development_request
from .calibration import require
from .humaneval_oracle_worker import TYPING_NAMES

TEMPLATE = "jev-artifact-assertion-specific-v2"
POLICY = "artifact-assertion-specific-v2"
SCHEMA = "buglecat.jev.artifact-assertion-specific.formal.v2"
KINDS = {"mbpp", "humaneval"}


def request(task, interface, code, source_kind, oracle_runtime_binding=None):
    require(source_kind in KINDS, "explicit source kind required")
    body = development_request(task, interface, code)
    state = body["state"]
    state.update(schema=SCHEMA, template_version=TEMPLATE, source_kind=source_kind)
    contract = state["execution_contract"]
    if oracle_runtime_binding is not None:
        require(isinstance(oracle_runtime_binding, dict), "original Oracle runtime binding required")
        require(oracle_runtime_binding.get("version_info") == list(sys.version_info[:3]),
                "packet builder and original Oracle runtime differ")
        raw_hash = oracle_runtime_binding.get("executable_sha256")
        require(isinstance(raw_hash, str) and len(raw_hash) == 64
                and all(char in "0123456789abcdef" for char in raw_hash), "original runtime hash required")
        public_runtime_fields = {"executable_sha256", "version_info", "implementation", "platform"}
        local_proof_fields = {"executable_file", "version", "platform_detail"}
        require(set(oracle_runtime_binding) <= public_runtime_fields | local_proof_fields,
                "unexpected fields in Oracle runtime binding")
        contract["oracle_runtime_binding"] = {key: oracle_runtime_binding[key]
                                              for key in sorted(public_runtime_fields) if key in oracle_runtime_binding}
        contract["runtime_binding"] = "actual frozen Oracle executable/version; original runtime receipts must be independently verified"
    contract["test_blocks"] = "Complete supplied test blocks execute in order; function definitions do not run their bodies until called"
    if source_kind == "humaneval":
        worker = Path(__file__).with_name("humaneval_oracle_worker.py")
        contract["worker_sha256"] = hashlib.sha256(worker.read_bytes()).hexdigest()
        contract["allowed_typing_from_import_names"] = sorted(TYPING_NAMES)
        contract["importer"] += "; from typing imports are limited to these annotation names; import typing is unsupported"
        contract["test_scope"] = "complete original check(candidate) definition followed by check(entry_point); no assertions removed"
    else:
        contract["test_scope"] = "complete frozen MBPP acceptance suite; no assertions removed"
    raw = json.dumps(body, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()
    require(len(raw) <= MAX_INPUT_BYTES, "artifact request budget")
    return body


def validate_request(body):
    require(isinstance(body, dict) and isinstance(body.get("state"), dict), "formal v2 packet required")
    state = body["state"]
    task = {"prompt": state.get("task_prompt"), "test_list": state.get("acceptance_assertions"),
            "test_imports": state.get("test_imports")}
    runtime = state.get("execution_contract", {}).get("oracle_runtime_binding")
    require(body == request(task, state.get("public_interface"), state.get("candidate_code"), state.get("source_kind"), runtime),
            "formal v2 packet contract changed")
    return body


def signal(body, response):
    validate_request(body)
    return {**answer_signal(body, response), "schema": "buglecat.verification.artifact-v2-numeric-signal.v1",
            "template_version": TEMPLATE, "policy_version": POLICY,
            "scope": "numeric response only; original transport and independent registration required"}
