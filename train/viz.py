#!/usr/bin/env python3
"""Lineage + Pareto visualization for ncx-forge M2.

Reads a population-run lineage JSON (written by forge.py --population) and emits
a SELF-CONTAINED HTML file (inline SVG, no external/CDN deps) with:
  * a Pareto scatter — x = cost (mean s), y = pass-rate; front points highlighted,
    champion starred;
  * a lineage table — each genome with its parent, the teacher that proposed it,
    generation, objectives, and whether it survived onto the final front.

Pure `build_html(lineage) -> str` so it is unit-tested without a browser.
"""
from __future__ import annotations

import html
import json
import sys
from pathlib import Path

W, H, PAD = 640, 380, 56  # scatter canvas


def _esc(s) -> str:
    return html.escape(str(s), quote=True)


def _scale(v, lo, hi, a, b):
    if hi <= lo:
        return (a + b) / 2
    return a + (v - lo) * (b - a) / (hi - lo)


def _scatter_svg(nodes: list) -> str:
    pts = [n for n in nodes if isinstance(n.get("cost"), (int, float))
           and isinstance(n.get("passrate"), (int, float))]
    if not pts:
        return "<p>(no scored nodes to plot)</p>"
    costs = [p["cost"] for p in pts]
    prs = [p["passrate"] for p in pts]
    cmin, cmax = min(costs), max(costs)
    pmin, pmax = min(prs), max(prs + [1.0])  # pass-rate axis up to 1.0
    pmin = min(pmin, 0.0)
    body = []
    # axes
    body.append(f'<line x1="{PAD}" y1="{H-PAD}" x2="{W-PAD}" y2="{H-PAD}" stroke="#888"/>')
    body.append(f'<line x1="{PAD}" y1="{PAD}" x2="{PAD}" y2="{H-PAD}" stroke="#888"/>')
    body.append(f'<text x="{W/2}" y="{H-12}" text-anchor="middle" font-size="13">cost — mean seconds (lower better →)</text>')
    body.append(f'<text x="16" y="{H/2}" text-anchor="middle" font-size="13" transform="rotate(-90 16 {H/2})">pass-rate (higher better ↑)</text>')
    for p in pts:
        x = _scale(p["cost"], cmin, cmax, PAD, W - PAD)
        y = _scale(p["passrate"], pmin, pmax, H - PAD, PAD)
        on_front = p.get("on_front_final")
        champ = p.get("champion")
        fill = "#e8a33d" if on_front else "#9bb0c9"
        r = 7 if on_front else 5
        title = f'{p.get("id","?")}: pass={p["passrate"]:.2f}, cost={p["cost"]:.1f}s, gen{p.get("gen","?")}'
        if champ:
            body.append(f'<polygon points="{x-9},{y+7} {x},{y-10} {x+9},{y+7}" fill="#d23" stroke="#900"><title>{_esc(title)} (CHAMPION)</title></polygon>')
        else:
            body.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="{r}" fill="{fill}" stroke="#456"><title>{_esc(title)}</title></circle>')
    return f'<svg viewBox="0 0 {W} {H}" width="100%" style="max-width:{W}px">{"".join(body)}</svg>'


def _lineage_rows(nodes: list) -> str:
    rows = []
    for n in sorted(nodes, key=lambda x: (x.get("gen", 0), str(x.get("id")))):
        if n.get("champion"):
            badge = '<span style="color:#d23">★champion</span>'
        elif n.get("on_front_final"):
            badge = '<span style="color:#b8860b">●front</span>'
        else:
            badge = ""
        pr = n.get("passrate")
        cost = n.get("cost")
        pr_cell = f"{pr:.2f}" if isinstance(pr, (int, float)) else "—"
        cost_cell = f"{cost:.1f}s" if isinstance(cost, (int, float)) else "—"
        rows.append(
            "<tr>"
            f"<td>{_esc(n.get('id'))}</td>"
            f"<td>{_esc(n.get('gen'))}</td>"
            f"<td>{_esc(n.get('parent') or '—')}</td>"
            f"<td>{_esc(n.get('teacher') or '—')}</td>"
            f"<td>{pr_cell}</td>"
            f"<td>{cost_cell}</td>"
            f"<td>{badge}</td></tr>"
        )
    return "".join(rows)


def build_html(lineage: dict) -> str:
    nodes = lineage.get("nodes", [])
    champ = lineage.get("champion") or {}
    champ_id = champ.get("id")
    for n in nodes:
        n["champion"] = (n.get("id") == champ_id)
    stamp = lineage.get("stamp", "?")
    test = lineage.get("test") or {}
    test_line = ""
    if test:
        test_line = (f"<p><b>Final (held-out test {_esc(test.get('tasks'))}):</b> "
                     f"baseline {_esc(test.get('baseline'))}/{_esc(test.get('runs'))} → "
                     f"champion {_esc(test.get('champion'))}/{_esc(test.get('runs'))}</p>")
    return f"""<!doctype html><meta charset="utf-8">
<title>ncx-forge lineage {_esc(stamp)}</title>
<style>
 body{{font:14px system-ui,sans-serif;margin:24px;color:#222}}
 h1{{font-size:18px}} table{{border-collapse:collapse;margin-top:12px}}
 td,th{{border:1px solid #ddd;padding:4px 8px;text-align:left;font-size:13px}}
 th{{background:#f4f4f4}} .card{{border:1px solid #e0e0e0;border-radius:8px;padding:16px;margin:12px 0}}
</style>
<h1>ncx-forge — population / Pareto lineage <code>{_esc(stamp)}</code></h1>
<p>train={_esc(lineage.get('train_tasks'))} · val={_esc(lineage.get('holdout_tasks'))} · pop_cap={_esc(lineage.get('pop_cap'))}</p>
{test_line}
<div class="card"><b>Pareto front</b> (orange = on final front, ★ = champion){_scatter_svg(nodes)}</div>
<div class="card"><b>Lineage</b>
<table><tr><th>genome</th><th>gen</th><th>parent</th><th>teacher</th><th>pass</th><th>cost</th><th></th></tr>
{_lineage_rows(nodes)}</table></div>
"""


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Render a forge lineage JSON to HTML.")
    ap.add_argument("lineage_json")
    ap.add_argument("-o", "--out", default="")
    a = ap.parse_args()
    lin = json.loads(Path(a.lineage_json).read_text(encoding="utf-8"))
    out = Path(a.out) if a.out else Path(a.lineage_json).with_suffix(".html")
    out.write_text(build_html(lin), encoding="utf-8")
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
