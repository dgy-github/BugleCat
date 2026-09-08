"""Explicitly invoked live route probe; reads credentials but never persists them."""
import asyncio
import json
import time
from pathlib import Path

import httpx


async def probe(route, model, tool):
    started = time.monotonic()
    row = {"model": model, "case": "tool_call" if tool else "short_text", "retries": 0}
    payload = {"model": model, "stream": False, "max_tokens": 2048,
               "thinking": {"type": "disabled"},
               "messages": [{"role": "user", "content":
                             'Call echo with text="probe-ok". Do not answer in prose.' if tool
                             else 'Reply exactly: probe-ok'}]}
    if tool:
        payload["tools"] = [{"type": "function", "function": {"name": "echo",
            "description": "Echo a harmless test string", "parameters": {"type": "object",
            "properties": {"text": {"type": "string"}}, "required": ["text"]}}}]
    async def request():
        async with httpx.AsyncClient(timeout=httpx.Timeout(45, connect=10)) as client:
            response = await client.post(route["base_url"].rstrip('/') + '/chat/completions',
                headers={"Authorization": "Bearer " + route["api_key"]}, json=payload)
            row["http_status"] = response.status_code
            if not response.is_success:
                row.update(ok=False, kind="http_error")
                return
            data = response.json()
            message = data["choices"][0]["message"]
            if tool:
                calls = message.get("tool_calls") or []
                ok = len(calls) == 1 and calls[0]["function"]["name"] == 'echo'
                ok = ok and json.loads(calls[0]["function"]["arguments"]) == {"text": "probe-ok"}
            else:
                ok = (message.get("content") or '').strip() == 'probe-ok'
            row.update(ok=ok, kind="success" if ok else "unexpected_response")
    try:
        await asyncio.wait_for(request(), timeout=60)
    except Exception as exc:
        row.update(ok=False, kind=type(exc).__name__)
    row["elapsed_s"] = round(time.monotonic() - started, 2)
    return row


async def main():
    routes = json.loads((Path.home() / '.nanocodex/providers.json').read_text(encoding='utf-8-sig'))
    route = next(r for r in routes if r['id'] == 'preset:deepseek')
    results = []
    for model in ['deepseek-v4-pro', 'deepseek-v4-flash-vision-exp']:
        if model not in route['models']:
            raise ValueError('Model not configured in route')
        for tool in [False, True]:
            row = await probe(route, model, tool)
            results.append(row)
            print(json.dumps(row, ensure_ascii=False), flush=True)
    output = Path(__file__).with_name('deepseek_probe_20260908.json')
    output.write_text(json.dumps(results, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    asyncio.run(main())
