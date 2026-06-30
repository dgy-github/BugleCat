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


# ── inline SVG diagrams (self-contained, match the doc's dark palette) ──────────
_DEF = ('<defs><marker id="ah" markerWidth="9" markerHeight="9" refX="7" refY="3" '
        'orient="auto"><path d="M0,0 L7,3 L0,6 Z" fill="#6b7785"/></marker></defs>')


def _box(x, y, w, h, title, sub="", accent="#2a313a", tcol="#e6edf3"):
    t = f'<text x="{x + w/2}" y="{y + (20 if sub else h/2 + 5)}" text-anchor="middle" font-size="13" font-weight="600" fill="{tcol}">{title}</text>'
    s = f'<text x="{x + w/2}" y="{y + 38}" text-anchor="middle" font-size="11.5" fill="#9aa7b4">{sub}</text>' if sub else ""
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="9" fill="#161b22" '
            f'stroke="{accent}" stroke-width="1.5"/>{t}{s}')


def _arrow(x1, y1, x2, y2, label="", dash=""):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    lab = (f'<text x="{(x1+x2)/2}" y="{(y1+y2)/2 - 6}" text-anchor="middle" '
           f'font-size="11" fill="#9aa7b4">{label}</text>') if label else ""
    return (f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#6b7785" '
            f'stroke-width="1.5" marker-end="url(#ah)"{d}/>{lab}')


def arch_svg():
    A = "#6ea8fe"; O = "#e8a33d"
    s = [f'<svg viewBox="0 0 780 430" role="img" aria-label="Runtime architecture and training meta-loop" style="width:100%;max-width:780px">{_DEF}']
    s.append('<text x="390" y="16" text-anchor="middle" font-size="12" fill="#6b7785">RUNTIME (one turn)</text>')
    s.append(_box(20, 70, 96, 46, "User", "prompt"))
    s.append(_box(150, 34, 380, 150, "Agent Turn Loop", "ncx-core · single-thread (!Send)", A, A))
    s.append(_box(178, 78, 130, 48, "call model"))
    s.append(_box(372, 78, 130, 48, "run tools"))
    s.append(_arrow(308, 92, 372, 92))
    s.append(_arrow(372, 112, 308, 112, "loop ≤ budget"))
    s.append('<text x="340" y="170" text-anchor="middle" font-size="11" fill="#9aa7b4">context-edit · read-only batch · cancel/budget · vision route</text>')
    s.append(_box(566, 60, 196, 46, "Provider", "DeepSeek / vision (OpenAI-compat)"))
    s.append(_arrow(308, 100, 178, 100))  # user-ish into call (decorative)
    s.append(_arrow(116, 93, 150, 100))
    s.append(_arrow(502, 96, 566, 90, "chat"))
    s.append(_box(372, 232, 150, 44, "Tools", "read_file·apply_patch·shell·…"))
    s.append(_arrow(437, 126, 437, 232))
    s.append(_box(566, 232, 196, 44, "Sandbox + Approval", "RO / ws-write / full · ask/auto/deny", O, O))
    s.append(_arrow(522, 254, 566, 254))
    s.append(_box(20, 232, 150, 44, "Project memory", "recall (leads) / remember"))
    s.append(_arrow(170, 250, 178, 120, "recall"))
    s.append('<text x="390" y="312" text-anchor="middle" font-size="12" fill="#6b7785">META-LOOP — ncx-forge (offline training)</text>')
    s.append(_box(20, 330, 150, 50, "Teacher panel", "codex / claude / api", O, O))
    s.append(_box(210, 330, 150, 50, "genome", "NCX_GENOME (prompt+desc)"))
    s.append(_box(400, 330, 150, 50, "Evaluator", "agent × bench → reward"))
    s.append(_box(590, 330, 150, 50, "accept gate", "noise-aware / Pareto", A, A))
    s.append(_arrow(170, 355, 210, 355, "mutate"))
    s.append(_arrow(360, 355, 400, 355, "inject"))
    s.append(_arrow(550, 355, 590, 355, "score"))
    s.append(_arrow(665, 380, 95, 380, "champion ↻ next gen", "5 4"))
    s.append('</svg>')
    return "".join(s)


def mindmap_svg():
    RT, TR = "#6ea8fe", "#e8a33d"  # runtime / training category colors
    root = ("nanocodex", "Rust coding agent + ncx-forge")
    # (name, sub, category): runtime = the live agent; training = the offline meta-loop.
    branches = [
        ("Harness / turn loop", "call↔tools · RO-batch · budget · !Send", "rt"),
        ("Context compression", "keep-recent · shrink tool results · drop prefix", "rt"),
        ("Tool system", "Tool trait · registry · dynamic tool_search view", "rt"),
        ("Sandbox + approval", "3 modes · auto/ask/deny · escalation", "rt"),
        ("Tiered orchestrator", "classify→plan→workers→verify · recurse", "rt"),
        ("Project memory", "remember/recall · jaccard consolidate · leads", "rt"),
        ("Skills · MCP · vision", "progressive disclosure · stdio JSON-RPC · routing", "rt"),
        ("ncx-forge training", "genome · teacher · bench fitness · Pareto · SFT/RL", "tr"),
    ]
    H = 60 * len(branches) + 56
    s = [f'<svg viewBox="0 0 800 {H}" role="img" aria-label="Subsystem mind map, colored by runtime vs training" style="width:100%;max-width:800px">{_DEF}']
    # legend
    s.append('<rect x="470" y="8" width="12" height="12" rx="3" fill="#6ea8fe"/>'
             '<text x="488" y="18" font-size="11" fill="#9aa7b4">runtime (live agent)</text>')
    s.append('<rect x="620" y="8" width="12" height="12" rx="3" fill="#e8a33d"/>'
             '<text x="638" y="18" font-size="11" fill="#9aa7b4">training (meta-loop)</text>')
    cy = (H + 36) / 2
    s.append(f'<rect x="14" y="{cy-28}" width="150" height="56" rx="12" fill="#1a2230" stroke="#cfd7e0" stroke-width="2"/>')
    s.append(f'<text x="89" y="{cy-4}" text-anchor="middle" font-size="14" font-weight="600" fill="#e6edf3">{root[0]}</text>')
    s.append(f'<text x="89" y="{cy+15}" text-anchor="middle" font-size="10.5" fill="#9aa7b4">{root[1]}</text>')
    for i, (name, sub, cat) in enumerate(branches):
        col = RT if cat == "rt" else TR
        by = 46 + i * 60
        s.append(f'<path d="M164,{cy} C 250,{cy} 250,{by+22} 320,{by+22}" fill="none" stroke="{col}" stroke-opacity="0.5" stroke-width="1.5"/>')
        s.append(f'<rect x="320" y="{by}" width="232" height="44" rx="9" fill="#161b22" stroke="{col}" stroke-width="1.5"/>')
        s.append(f'<text x="332" y="{by+19}" font-size="12.5" font-weight="600" fill="{col}">{name}</text>')
        s.append(f'<text x="332" y="{by+35}" font-size="10.5" fill="#9aa7b4">{esc(sub)}</text>')
        s.append(f'<text x="566" y="{by+27}" font-size="11" fill="#5b6673">§{i+1}</text>')
    s.append('</svg>')
    return "".join(s)


def harness_svg():
    s = [f'<svg viewBox="0 0 760 250" role="img" aria-label="Agent turn loop" style="width:100%;max-width:760px">{_DEF}']
    s.append(_box(20, 100, 120, 46, "call model", "+ tool schemas", "#6ea8fe", "#6ea8fe"))
    s.append(_box(200, 100, 130, 46, "tool_calls?", "finish_reason"))
    s.append(_arrow(140, 123, 200, 123))
    s.append(_box(390, 30, 200, 46, "batch read-only ∥", "join_all (concurrent)"))
    s.append(_box(390, 100, 200, 46, "run writes serially", "ordered, deterministic"))
    s.append(_arrow(330, 110, 390, 60, "reads"))
    s.append(_arrow(330, 123, 390, 123, "write"))
    s.append(_box(640, 65, 100, 46, "append results"))
    s.append(_arrow(590, 53, 690, 65))
    s.append(_arrow(590, 123, 690, 111))
    s.append(_arrow(690, 111, 80, 146, "loop (≤ model/tool budget)", "5 4"))
    s.append(_box(200, 185, 130, 44, "final answer", "no tool_calls", "#e8a33d", "#e8a33d"))
    s.append(_arrow(265, 146, 265, 185, "done"))
    s.append('<text x="120" y="210" text-anchor="middle" font-size="11" fill="#9aa7b4">cancel closure polled at each step · synthetic tool results backfilled on stop</text>')
    s.append('</svg>')
    return "".join(s)


def compress_svg():
    s = [f'<svg viewBox="0 0 760 250" role="img" aria-label="Context compression pipeline" style="width:100%;max-width:760px">{_DEF}']
    s.append(_box(16, 90, 120, 56, "full history", "session.messages"))
    s.append(_box(210, 20, 230, 44, "keep recent N msgs", "verbatim (most relevant)"))
    s.append(_box(210, 92, 230, 44, "older tool results", "→ shrink to max_chars"))
    s.append(_box(210, 164, 230, 44, "over max_chars total", "drop oldest prefix → align to user"))
    s.append(_arrow(136, 110, 210, 42))
    s.append(_arrow(136, 118, 210, 114))
    s.append(_arrow(136, 126, 210, 186))
    s.append(_box(510, 90, 140, 56, "provider view", "sent this turn", "#6ea8fe", "#6ea8fe"))
    s.append(_arrow(440, 42, 510, 108))
    s.append(_arrow(440, 114, 510, 116))
    s.append(_arrow(440, 186, 510, 124))
    s.append('<text x="380" y="238" text-anchor="middle" font-size="11" fill="#9aa7b4">non-destructive: builds a send-time copy; session.messages stays intact (resume-safe)</text>')
    s.append('</svg>')
    return "".join(s)


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
    fs = rec.get("flow_steps", [])
    parts = []
    for i, st in enumerate(fs):
        parts.append(f'<div class="fc-step"><span class="fc-n">{i + 1}</span>'
                     f'<span class="fc-t">{esc(st)}</span></div>')
        if i < len(fs) - 1:
            parts.append('<div class="fc-arrow" aria-hidden="true">&#9660;</div>')
    steps = "".join(parts)
    talk = "".join(f"<li>{esc(t)}</li>" for t in rec.get("interview_talking_points", []))
    gotchas = "".join(f"<li>{esc(g)}</li>" for g in rec.get("tradeoffs_or_gotchas", []))
    refs = "".join(f"<code class='ref'>{esc(r)}</code>" for r in rec.get("code_refs", []))
    diagram = {"harness": harness_svg(), "context": compress_svg()}.get(sid, "")
    diagram_html = f'<div class="fig">{diagram}</div>' if diagram else ""
    return f"""
<section id="{sid}">
  <h2><span class="num">{idx + 1}</span>{esc(title)}</h2>
  <p class="oneliner">{esc(rec.get("one_liner", ""))}</p>

  <h3>How it works</h3>
  <p>{esc(rec.get("how_it_works", ""))}</p>
  {diagram_html}

  <div class="callout why"><div class="callout-h">Design rationale — why this way</div>
    <p>{esc(rec.get("design_rationale", ""))}</p></div>

  <h3>Key mechanisms</h3>
  <div class="mechs">{mechs}</div>

  <h3>Control / data flow</h3>
  <div class="flowchart">{steps}</div>

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
    nav = '<a href="#overview">Architecture &amp; mind-map</a>' + "".join(
        f'<a href="#{sid}">{esc(title)}</a>' for (_, title, sid), _ in ordered)
    overview = f"""
<section id="overview">
  <h2><span class="num">◆</span>Architecture &amp; mind-map</h2>
  <p class="oneliner">One turn = call-model ↔ run-tools under a sandbox; an optional orchestrator wraps it; ncx-forge is an offline meta-loop that evolves the agent's own prompt/tool-descriptions against a benchmark.</p>
  <h3>Runtime &amp; training meta-loop</h3>
  <div class="fig">{arch_svg()}</div>
  <h3>Subsystem mind-map</h3>
  <div class="fig">{mindmap_svg()}</div>
</section>"""
    body = overview + "".join(section(r, i, title, sid)
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
 .flowchart{{display:flex;flex-direction:column;align-items:stretch;gap:0;margin:10px 0;max-width:760px}}
 .fc-step{{display:flex;gap:11px;align-items:flex-start;background:var(--panel);border:1px solid var(--line);border-radius:9px;padding:10px 13px}}
 .fc-n{{flex:none;width:24px;height:24px;background:var(--code);border:1px solid var(--acc);border-radius:50%;display:grid;place-items:center;font-size:12px;color:var(--acc);font-weight:600}}
 .fc-t{{color:#cdd7e1;font-size:14px}}
 .fc-arrow{{text-align:center;color:#4a5563;font-size:13px;line-height:1.1;padding:3px 0}}
 .grid2{{display:grid;grid-template-columns:1fr 1fr;gap:14px}}
 .refs{{display:flex;flex-wrap:wrap;gap:6px}} code.ref{{background:#11161d;border:1px solid var(--line);font-size:12px}}
 .fig{{background:#0c1014;border:1px solid var(--line);border-radius:10px;padding:14px;margin:14px 0;overflow-x:auto}}
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
