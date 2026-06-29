#!/usr/bin/env python3
"""Render docs/design_data.json (subsystem design notes) into a self-contained,
interview-grade HTML doc: docs/nanocodex-design.html.

Data is the structured output of the nanocodex-design-readers workflow (8 deep
reads, one per subsystem). This generator owns layout/styling only — content is
the readers' verified notes, so the doc stays faithful + regenerable.
"""
from __future__ import annotations

import html
import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE / "design_data.json"
OUT = HERE / "nanocodex-design.html"

# (match-key in the subsystem string, clean title, section id) in reading order.
ORDER = [
    ("harness", "The Harness — Agent Turn Loop", "harness"),
    ("context-compression", "Context Compression — Context Editing", "context"),
    ("tools", "Tool System & Dynamic Exposure", "tools"),
    ("sandbox", "Sandbox & Approval State Machine", "sandbox"),
    ("orchestrator", "Tiered Orchestrator (flash / pro)", "orch"),
    ("memory", "Project Memory — Self-Evolution", "memory"),
    ("skills", "Skills · MCP · Vision", "skills"),
    ("ncx-forge", "ncx-forge — Harness Training Framework", "forge"),
]


def esc(s: str) -> str:
    """HTML-escape, then turn `code` spans into <code>."""
    s = html.escape(str(s), quote=True)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    return s


def classify(rec: dict):
    # Match the LONGEST key contained in the subsystem string, so 'ncx-forge'
    # wins over 'harness' (its description says "harness-evolution").
    s = rec.get("subsystem", "").lower()
    best = None
    for i, (key, title, sid) in enumerate(ORDER):
        if key in s and (best is None or len(key) > len(best[3])):
            best = (i, title, sid, key)
    if best:
        return best[0], best[1], best[2]
    return 99, rec.get("subsystem", "?"), "x"


def section(rec: dict, idx: int, title: str, sid: str) -> str:
    mechs = "".join(
        f'<div class="mech"><div class="mech-name">{esc(m["name"])}</div>'
        f'<div class="mech-detail">{esc(m["detail"])}</div></div>'
        for m in rec.get("key_mechanisms", [])
    )
    steps = "".join(f"<li>{esc(s)}</li>" for s in rec.get("flow_steps", []))
    talk = "".join(f"<li>{esc(t)}</li>" for t in rec.get("interview_talking_points", []))
    gotchas = "".join(f"<li>{esc(g)}</li>" for g in rec.get("tradeoffs_or_gotchas", []))
    refs = "".join(f"<code class='ref'>{esc(r)}</code>" for r in rec.get("code_refs", []))
    return f"""
<section id="{sid}">
  <h2><span class="num">{idx + 1}</span>{esc(title)}</h2>
  <p class="oneliner">{esc(rec.get("one_liner", ""))}</p>

  <h3>How it works</h3>
  <p>{esc(rec.get("how_it_works", ""))}</p>

  <div class="callout why"><div class="callout-h">Design rationale — why this way</div>
    <p>{esc(rec.get("design_rationale", ""))}</p></div>

  <h3>Key mechanisms</h3>
  <div class="mechs">{mechs}</div>

  <h3>Control / data flow</h3>
  <ol class="flow">{steps}</ol>

  <div class="grid2">
    <div class="callout talk"><div class="callout-h">★ Interview talking points</div><ul>{talk}</ul></div>
    <div class="callout trade"><div class="callout-h">Trade-offs &amp; gotchas</div><ul>{gotchas}</ul></div>
  </div>

  <h3>Code references</h3>
  <div class="refs">{refs}</div>
</section>"""


def build() -> str:
    recs = json.loads(DATA.read_text(encoding="utf-8"))
    ordered = sorted(((classify(r), r) for r in recs), key=lambda x: x[0][0])
    nav = "".join(f'<a href="#{sid}">{esc(title)}</a>'
                  for (_, title, sid), _ in ordered)
    body = "".join(section(r, i, title, sid)
                   for (i, ((_, title, sid), r)) in enumerate(ordered))
    return f"""<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>nanocodex — Design &amp; Architecture</title>
<style>
 :root{{--bg:#0f1216;--panel:#161b22;--ink:#e6edf3;--mut:#9aa7b4;--acc:#6ea8fe;--acc2:#e8a33d;
        --line:#2a313a;--code:#1c2330;--ok:#3fb950;--warn:#d29922}}
 *{{box-sizing:border-box}}
 body{{margin:0;background:var(--bg);color:var(--ink);font:15px/1.65 -apple-system,Segoe UI,Roboto,sans-serif}}
 .wrap{{display:grid;grid-template-columns:260px 1fr;max-width:1280px;margin:0 auto}}
 nav{{position:sticky;top:0;align-self:start;height:100vh;overflow:auto;padding:24px 16px;border-right:1px solid var(--line)}}
 nav .brand{{font-weight:700;font-size:17px;margin-bottom:4px}}
 nav .tag{{color:var(--mut);font-size:12px;margin-bottom:18px}}
 nav a{{display:block;color:var(--mut);text-decoration:none;padding:7px 10px;border-radius:7px;font-size:13.5px;margin:2px 0}}
 nav a:hover{{background:var(--panel);color:var(--ink)}}
 main{{padding:34px 44px;min-width:0}}
 header.hero{{margin-bottom:30px}}
 header.hero h1{{font-size:30px;margin:0 0 6px}}
 header.hero p{{color:var(--mut);margin:0;max-width:70ch}}
 section{{border-top:1px solid var(--line);padding:30px 0}}
 h2{{font-size:23px;display:flex;align-items:center;gap:12px;margin:0 0 6px;scroll-margin-top:18px}}
 h2 .num{{background:var(--acc);color:#08111f;width:30px;height:30px;border-radius:8px;display:inline-grid;place-items:center;font-size:15px;font-weight:700}}
 h3{{font-size:15px;text-transform:uppercase;letter-spacing:.06em;color:var(--mut);margin:24px 0 8px}}
 .oneliner{{font-size:16px;color:var(--ink);background:var(--panel);border-left:3px solid var(--acc);padding:10px 14px;border-radius:0 8px 8px 0;margin:4px 0 0}}
 p{{margin:8px 0;color:#cdd7e1}}
 code{{background:var(--code);color:#bcd;padding:1px 5px;border-radius:5px;font:13px ui-monospace,Consolas,monospace;word-break:break-word}}
 .callout{{border-radius:10px;padding:14px 16px;margin:14px 0}}
 .callout-h{{font-weight:700;font-size:13px;text-transform:uppercase;letter-spacing:.05em;margin-bottom:6px}}
 .why{{background:#16202e;border:1px solid #24405f}} .why .callout-h{{color:var(--acc)}}
 .talk{{background:#1a1606;border:1px solid #4a3a12}} .talk .callout-h{{color:var(--acc2)}}
 .trade{{background:#1c1414;border:1px solid #4a2424}} .trade .callout-h{{color:#f08a8a}}
 .callout ul{{margin:6px 0 0;padding-left:18px}} .callout li{{margin:5px 0;color:#cdd7e1}}
 .mechs{{display:grid;gap:8px}}
 .mech{{background:var(--panel);border:1px solid var(--line);border-radius:9px;padding:11px 14px}}
 .mech-name{{font-weight:600;color:#fff}} .mech-detail{{color:#bcc7d2;font-size:14px;margin-top:3px}}
 ol.flow{{counter-reset:s;list-style:none;padding-left:0;margin:8px 0}}
 ol.flow li{{position:relative;padding:7px 0 7px 38px;border-left:2px solid var(--line);margin-left:13px;color:#cdd7e1}}
 ol.flow li:before{{counter-increment:s;content:counter(s);position:absolute;left:-13px;top:7px;width:24px;height:24px;background:var(--code);border:1px solid var(--line);border-radius:50%;display:grid;place-items:center;font-size:12px;color:var(--acc)}}
 .grid2{{display:grid;grid-template-columns:1fr 1fr;gap:14px}}
 .refs{{display:flex;flex-wrap:wrap;gap:6px}} code.ref{{background:#11161d;border:1px solid var(--line);font-size:12px}}
 @media(max-width:900px){{.wrap{{grid-template-columns:1fr}} nav{{position:static;height:auto;border-right:0;border-bottom:1px solid var(--line)}} .grid2{{grid-template-columns:1fr}} main{{padding:22px}}}}
 @media print{{nav{{display:none}} .wrap{{display:block}} body{{background:#fff;color:#000}} section{{break-inside:avoid}}}}
</style></head>
<body><div class="wrap">
<nav><div class="brand">nanocodex</div><div class="tag">Rust coding agent · design reference</div>{nav}
<a href="#" style="margin-top:14px;color:#5b6673;font-size:12px">— built from source by 8-agent deep read —</a></nav>
<main>
<header class="hero"><h1>nanocodex — Design &amp; Architecture</h1>
<p>A from-scratch, single-binary coding agent in Rust (an OpenAI-compatible LLM loop with sandboxed tools), plus <b>ncx-forge</b>, a framework that evolves the agent's own harness against a verifiable benchmark. This reference walks each subsystem: how it works, <i>why</i> it's built that way, and the talking points behind each decision.</p></header>
{body}
</main></div></body></html>"""


if __name__ == "__main__":
    OUT.write_text(build(), encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
