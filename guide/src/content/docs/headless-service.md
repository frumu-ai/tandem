---
title: Headless Service
---

Run Tandem as a standalone headless service with optional embedded web admin.

## Start the Engine (Headless)

```bash
tandem-engine serve \
  --hostname 127.0.0.1 \
  --port 39731 \
  --api-token "$(tandem-engine token generate)"
```

This starts the HTTP/SSE engine runtime without desktop UI requirements.

## Enable Embedded Web Admin

```bash
tandem-engine serve \
  --hostname 127.0.0.1 \
  --port 39731 \
  --api-token "tk_your_token" \
  --web-ui \
  --web-ui-prefix /admin
```

$env:TANDEM_WEB_UI="true"; .\src-tauri\binaries\tandem-engine.exe serve --hostname 127.0.0.1 --port 39731 --web-ui --state-dir .tandem-test

Open:

- `http://127.0.0.1:39731/admin`

The admin page expects a valid API token and keeps it in memory for the current tab/session.

## Environment Variable Mode

```bash
TANDEM_API_TOKEN=tk_your_token
TANDEM_WEB_UI=true
TANDEM_WEB_UI_PREFIX=/admin
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

## Common Headless Admin Endpoints

- `GET /global/health`
- `GET /channels/status`
- `PUT /channels/{name}`
- `DELETE /channels/{name}`
- `POST /admin/reload-config`
- `GET /memory`
- `DELETE /memory/{id}`

## Example: Check Health

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Tandem-Token: tk_your_token"
```

## Example: Check Channel Status

```bash
curl -s http://127.0.0.1:39731/channels/status \
  -H "X-Tandem-Token: tk_your_token"
```

## Security Notes

- Use `--api-token` (or `TANDEM_API_TOKEN`) whenever binding beyond localhost.
- Put TLS in front of Tandem when exposing it on a network.
- Do not expose the service directly to the public internet without a reverse proxy.

## See Also

- [Engine Commands](./reference/engine-commands/)
- [Configuration](./configuration/)
- [Running Tandem](./usage/)
- [Headless Deployment (Docker/systemd)](./desktop/headless-deployment/)
