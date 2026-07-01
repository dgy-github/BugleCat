# NCX Dreamina Gateway

Local Rust test gateway for OpenAI-compatible Dreamina/Jimeng image workflows.

This crate intentionally starts with a **mock provider**. It lets you test:

- `http://127.0.0.1:8000/v1/models`
- `http://127.0.0.1:8000/v1/images/generations`
- `http://127.0.0.1:8000/v1/chat/completions`
- `http://127.0.0.1:8001` admin console

The gateway does not automate browser cookie extraction and does not send real
Dreamina requests in this first local-test stage. Session IDs can be entered
manually for pool-management testing and are only shown redacted in the UI.

## Run

```powershell
cd D:\agent_prac\ncx-dreamina-gateway\rust
cargo run -p ncx-dreamina-gateway
```

Optional environment variables:

```powershell
$env:NCX_DREAMINA_API_ADDR = "127.0.0.1:8000"
$env:NCX_DREAMINA_ADMIN_ADDR = "127.0.0.1:8001"
$env:NCX_DREAMINA_STATE = ".ncx-dreamina-gateway/state.json"
```

## Local API Test

The default development API key is `sk-local-dev`.

```powershell
curl.exe http://127.0.0.1:8000/v1/models `
  -H "Authorization: Bearer sk-local-dev"

curl.exe http://127.0.0.1:8000/v1/images/generations `
  -H "Authorization: Bearer sk-local-dev" `
  -H "Content-Type: application/json" `
  -d "{\"model\":\"jimeng-image-3.0\",\"prompt\":\"a cozy anime cat in a workshop\",\"n\":1}"
```

For NextChat, Dify, or Chatbox local testing:

- API Base: `http://127.0.0.1:8000`
- API Key: `sk-local-dev` or a key generated in the admin console
- Model: `jimeng-image-3.0`

## Safety Boundary

Only use session IDs from accounts you own or are authorized to use. Treat
session IDs as passwords. Do not commit the state file.
