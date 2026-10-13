"""Render measured policy-only regression results without claiming certification."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def render(data):
    if data["schema"] != "buglecat.verification.claim-drift-evaluation.v2":
        raise ValueError("Regenerate the report with the corrected v2 evaluator first")
    total = data["total_cases"]
    blocked = data["compliant_cases_total"] - data["compliant_passed"]
    lines = [
        "# 声明核验规则层合成回归报告（v2）", "",
        "本报告直接评测预先构造的声明与证据；不覆盖声明抽取、真实工具证据组装或 Agent 纠正流程。",
        "这些结果不构成独立留出认证或生产准确率保证。", "",
        f"- 样本数：{total}",
        f"- 明确干预的漂移样本：{data['drift_detected']}/{data['drift_cases_total']}",
        f"- 合规样本干预数：{blocked}/{data['compliant_cases_total']}",
        f"- 裁决匹配：{data['exact_verdict_matches']}/{total}",
        f"- 原因匹配：{data['exact_reason_matches']}/{total}",
        f"- 规则求值耗时：{data['elapsed_ms']} ms（不含证据采集和网络）", "",
        "检出和误拦使用同一口径：NeedEvidence 或 Contradiction；Unknown 不算检出。",
        "以上为样本内经验比例，未据此认证误拦率上界。", "",
    ]
    for name, group in data["groups"].items():
        lines.append(f"- {name}：裁决匹配 {group['verdict_matches']}/{group['total']}，"
                     f"原因匹配 {group['reason_matches']}/{group['total']}")
    return "\n".join(lines) + "\n"


def main():
    source = ROOT / "reports/claim_drift_eval_report_v1.json"
    data = json.loads(source.read_text(encoding="utf-8"))
    (ROOT / "reports/claim_drift_eval_report_v1.md").write_text(render(data), encoding="utf-8")


if __name__ == "__main__":
    main()
