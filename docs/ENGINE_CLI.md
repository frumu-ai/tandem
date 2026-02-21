# Tandem Engine CLI Guide

This guide documents `tandem-engine` using bash commands (macOS/Linux/WSL).

## Quick Start

```bash
tandem-engine --help
tandem-engine serve --hostname 127.0.0.1 --port 39731
tandem-engine run "Summarize this repository"
```

## Command Overview

### `serve`

Starts the HTTP/SSE runtime used by desktop and TUI clients.

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

Useful options:

- `--hostname` (alias: `--host`)
- `--port`
- `--state-dir`
- `--provider`
- `--model`
- `--api-key`
- `--config`
- `TANDEM_ENGINE_HOST` (env override)
- `TANDEM_ENGINE_PORT` (env override)
- `TANDEM_API_TOKEN` (optional API auth token requirement)

### `status`

Checks engine health by calling `GET /global/health`.

```bash
tandem-engine status
tandem-engine status --hostname 127.0.0.1 --port 39731
```

## Engine-Detected Execution Environment

Engine runtime now detects host environment once at startup and treats it as canonical:

- `os`: `windows|linux|macos`
- `shell_family`: `powershell|posix`
- `path_style`: `windows|posix`
- `arch`: host architecture (for example `x86_64`, `aarch64`)

This environment is injected into run prompts, exposed via `GET /global/health` (`environment`),
and attached to `session.run.started` events so all clients (Desktop, TUI, HTTP) see identical OS context.

OS-aware prompt behavior can be controlled with:

- `TANDEM_OS_AWARE_PROMPTS=1` (default on)
- `TANDEM_OS_AWARE_PROMPTS=0` to disable environment prompt injection

### `run`

Runs one prompt and prints the model response.

```bash
tandem-engine run "Write a status update" --provider openrouter --model openai/gpt-4o-mini
```

### `parallel` (Concurrent Tasks)

Runs multiple prompts concurrently and prints a JSON summary.

```bash
cat > tasks.json << 'JSON'
{
  "tasks": [
    { "id": "science", "prompt": "Explain why the sky appears blue in 5 bullet points", "provider": "openrouter" },
    { "id": "writing", "prompt": "Write a concise professional status update for a weekly team sync", "provider": "openrouter" },
    { "id": "planning", "prompt": "Create a simple 3-step plan to learn Rust over 4 weeks", "provider": "openrouter" }
  ]
}
JSON

tandem-engine parallel --json @tasks.json --concurrency 3
```

### Web Research From CLI

Prompt-driven:

```bash
tandem-engine run "Summarize this repository https://github.com/frumu-ai/tandem"
```

Direct tools:

```bash
tandem-engine tool --json '{"tool":"webfetch_document","args":{"url":"https://github.com/frumu-ai/tandem","return":"both","mode":"auto"}}'
tandem-engine tool --json '{"tool":"websearch","args":{"query":"frumu tandem engine architecture","limit":5}}'
```

### `tool`

Executes a built-in tool call.

Input formats:

- raw JSON string
- `@path/to/file.json`
- `-` (stdin)

```bash
tandem-engine tool --json '{"tool":"workspace_list_files","args":{"path":"."}}'
tandem-engine tool --json @payload.json
cat payload.json | tandem-engine tool --json -
```

Payload shape:

```json
{
  "tool": "workspace_list_files",
  "args": {
    "path": "."
  }
}
```

### `providers`

Lists supported provider IDs.

```bash
tandem-engine providers
```

## Provider API Keys (CLI/API)

`tandem-engine` does not yet expose a direct `key set` subcommand.  
Use the engine HTTP API while `serve` is running.

Temporary (in-memory) auth:

- `PUT /auth/{provider}` stores a runtime-only token.
- It does **not** persist across engine restarts.

```bash
API="http://127.0.0.1:39731"
```

Set a provider key (example: `openrouter`):

```bash
curl -s -X PUT "$API/auth/openrouter" \
  -H 'content-type: application/json' \
  -d '{"apiKey":"YOUR_OPENROUTER_KEY"}'
```

Set another provider key (example: `openai`):

```bash
curl -s -X PUT "$API/auth/openai" \
  -H 'content-type: application/json' \
  -d '{"apiKey":"YOUR_OPENAI_KEY"}'
```

Persistent provider defaults (safe):

- `PATCH /config` for non-secret defaults only (for example `default_provider`, `default_model`).
- `providers.<id>.api_key` is rejected by design.

```bash
curl -s -X PATCH "$API/config" \
  -H 'content-type: application/json' \
  -d '{"default_provider":"openrouter","providers":{"openrouter":{"default_model":"openai/gpt-4o-mini"}}}' | jq .
```

Verify configured providers:

```bash
curl -s "$API/config/providers" | jq .
```

Attempting to write secret keys via config now returns:

- `400 CONFIG_SECRET_REJECTED`
- Use `PUT /auth/{provider}` or environment variables instead.

Delete a provider key:

```bash
curl -s -X DELETE "$API/auth/openrouter"
```

WSL note:

- If engine runs on Windows and curl runs in WSL, replace `127.0.0.1` with your Windows host IP:

```bash
WIN_HOST="$(awk '/nameserver/ {print $2; exit}' /etc/resolv.conf)"
API="http://${WIN_HOST}:39731"
```

Config file locations:

- State/project config (engine-local): `<state_dir>/config.json`
- Global config (from `dirs::config_dir()`):
- Linux: `~/.config/tandem/config.json`
- macOS: `~/Library/Application Support/tandem/config.json`
- Windows: `%APPDATA%\\tandem\\config.json`
- Override global location with `TANDEM_GLOBAL_CONFIG`

## API Token Security (VPS/Public Deployments)

Generate a token:

```bash
tandem-engine token generate
```

Run engine with token requirement:

```bash
TANDEM_API_TOKEN="tk_your_token_here" tandem-engine serve --hostname 0.0.0.0 --port 39731
```

Send authenticated requests:

```bash
curl -s "$API/global/health" -H "Authorization: Bearer $TANDEM_API_TOKEN" | jq .
curl -s "$API/config/providers" -H "X-Tandem-Token: $TANDEM_API_TOKEN" | jq .
```

Rotate token at runtime (requires current token header):

```bash
curl -s -X POST "$API/auth/token/generate" \
  -H "Authorization: Bearer $TANDEM_API_TOKEN" | jq .
```

Desktop first-run behavior:

- Tandem Desktop auto-generates an engine API token on first launch.
- Desktop passes this token to the sidecar via `TANDEM_API_TOKEN`.
- Desktop also sends the token on sidecar HTTP requests using `X-Tandem-Token`.
- TUI uses the same shared token and sends `X-Tandem-Token` as well.
- Token storage is keychain-first with fallback.
- Primary backend is `keychain` (Windows Credential Manager / macOS Keychain / Linux Secret Service).
- Fallback backend is shared token file storage when keychain is unavailable/locked.

Desktop token storage path:

- Windows: `%APPDATA%\\tandem\\security\\engine_api_token`
- macOS: `~/Library/Application Support/tandem/security/engine_api_token`
- Linux: `~/.local/share/tandem/security/engine_api_token`

Notes:

- Desktop Settings shows token masked by default with `Reveal` and `Copy`.
- Settings also shows token storage backend (`keychain`, `file`, `env`, or `memory`).
- TUI token commands include `/engine token` (masked) and `/engine token show` (full token + storage backend + fallback path).

### `chat`

Reserved for future interactive REPL support.

## Serve + API Workflow

Start engine:

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

In a second terminal:

```bash
# 1) Create session
SID="$(curl -s -X POST 'http://127.0.0.1:39731/session' -H 'content-type: application/json' -d '{}' | jq -r '.id')"

# 2) Build message payload
MSG='{"parts":[{"type":"text","text":"Give me 3 practical Rust learning tips."}]}'

# 3) Append message
curl -s -X POST "http://127.0.0.1:39731/session/$SID/message" -H 'content-type: application/json' -d "$MSG" >/dev/null

# 4) Start async run and get stream path
RUN_JSON="$(curl -s -X POST "http://127.0.0.1:39731/session/$SID/prompt_async?return=run" -H 'content-type: application/json' -d "$MSG")"
ATTACH_PATH="$(echo "$RUN_JSON" | jq -r '.attachEventStream')"
echo "$RUN_JSON" | jq .

# 5) Stream events
curl -N "http://127.0.0.1:39731${ATTACH_PATH}"
```

Synchronous one-shot response:

```bash
RESP="$(curl -s -X POST "http://127.0.0.1:39731/session/$SID/prompt_sync" -H 'content-type: application/json' -d "$MSG")"
echo "$RESP" | jq .
```

Extract latest assistant text from response history:

```bash
echo "$RESP" | jq -r '[.[] | select(.info.role=="assistant")][-1].parts[] | select(.type=="text") | .text'
```

## State Directory Resolution

When `--state-dir` is omitted:

1. `--state-dir`
2. `TANDEM_STATE_DIR`
3. Shared Tandem canonical path
4. Local fallback `.tandem`

## Troubleshooting

- `unsupported provider ...`: run `tandem-engine providers`
- `tool is required in input json`: include non-empty `tool`
- `invalid hostname or port`: verify `--hostname` / `--port`

For Windows users, run these commands in WSL for the same behavior.
