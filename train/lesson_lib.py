#!/usr/bin/env python3
"""Harness² Meta-Memory Lesson Library (`train/lesson_lib.py`).

Implements harnessing-level recursion across `ncx-forge` runs:
- Stores structured lessons (`when / trigger -> edit_pattern -> why`) with
  empirical `support` and `refute` counters in `train/runs/lessons.jsonl`.
- Retrieves top-k active lessons by `symptom_overlap * (support+1)/(support+refute+2)`.
- Retires lessons automatically when `refute >= support + 2`, surfacing them as
  falsified rules so the proposer does not repeat refuted patterns.
- Enforces the same anti-memorization forbidden-term filter as `critic_check`
  so benchmark task IDs never leak into lessons.
"""
from __future__ import annotations

import hashlib
import json
import re
import time
from pathlib import Path

GRADER_FORBIDDEN = ("check.py", "_check.py", "hidden test", "eval_holdout")


def _norm_text(s: str) -> str:
    return " ".join((s or "").lower().split())


def task_signature(traj: str, task_family: str = "python-bugfix") -> dict:
    """Extract deterministic symptom tags, keywords, and a short signature hash
    from an agent execution trajectory (never includes the task name)."""
    low = (traj or "").lower()
    symptoms: list[str] = []
    if "timeout" in low or "timed out" in low or "did not finish within" in low:
        symptoms.append("timeout")
    if "produced no usable session log" in low or not low.strip():
        symptoms.append("no_usable_log")
    if low.count("pytest") >= 3 or low.count("unittest") >= 3:
        symptoms.append("repeated full-suite pytest")
    if "pytest" not in low and "unittest" not in low and "cargo test" not in low:
        symptoms.append("no_test_run")
    if "apply_patch" in low and ("failed" in low or "error" in low or "conflict" in low):
        symptoms.append("patch_failed")
    if "syntaxerror" in low or "indentationerror" in low:
        symptoms.append("syntax_error")

    # Extract tool call sequence to detect repetitive tool loops.
    tools = re.findall(r"-\s+([a-zA-Z0-9_]+)\(", traj or "")
    if len(tools) >= 6 and len(set(tools[-4:])) == 1:
        symptoms.append("repeated_tool_call")
    if not symptoms:
        symptoms.append("general_failure")

    words = re.findall(r"[a-z_][a-z0-9_]{2,}", low)
    stop = {
        "the", "and", "for", "with", "that", "this", "from", "into", "tool",
        "calls", "agent", "made", "final", "message", "file", "line", "true",
        "false", "none",
    }
    keywords: list[str] = []
    for w in words:
        if w not in stop and w not in keywords:
            keywords.append(w)
        if len(keywords) >= 16:
            break

    raw_sig = json.dumps({"s": symptoms, "k": keywords[:8]}, sort_keys=True)
    sig_hash = hashlib.sha1(raw_sig.encode("utf-8")).hexdigest()[:12]
    return {
        "symptoms": symptoms,
        "keywords": keywords,
        "task_family": task_family,
        "sig_hash": sig_hash,
    }


def _contains_forbidden(text: str, forbidden_terms: list[str] | None) -> bool:
    low = (text or "").lower()
    for marker in GRADER_FORBIDDEN:
        if marker in low:
            return True
    for term in (forbidden_terms or []):
        t = term.strip().lower()
        if not t:
            continue
        if len(t) >= 4:
            if re.search(rf"\b{re.escape(t)}\b", low):
                return True
        elif t in low:
            return True
    return False


class LessonLibrary:
    """Persistent JSONL store of Harness² meta-lessons."""

    def __init__(self, path: Path | str, *, adopted_path: Path | str | None = None):
        self.path = Path(path)
        self._adopted_path = Path(adopted_path) if adopted_path else None

    @property
    def adopted_path(self) -> Path:
        return self._adopted_path or (self.path.parent / "adopted_harnesses.jsonl")

    def load_all(self) -> list[dict]:
        if not self.path.exists():
            return []
        out: list[dict] = []
        for raw in self.path.read_text(encoding="utf-8", errors="replace").splitlines():
            raw = raw.strip()
            if not raw:
                continue
            try:
                out.append(json.loads(raw))
            except json.JSONDecodeError:
                continue
        return out

    def save_all(self, records: list[dict]) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        lines = [json.dumps(r, ensure_ascii=False) for r in records]
        self.path.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")

    def retrieve(self, sig: dict, k: int = 5) -> list[dict]:
        """Return top-k active lessons ranked by symptom/keyword overlap times
        Beta-smoothed empirical reliability `(support + 1) / (support + refute + 2)`."""
        records = [r for r in self.load_all() if r.get("status", "active") == "active"]
        if not records or k <= 0:
            return []
        q_symptoms = set(sig.get("symptoms") or [])
        q_keywords = set(sig.get("keywords") or [])
        scored: list[tuple[float, dict]] = []
        for r in records:
            trig = r.get("trigger") or {}
            l_symptoms = set(trig.get("symptoms") or [])
            sym_overlap = len(q_symptoms & l_symptoms)
            pat_words = set(re.findall(r"[a-z_][a-z0-9_]{2,}", _norm_text(
                f"{r.get('edit_pattern', '')} {r.get('why', '')}"
            )))
            kw_overlap = len(q_keywords & pat_words)
            support = int(r.get("support", 1))
            refute = int(r.get("refute", 0))
            posterior = (support + 1.0) / (support + refute + 2.0)
            relevance = sym_overlap * 1.0 + min(kw_overlap, 4) * 0.25 + 0.5
            scored.append((relevance * posterior, r))
        scored.sort(key=lambda pair: pair[0], reverse=True)
        return [r for _, r in scored[:k]]

    def retired_lessons(self, limit: int = 5) -> list[dict]:
        """Return recently retired lessons (`refute >= support + 2`) to inject as
        falsified patterns."""
        retired = [r for r in self.load_all() if r.get("status") == "retired"]
        return retired[-limit:]

    def record_lesson(
        self,
        draft: dict,
        *,
        stamp: str,
        round_idx: int = 0,
        round_num: int | None = None,
        task_sig: str | dict = "",
        surfaces: list[str] | None = None,
        margin: int = 0,
        token_saving: float = 0.0,
        forbidden_terms: list[str] | None = None,
    ) -> dict | None:
        """Validate and persist a lesson draft after its composed genome passes
        the outer train + holdout gate (`support=1`). Returns the lesson record
        dict or `None` if rejected by anti-memorization validation."""
        if not isinstance(draft, dict):
            return None
        rnd = round_num if round_num is not None else round_idx
        sig_str = (
            str(task_sig.get("sig_hash", ""))
            if isinstance(task_sig, dict)
            else str(task_sig)
        )
        edit_pattern = str(draft.get("edit_pattern") or "").strip()
        why = str(draft.get("why") or "").strip()
        if not edit_pattern:
            return None
        trig = draft.get("trigger") if isinstance(draft.get("trigger"), dict) else {}
        raw_syms = trig.get("symptoms") or draft.get("symptoms") or []
        if not raw_syms and isinstance(task_sig, dict):
            raw_syms = task_sig.get("symptoms") or []
        symptoms = [str(s).strip() for s in raw_syms if str(s).strip()]
        raw_surfs = trig.get("surfaces") or draft.get("surfaces") or surfaces or []
        trig_surfaces = [
            str(s).strip() for s in raw_surfs if str(s).strip()
        ]
        task_family = str(
            trig.get("task_family") or draft.get("task_family") or "python-bugfix"
        ).strip()

        blob = f"{edit_pattern}\n{why}\n{' '.join(symptoms)}"
        if _contains_forbidden(blob, forbidden_terms):
            return None

        records = self.load_all()
        norm_pat = _norm_text(edit_pattern)
        ev_entry = {
            "stamp": stamp,
            "round": rnd,
            "task_sig": sig_str,
            "verdict": "accepted",
            "margin": margin,
            "token_saving": round(float(token_saving), 1),
        }

        # Compaction: merge into existing lesson if normalized edit_pattern matches.
        for r in records:
            if _norm_text(r.get("edit_pattern", "")) == norm_pat:
                r["support"] = int(r.get("support", 1)) + 1
                r.setdefault("evidence", []).append(ev_entry)
                existing_trig = r.setdefault("trigger", {})
                merged_sym = list(dict.fromkeys((existing_trig.get("symptoms") or []) + symptoms))
                merged_surf = list(
                    dict.fromkeys((existing_trig.get("surfaces") or []) + trig_surfaces)
                )
                existing_trig["symptoms"] = merged_sym
                existing_trig["surfaces"] = merged_surf
                if int(r.get("refute", 0)) < int(r["support"]) + 2:
                    r["status"] = "active"
                self.save_all(records)
                return r

        lesson_id = f"L-{len(records) + 1:04d}"
        new_rec = {
            "id": lesson_id,
            "created": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "stamp": stamp,
            "trigger": {
                "symptoms": symptoms or ["general_failure"],
                "surfaces": trig_surfaces,
                "task_family": task_family,
            },
            "edit_pattern": edit_pattern[:300],
            "why": why[:300],
            "evidence": [ev_entry],
            "support": 1,
            "refute": 0,
            "status": "active",
        }
        records.append(new_rec)
        self.save_all(records)
        return new_rec

    def update_outcomes(
        self,
        lesson_ids: list[str],
        *,
        accepted: bool,
        stamp: str = "",
        round_idx: int = 0,
        round_num: int | None = None,
        task_sig: str = "",
        task_sig_hash: str = "",
        margin: int = 0,
        token_saving: float = 0.0,
        reason: str = "",
    ) -> None:
        """Update `support` / `refute` for retrieved lessons used in a compose
        attempt, retiring any lesson where `refute >= support + 2`."""
        if not lesson_ids:
            return
        rnd = round_num if round_num is not None else round_idx
        sig_str = task_sig_hash or task_sig
        id_set = set(lesson_ids)
        records = self.load_all()
        changed = False
        for r in records:
            if r.get("id") not in id_set:
                continue
            changed = True
            if accepted:
                r["support"] = int(r.get("support", 1)) + 1
            else:
                r["refute"] = int(r.get("refute", 0)) + 1
            r.setdefault("evidence", []).append({
                "stamp": stamp,
                "round": rnd,
                "task_sig": sig_str,
                "verdict": "accepted" if accepted else "rejected",
                "reason": reason,
                "margin": margin,
                "token_saving": round(float(token_saving), 1),
            })
            if int(r.get("refute", 0)) >= int(r.get("support", 1)) + 2:
                r["status"] = "retired"
        if changed:
            self.save_all(records)

    def refute_by_surface(
        self, surface: str | list[str], *, stamp: str = ""
    ) -> list[str]:
        """Increment `refute` on active lessons targeting `surface` (or any surface
        in a list) when ablation pruning (`--prune`) removes that surface."""
        target_set = {surface} if isinstance(surface, str) else set(surface or [])
        if not target_set:
            return []
        records = self.load_all()
        affected: list[str] = []
        for r in records:
            if r.get("status", "active") != "active":
                continue
            surfaces = set((r.get("trigger") or {}).get("surfaces") or [])
            if surfaces & target_set:
                r["refute"] = int(r.get("refute", 0)) + 1
                affected.append(str(r.get("id")))
                if int(r["refute"]) >= int(r.get("support", 1)) + 2:
                    r["status"] = "retired"
        if affected:
            self.save_all(records)
        return affected

    def record_adopted_harness(
        self,
        genome_path: Path | str = "",
        lesson_ids: list[str] | None = None,
        stamp: str = "",
        round_idx: int = 0,
        *,
        round_num: int | None = None,
        surfaces: list[str] | None = None,
        lessons_used: list[str] | None = None,
        train_passes: int = 0,
        holdout_passes: int = 0,
        mean_tokens: float = 0.0,
    ) -> None:
        self.adopted_path.parent.mkdir(parents=True, exist_ok=True)
        rnd = round_num if round_num is not None else round_idx
        used = lessons_used if lessons_used is not None else (lesson_ids or [])
        entry = {
            "stamp": stamp,
            "round": rnd,
            "genome": str(genome_path),
            "surfaces": list(surfaces or []),
            "lessons_used": list(used),
            "train_passes": train_passes,
            "holdout_passes": holdout_passes,
            "mean_tokens": mean_tokens,
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        }
        with self.adopted_path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(entry, ensure_ascii=False) + "\n")

    def bootstrap_from_ledgers(self, runs_dir: Path | str) -> int:
        """Cold-start lessons from historical `ledger_*.jsonl` accepted entries."""
        rdir = Path(runs_dir)
        if not rdir.exists():
            return 0
        added = 0
        for ledger_file in sorted(rdir.glob("ledger_*.jsonl")):
            stamp = ledger_file.stem.replace("ledger_", "")
            for raw in ledger_file.read_text(encoding="utf-8", errors="replace").splitlines():
                raw = raw.strip()
                if not raw:
                    continue
                try:
                    row = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                if row.get("verdict") != "accepted":
                    continue
                surfaces = row.get("target_surfaces") or []
                reason = str(row.get("reason") or "")
                diff_preview = str(row.get("diff") or "")[:200]
                if not surfaces or not diff_preview:
                    continue
                lid = self.record_lesson(
                    {
                        "trigger": {"symptoms": ["general_failure"], "surfaces": surfaces},
                        "edit_pattern": f"Refine {', '.join(surfaces)}",
                        "why": f"Accepted in ledger ({reason})",
                    },
                    stamp=stamp,
                    round_idx=int(row.get("round", 0)),
                    task_sig="ledger_bootstrap",
                    surfaces=surfaces,
                )
                if lid:
                    added += 1
        return added
