import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_rustfmt", ROOT / "scripts/check_rustfmt.py")
check_rustfmt = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(check_rustfmt)


def test_frozen_example_hashes_match_repository():
    assert check_rustfmt.verify_frozen_hashes(ROOT) == []


def test_normalize_relpath_strips_windows_extended_prefix():
    raw = "\\\\?\\" + str(ROOT / "rust/crates/ncx-core/src/lib.rs")
    assert check_rustfmt.normalize_relpath(raw, ROOT) == "rust/crates/ncx-core/src/lib.rs"


def test_evaluate_workspace_fmt_rejects_unformatted_active_file(monkeypatch):
    monkeypatch.setattr(
        check_rustfmt,
        "run_cargo_fmt_check",
        lambda manifest, root=ROOT: (
            1,
            [
                "rust/crates/ncx-core/examples/verification_artifact_v2_formal.rs",
                "rust/crates/ncx-core/src/lib.rs",
            ],
            "",
        ),
    )
    report = check_rustfmt.evaluate_workspace_fmt(ROOT)
    assert not report["ok"]
    assert report["unformatted_active_files"] == ["rust/crates/ncx-core/src/lib.rs"]


def test_evaluate_workspace_fmt_rejects_tampered_frozen_file(tmp_path):
    mismatches = check_rustfmt.verify_frozen_hashes(
        tmp_path,
        {"rust/crates/ncx-core/examples/verification_artifact_v2_formal.rs": "0" * 64},
    )
    assert len(mismatches) == 1
