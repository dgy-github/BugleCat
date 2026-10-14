"""Check rustfmt across workspace and GUI crates while preserving frozen evaluation files."""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

FROZEN_EXAMPLE_SHA256 = {
    "rust/crates/ncx-core/examples/verification_apps_v3_jev_development.rs": (
        "18b308acaabdda73be1bb16b3358fdcd61d5e87d9a69e36bf210b798dcb74d9d"
    ),
    "rust/crates/ncx-core/examples/verification_artifact_v2_development.rs": (
        "fb3a1ed9ee0062136687429ce7a2c9d356e2858d4204dce25ee75f0353b0194e"
    ),
    "rust/crates/ncx-core/examples/verification_artifact_v2_formal.rs": (
        "70421c1cc20bfb2cde2986a69296edfcc4e90c5616062234160a192915ca8864"
    ),
    "rust/crates/ncx-core/examples/verification_connectivity_probe.rs": (
        "5efd6227552c0d3722d428d1f8cf4a124caad6c9c8cd02e36b217421682294e6"
    ),
}

MANIFESTS = (
    "rust/Cargo.toml",
    "rust/gui/src-tauri/Cargo.toml",
)


def normalize_relpath(raw_line: str, root: Path = ROOT) -> str:
    cleaned = raw_line.strip()
    if cleaned.startswith("\\\\?\\"):
        cleaned = cleaned[4:]
    candidate = Path(cleaned)
    if candidate.is_absolute():
        try:
            candidate = candidate.resolve().relative_to(root.resolve())
        except ValueError:
            return cleaned.replace("\\", "/")
    return candidate.as_posix()


def verify_frozen_hashes(root: Path = ROOT, frozen: dict[str, str] = FROZEN_EXAMPLE_SHA256) -> list[str]:
    mismatches = []
    for relpath, expected in frozen.items():
        target = root / relpath
        if not target.is_file():
            mismatches.append(f"{relpath}: missing")
            continue
        actual = hashlib.sha256(target.read_bytes()).hexdigest()
        if actual != expected:
            mismatches.append(f"{relpath}: expected {expected}, got {actual}")
    return mismatches


def run_cargo_fmt_check(manifest: str, root: Path = ROOT) -> tuple[int, list[str], str]:
    proc = subprocess.run(
        ["cargo", "fmt", "--manifest-path", manifest, "--all", "--", "--check", "-l"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    flagged = [
        normalize_relpath(line, root)
        for line in proc.stdout.splitlines()
        if line.strip()
    ]
    return proc.returncode, flagged, proc.stderr.strip()


def evaluate_workspace_fmt(root: Path = ROOT) -> dict:
    hash_mismatches = verify_frozen_hashes(root)
    unformatted_active = set()
    frozen_seen = set()
    stderr_errors = []

    for manifest in MANIFESTS:
        code, flagged, stderr = run_cargo_fmt_check(manifest, root)
        if stderr:
            stderr_errors.append(f"{manifest}: {stderr}")
        if code != 0 and not flagged and not stderr:
            stderr_errors.append(f"{manifest}: cargo fmt exited with {code}")
        for relpath in flagged:
            if relpath in FROZEN_EXAMPLE_SHA256:
                frozen_seen.add(relpath)
            else:
                unformatted_active.add(relpath)

    ok = not hash_mismatches and not unformatted_active and not stderr_errors
    return {
        "ok": ok,
        "manifests_checked": list(MANIFESTS),
        "frozen_verified": len(FROZEN_EXAMPLE_SHA256) - len(hash_mismatches),
        "frozen_flagged_by_rustfmt": sorted(frozen_seen),
        "hash_mismatches": hash_mismatches,
        "unformatted_active_files": sorted(unformatted_active),
        "stderr_errors": stderr_errors,
    }


def main() -> int:
    report = evaluate_workspace_fmt(ROOT)
    print(json.dumps(report, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
