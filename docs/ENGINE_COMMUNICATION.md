# Engine Communication Guide

This document explains how Tandem clients communicate with `tandem-engine`, how runs stream, and how desktop/TUI coordinate engine lifecycle.

## Components

- `tandem-engine` (Rust binary): the shared HTTP + SSE runtime and source of truth.
- Tauri desktop app (`apps/tandem-desktop/src/` + `apps/tandem-desktop/src-tauri/`): the native client shell; its root `index.html` is the desktop UI entrypoint, and it starts/stops an engine sidecar when needed.
- TUI (`crates/tandem-tui`): a terminal client that attaches to an existing engine when available, otherwise bootstraps/spawns one.
- Web control panel (`packages/tandem-control-panel/`): a browser-based client plus service bootstrap layer that connects to the same engine runtime.

## Default Endpoint Strategy

- Host: `127.0.0.1`
- Default port: `39731` (moved away from `3000` to avoid common frontend dev collisions)
- Desktop sidecar behavior:

1. Prefer configured/default port.
2. If unavailable, fall back to an ephemeral local port.

- TUI behavior:

1. Try configured/default base URL first.
2. If not healthy, spawn engine with configured/default port.

## Environment Overrides

- `TANDEM_ENGINE_PORT`
  - Engine CLI `serve` default (`--port`) via clap env binding.
  - Desktop sidecar preferred port default.
  - TUI connect/spawn port default.
- `TANDEM_ENGINE_HOST`
  - Engine CLI `serve` default (`--hostname`) via clap env binding.
- `TANDEM_ENGINE_URL`
  - TUI explicit base URL override (takes precedence over host/port composition).
- `TANDEM_API_TOKEN`
  - Sets the engine API token explicitly. If unset, `tandem-engine serve` loads or creates the shared token by default.
- `TANDEM_API_TOKEN_FILE`
  - Points the engine at an explicit non-empty token file before falling back to shared token storage.
- `TANDEM_UNSAFE_NO_API_TOKEN`
  - Advanced local-only opt-out. When set to `1`, direct engine serving runs without API token auth.
- `TANDEM_SHARED_ENGINE_MODE`
  - Desktop/TUI shared-engine behavior toggle.

## Runtime API Surface (High Level)

Core session/run endpoints:

- `POST /session` create session
- `GET /session` list sessions
- `POST /session/{id}/message` append message
- `POST /session/{id}/prompt_async` start async run
- `POST /session/{id}/prompt_sync` sync run path
- `GET /session/{id}/run` inspect active run
- `POST /session/{id}/run/{run_id}/cancel` cancel by run ID
- `POST /session/{id}/cancel` cancel active run
- `GET /event` SSE stream
- `GET /global/health` readiness/phase/build info

Compatibility aliases under `/api/...` are maintained where noted in server routes.

## Sampling Parameters

`POST /session` and `POST /session/{id}/prompt_async` (and `prompt_sync`)
accept optional sampling parameters that control the provider's decoding:

| Field         | Type    | Range (generic) | Notes                                                                           |
| ------------- | ------- | --------------- | ------------------------------------------------------------------------------- |
| `temperature` | number  | `0.0`–`2.0`     | Clamped per provider (Anthropic caps at `1.0`).                                 |
| `top_p`       | number  | `0.0`–`1.0`     | Also accepted as `topP`.                                                        |
| `max_tokens`  | integer | `>= 1`          | Overrides the engine's default max-tokens budget. Also accepted as `maxTokens`. |

All three are **optional**. Each field is sent only when present, so omitting
them produces a byte-identical provider request to prior behavior.

### Placement and precedence

- **Session default**: set on `POST /session` to apply to every prompt run on
  the session.
- **Per-prompt override**: set on the prompt request to override the session
  default for that run only.
- **Precedence** is resolved field by field: a per-prompt field wins; otherwise
  the session default is used; otherwise the field is omitted and the provider /
  engine default applies.

```jsonc
// POST /session  — session-level defaults
{ "title": "reviewer", "provider": "anthropic", "model": "claude-sonnet-4-6",
  "temperature": 0.1, "max_tokens": 2048 }

// POST /session/{id}/prompt_async  — per-prompt override
{ "parts": [{ "type": "text", "text": "..." }], "temperature": 0.7 }
```

### Per-provider mapping and clamping

- OpenAI-compatible providers (`openai`, `openrouter`, `together`, `groq`,
  `mistral`, `minimax`, custom `base_url`): mapped to `temperature`, `top_p`,
  `max_tokens` on the Chat Completions body. The OpenAI Responses API path uses
  `max_output_tokens`.
- Anthropic: mapped to `temperature` (clamped to `[0, 1]`), `top_p`, and
  `max_tokens`.
- Values outside a provider's accepted range are **clamped**, not rejected.
- Models that reject an explicit `temperature` (OpenAI reasoning models such as
  the o-series and gpt-5 reasoning variants) have the parameter **dropped with a
  logged warning** rather than failing the run.

SDK callers pass these as keyword arguments
(`sessions.create(..., temperature=, top_p=, max_tokens=)` and
`prompt_async(..., temperature=, top_p=, max_tokens=)`); see the Python/TypeScript
SDK reference.

## Desktop Flow

1. Resolve sidecar binary path (bundled/update/dev fallbacks).
2. Pick port (`TANDEM_ENGINE_PORT` or default `39731`, fallback ephemeral if occupied).
3. Spawn `tandem-engine serve --hostname 127.0.0.1 --port <port> --state-dir <canonical>`.
4. Poll `/global/health` until ready.
5. Route UI actions to engine HTTP APIs through `SidecarManager`.
6. Subscribe once to `/event` and fan out via `stream_hub` (`sidecar_event` + `sidecar_event_v2`).

Reference code:

- `apps/tandem-desktop/src-tauri/src/sidecar.rs`
- `apps/tandem-desktop/src-tauri/src/stream_hub.rs`
- `apps/tandem-desktop/src-tauri/src/commands.rs`

## TUI Flow

1. Compute base URL:

- `TANDEM_ENGINE_URL` if set
- else `http://127.0.0.1:<TANDEM_ENGINE_PORT|39731>`

2. Health-check existing engine.
3. If unavailable, ensure/download binary and spawn:

- `tandem-engine serve --port <configured_port>`

4. Use HTTP APIs directly through `EngineClient`.

Reference code:

- `crates/tandem-tui/src/app.rs`
- `crates/tandem-tui/src/net/client.rs`

## Run Lifecycle Contract

Recommended async pattern:

1. Append user message to session (`/session/{id}/message`).
2. Start run with `POST /session/{id}/prompt_async?return=run`.
3. Read response `runID` and `attachEventStream`.
4. Stream events from `/event?sessionID=<id>&runID=<runID>`.
5. On reconnect, recover via `GET /session/{id}/run` then re-attach.
6. Cancel with `/session/{id}/run/{run_id}/cancel` (preferred) or `/session/{id}/cancel`.

This is the contract used by desktop and validated in server/sidecar tests.

## Permissions and Questions

Engine emits permission/question requests during tool execution:

- Pending permissions: `GET /permission`
- Reply: `POST /permission/{id}/reply`
- Pending questions: `GET /question`
- Reply: `POST /question/{id}/reply`
- Reject: `POST /question/{id}/reject`

Desktop/TUI map these into their request-center UI flows.

## Observability and Diagnostics

- Health/readiness:
  - `GET /global/health`
- Structured logs:
  - Desktop: `tandem.desktop.*.jsonl`
  - Engine: `tandem.engine.*.jsonl`
  - TUI: `tandem.tui.*.jsonl`
- Correlation fields:
  - `correlation_id`, `session_id`, `run_id`, `org_id`, `workspace_id`
- Prometheus export:
  - `GET /metrics` when `TANDEM_OBSERVABILITY_PROMETHEUS_ENABLED=true`
  - See `docs/OBSERVABILITY_EXPORTS.md` for exported fields and scrub rules.

## Security Notes

- Engine binds to loopback (`127.0.0.1`) by default.
- Token auth is enabled by default. `TANDEM_API_TOKEN` sets an explicit token, otherwise the engine loads or creates the shared token file.
- `TANDEM_UNSAFE_NO_API_TOKEN=1` is the advanced local-only opt-out and should not be used for exposed deployments.
- Desktop sidecar mode uses the shared local API token and sends `X-Tandem-Token` on requests.
- Token persistence is keychain-first with fallback to the shared token file path.
- TUI uses the same shared token material and also sends `X-Tandem-Token`.
- Desktop Settings exposes token management UX: masked by default, explicit reveal, and copy.

## Practical Recommendations

- Keep `39731` as the default shared port for predictable desktop/TUI attach behavior.
- Use `TANDEM_ENGINE_PORT` when running multiple isolated dev stacks.
- Use `TANDEM_ENGINE_URL` in TUI for explicit remote/forwarded test setups.
- Avoid `3000` for engine defaults to reduce collisions with frontend dev servers.
- For headless installs, prefer `npm i -g @frumu/tandem` followed by `tandem install panel` and `tandem panel init` when you want the control-panel gateway layer. Use `tandem-engine serve` directly when you want the raw engine exposed only on localhost.
