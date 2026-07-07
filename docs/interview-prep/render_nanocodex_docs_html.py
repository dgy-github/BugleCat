#!/usr/bin/env python3
"""Render the remaining nanocodex-*.md interview docs into standalone HTML,
one input .md -> one output .html, same visual convention as
render_portrait_html.py (dark theme, collapsible <details> per H2 section).
Deterministic markdown->HTML via python-markdown; no content is retyped.

Excludes: nanocodex-portrait.zh.md (has its own dedicated renderer) and
portrait_parts/*.md (already fully represented inside nanocodex-portrait.zh-CN.html).
"""
from __future__ import annotations

import html
import re
from pathlib import Path

import markdown

HERE = Path(__file__).resolve().parent
MD_EXT = ["tables", "fenced_code", "sane_lists"]

TARGETS = [
    "nanocodex-drive-engineering-interview-drills.zh.md",
    "nanocodex-interview-final.zh.md",
    "nanocodex-interview-pack.zh.md",
    "nanocodex-interview-project-intro.zh-CN.md",
    "nanocodex-interview-video-script.zh-CN.md",
    "nanocodex-module-followups.zh.md",
    "nanocodex-portrait-cram.zh.md",
]


def out_path(md_name: str) -> Path:
    stem = md_name
    for suf in (".zh-CN.md", ".zh.md", ".md"):
        if stem.endswith(suf):
            stem = stem[: -len(suf)]
            break
    return HERE / f"{stem}.zh-CN.html"


def md_to_html(src: str) -> str:
    return markdown.markdown(src, extensions=MD_EXT)


def first_line_peek(body_md: str, limit: int = 70) -> str:
    for line in body_md.split("\n"):
        line = line.strip().lstrip("*-—- ")
        if not line or line.startswith("#"):
            continue
        text = re.sub(r"\*\*(.+?)\*\*", r"\1", line)
        text = re.sub(r"`([^`]+)`", r"\1", text)
        text = re.sub(r"^>\s*", "", text)
        return text[:limit] + ("…" if len(text) > limit else "")
    return ""


def split_h2_sections(text: str) -> tuple[str, list[tuple[str, str]]]:
    """Return (preamble_before_first_h2, [(h2_title, body_md), ...])."""
    parts = re.split(r"^## (.+)$", text, flags=re.M)
    preamble = parts[0]
    sections = []
    for i in range(1, len(parts), 2):
        title = parts[i].strip()
        body = parts[i + 1] if i + 1 < len(parts) else ""
        sections.append((title, body))
    return preamble, sections


TEMPLATE = """<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
  :root{{--bg:#0d1117;--fg:#e6edf3;--mut:#8a919c;--card:#161b22;--line:#30363d;--acc:#1f6feb;
    --q:#3fb950;--l:#d29922;--r:#f85149;--p:#8957e5}}
  *{{box-sizing:border-box}}
  body{{background:var(--bg);color:var(--fg);font:15px/1.65 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;
    max-width:900px;margin:0 auto;padding:26px 20px 90px}}
  h1{{font-size:23px;margin:.2em 0}}
  h2{{font-size:17px;margin:1.2em 0 .4em}}
  h3{{font-size:15px;color:#cbd3db;margin:1.1em 0 .3em}}
  blockquote{{color:var(--mut);font-size:13.5px;border-left:3px solid var(--line);margin:.6em 0;padding:2px 14px;
    background:var(--card);border-radius:0 8px 8px 0}}
  blockquote p{{margin:.4em 0}}
  code{{background:#21262d;padding:1px 6px;border-radius:5px;font-size:.9em;color:#e6edf3}}
  pre{{background:#0a0d12;border:1px solid var(--line);border-radius:8px;padding:10px 14px;overflow-x:auto}}
  a{{color:#58a6ff}}
  .toc{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 16px;font-size:13.5px;margin:12px 0}}
  .toc a{{margin:2px 10px 2px 0;display:inline-block}}
  table{{border-collapse:collapse;width:100%;font-size:13px;margin:.6em 0}}
  th,td{{border:1px solid var(--line);padding:5px 9px;text-align:left}}
  th{{background:#1b2129}}
  ul,ol{{padding-left:1.4em}}
  li{{margin:.25em 0}}
  hr{{border:none;border-top:1px solid var(--line);margin:1.1em 0}}
  details.c{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:0;overflow:hidden;margin:10px 0}}
  details.c>summary{{cursor:pointer;padding:13px 16px;list-style:none;display:flex;align-items:baseline;gap:10px;flex-wrap:wrap}}
  details.c>summary::-webkit-details-marker{{display:none}}
  details.c[open]>summary{{border-bottom:1px solid var(--line);background:#1b2129}}
  .t{{font-weight:700;font-size:15.5px}}
  .peek{{color:var(--mut);font-size:12.5px}}
  .body{{padding:2px 18px 14px}}
  .btns{{margin:10px 0 16px}}
  .btns button{{background:#21262d;color:var(--fg);border:1px solid var(--line);border-radius:7px;padding:6px 14px;
    font-size:13px;cursor:pointer;margin-right:8px}}
</style>

<h1>{h1}</h1>
{intro_html}

<div class="toc"><b>目录：</b><br>{toc_html}</div>

<div class="btns">
  <button onclick="document.querySelectorAll('details.c').forEach(d=>d.open=true)">全部展开</button>
  <button onclick="document.querySelectorAll('details.c').forEach(d=>d.open=false)">全部折叠</button>
</div>

{body_html}
"""


def render_one(md_path: Path) -> Path:
    raw = md_path.read_text(encoding="utf-8")
    lines = raw.split("\n")
    h1 = lines[0].lstrip("# ").strip() if lines and lines[0].startswith("# ") else md_path.stem
    preamble, sections = split_h2_sections("\n".join(lines[1:]))
    intro_html = md_to_html(preamble)

    toc_entries = []
    body_parts = []
    for title, body in sections:
        anchor = re.sub(r"[^\w一-鿿]+", "-", title).strip("-")
        peek = first_line_peek(body)
        toc_entries.append(f'<a href="#{anchor}">{html.escape(title)}</a>')
        section_html = md_to_html(body)
        body_parts.append(
            f'<details class="c" id="{anchor}">\n'
            f'<summary><span class="t">{html.escape(title)}</span>'
            f'<div class="peek">{html.escape(peek)}</div></summary>\n'
            f'<div class="body">\n{section_html}\n</div>\n'
            f"</details>\n"
        )

    page = TEMPLATE.format(
        title=html.escape(h1),
        h1=html.escape(h1),
        intro_html=intro_html,
        toc_html="\n".join(toc_entries),
        body_html="\n".join(body_parts),
    )
    out = out_path(md_path.name)
    out.write_text(page, encoding="utf-8")
    return out


def main() -> None:
    for name in TARGETS:
        md_path = HERE / name
        if not md_path.exists():
            print(f"SKIP (not found): {name}")
            continue
        out = render_one(md_path)
        print(f"wrote {out.name} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
