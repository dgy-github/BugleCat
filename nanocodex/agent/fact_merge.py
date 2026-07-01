"""Merge facts from concurrent research workers, surfacing conflicts.

The review's concurrency finding: two research workers can reach opposite
conclusions about the same thing, and if both get written into repo_facts a
later code worker builds on a contradiction. This module reconciles a batch of
facts BEFORE they are trusted:

* Exact/normalized duplicates collapse to one fact.
* Facts that assert different values for the same subject (parsed from simple
  "<subject> is/are/uses/= <value>" statements) are marked ``disputed`` and
  reported as conflicts, so the orchestrator can route them to a
  needs_clarification node instead of trusting either.

Conflict detection here is deterministic and structural — it catches the common
"A uses X" vs "A uses Y" case without a model call. A semantic merge step (model
based) can be layered on top later; this is the safe floor, not the ceiling.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from nanocodex.agent.state import Fact

_WS = re.compile(r"\s+")
# "<subject> is/are/uses/use/= <value>" — lowercased, loose on purpose.
_REL = re.compile(
    r"^(?P<subject>.+?)\s+(?:is|are|uses|use|=|:)\s+(?P<value>.+?)[.\s]*$",
    re.IGNORECASE,
)


def _normalize(text: str) -> str:
    return _WS.sub(" ", text.strip().lower()).rstrip(".")


def _subject_value(text: str) -> "tuple[str | None, str | None]":
    m = _REL.match(text.strip())
    if not m:
        return None, None
    return _normalize(m.group("subject")), _normalize(m.group("value"))


@dataclass
class MergeOutcome:
    merged: list[Fact]
    # subject -> the conflicting facts asserting different values for it.
    conflicts: dict[str, list[Fact]] = field(default_factory=dict)


def merge_facts(facts: list[Fact]) -> MergeOutcome:
    """Dedup *facts* and flag subject-level contradictions as disputed."""
    merged: list[Fact] = []
    seen_norm: set[str] = set()
    # subject -> (value -> fact)
    by_subject: dict[str, dict[str, Fact]] = {}
    conflicts: dict[str, list[Fact]] = {}

    for fact in facts:
        norm = _normalize(fact.text)
        if norm in seen_norm:
            continue  # exact duplicate
        seen_norm.add(norm)
        merged.append(fact)

        subject, value = _subject_value(fact.text)
        if subject is None:
            continue
        bucket = by_subject.setdefault(subject, {})
        if value not in bucket and bucket:
            # A different value already exists for this subject -> conflict.
            conflicting = list(bucket.values()) + [fact]
            for f in conflicting:
                f.disputed = True
            conflicts[subject] = conflicting
        bucket[value] = fact

    return MergeOutcome(merged=merged, conflicts=conflicts)
