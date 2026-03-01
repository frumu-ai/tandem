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
- `GET /global/storage/files?path=channel_uploads&limit=200`
- `POST /admin/reload-config`
- `POST /memory/put`
- `POST /memory/search`
- `GET /memory`
- `POST /memory/promote`
- `POST /memory/demote`
- `DELETE /memory/{id}`

## Example: Check Health

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

## Example: Check Channel Status

```bash
curl -s http://127.0.0.1:39731/channels/status \
  -H "X-Agent-Token: tk_your_token"
```

## Channel Uploads and Media

Channel adapters can persist inbound attachments under the engine storage root in `channel_uploads/...`.
You can inspect saved files with:

```bash
curl -s "http://127.0.0.1:39731/global/storage/files?path=channel_uploads&limit=200" \
  -H "X-Agent-Token: tk_your_token"
```

Typical media flow:

1. Channel receives media (`photo`, `document`, etc).
2. Adapter stores file under `channel_uploads/<channel>/<chat_or_user>/...`.
3. Adapter forwards prompt parts to engine with both text and file parts.
4. Runs stream back over SSE like normal chat runs.

If provider/model cannot use a given media type, the run should still complete with text fallback guidance instead of hanging.

## Security Notes

- Use `--api-token` (or `TANDEM_API_TOKEN`) whenever binding beyond localhost.
- Put TLS in front of Tandem when exposing it on a network.
- Do not expose the service directly to the public internet without a reverse proxy.

## See Also

- [Engine Commands](./reference/engine-commands/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Configuration](./configuration/)
- [Running Tandem](./usage/)
- [Headless Deployment (Docker/systemd)](./desktop/headless-deployment/)
