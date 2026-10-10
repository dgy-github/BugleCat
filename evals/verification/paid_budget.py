"""Conservative cross-process reservations before explicit paid requests.

A reserved group cannot be retried, even after interruption. Reservations are
not a claim that HTTP happened: actual attempts come from transport receipts.
"""

import hashlib
import json
import os
from pathlib import Path

from .calibration import digest, load, require


def reserve(authorization, kind, group, request_hash):
    require(kind in {"jev", "main_model"}, "unknown paid service")
    auth_path = Path(authorization).resolve()
    auth_bytes = auth_path.read_bytes()
    auth_hash = hashlib.sha256(auth_bytes).hexdigest()
    auth = load(auth_path)
    settings = auth[kind]
    require(settings["max_attempts"] == 400 and auth["automatic_retries"] == 0, "unsupported authorization")
    require(isinstance(group, str) and bool(group), "missing group")
    require(isinstance(request_hash, str) and len(request_hash) == 64
            and all(c in "0123456789abcdef" for c in request_hash), "invalid request hash")
    root = Path(settings["ledger_dir"]).resolve()
    root.mkdir(parents=True, exist_ok=True)
    # Exclusive owner for the short reservation transaction. A stale lock blocks
    # spending; it is never silently removed or retried by this module.
    lock = root / "reservation.lock"
    descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    try:
        os.write(descriptor, str(os.getpid()).encode())
        os.fsync(descriptor)
        metadata = root / "authorization.json"
        if metadata.exists():
            require(load(metadata)["authorization_sha256"] == auth_hash, "authorization changed")
        else:
            durable_new(metadata, {"authorization_sha256": auth_hash, "kind": kind})
        reservations = root / "reservations"
        reservations.mkdir(exist_ok=True)
        existing = list(reservations.glob("*.json"))
        require(len(existing) < settings["max_attempts"], "paid request budget exhausted")
        target = reservations / (digest(group) + ".json")
        value = {"schema": "buglecat.verification.paid-reservation.v1", "kind": kind,
                 "group_id": group, "request_hash": request_hash,
                 "authorization_sha256": auth_hash, "reservation_index": len(existing) + 1,
                 "status": "reserved_before_transport", "automatic_retries": 0}
        durable_new(target, value)
        return {**value, "path": str(target)}
    finally:
        os.close(descriptor)
        lock.unlink()


def durable_new(path, value):
    with Path(path).open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(value, sort_keys=True) + "\n")
        stream.flush()
        os.fsync(stream.fileno())
