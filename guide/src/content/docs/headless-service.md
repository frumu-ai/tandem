---
title: Headless Service
---

Run Tandem as a standalone headless service with the master `tandem` CLI and optional embedded web admin.

## Managed install

```bash
npm i -g @frumu/tandem
tandem doctor
tandem service install
tandem install panel
tandem panel init
```

Use `tandem status` and `tandem service status` to confirm the engine service is running.

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
tandem panel status
tandem panel open
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
tandem status
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

## Common Headless Admin Endpoints

- `GET /global/health`
- `GET /browser/status`
- `POST /browser/install`
- `POST /browser/smoke-test`
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

These browser endpoints are for readiness, installation, and smoke testing. Actual browser automation in headless deployments goes through engine tools such as `browser_open`, `browser_click`, and `browser_screenshot` via `POST /tool/execute` or session-based runs with explicit tool allowlists.

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

## Example: Browser Readiness + Install

```bash
curl -s http://127.0.0.1:39731/browser/status \
  -H "X-Agent-Token: tk_your_token"

curl -s -X POST http://127.0.0.1:39731/browser/install \
  -H "X-Agent-Token: tk_your_token"

curl -s -X POST http://127.0.0.1:39731/browser/smoke-test \
  -H "X-Agent-Token: tk_your_token"
```

## Example: Browser Automation via Tool Execution

```bash
curl -s -X POST http://127.0.0.1:39731/tool/execute \
  -H "X-Agent-Token: tk_your_token" \
  -H "content-type: application/json" \
  -d '{"tool":"browser_open","args":{"url":"https://example.com"}}'
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

## Headless and Channel Memory

When channels are enabled on a headless engine, Tandem keeps memory in two layers:

1. full session transcript history in normal session storage
2. compact global retrieval memory containing exact user-visible completed user+assistant exchanges

This is designed so that:

- long-running channel bots can recall prior work across sessions
- memory retrieval stays much smaller than full transcript replay
- the raw transcript remains preserved even if retrieval memory is compacted later

For the storage-level breakdown of these layers, see [Memory Internals](https://docs.tandem.ac/memory-internals/).

## Security Notes

- Use `--api-token` (or `TANDEM_API_TOKEN`) whenever binding beyond localhost.
- Put TLS in front of Tandem when exposing it on a network.
- Do not expose the service directly to the public internet without a reverse proxy.

If an agent or external service needs to create workflows, missions, or automations against this engine, start with [Engine Authentication For Agents](https://docs.tandem.ac/engine-authentication-for-agents/) and [Creating And Running Workflows And Missions](https://docs.tandem.ac/creating-and-running-workflows-and-missions/).

## See Also

- [Engine Commands](https://docs.tandem.ac/reference/engine-commands/)
- [MCP Automated Agents](https://docs.tandem.ac/mcp-automated-agents/)
- [Configuration](https://docs.tandem.ac/configuration/)
- [Start Here](https://docs.tandem.ac/start-here/)
- [Headless Deployment (Docker/systemd)](https://docs.tandem.ac/desktop/headless-deployment/)
