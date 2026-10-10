"""Prepare local, blinded human review without exporting raw transcripts or scores.

Legacy execution labels are clues about recorded exits, never completion labels.
The private source files stay local; show reads them on demand after hash checking.
"""

import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path

from .history_inventory import inventory
from .trials import check_runs

SCHEMA = "buglecat.verification.review-dataset.v1"
LABELS = {"supported", "unsupported", "unknown", "unlabeled"}


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def text_content(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(p.get("text", "") for p in content if isinstance(p, dict))
    return ""


def safe_preview(text: str, limit: int = 2400) -> str:
    """Local review convenience, not a guarantee to find arbitrary secrets."""
    if re.search(r"(?i)PRIVATE KEY|set-cookie|\bcookie\s*[:=]", text):
        return "[sensitive content withheld; inspect the original locally]"
    replacements = [
        (r"(?i)\b(?:apikey_|sk-|sk_)[A-Za-z0-9_-]+", "[credential]"),
        (r"(?i)\bBearer\s+\S+", "Bearer [credential]"),
        (r"(?i)\b(?:api[_-]?key|password|token|secret)\s*[:=]\s*[^\s,;]+", "[secret assignment]"),
        (r"https?://[^\s<>\"']+", "[url]"),
        (r"[A-Za-z]:[\\/][^\r\n<>\"']+", "[local path]"),
        (r"\b[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}\b", "[email]"),
        (r"\b[A-Za-z0-9_-]{40,}\b", "[opaque value]"),
    ]
    for pattern, replacement in replacements:
        text = re.sub(pattern, replacement, text)
    return text[:limit] + ("\n[preview truncated]" if len(text) > limit else "")


def read_source(source: Path):
    raw = source.read_bytes()
    messages = [json.loads(line) for line in raw.decode("utf-8").splitlines() if line.strip()]
    return digest(raw), messages


def content_kind(candidate: str) -> str:
    value = candidate.lstrip()
    if value.startswith("[Runtime syntax verification]"):
        return "embedded_host_note"
    if value.startswith(("<｜｜DSML", "<|DSML")):
        return "tool_markup"
    if re.match(r"(?:\[error\]\s*)?(?:HTTP \d{3}\s*\||StreamError:)", value):
        return "provider_fault"
    return "assistant_narrative"


def source_cases(source: Path) -> tuple[list[dict], dict]:
    source_hash, messages = read_source(source)
    facts = inventory(source)
    if facts["source_sha256"] != source_hash:
        raise ValueError("source changed during preparation")
    by_turn = {}
    for fact in facts["rows"]:
        by_turn.setdefault(fact["turn"], []).append(fact)
    cases = []
    turn = 0
    prior_tools = []
    for index, message in enumerate(messages):
        if message.get("role") == "user":
            turn += 1
            prior_tools = []
        if message.get("role") == "tool":
            prior_tools.append(index)
        candidate = text_content(message.get("content"))
        # A candidate is the actual assistant prose after tools, not an invented
        # success claim or the model's tool invocation markup.
        if (message.get("role") != "assistant" or message.get("tool_calls")
                or not candidate.strip() or not prior_tools
                or content_kind(candidate) != "assistant_narrative"):
            continue
        evidence = [row for row in by_turn.get(turn, []) if row["source_index"] in prior_tools]
        candidate_hash = digest(candidate.encode())
        case_id = digest(f"{source_hash}:{index}:{candidate_hash}".encode())[:24]
        cases.append({
            "id": case_id, "group_id": source_hash, "split": "development",
            "category": "semantic", "label": "unlabeled",
            "label_basis": "Completion truth requires review of the actual candidate and its scope.",
            "review_status": "pending", "source_kind": "real_legacy_transcript",
            "author_exposed": True, "author_id": "codex-development-20261010",
            "source_sha256": source_hash, "source_index": index, "turn": turn,
            "candidate_sha256": candidate_hash, "candidate_chars": len(candidate),
            "evidence": evidence,
            "scope_status": "not certified: legacy lacks durable scope and file snapshots",
            "completion_label_inferred_from_exit": False,
        })
    return cases, {k: facts[k] for k in (
        "source_sha256", "message_n", "turn_n", "tool_result_n", "execution_counts")}


def trial_sources(reports: list[Path]) -> tuple[list[Path], dict]:
    sources, groups = [], {}
    for report in reports:
        raw = report.read_bytes()
        for line in raw.decode("utf-8").splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            if row.get("schema") != "buglecat.verification.closed-loop-trial.v1":
                raise ValueError("only recorded closed-loop reports accepted")
            source = Path(row["session_log"])
            source_hash, _ = read_source(source)
            group = digest(f"{row['manifest_hash']}:{row['group']}".encode())
            info = {"group_id": group, "task_artifact_assertions": {
                "report_sha256": digest(raw), "task_id": row["case_id"],
                "grader_passed": row["grader_passed"],
                "protected_unchanged": row["protected_unchanged"],
                "check_exit_sequence": check_runs(source),
                "scope": "recorded development task artifact only; not candidate truth",
                "completion_label_inferred": False}}
            if source_hash in groups and groups[source_hash] != info:
                raise ValueError("conflicting task group for source")
            groups[source_hash] = info
            sources.append(source)
    return sources, groups


def prepare(sources: list[Path], trial_reports: list[Path] | None = None) -> dict:
    trial_paths, group_info = trial_sources(trial_reports or [])
    cases, summaries, seen = [], [], set()
    for source in [*sources, *trial_paths]:
        rows, summary = source_cases(source)
        if summary["source_sha256"] in seen:
            continue
        seen.add(summary["source_sha256"])
        for row in rows:
            row.update(group_info.get(summary["source_sha256"], {}))
        cases.extend(rows)
        summaries.append(summary)
    return {"schema": SCHEMA, "purpose": "local independent completion annotation queue",
            "privacy": "hashes/positions/exit metadata only; source prose and commands stay local",
            "scores_hidden": True, "labels_independent_of_jev": True,
            "holdout_status": "none: all historical preparation is development; no blind claim",
            "sources": summaries, "cases": cases}


def review_template(dataset: dict) -> dict:
    return {"schema": "buglecat.verification.human-review.v1",
            "dataset_sha256": digest(json.dumps(dataset, sort_keys=True).encode()),
            "reviewer_id": "", "reviewer_kind": "human", "reviews": [
                {"id": c["id"], "candidate_sha256": c["candidate_sha256"],
                 "source_sha256": c["source_sha256"], "label": "unlabeled",
                 "label_basis": "", "evidence_indices": [],
                 "scope_reviewed": False, "full_candidate_reviewed": False}
                for c in dataset["cases"]]}


def merge_reviews(dataset: dict, reviews: dict) -> dict:
    expected = digest(json.dumps(dataset, sort_keys=True).encode())
    if reviews.get("dataset_sha256") != expected:
        raise ValueError("review dataset changed")
    reviewer = reviews.get("reviewer_id", "").strip()
    kind = reviews.get("reviewer_kind")
    if not reviewer or kind not in {"human", "model"}:
        raise ValueError("named human or model reviewer required")
    cases = {c["id"]: dict(c) for c in dataset["cases"]}
    seen = set()
    for review in reviews.get("reviews", []):
        case_id = review["id"]
        if case_id not in cases or case_id in seen:
            raise ValueError("foreign or duplicate review")
        seen.add(case_id)
        case = cases[case_id]
        if reviewer == case["author_id"]:
            raise ValueError("self review cannot be independent")
        for name in ("candidate_sha256", "source_sha256"):
            if review.get(name) != case[name]:
                raise ValueError("review source or candidate changed")
        label = review.get("label")
        if label not in LABELS:
            raise ValueError("invalid label")
        if label == "unlabeled":
            continue
        if not review.get("label_basis", "").strip():
            raise ValueError("label requires an evidence rationale")
        evidence = {e["source_index"] for e in case["evidence"]}
        refs = review.get("evidence_indices", [])
        if not refs or any(index not in evidence for index in refs):
            raise ValueError("review requires evidence from this candidate")
        if not review.get("scope_reviewed") or not review.get("full_candidate_reviewed"):
            raise ValueError("exit status alone or a preview cannot label completion")
        case.update(label=label, label_basis=review["label_basis"],
                    review_status=f"{kind}_reviewed", reviewer_id=reviewer,
                    review_evidence_indices=refs)
    return {**dataset, "cases": list(cases.values()),
            "review_counts": dict(Counter(c["review_status"] for c in cases.values())),
            "independent_human_review": kind == "human",
            "review_note": "model review is a separate signal, not human approval or executable proof"}


def show(dataset: dict, sources: list[Path], case_id: str) -> dict:
    case = next(c for c in dataset["cases"] if c["id"] == case_id)
    for source in sources:
        source_hash, messages = read_source(source)
        if source_hash != case["source_sha256"]:
            continue
        candidate = text_content(messages[case["source_index"]].get("content"))
        if digest(candidate.encode()) != case["candidate_sha256"]:
            raise ValueError("candidate changed")
        return {"id": case_id, "candidate_preview": safe_preview(candidate),
                "preview_only": True, "evidence": case["evidence"],
                "warning": "Full original candidate/scope must be reviewed locally before labeling."}
    raise ValueError("matching immutable local source not supplied")


def write_new(path: Path, value: dict):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    prep = sub.add_parser("prepare")
    prep.add_argument("sources", nargs="+", type=Path)
    prep.add_argument("--trial-reports", nargs="+", type=Path, default=[])
    prep.add_argument("--output", required=True, type=Path)
    prep.add_argument("--template", required=True, type=Path)
    inspect = sub.add_parser("show")
    inspect.add_argument("--dataset", required=True, type=Path)
    inspect.add_argument("--sources", nargs="+", required=True, type=Path)
    inspect.add_argument("--case", required=True)
    merge = sub.add_parser("merge")
    merge.add_argument("--dataset", required=True, type=Path)
    merge.add_argument("--reviews", required=True, type=Path)
    merge.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.action == "prepare":
        if args.output.exists() or args.template.exists():
            raise FileExistsError("use new queue and template paths")
        data = prepare(args.sources, args.trial_reports)
        write_new(args.output, data)
        write_new(args.template, review_template(data))
        print(json.dumps({"candidate_n": len(data["cases"]), "source_n": len(data["sources"]),
                          "review_status": "pending", "new_jev_calls": 0}))
    elif args.action == "show":
        print(json.dumps(show(json.loads(args.dataset.read_text(encoding="utf-8")),
                              args.sources, args.case), ensure_ascii=False, indent=2))
    else:
        data = merge_reviews(json.loads(args.dataset.read_text(encoding="utf-8")),
                             json.loads(args.reviews.read_text(encoding="utf-8")))
        write_new(args.output, data)
        print(json.dumps(data["review_counts"]))


if __name__ == "__main__":
    main()
