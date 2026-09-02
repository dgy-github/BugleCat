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
# Prefer the Chinese-translated notes when present; fall back to English.
DATA = HERE / "design_data.zh.json"
if not DATA.exists():
    DATA = HERE / "design_data.json"
OUT = HERE / "nanocodex-design.html"

# (match-key in the subsystem string, clean title, section id) in reading order.
ORDER = [
    ("harness", "Harness · Agent 回合循环", "harness"),
    ("context-compression", "上下文压缩 · Context Editing", "context"),
    ("tools", "工具系统 · 动态暴露", "tools"),
    ("sandbox", "沙箱 · 审批状态机", "sandbox"),
    ("orchestrator", "分层编排器 (flash / pro)", "orch"),
    ("memory", "项目记忆 · 自进化", "memory"),
    ("skills", "Skills · MCP · 视觉", "skills"),
    ("ncx-forge", "ncx-forge · 骨架训练框架", "forge"),
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
    accent_blue = "#6ea8fe"
    accent_orange = "#e8a33d"
    s = [f'<svg viewBox="0 0 780 430" role="img" aria-label="Runtime architecture and training meta-loop" style="width:100%;max-width:780px">{_DEF}']
    s.append('<text x="390" y="16" text-anchor="middle" font-size="12" fill="#6b7785">运行时（单回合）</text>')
    s.append(_box(20, 70, 96, 46, "用户", "提示"))
    s.append(_box(150, 34, 380, 150, "Agent 回合循环", "ncx-core · 单线程 (!Send)", accent_blue, accent_blue))
    s.append(_box(178, 78, 130, 48, "调模型"))
    s.append(_box(372, 78, 130, 48, "跑工具"))
    s.append(_arrow(308, 92, 372, 92))
    s.append(_arrow(372, 112, 308, 112, "循环 ≤ 预算"))
    s.append('<text x="340" y="170" text-anchor="middle" font-size="11" fill="#9aa7b4">上下文编辑 · 只读并发批 · 取消/预算 · 视觉路由</text>')
    s.append(_box(566, 60, 196, 46, "Provider", "DeepSeek / 视觉 (OpenAI 兼容)"))
    s.append(_arrow(308, 100, 178, 100))  # user-ish into call (decorative)
    s.append(_arrow(116, 93, 150, 100))
    s.append(_arrow(502, 96, 566, 90, "对话"))
    s.append(_box(372, 232, 150, 44, "工具", "read_file·apply_patch·shell·…"))
    s.append(_arrow(437, 126, 437, 232))
    s.append(_box(566, 232, 196, 44, "沙箱 + 审批", "只读/可写/全权 · 询问/自动/拒绝", accent_orange, accent_orange))
    s.append(_arrow(522, 254, 566, 254))
    s.append(_box(20, 232, 150, 44, "项目记忆", "召回(线索) / 记忆"))
    s.append(_arrow(170, 250, 178, 120, "召回"))
    s.append('<text x="390" y="312" text-anchor="middle" font-size="12" fill="#6b7785">元循环 — ncx-forge（离线训练）</text>')
    s.append(_box(20, 330, 150, 50, "教师面板", "codex / claude / api", accent_orange, accent_orange))
    s.append(_box(210, 330, 150, 50, "genome", "NCX_GENOME (提示+描述)"))
    s.append(_box(400, 330, 150, 50, "评测器", "agent × bench → reward"))
    s.append(_box(590, 330, 150, 50, "接受门", "噪声感知 / Pareto", accent_blue, accent_blue))
    s.append(_arrow(170, 355, 210, 355, "变异"))
    s.append(_arrow(360, 355, 400, 355, "注入"))
    s.append(_arrow(550, 355, 590, 355, "评分"))
    s.append(_arrow(665, 380, 95, 380, "冠军 ↻ 下一代", "5 4"))
    s.append('</svg>')
    return "".join(s)


def mindmap_svg():
    runtime_color, training_color = "#6ea8fe", "#e8a33d"
    root = ("nanocodex", "Rust 编码 agent + ncx-forge")
    # (name, sub, category): runtime = the live agent; training = the offline meta-loop.
    branches = [
        ("Harness · 回合循环", "调模型↔工具 · 只读批 · 预算 · !Send", "rt"),
        ("上下文压缩", "保留最近 · 截断 tool 结果 · 丢最老前缀", "rt"),
        ("工具系统", "Tool trait · registry · 动态 tool_search 视图", "rt"),
        ("沙箱 + 审批", "3 种模式 · 自动/询问/拒绝 · 升级", "rt"),
        ("分层编排器", "classify→plan→workers→verify · 递归", "rt"),
        ("项目记忆", "remember/recall · jaccard 合并 · 当线索", "rt"),
        ("Skills · MCP · 视觉", "渐进披露 · stdio JSON-RPC · 路由", "rt"),
        ("ncx-forge 训练", "genome · 教师 · bench fitness · Pareto · SFT/RL", "tr"),
    ]
    height = 60 * len(branches) + 56
    s = [f'<svg viewBox="0 0 800 {height}" role="img" aria-label="Subsystem mind map, colored by runtime vs training" style="width:100%;max-width:800px">{_DEF}']
    # legend
    s.append('<rect x="470" y="8" width="12" height="12" rx="3" fill="#6ea8fe"/>'
             '<text x="488" y="18" font-size="11" fill="#9aa7b4">运行时（在线 agent）</text>')
    s.append('<rect x="628" y="8" width="12" height="12" rx="3" fill="#e8a33d"/>'
             '<text x="646" y="18" font-size="11" fill="#9aa7b4">训练（元循环）</text>')
    cy = (height + 36) / 2
    s.append(f'<rect x="14" y="{cy-28}" width="150" height="56" rx="12" fill="#1a2230" stroke="#cfd7e0" stroke-width="2"/>')
    s.append(f'<text x="89" y="{cy-4}" text-anchor="middle" font-size="14" font-weight="600" fill="#e6edf3">{root[0]}</text>')
    s.append(f'<text x="89" y="{cy+15}" text-anchor="middle" font-size="10.5" fill="#9aa7b4">{root[1]}</text>')
    for i, (name, sub, cat) in enumerate(branches):
        col = runtime_color if cat == "rt" else training_color
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
    s.append(_box(20, 100, 120, 46, "调模型", "+ 工具 schema", "#6ea8fe", "#6ea8fe"))
    s.append(_box(200, 100, 130, 46, "有 tool_calls?", "finish_reason"))
    s.append(_arrow(140, 123, 200, 123))
    s.append(_box(390, 30, 200, 46, "只读并发批 ∥", "join_all (并发)"))
    s.append(_box(390, 100, 200, 46, "写操作串行", "有序、确定"))
    s.append(_arrow(330, 110, 390, 60, "读"))
    s.append(_arrow(330, 123, 390, 123, "写"))
    s.append(_box(640, 65, 100, 46, "追加结果"))
    s.append(_arrow(590, 53, 690, 65))
    s.append(_arrow(590, 123, 690, 111))
    s.append(_arrow(690, 111, 80, 146, "循环 (≤ 模型/工具预算)", "5 4"))
    s.append(_box(200, 185, 130, 44, "最终回答", "无 tool_calls", "#e8a33d", "#e8a33d"))
    s.append(_arrow(265, 146, 265, 185, "完成"))
    s.append('<text x="120" y="210" text-anchor="middle" font-size="11" fill="#9aa7b4">每步轮询取消闭包 · 中止时回填合成 tool 结果</text>')
    s.append('</svg>')
    return "".join(s)


def compress_svg():
    s = [f'<svg viewBox="0 0 760 250" role="img" aria-label="Context compression pipeline" style="width:100%;max-width:760px">{_DEF}']
    s.append(_box(16, 90, 120, 56, "完整历史", "session.messages"))
    s.append(_box(210, 20, 230, 44, "保留最近 N 条", "原样（最相关）"))
    s.append(_box(210, 92, 230, 44, "更老的 tool 结果", "→ 截断到 max_chars"))
    s.append(_box(210, 164, 230, 44, "总量超 max_chars", "丢最老前缀 → 对齐到 user"))
    s.append(_arrow(136, 110, 210, 42))
    s.append(_arrow(136, 118, 210, 114))
    s.append(_arrow(136, 126, 210, 186))
    s.append(_box(510, 90, 140, 56, "发送视图", "本回合发出", "#6ea8fe", "#6ea8fe"))
    s.append(_arrow(440, 42, 510, 108))
    s.append(_arrow(440, 114, 510, 116))
    s.append(_arrow(440, 186, 510, 124))
    s.append('<text x="380" y="238" text-anchor="middle" font-size="11" fill="#9aa7b4">非破坏：构造发送时副本；session.messages 原封不动（resume 安全）</text>')
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

  <h3>工作原理</h3>
  <p>{esc(rec.get("how_it_works", ""))}</p>
  {diagram_html}

  <div class="callout why"><div class="callout-h">设计理由 · 为什么这么设计</div>
    <p>{esc(rec.get("design_rationale", ""))}</p></div>

  <h3>关键机制</h3>
  <div class="mechs">{mechs}</div>

  <h3>控制 / 数据流</h3>
  <div class="flowchart">{steps}</div>

  <div class="grid2">
    <div class="callout talk"><div class="callout-h">★ 面试话术点</div><ul>{talk}</ul></div>
    <div class="callout trade"><div class="callout-h">取舍与坑</div><ul>{gotchas}</ul></div>
  </div>

  <h3>代码引用</h3>
  <div class="refs">{refs}</div>
</section>"""


def build() -> str:
    recs = json.loads(DATA.read_text(encoding="utf-8"))
    ordered = sorted(((classify(r), r) for r in recs), key=lambda x: x[0][0])
    nav = '<a href="#overview">架构 · 思维导图</a>' + "".join(
        f'<a href="#{sid}">{esc(title)}</a>' for (_, title, sid), _ in ordered)
    overview = f"""
<section id="overview">
  <h2><span class="num">◆</span>架构 · 思维导图</h2>
  <p class="oneliner">一个回合 = 在沙箱下「调模型 ↔ 跑工具」；可选的编排器在外层包裹它；ncx-forge 是一条离线元循环，用基准测试反向进化 agent 自己的 prompt / 工具描述。</p>
  <h3>运行时 · 训练元循环</h3>
  <div class="fig">{arch_svg()}</div>
  <h3>子系统思维导图</h3>
  <div class="fig">{mindmap_svg()}</div>
</section>"""
    body = overview + "".join(section(r, i, title, sid)
                              for (i, ((_, title, sid), r)) in enumerate(ordered))
    return f"""<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>nanocodex — 设计与架构</title>
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
<nav><div class="brand">nanocodex</div><div class="tag">Rust 编码 agent · 设计参考</div>{nav}
<a href="#" style="margin-top:14px;color:#5b6673;font-size:12px">— 由 8 个 agent 深读源码生成 —</a></nav>
<main>
<header class="hero"><h1>nanocodex — 设计与架构</h1>
<p>一个用 Rust 从零写的单二进制编码 agent（OpenAI 兼容的 LLM 循环 + 沙箱化工具），外加 <b>ncx-forge</b> —— 一个用可验证基准反向进化 agent 自身骨架的框架。本参考逐个子系统讲清：怎么工作、<i>为什么</i>这么设计、以及每个决策背后的面试话术。</p></header>
{body}
</main></div></body></html>"""


if __name__ == "__main__":
    OUT.write_text(build(), encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
