"""Replay and audit BugleCat host verification logs (.verification.jsonl).

Reads verification records, inspects claim-level decisions, detects agent drift
(e.g., claiming success despite non-zero exit codes, stale evidence, or scope exaggerations),
and reports Jev semantic observations.
"""

import argparse
import json
import sys
from collections import Counter
from pathlib import Path


def inspect_record(record: dict) -> dict:
    session_id = record.get("session_id", "unknown")
    turn_id = record.get("turn_id")
    mode = record.get("mode", "unknown")
    result = record.get("result", {})

    verdict = result.get("verdict", "unknown")
    reason_code = result.get("reason_code", "unknown")
    call_status = result.get("call_status", "not_called")
    decisions = result.get("decisions", [])
    observations = result.get("observations", [])

    contradictions = []
    need_evidence = []
    passed_claims = []

    for d in decisions:
        claim_id = d.get("claim_id", "")
        v = d.get("verdict", "")
        rc = d.get("reason_code", "")
        eids = d.get("evidence_ids", [])
        entry = {"claim_id": claim_id, "verdict": v, "reason_code": rc, "evidence_ids": eids}
        if v == "contradiction":
            contradictions.append(entry)
        elif v == "need_evidence":
            need_evidence.append(entry)
        elif v == "pass":
            passed_claims.append(entry)

    return {
        "session_id": session_id,
        "turn_id": turn_id,
        "mode": mode,
        "verdict": verdict,
        "reason_code": reason_code,
        "call_status": call_status,
        "total_claims": len(decisions),
        "contradictions": contradictions,
        "need_evidence": need_evidence,
        "passed_claims": passed_claims,
        "jev_observations_count": len(observations),
        "observations": observations,
    }


def analyze_files(paths: list[Path]) -> dict:
    records = []
    for path in paths:
        if not path.is_file():
            continue
        try:
            for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                line = line.strip()
                if not line:
                    continue
                data = json.loads(line)
                if data.get("schema") != "buglecat.verification.record.v2":
                    continue
                info = inspect_record(data)
                info["file"] = str(path)
                info["line"] = line_no
                records.append(info)
        except Exception as err:
            print(f"Warning: could not parse {path}: {err}", file=sys.stderr)

    verdict_counter = Counter(r["verdict"] for r in records)
    mode_counter = Counter(r["mode"] for r in records)
    reason_counter = Counter(r["reason_code"] for r in records)
    call_status_counter = Counter(r["call_status"] for r in records)

    all_contradictions = []
    for r in records:
        if r["contradictions"]:
            all_contradictions.append({
                "session_id": r["session_id"],
                "turn_id": r["turn_id"],
                "file": r["file"],
                "line": r["line"],
                "contradictions": r["contradictions"],
            })

    all_drift_cases = []
    for r in records:
        drift_claims = [c for c in r["contradictions"] if c["reason_code"] == "explicit_failure"]
        drift_claims.extend([c for c in r["need_evidence"] if c["reason_code"] in {"missing_evidence", "stale_evidence", "scope_mismatch"}])
        if drift_claims:
            all_drift_cases.append({
                "session_id": r["session_id"],
                "turn_id": r["turn_id"],
                "file": r["file"],
                "line": r["line"],
                "drift_claims": drift_claims,
            })

    return {
        "total_records": len(records),
        "modes": dict(mode_counter),
        "verdicts": dict(verdict_counter),
        "reason_codes": dict(reason_counter),
        "call_status": dict(call_status_counter),
        "contradictions_found": len(all_contradictions),
        "drift_alerts": len(all_drift_cases),
        "drift_cases": all_drift_cases,
        "contradiction_details": all_contradictions,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path, help="Path(s) to .verification.jsonl file(s) or directories")
    parser.add_argument("--json", action="store_true", help="Output summary as JSON")
    parser.add_argument("--strict", action="store_true", help="Exit with 1 if contradictions or drift alerts are found")
    args = parser.parse_args()

    files = []
    for p in args.paths:
        if p.is_dir():
            files.extend(sorted(p.rglob("*.verification.jsonl")))
        elif p.is_file():
            files.append(p)
        else:
            print(f"Error: path not found: {p}", file=sys.stderr)

    if not files:
        print("No .verification.jsonl files found.", file=sys.stderr)
        return 0

    report = analyze_files(files)

    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(f"=== BugleCat Verification Replay Report ===")
        print(f"Files scanned: {len(files)}")
        print(f"Total verification records: {report['total_records']}")
        print(f"Modes: {report['modes']}")
        print(f"Verdicts: {report['verdicts']}")
        print(f"Reason codes: {report['reason_codes']}")
        print(f"Call status: {report['call_status']}")
        print(f"Contradictions found: {report['contradictions_found']}")
        print(f"Agent drift alerts: {report['drift_alerts']}")

        if report["drift_cases"]:
            print(f"\n--- Drift Alert Details ({len(report['drift_cases'])} cases) ---")
            for c in report["drift_cases"][:10]:
                print(f"  Session: {c['session_id']}, Turn: {c['turn_id']} ({c['file']}:{c['line']})")
                for claim in c["drift_claims"]:
                    print(f"    - Claim {claim['claim_id']}: {claim['reason_code']} (verdict: {claim['verdict']})")

    if args.strict and (report["contradictions_found"] > 0 or report["drift_alerts"] > 0):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
