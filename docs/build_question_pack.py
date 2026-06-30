#!/usr/bin/env python3
"""Render design_data.zh.json into a MODEL-FACING interview question pack
(docs/nanocodex-interview-pack.zh.md): a dense, plain-markdown context bundle +
an instruction preamble, so you can paste it into any model and have it generate
nanocodex interview questions. No HTML/styling — just the material a model needs.
"""
from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE / "design_data.zh.json" if (HERE / "design_data.zh.json").exists() else HERE / "design_data.json"
OUT = HERE / "nanocodex-interview-pack.zh.md"

TITLES = [
    ("harness", "Harness · Agent 回合循环"),
    ("context-compression", "上下文压缩 · context editing"),
    ("tools", "工具系统 · 动态暴露"),
    ("sandbox", "沙箱 · 审批状态机"),
    ("orchestrator", "分层编排器 (flash / pro)"),
    ("memory", "项目记忆 · 自进化"),
    ("skills", "Skills · MCP · 视觉"),
    ("ncx-forge", "ncx-forge · 骨架训练框架"),
]

PREAMBLE = """\
# nanocodex 技术面试 · 出题包（给模型读）

> 用法：把本文件整篇喂给一个强模型，它就能据此出面试题。下面是给模型的指令 + 设计资料。

## 给模型的指令

你是一位资深的系统/AI-infra 面试官。请**只依据下面《设计资料》**对候选人进行关于 **nanocodex**
（一个用 Rust 从零写的单二进制编码 agent + `ncx-forge` 骨架训练框架）的技术面试出题。要求：

1. **按子系统组织**，覆盖全部 8 个子系统；每个子系统出 3–5 题。
2. **按难度分层**标注每题：`[L1 理解]` / `[L2 权衡]` / `[L3 深挖]` / `[L4 开放设计]`。
3. **每题给出四件套**：① 题目；② 考察点（一句话）；③ 参考答案要点（bullet，扣住"为什么这么设计/约束是什么"）；④ 1–2 个追问（follow-up）。
4. **重设计、轻记忆**：优先考"为什么这么设计、放弃了哪个备选、被什么约束逼出来的"，而不是背 API。
5. **代码标识符保留英文**（如 `AgentLoop`、`apply_patch`、`!Send`、`ContextEditPolicy`、`NCX_GENOME`），中文叙述。
6. 末尾另出 **2 道跨子系统综合题**（串起 ≥2 个子系统的设计取舍）。

可套用的出题维度（每个子系统都适用）：
- **机制题**：解释 X 是怎么工作的（数据/控制流）。
- **权衡题**：为什么选 A 不选 B？背后的约束是什么？
- **故障题**：如果去掉 / 改坏 / 调错 X 会发生什么？怎么暴露？
- **对比题**：X 和 Y（如单 champion vs Pareto 种群、refuse-genome vs sentinel 自检）有何不同？
- **设计题**：要新增能力 Z，你会怎么改？会触碰哪些约束？

---

## 设计资料（8 个子系统）
"""

FOOTER = """\

---

## 出题自检（给模型）
- 是否每个子系统都出了题、且标了难度层级？
- 是否至少一半题在考"为什么/权衡/约束"，而非记忆？
- 参考答案要点是否能从上面的《设计资料》里找到依据？
- 追问是否能逼出更深的理解（而非换个问法重复）？
- 是否出了 2 道跨子系统综合题？
"""


def title_for(subsystem: str) -> str:
    s = subsystem.lower()
    best = None
    for key, t in TITLES:
        if key in s and (best is None or len(key) > best[1]):
            best = (t, len(key))
    return best[0] if best else subsystem


def render(rec: dict, idx: int) -> str:
    L = [f"\n### {idx}. {title_for(rec.get('subsystem',''))}\n"]
    L.append(f"**一句话**：{rec.get('one_liner','')}\n")
    L.append(f"**工作原理**：{rec.get('how_it_works','')}\n")
    L.append(f"**设计理由（为什么）**：{rec.get('design_rationale','')}\n")
    L.append("**关键机制**：")
    for m in rec.get("key_mechanisms", []):
        L.append(f"- **{m['name']}** — {m['detail']}")
    L.append("\n**控制 / 数据流**：")
    for i, st in enumerate(rec.get("flow_steps", []), 1):
        L.append(f"{i}. {st}")
    L.append("\n**面试话术点 / 候选人应能说出**：")
    for t in rec.get("interview_talking_points", []):
        L.append(f"- {t}")
    L.append("\n**取舍与坑（适合做故障题/追问）**：")
    for g in rec.get("tradeoffs_or_gotchas", []):
        L.append(f"- {g}")
    refs = rec.get("code_refs", [])
    if refs:
        L.append("\n**代码引用**：" + " · ".join(f"`{r}`" for r in refs))
    return "\n".join(L) + "\n"


def build() -> str:
    recs = json.loads(DATA.read_text(encoding="utf-8"))
    order = {k: i for i, (k, _) in enumerate(TITLES)}

    def rank(r):
        # Longest-key match so 'ncx-forge' beats 'harness' (its desc says
        # "harness-evolution"), matching title_for's logic.
        s = r.get("subsystem", "").lower()
        best = (99, -1)
        for k, i in order.items():
            if k in s and len(k) > best[1]:
                best = (i, len(k))
        return best[0]
    recs = sorted(recs, key=rank)
    body = "".join(render(r, i + 1) for i, r in enumerate(recs))
    return PREAMBLE + body + FOOTER


if __name__ == "__main__":
    OUT.write_text(build(), encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")
