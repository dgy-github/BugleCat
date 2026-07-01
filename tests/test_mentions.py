"""@file mention expansion: pull referenced files inline, ignore non-files."""

from __future__ import annotations

from nanocodex.agent.mentions import expand_file_mentions, find_mentions


def test_find_mentions_basic_and_trailing_punct():
    assert find_mentions("look at @src/a.py and @b.txt.") == ["src/a.py", "b.txt"]
    # @ not at a word boundary (e.g. email) is not a mention.
    assert find_mentions("mail me@example.com") == []


def test_expand_inlines_existing_file(tmp_path):
    f = tmp_path / "hello.py"
    f.write_text("print('hi')\n", encoding="utf-8")
    out = expand_file_mentions("explain @hello.py please", tmp_path)
    assert "explain @hello.py please" in out          # original text preserved
    assert '<file path="hello.py">' in out
    assert "print('hi')" in out


def test_nonexistent_mention_is_left_alone(tmp_path):
    out = expand_file_mentions("see @nope.py", tmp_path)
    assert out == "see @nope.py"                       # unchanged, no block


def test_dedup_and_multiple(tmp_path):
    (tmp_path / "a.txt").write_text("AAA", encoding="utf-8")
    (tmp_path / "b.txt").write_text("BBB", encoding="utf-8")
    out = expand_file_mentions("@a.txt @b.txt @a.txt", tmp_path)
    assert out.count('<file path="a.txt">') == 1       # de-duplicated
    assert '<file path="b.txt">' in out


def test_binary_file_skipped(tmp_path):
    b = tmp_path / "img.bin"
    b.write_bytes(b"\xff\xfe\x00\x01\x02")
    out = expand_file_mentions("@img.bin", tmp_path)
    assert out == "@img.bin"                           # non-UTF-8 -> ignored


def test_large_file_truncated(tmp_path):
    big = tmp_path / "big.txt"
    big.write_text("x" * 60_000, encoding="utf-8")
    out = expand_file_mentions("@big.txt", tmp_path)
    assert "... (truncated)" in out
