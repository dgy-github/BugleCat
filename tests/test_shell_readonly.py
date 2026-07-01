"""The shell tool's read-only heuristic must not be fooled by command chaining.

Under `read-only` sandbox mode, a command classified read-only runs WITHOUT an
approval prompt. The classifier must therefore be conservative: a write hidden
behind `&&`, a redirect, or command substitution must NOT pass.
"""

from __future__ import annotations

from nanocodex.tools.shell import _looks_read_only


def test_plain_read_only_commands_pass():
    for cmd in ("ls", "ls -la", "cat foo.py", "git status", "git diff HEAD~1",
                "rg pattern src", "pwd", "git log --oneline"):
        assert _looks_read_only(cmd), cmd


def test_plain_writes_do_not_pass():
    for cmd in ("rm -rf build", "mkdir x", "git commit -m x", "pip install foo"):
        assert not _looks_read_only(cmd), cmd


def test_command_chain_with_write_does_not_pass():
    # The core bug: a read-only leading token must not whitelist a chained write.
    for cmd in ("ls && rm -rf x", "cat a; rm b", "pwd || mkdir hack",
                "ls & rm -rf x", "ls\nrm -rf x"):
        assert not _looks_read_only(cmd), cmd


def test_all_segments_read_only_passes():
    # Every chained segment is read-only -> the whole line is read-only.
    assert _looks_read_only("git log --oneline | head")
    assert _looks_read_only("cat a && ls && pwd")
    assert _looks_read_only("grep foo . | wc -l")


def test_redirection_does_not_pass():
    # `cat x > y` writes y despite the read-only-looking leading token.
    for cmd in ("cat a > out.txt", "echo hi >> log", "ls > files.txt", "cat a &> b"):
        assert not _looks_read_only(cmd), cmd


def test_command_substitution_does_not_pass():
    for cmd in ("echo $(rm -rf x)", "cat `rm x`", "diff <(ls) <(rm y)"):
        assert not _looks_read_only(cmd), cmd


def test_arbitrary_code_runners_not_assumed_read_only():
    # python -c / node -e can write/network; they were removed from the allowlist.
    assert not _looks_read_only("python -c \"open('x','w').write('1')\"")
    assert not _looks_read_only("node -e \"require('fs').writeFileSync('x','1')\"")


def test_prefix_lookalike_does_not_pass():
    # "lsof" must not match the "ls" prefix (old startswith bug).
    assert not _looks_read_only("lsof -i :8080")
    assert not _looks_read_only("catalog-build")


def test_empty_is_not_read_only():
    assert not _looks_read_only("")
    assert not _looks_read_only("   ")
