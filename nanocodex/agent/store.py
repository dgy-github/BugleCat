"""Durable, crash-tolerant persistence for AgentState.

The review's high-severity persistence finding: state.json and the checkpoint
files are written by separate steps, so a crash between them leaves state.json
disagreeing with what actually happened. The fix is a write order plus a
reconcile-on-load:

1. A checkpoint is written FIRST, atomically (temp file + ``os.replace``).
2. state.json is then written atomically.

``os.replace`` is atomic on both POSIX and Windows, so a reader never sees a
half-written file. On load we reconcile: any checkpoint on disk whose node is
not marked terminal in state.json is surfaced, so the orchestrator can trust the
checkpoint directory as the source of truth for "what work actually finished".

Layout (under ``<workspace>/.nanocodex``):
    state.json
    checkpoints/<checkpoint_id>.json
"""

from __future__ import annotations

import json
import os
from datetime import datetime
from pathlib import Path

from nanocodex.agent.state import AgentCheckpoint, AgentState


class AgentStateStore:
    """Reads/writes :class:`AgentState` under a workspace's ``.nanocodex`` dir."""

    def __init__(self, state_dir: Path) -> None:
        self.state_dir = Path(state_dir)
        self.checkpoints_dir = self.state_dir / "checkpoints"
        self.state_path = self.state_dir / "state.json"

    def _ensure_dirs(self) -> None:
        self.checkpoints_dir.mkdir(parents=True, exist_ok=True)

    @staticmethod
    def _atomic_write_json(path: Path, payload: dict) -> None:
        """Write JSON via temp file + atomic rename so readers never see a tear.

        The temp file sits in the same directory as the target so ``os.replace``
        is a same-filesystem rename (atomic). fsync keeps the bytes from lingering
        only in the page cache when the process is about to die.
        """
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_name(path.name + ".tmp")
        data = json.dumps(payload, ensure_ascii=False, indent=2)
        with tmp.open("w", encoding="utf-8") as fh:
            fh.write(data)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp, path)  # atomic on POSIX and Windows

    # --- checkpoints ------------------------------------------------------
    def write_checkpoint(self, ckpt: AgentCheckpoint) -> Path:
        """Persist one checkpoint atomically. Call BEFORE saving state."""
        self._ensure_dirs()
        path = self.checkpoints_dir / f"{ckpt.id}.json"
        self._atomic_write_json(path, ckpt.to_dict())
        return path

    def load_checkpoints(self) -> list[AgentCheckpoint]:
        """All checkpoints on disk, oldest first (by created_at then id)."""
        if not self.checkpoints_dir.is_dir():
            return []
        out: list[AgentCheckpoint] = []
        for p in self.checkpoints_dir.glob("*.json"):
            try:
                d = json.loads(p.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                continue  # tolerate a half-written/corrupt checkpoint
            try:
                out.append(AgentCheckpoint.from_dict(d))
            except (KeyError, ValueError):
                continue
        out.sort(key=lambda c: (c.created_at, c.id))
        return out

    # --- state ------------------------------------------------------------
    def save(self, state: AgentState) -> None:
        """Persist state.json atomically (checkpoints must already be on disk)."""
        self._ensure_dirs()
        self._atomic_write_json(self.state_path, state.to_dict())

    def exists(self) -> bool:
        return self.state_path.is_file()

    def load(self) -> AgentState | None:
        """Load state.json, reconciled against the checkpoint directory.

        Reconciliation handles the crash window between "checkpoint written" and
        "state.json written": any checkpoint whose node is missing from state is
        re-attached so the recovered state knows that node produced output. We do
        NOT auto-mark such nodes `done` (the verifier gate owns that decision);
        we only ensure the evidence isn't lost.
        """
        if not self.state_path.is_file():
            return None
        try:
            d = json.loads(self.state_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return None
        state = AgentState.from_dict(d)

        known = {c.id for c in state.checkpoints}
        for ckpt in self.load_checkpoints():
            if ckpt.id not in known:
                # state.json was written before this checkpoint was recorded into
                # it (or never was) — re-attach so recovery sees the work.
                state.checkpoints.append(ckpt)
                known.add(ckpt.id)
        return state

    # --- helpers ----------------------------------------------------------
    def new_checkpoint(
        self, state: AgentState, node_id: str, summary: str, **fields
    ) -> AgentCheckpoint:
        """Mint a checkpoint with a deterministic id and an ISO timestamp.

        Persistence order is the caller's responsibility: write the checkpoint,
        append it to ``state.checkpoints``, then ``save(state)``.
        """
        ckpt_id = f"ckpt-{state.next_seq():04d}-{node_id}"
        return AgentCheckpoint(
            id=ckpt_id,
            node_id=node_id,
            created_at=datetime.now().isoformat(timespec="seconds"),
            summary=summary,
            files_touched=list(fields.get("files_touched", [])),
            tests_run=list(fields.get("tests_run", [])),
            artifacts=dict(fields.get("artifacts", {})),
            open_risks=list(fields.get("open_risks", [])),
        )
