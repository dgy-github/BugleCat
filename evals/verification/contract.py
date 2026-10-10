"""Executable v1 contract; offline evaluator only, not a Rust runtime gate."""

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

Verdict = Literal["pass", "need_evidence", "contradiction", "unknown"]


class StrictModel(BaseModel):
    model_config = ConfigDict(extra="forbid")


class Evidence(StrictModel):
    id: str
    session_id: str
    turn_id: int = Field(ge=0)
    source_ref: str
    kind: Literal["tool_result", "runtime_verification", "file_read"]
    status: Literal["passed", "failed", "timeout", "cancelled", "unknown"]
    exit_code: int | None = None
    excerpt: str
    truncated: bool = False
    scope: list[str]
    # SHA-256 per affected file, captured by the host when evidence was obtained.
    file_hashes: dict[str, str] = Field(default_factory=dict)


class Claim(StrictModel):
    id: str
    text: str
    kind: Literal["execution_success", "tests_passed", "semantic", "limitation"]
    evidence_ids: list[str]
    scope: list[str]


class VerificationInput(StrictModel):
    schema_version: Literal["buglecat.verification.input.v1"]
    session_id: str
    turn_id: int = Field(ge=0)
    requirements: list[str]
    candidate: str
    claims: list[Claim]
    evidence: list[Evidence]
    current_file_hashes: dict[str, str]
    unresolved_call_ids: list[str]
    # Host assessment, not supplied by the candidate model as trusted truth.
    coverage: Literal["complete", "incomplete", "unknown"]

    @model_validator(mode="after")
    def unique_ids(self):
        for records in (self.claims, self.evidence):
            ids = [record.id for record in records]
            if len(ids) != len(set(ids)):
                raise ValueError("duplicate IDs")
        for claim in self.claims:
            if claim.text not in self.candidate:
                raise ValueError("claim text must be an exact candidate excerpt")
        return self


class ClaimDecision(StrictModel):
    claim_id: str
    verdict: Verdict
    reason: Literal[
        "supported", "missing_evidence", "foreign_evidence", "explicit_failure",
        "incomplete_evidence", "scope_mismatch", "stale_evidence",
        "semantic_review_required", "honest_limitation",
    ]
    evidence_ids: list[str]


class VerificationResult(StrictModel):
    schema_version: Literal["buglecat.verification.result.v1"]
    verdict: Verdict
    reason: str
    decisions: list[ClaimDecision]
    method: Literal["deterministic", "jev"]
    call_status: Literal["not_called", "ok", "timeout", "cancelled", "error"]
    model: str | None = None
    policy_version: str

    @model_validator(mode="after")
    def no_false_verification(self):
        if self.method == "jev" and (self.call_status != "ok" or not self.model):
            if self.verdict != "unknown":
                raise ValueError("unavailable Jev must remain unknown")
        if self.verdict == "pass" and (
            not self.decisions or any(d.verdict != "pass" for d in self.decisions)
        ):
            raise ValueError("pass requires nonempty all-pass decisions")
        return self
