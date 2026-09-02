"""Tests for A/B configuration comparison (ab_compare).

Pure logic (dataclasses, summaries, deterministic naming) tests with plain
data. The thin git I/O helpers test against a REAL throwaway git repo built in
tmp_path — no mocks, so the worktree/diff/adopt/cleanup contract is verified
end to end the way the GUI will drive it.
"""

from __future__ import annotations

import shutil
import subprocess

import pytest

from nanocodex.agent.ab_compare import (
    ABConfig,
    ABGitError,
    ABResult,
    adopt_diff,
    build_result,
    cleanup_worktree,
    collect_worktree_diff,
    create_worktree,
    ensure_clean_git_workspace,
    format_ab_comparison,
    summarize_result,
    worktree_name,
)

# --- a duck-typed stand-in for loop.TurnResult ----------------------------

class _FakeTurn:
    def __init__(self, text="", stop_reason="completed", iterations=1, usage=None):
        self.text = text
        self.stop_reason = stop_reason
        self.iterations = iterations
        self.usage = usage or {}


# --- pure: build_result ----------------------------------------------------

def test_build_result_prices_known_model():
    cfg = ABConfig(label="A", overrides={"model": "deepseek-v4-pro"})
    turn = _FakeTurn(text="done", stop_reason="completed", iterations=3,
                     usage={"prompt_tokens": 1000, "completion_tokens": 500})
    r = build_result(cfg, turn, elapsed_s=1.234, diff="x", worktree_path="/tmp/wt")
    assert r.label == "A"
    assert r.text == "done"
    assert r.iterations == 3
    assert r.cost is not None and r.cost > 0   # priced against a known model
    assert r.elapsed_s == 1.23                 # rounded to 2dp


def test_build_result_unknown_model_cost_none():
    cfg = ABConfig(label="B", overrides={"model": "totally-made-up-model"})
    turn = _FakeTurn(usage={"prompt_tokens": 100, "completion_tokens": 50})
    r = build_result(cfg, turn, elapsed_s=0.5, diff="", worktree_path="")
    assert r.cost is None                       # unknown price -> None, not $0.00


def test_build_result_no_model_cost_none():
    cfg = ABConfig(label="A", overrides={})     # no model in overrides
    turn = _FakeTurn(usage={"prompt_tokens": 10})
    r = build_result(cfg, turn, elapsed_s=0.0, diff="", worktree_path="")
    assert r.cost is None


# --- pure: summaries -------------------------------------------------------

def test_summarize_result_happy():
    r = ABResult(label="A", stop_reason="completed", iterations=2, cost=0.0123,
                 elapsed_s=1.5, diff="+++ b/x\n+added line\n-removed line\n")
    s = summarize_result(r)
    assert "[A]" in s
    assert "stop=completed" in s
    assert "steps=2" in s
    assert "$" in s                             # cost rendered


def test_summarize_result_error():
    r = ABResult(label="B", error="boom")
    s = summarize_result(r)
    assert s == "[B] ERROR: boom"


def test_diff_stat_counts_lines():
    # +++/--- file headers must NOT be counted as add/remove lines.
    diff = "+++ b/file\n--- a/file\n+one\n+two\n-three\n"
    r = ABResult(label="A", stop_reason="completed", diff=diff)
    s = summarize_result(r)
    assert "+2 -1 lines" in s


def test_diff_stat_no_changes():
    r = ABResult(label="A", stop_reason="completed", diff="")
    assert "no file changes" in summarize_result(r)


def test_format_ab_comparison_has_both_sides():
    a = ABResult(label="A", stop_reason="completed", iterations=1)
    b = ABResult(label="B", stop_reason="max_iterations", iterations=9)
    out = format_ab_comparison(a, b)
    assert "[A]" in out and "[B]" in out
    assert "A/B comparison" in out


# --- pure: worktree naming -------------------------------------------------

def test_worktree_name_deterministic_and_safe():
    n1 = worktree_name("A", "12345")
    n2 = worktree_name("A", "12345")
    assert n1 == n2                             # deterministic
    assert n1.startswith("nanocodex-ab-")


def test_worktree_name_sanitizes_unsafe_chars():
    n = worktree_name("../evil", "a/b c")
    assert "/" not in n and "\\" not in n and " " not in n


# --- git I/O against a real throwaway repo ---------------------------------

def _git(args, cwd):
    subprocess.run(["git", *args], cwd=str(cwd), check=True,
                   capture_output=True, text=True)


def _has_git() -> bool:
    return shutil.which("git") is not None


@pytest.fixture
def clean_repo(tmp_path):
    """A clean git repo with one committed file, for worktree tests."""
    repo = tmp_path / "repo"
    repo.mkdir()
    _git(["init"], repo)
    _git(["config", "user.email", "t@t.test"], repo)
    _git(["config", "user.name", "Test"], repo)
    (repo / "hello.txt").write_text("original\n", encoding="utf-8")
    _git(["add", "-A"], repo)
    _git(["commit", "-m", "init"], repo)
    return repo


@pytest.mark.skipif(not _has_git(), reason="git not installed")
def test_ensure_clean_workspace_returns_head(clean_repo):
    head = ensure_clean_git_workspace(clean_repo)
    assert head and len(head) >= 7              # a commit hash


@pytest.mark.skipif(not _has_git(), reason="git not installed")
def test_ensure_clean_rejects_dirty(clean_repo):
    (clean_repo / "hello.txt").write_text("changed\n", encoding="utf-8")
    with pytest.raises(ABGitError):
        ensure_clean_git_workspace(clean_repo)


@pytest.mark.skipif(not _has_git(), reason="git not installed")
def test_ensure_clean_rejects_non_git(tmp_path):
    plain = tmp_path / "plain"
    plain.mkdir()
    with pytest.raises(ABGitError):
        ensure_clean_git_workspace(plain)


@pytest.mark.skipif(not _has_git(), reason="git not installed")
def test_worktree_diff_adopt_cleanup_roundtrip(clean_repo, tmp_path):
    """The full path the GUI drives: make worktrees, edit, diff, adopt, clean."""
    base = ensure_clean_git_workspace(clean_repo)
    tmp_root = tmp_path / "wts"
    tmp_root.mkdir()

    wt_a = create_worktree(clean_repo, base, worktree_name("A", "t"), tmp_root)
    wt_b = create_worktree(clean_repo, base, worktree_name("B", "t"), tmp_root)

    # Each side edits the same file differently + adds a new file.
    (wt_a / "hello.txt").write_text("from A\n", encoding="utf-8")
    (wt_a / "new_a.txt").write_text("only in A\n", encoding="utf-8")
    (wt_b / "hello.txt").write_text("from B\n", encoding="utf-8")

    diff_a = collect_worktree_diff(wt_a)
    diff_b = collect_worktree_diff(wt_b)
    assert "from A" in diff_a and "new_a.txt" in diff_a
    assert "from B" in diff_b
    # Isolation: A's changes never leak into B's diff.
    assert "from A" not in diff_b and "new_a.txt" not in diff_b

    # Adopt A onto the real repo; B's changes must NOT appear.
    adopt_diff(clean_repo, diff_a)
    assert (clean_repo / "hello.txt").read_text(encoding="utf-8") == "from A\n"
    assert (clean_repo / "new_a.txt").exists()

    # Cleanup removes the worktrees.
    cleanup_worktree(clean_repo, wt_a)
    cleanup_worktree(clean_repo, wt_b)
    listing = subprocess.run(["git", "worktree", "list"], cwd=str(clean_repo),
                             capture_output=True, text=True).stdout
    assert "nanocodex-ab-A" not in listing and "nanocodex-ab-B" not in listing


@pytest.mark.skipif(not _has_git(), reason="git not installed")
def test_adopt_empty_diff_is_noop(clean_repo):
    before = (clean_repo / "hello.txt").read_text(encoding="utf-8")
    adopt_diff(clean_repo, "")                  # nothing to apply
    assert (clean_repo / "hello.txt").read_text(encoding="utf-8") == before


@pytest.mark.skipif(not _has_git(), reason="git not installed")
def test_cleanup_worktree_never_raises(clean_repo, tmp_path):
    # Removing a path that isn't a worktree is best-effort, must not raise.
    cleanup_worktree(clean_repo, tmp_path / "does-not-exist")
