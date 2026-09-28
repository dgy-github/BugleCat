"""Optional LLM-as-judge for open-ended outputs (buglecat.golden.judge.v1).

Standard enforced here (matches the eval governance rules):
  - The judge is ONLY used for outputs that deterministic assertions cannot
    grade (e.g. compaction summary faithfulness, final-answer quality).
  - A judge verdict requires method="llm" and score >= 4 (1-5 scale). Anything
    else cannot count as a pass.
  - Without a configured endpoint the result is method="skip". A skip NEVER
    counts as a pass; callers must surface it as skipped/unknown coverage.

Endpoint safety:
  - Only https:// endpoints are accepted, except plain http on loopback
    (127.0.0.1 / ::1 / localhost) for local gateways.
  - Link-local / private / cloud-metadata addresses are rejected.

Configuration (environment):
    NCX_JUDGE_BASE_URL   OpenAI-compatible base, e.g. https://api.deepseek.com
    NCX_JUDGE_MODEL      model id, e.g. deepseek-chat
    NCX_JUDGE_API_KEY    API key

Usage:
    python -m evals.golden.judge --text-file summary.md --criteria-file criteria.txt
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
import sys
import urllib.request
from pathlib import Path
from urllib.parse import urlparse

PASS_SCORE_MIN = 4
TIMEOUT_S = 120

_JUDGE_SYSTEM_TOP = (
    "你是一名严格的评测裁判。根据评分标准对被评文本打 1-5 分：\n"
    "5 = 完全满足标准，无遗漏无编造；4 = 满足标准，瑕疵不影响结论；\n"
    "3 = 部分满足，有明显缺口；2 = 大部分不满足；1 = 不满足或存在编造。\n"
    "只输出严格 JSON：{\"score\": <1-5 整数>, \"reasoning\": \"<一句话依据>\", \"issues\": [\"<问题>\", ...]}\n"
    "评分时：答案中任何无法从被评材料推出的具体事实都视为编造，直接 <=2 分。"
)


def judge_configured() -> bool:
    return bool(
        os.environ.get("NCX_JUDGE_BASE_URL")
        and os.environ.get("NCX_JUDGE_MODEL")
        and os.environ.get("NCX_JUDGE_API_KEY")
    )


def resolve_endpoint(base_url: str) -> str | None:
    """Validate the operator-provided base URL and return the chat endpoint.

    Returns None when the URL fails the safety contract."""
    parsed = urlparse((base_url or "").strip())
    host = (parsed.hostname or "").lower()
    if not host:
        return None

    is_loopback = host in {"localhost", "::1"} or host.endswith(".localhost")
    if not is_loopback and host in {"127.0.0.1", "0.0.0.0"}:
        is_loopback = True
    if not is_loopback:
        try:
            addr = ipaddress.ip_address(host)
            if addr.is_loopback:
                is_loopback = True
            elif addr.is_link_local or addr.is_private or addr.is_reserved:
                return None  # link-local (云元数据) / 内网 / 保留段一律拒绝
        except ValueError:
            pass  # domain name

    if parsed.scheme == "https":
        pass
    elif parsed.scheme == "http" and is_loopback:
        pass
    else:
        return None
    if parsed.username or parsed.password:
        return None

    stripped = base_url.strip().rstrip("/")
    if stripped.endswith("/chat/completions"):
        return stripped
    if stripped.endswith("/v1"):
        return stripped + "/chat/completions"
    return stripped + "/v1/chat/completions"


def judge_text(text: str, criteria: str) -> dict:
    """Grade `text` against `criteria`. Returns a verdict dict:
    {"method": "llm"|"skip"|"llm_error", "score": int|None, "passed": bool, ...}."""
    base_url = (os.environ.get("NCX_JUDGE_BASE_URL") or "").strip()
    model = os.environ.get("NCX_JUDGE_MODEL") or ""
    api_key = os.environ.get("NCX_JUDGE_API_KEY") or ""
    if not (base_url and model and api_key):
        return {"method": "skip", "score": None, "passed": False,
                "reason": "judge endpoint not configured (NCX_JUDGE_* env)"}
    endpoint = resolve_endpoint(base_url)
    if endpoint is None:
        return {"method": "skip", "score": None, "passed": False,
                "reason": "judge base_url rejected by endpoint safety policy"}

    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": _JUDGE_SYSTEM_TOP},
            {"role": "user", "content": f"## 评分标准\n{criteria}\n\n## 被评文本\n{text}"},
        ],
        "temperature": 0,
        "response_format": {"type": "json_object"},
    }
    request = urllib.request.Request(
        endpoint,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=TIMEOUT_S) as resp:
            body = json.loads(resp.read().decode("utf-8"))
        content = body["choices"][0]["message"]["content"]
        verdict = json.loads(content)
        score = int(verdict.get("score") or 0)
        return {
            "method": "llm",
            "score": score,
            "passed": score >= PASS_SCORE_MIN,
            "reasoning": str(verdict.get("reasoning") or ""),
            "issues": verdict.get("issues") or [],
        }
    except Exception as exc:
        # network/parse failures must NOT become passes
        return {"method": "llm_error", "score": None, "passed": False, "reason": str(exc)[:300]}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="LLM-as-judge (skip never counts as pass)")
    parser.add_argument("--text-file", required=True)
    parser.add_argument("--criteria-file", required=True)
    parser.add_argument("--output", default=None, help="write verdict JSON here")
    args = parser.parse_args(argv)

    text = Path(args.text_file).read_text(encoding="utf-8")
    criteria = Path(args.criteria_file).read_text(encoding="utf-8")
    verdict = judge_text(text, criteria)
    rendered = json.dumps(verdict, ensure_ascii=False, indent=2)
    print(rendered)
    if args.output:
        Path(args.output).write_text(rendered, encoding="utf-8")
    # Exit 0 only for a real LLM pass; skip/error/low-score are all non-zero so
    # this can gate CI without ever equating "not judged" with "judged OK".
    return 0 if verdict.get("method") == "llm" and verdict.get("passed") else 1


if __name__ == "__main__":
    raise SystemExit(main())
