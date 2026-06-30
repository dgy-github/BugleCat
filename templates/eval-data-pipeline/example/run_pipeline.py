#!/usr/bin/env python3
"""Minimal, deterministic reference of the eval-data pipeline — placeholders FILLED.

This is the runnable demo of the loop the .md agent-commands describe:

    pull -> analyze (dedupe + triage) -> report          (read-only on the eval set)

over a self-contained synthetic snapshot (fixtures/prod_snapshot/audit_log.jsonl).
It is stdlib-only and LLM-free so the closed loop runs and verifies anywhere. The
agent-driven `.md` commands layer the *smart* triage on top of these same steps.

To wire it to a real project, replace the CONFIG block and the fixture with your
prod export + eval; the conventions (dirs, schema, gate, append-only, read-only
daily loop) stay the same.

    python run_pipeline.py [--date YYYYMMDD] [--self-check]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
from collections import OrderedDict
from datetime import date as date_cls
from pathlib import Path

ROOT = Path(__file__).resolve().parent

# ── "filled placeholders" (CONFIG) — swap these for a real project ────────────
CONFIG = {
    "SNAPSHOT_SOURCE": ROOT / "fixtures" / "prod_snapshot",  # {{SNAPSHOT_EXPORT}} source
    "SOURCE_TABLES": ["audit_log"],                          # {{SOURCE_TABLES}}
    "HARD_CASES_FILE": ROOT / "eval" / "online_hard_cases.json",  # {{HARD_CASES_FILE}}
    "DEDUPE_KEY": ("intent", "input", "error_code"),         # {{DEDUPE_KEY}}
    "FAIL_STATUSES": {"fail", "error"},
    "LOW_CONFIDENCE": 0.60,
    "HIGH_CONFIDENCE": 0.95,
}

# Mechanical root-cause map (the eval-triage skill refines this with judgement).
CAUSE = {
    "OVERLAP_CONSTRAINT": "contract/spec",
    "MISPLACED_DURATION": "data_quality",
    "TIMEOUT": "flaky",
    "WRONG_RESULT": "model_capability",
    "NEG_SETUP": "harness/prompt",
}

_VOLATILE = [
    (re.compile(r"\b[0-9a-f]{8}-[0-9a-f-]{27,}\b"), "<uuid>"),
    (re.compile(r"\b\d{4}-\d{2}-\d{2}[t ]\d{2}:\d{2}(?::\d{2})?z?\b"), "<ts>"),
    (re.compile(r"\d+"), "#"),
]


def normalize(s: str) -> str:
    s = (s or "").lower().strip()
    for rx, repl in _VOLATILE:
        s = rx.sub(repl, s)
    return re.sub(r"\s+", " ", s)


def stable_key(row: dict) -> str:
    basis = "|".join(normalize(str(row.get(f, ""))) for f in CONFIG["DEDUPE_KEY"])
    return hashlib.sha1(basis.encode("utf-8")).hexdigest()[:12]


def load_jsonl(path: Path) -> list[dict]:
    if not path.exists():
        return []
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows), encoding="utf-8"
    )


# ── steps ─────────────────────────────────────────────────────────────────────
def pull_snapshot(stamp: str) -> Path:
    """`/pull-snapshot`: copy the nightly snapshot down (idempotent, read-only on prod)."""
    dest = ROOT / "data" / "prod_snapshots" / stamp
    if dest.exists() and any(dest.iterdir()):
        return dest
    dest.mkdir(parents=True, exist_ok=True)
    for f in sorted(CONFIG["SNAPSHOT_SOURCE"].glob("*")):
        shutil.copy2(f, dest / f.name)
    return dest


def covered_keys() -> set[str]:
    """Keys already in the committed eval set — never re-propose these."""
    cases = json.loads(CONFIG["HARD_CASES_FILE"].read_text(encoding="utf-8"))
    return {stable_key(c) for c in cases}


def prior_candidate_keys(stamp: str) -> set[str]:
    """Keys seen in earlier days' candidate files — append-only, no re-emit."""
    keys: set[str] = set()
    cdir = ROOT / "eval" / "candidates"
    for f in sorted(cdir.glob("*.jsonl")) if cdir.exists() else []:
        if f.stem == stamp:
            continue
        for row in load_jsonl(f):
            keys.add(row["key"])
    return keys


def analyze(snapshot_dir: Path, stamp: str) -> tuple[list[dict], list[dict]]:
    """`/analyze-candidates`: extract failures, dedupe by stable key, triage."""
    rows = load_jsonl(snapshot_dir / "audit_log.jsonl")
    skip = covered_keys() | prior_candidate_keys(stamp)

    # extract + dedupe (representative + occurrences + first/last_seen)
    grouped: "OrderedDict[str, dict]" = OrderedDict()
    for r in rows:
        failed = r.get("status") in CONFIG["FAIL_STATUSES"] or (
            r.get("confidence", 1.0) < CONFIG["LOW_CONFIDENCE"]
        )
        if not failed:
            continue
        k = stable_key(r)
        if k in skip:
            continue  # already covered by the eval set or a prior day
        g = grouped.get(k)
        if g is None:
            grouped[k] = {
                "key": k,
                "intent": r.get("intent", ""),
                "error_code": r.get("error_code", ""),
                "example": {
                    "input": r.get("input", ""),
                    "observed": r.get("response_summary", ""),
                    "trace_id": r.get("trace_id", ""),
                },
                "expected": r.get("expected"),
                "max_confidence": r.get("confidence", 0.0),
                "occurrences": 1,
                "first_seen": r.get("ts", ""),
                "last_seen": r.get("ts", ""),
            }
        else:
            g["occurrences"] += 1
            g["last_seen"] = r.get("ts", g["last_seen"])
            g["max_confidence"] = max(g["max_confidence"], r.get("confidence", 0.0))
            if not g.get("expected") and r.get("expected"):
                g["expected"] = r["expected"]

    candidates = list(grouped.values())
    proposals = [triage(c) for c in candidates]
    return candidates, proposals


def triage(c: dict) -> dict:
    """Mechanical triage → the proposal schema (honest: unknown expected ⇒ report_only)."""
    cause = CAUSE.get(c["error_code"], "unknown")
    occ = c["occurrences"]
    expected = c.get("expected")
    flaky = cause == "flaky"
    capability = cause == "model_capability"
    data_quality = cause == "data_quality"

    # Only a reproducible, explicitly-specified harness/contract miss is promotable.
    promotable = bool(expected) and not flaky and not capability and not data_quality
    if promotable:
        action = "promote"
    elif flaky:
        action = "mark_flaky"
    elif data_quality:
        action = "report_only"   # → production data-fix channel, not an eval case
    else:
        action = "report_only"   # capability gap or under-specified ⇒ human decides

    risk = "critical" if occ >= 3 else ("high" if occ == 2 else "medium")
    if data_quality:
        risk = "high"
    confidence = "high" if c["max_confidence"] >= CONFIG["HIGH_CONFIDENCE"] else "medium"

    return {
        "key": c["key"],
        "cluster": c["error_code"],
        "intent": c["intent"],
        "example": c["example"],
        "expected": expected,
        "cause": cause,
        "confidence": confidence,
        "risk": risk,
        "occurrences": occ,
        "first_seen": c["first_seen"],
        "last_seen": c["last_seen"],
        "promotable": promotable,
        "action": action,
    }


def write_report(stamp: str, candidates: list[dict], proposals: list[dict], rows_total: int) -> Path:
    """`/daily-report`: real numbers, mirrors reports/schedule_health.template.md."""
    by_action: dict[str, int] = {}
    for p in proposals:
        by_action[p["action"]] = by_action.get(p["action"], 0) + 1
    promotable = sum(1 for p in proposals if p["promotable"])
    clusters = sorted(proposals, key=lambda p: p["occurrences"], reverse=True)
    dq = [p for p in proposals if p["cause"] == "data_quality"]
    dq_occ = sum(p["occurrences"] for p in dq)
    dq_high = sum(p["occurrences"] for p in dq if p["confidence"] == "high")

    lines = [
        f"# 排产健康日报 — {stamp}",
        "",
        "> Generated by run_pipeline.py (reference). Reporting only; eval set untouched.",
        "",
        "## At a glance",
        f"- snapshot rows: {rows_total}",
        f"- new candidates (deduped): {len(candidates)}",
        f"- proposals — promotable: {promotable} · report-only: "
        f"{by_action.get('report_only', 0)} · flaky: {by_action.get('mark_flaky', 0)}",
        "",
        "## Top clusters",
        "| cluster | intent | occ | cause | risk | action |",
        "|---|---|---|---|---|---|",
    ]
    for p in clusters:
        lines.append(
            f"| {p['cluster']} | {p['intent']} | {p['occurrences']} | "
            f"{p['cause']} | {p['risk']} | {p['action']} |"
        )
    lines += [
        "",
        "## Data quality",
        "| issue | occurrences | high-confidence |",
        "|---|---|---|",
        f"| MISPLACED_DURATION | {dq_occ} | {dq_high} |",
        "",
        "## Recommended actions",
    ]
    for p in clusters:
        if p["action"] == "promote":
            lines.append(f"- [ ] promote `{p['cluster']}` (occ {p['occurrences']}) — `needs-human` (run /promote-proposals)")
        elif p["cause"] == "data_quality":
            lines.append(f"- [ ] data fix `{p['cluster']}` (occ {p['occurrences']}) — `auto-safe` if high-confidence → migration")
        elif p["action"] == "mark_flaky":
            lines.append(f"- [ ] quarantine flaky `{p['cluster']}` — needs fixture before it can be an eval case")
    lines.append("")
    lines.append("## Pointers")
    lines.append(f"- snapshot: `data/prod_snapshots/{stamp}/`")
    lines.append(f"- candidates: `eval/candidates/{stamp}.jsonl` · proposals: `eval/proposals/{stamp}.jsonl`")

    path = ROOT / "reports" / "schedule_health" / f"{stamp}.md"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def run(stamp: str) -> dict:
    hard_before = CONFIG["HARD_CASES_FILE"].read_text(encoding="utf-8")
    snap = pull_snapshot(stamp)
    rows_total = len(load_jsonl(snap / "audit_log.jsonl"))
    candidates, proposals = analyze(snap, stamp)
    write_jsonl(ROOT / "eval" / "candidates" / f"{stamp}.jsonl", candidates)
    write_jsonl(ROOT / "eval" / "proposals" / f"{stamp}.jsonl", proposals)
    report = write_report(stamp, candidates, proposals, rows_total)
    # Invariant: the daily loop NEVER mutates the eval set.
    assert CONFIG["HARD_CASES_FILE"].read_text(encoding="utf-8") == hard_before, \
        "daily loop must not modify the eval set"
    return {
        "snapshot_dir": snap,
        "rows_total": rows_total,
        "candidates": candidates,
        "proposals": proposals,
        "report": report,
    }


def self_check(res: dict) -> None:
    """Deterministic assertions on the demo fixture — proves the loop behaves."""
    props = {p["cluster"]: p for p in res["proposals"]}
    assert res["rows_total"] == 13, res["rows_total"]
    assert len(res["candidates"]) == 4, [c["error_code"] for c in res["candidates"]]
    assert "NEG_SETUP" not in props, "a10 should be dropped (already in the eval set)"
    assert props["OVERLAP_CONSTRAINT"]["occurrences"] == 4
    assert props["OVERLAP_CONSTRAINT"]["action"] == "promote"
    assert props["OVERLAP_CONSTRAINT"]["risk"] == "critical"
    assert props["MISPLACED_DURATION"]["action"] == "report_only"
    assert props["MISPLACED_DURATION"]["cause"] == "data_quality"
    assert props["TIMEOUT"]["action"] == "mark_flaky"
    assert props["WRONG_RESULT"]["promotable"] is False
    print("self-check: OK (dedupe, covered-drop, triage, read-only eval set)")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--date", default=date_cls.today().strftime("%Y%m%d"))
    ap.add_argument("--self-check", action="store_true")
    args = ap.parse_args()

    res = run(args.date)
    print(f"snapshot rows : {res['rows_total']}")
    print(f"candidates    : {len(res['candidates'])}")
    counts: dict[str, int] = {}
    for p in res["proposals"]:
        counts[p["action"]] = counts.get(p["action"], 0) + 1
    print(f"proposals     : {counts}")
    print(f"report        : {res['report'].relative_to(ROOT)}")
    if args.self_check:
        self_check(res)
    return 0


if __name__ == "__main__":
    sys.exit(main())
