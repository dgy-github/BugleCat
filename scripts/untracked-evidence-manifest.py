"""Inventory and verify local evaluation evidence that is deliberately kept out of git.

The evidence (raw collections, frozen run directories, screenshots, release
preflight copies) is too large for the repository, but tracked tests and
status files reference it by exact path, and parts of it are hash-frozen.
So it stays in place, ignored by git, and this manifest records what must
exist and with which bytes.

    python scripts/untracked-evidence-manifest.py verify   # default
    python scripts/untracked-evidence-manifest.py write    # re-inventory (only after a deliberate change)
"""
from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "evals" / "verification" / "untracked_evidence_manifest.json"
EVIDENCE_ROOTS = (
    "reports/",
    "evals/verification/reports/",
    "evals/golden/reports/",
    "evals/verification/data/",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


EVIDENCE_RULE_MARKER = "untracked-evidence"


def _git(*args: str, stdin: bytes | None = None) -> list[str]:
    out = subprocess.run(
        ["git", *args], cwd=ROOT, input=stdin, capture_output=True, check=False
    ).stdout
    return [p.decode("utf-8") for p in out.split(b"\0") if p]


def untracked_evidence() -> list[str]:
    """Untracked files under the evidence roots that only the `.gitignore`
    block tagged `untracked-evidence` hides. Files that the general rules
    already ignore (session stores, logs, build output) are out of scope."""
    with tempfile.TemporaryDirectory() as tmp:
        general = Path(tmp) / "general.gitignore"
        general.write_text("\n".join(_rules_without_evidence_block()) + "\n", encoding="utf-8")
        # No --exclude-per-directory: it would re-read the root .gitignore
        # (evidence block included). The evidence roots hold no nested ones.
        args = ["ls-files", "--others", "-z", f"--exclude-from={general}"]
        info_exclude = ROOT / ".git" / "info" / "exclude"
        if info_exclude.is_file():
            args.append(f"--exclude-from={info_exclude}")
        return sorted(_git(*args, "--", *EVIDENCE_ROOTS))


def _rules_without_evidence_block() -> list[str]:
    lines = (ROOT / ".gitignore").read_text(encoding="utf-8").splitlines()
    kept, inside = [], False
    for line in lines:
        if line.startswith("#") and EVIDENCE_RULE_MARKER in line:
            inside = True
            continue
        if inside and not line.strip():
            inside = False
        if not inside:
            kept.append(line)
    return kept


def write() -> int:
    files = [
        {"path": rel, "bytes": (ROOT / rel).stat().st_size, "sha256": sha256(ROOT / rel)}
        for rel in untracked_evidence()
    ]
    MANIFEST.write_text(
        json.dumps(
            {
                "schema": "buglecat.untracked_evidence_manifest.v1",
                "roots": list(EVIDENCE_ROOTS),
                "file_count": len(files),
                "total_bytes": sum(f["bytes"] for f in files),
                "files": files,
            },
            ensure_ascii=False,
            indent=1,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {MANIFEST.relative_to(ROOT)}: {len(files)} files")
    return 0


def verify() -> int:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    missing, changed = [], []
    for entry in manifest["files"]:
        path = ROOT / entry["path"]
        if not path.is_file():
            missing.append(entry["path"])
        elif path.stat().st_size != entry["bytes"] or sha256(path) != entry["sha256"]:
            changed.append(entry["path"])
    print(
        f"{manifest['file_count']} files in manifest: "
        f"{len(missing)} missing, {len(changed)} changed"
    )
    for label, items in (("missing", missing), ("changed", changed)):
        for item in items[:20]:
            print(f"  {label}: {item}")
    return 1 if missing or changed else 0


if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "verify"
    sys.exit(write() if mode == "write" else verify())
