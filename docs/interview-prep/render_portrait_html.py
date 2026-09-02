#!/usr/bin/env python3
"""Render nanocodex-portrait.zh.md into a single self-contained HTML doc
(nanocodex-portrait.zh-CN.html): connection map flat on top, each of the 9
subsystem cards as a collapsible <details>, same dark theme/pattern as the
other flowchart HTML docs in this folder. Deterministic markdown->HTML via
python-markdown, no content is retyped or summarized by a model call.
"""
from __future__ import annotations

import html
import re
from pathlib import Path

import markdown

HERE = Path(__file__).resolve().parent
SRC = HERE / "nanocodex-portrait.zh.md"
OUT = HERE / "nanocodex-portrait.zh-CN.html"

MD_EXT = ["tables", "fenced_code", "sane_lists"]

ICONS = {
    "Harness 工程管理": "🔄",
    "上下文压缩 · context editing": "🗜",
    "工具系统 · 动态暴露": "🔧",
    "沙箱 · 审批状态机": "🚪",
    "主子 agent 如何通讯": "🤝",
    "项目记忆 · 自进化": "🧠",
    "Skills · MCP · 视觉": "🧩",
    "ncx-forge · 骨架训练框架": "🧬",
    "定时器设计 · 离线调度": "🕒",
}


def md_to_html(src: str) -> str:
    return markdown.markdown(src, extensions=MD_EXT)


def one_liner(body_md: str) -> str:
    """Grab the '一句话主线' paragraph as the <summary> peek text."""
    m = re.search(r"### 一句话主线.*?\n+(.+?)(?:\n\n|\n###)", body_md, re.S)
    if not m:
        return ""
    text = re.sub(r"\*\*(.+?)\*\*", r"\1", m.group(1))
    text = re.sub(r"`([^`]+)`", r"\1", text)
    text = text.replace("\n", "")
    return text[:70] + ("…" if len(text) > 70 else "")


def split_sections(text: str) -> list[tuple[str, str]]:
    """Split on top-level '## ' headers, return [(title, body_md), ...]."""
    parts = re.split(r"^## (.+)$", text, flags=re.M)
    # parts[0] is preamble before first "##"; then alternating title/body
    sections = []
    for i in range(1, len(parts), 2):
        title = parts[i].strip()
        body = parts[i + 1] if i + 1 < len(parts) else ""
        sections.append((title, body))
    return sections


def main() -> None:
    raw = SRC.read_text(encoding="utf-8")
    preamble, _, rest = raw.partition("## 目录")
    sections = split_sections(raw)

    # preamble (title + usage note before "## 目录")
    preamble_lines = [line for line in preamble.split("\n") if line.strip()]
    title_line = preamble_lines[0].lstrip("# ").strip()
    intro_md = "\n".join(line for line in preamble_lines[1:])
    intro_html = md_to_html(intro_md)

    toc_entries = []
    body_html_parts = []

    for title, body in sections:
        if title == "目录":
            continue
        anchor = re.sub(r"[^\w一-鿿]+", "-", title).strip("-")
        is_connection_map = "连接图" in title
        peek = "" if is_connection_map else one_liner(body)
        icon = ICONS.get(title, "📌")

        if is_connection_map:
            toc_entries.append(f'<a href="#{anchor}">🗺 {html.escape(title)}</a>')
            section_html = md_to_html(body)
            body_html_parts.append(
                f'<section id="{anchor}" class="flat">\n'
                f"<h2>🗺 {html.escape(title)}</h2>\n"
                f"{section_html}\n"
                f"</section>\n"
            )
        else:
            toc_entries.append(f'<a href="#{anchor}">{icon} {html.escape(title)}</a>')
            section_html = md_to_html(body)
            body_html_parts.append(
                f'<details class="c" id="{anchor}">\n'
                f'<summary><span class="t">{icon} {html.escape(title)}</span>'
                f'<div class="peek">{html.escape(peek)}</div></summary>\n'
                f'<div class="body">\n{section_html}\n</div>\n'
                f"</details>\n"
            )

    toc_html = "\n".join(toc_entries)
    body_html = "\n".join(body_html_parts)

    page = f"""<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{html.escape(title_line)}</title>
<style>
  :root{{--bg:#0d1117;--fg:#e6edf3;--mut:#8a919c;--card:#161b22;--line:#30363d;--acc:#1f6feb;
    --q:#3fb950;--l:#d29922;--r:#f85149;--p:#8957e5}}
  *{{box-sizing:border-box}}
  body{{background:var(--bg);color:var(--fg);font:15px/1.65 -apple-system,"Segoe UI","Microsoft YaHei",sans-serif;
    max-width:900px;margin:0 auto;padding:26px 20px 90px}}
  h1{{font-size:23px;margin:.2em 0}}
  h2{{font-size:18px;margin:1.4em 0 .5em;border-left:4px solid var(--acc);padding-left:10px}}
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
  section.flat{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:6px 18px 14px;margin:10px 0}}
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

<h1>🧭 {html.escape(title_line)}</h1>
{intro_html}

<div class="toc"><b>目录：</b><br>{toc_html}</div>

<div class="btns">
  <button onclick="document.querySelectorAll('details.c').forEach(d=>d.open=true)">全部展开</button>
  <button onclick="document.querySelectorAll('details.c').forEach(d=>d.open=false)">全部折叠</button>
</div>

{body_html}
"""
    OUT.write_text(page, encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes), {len(sections)-1} cards")


if __name__ == "__main__":
    main()
