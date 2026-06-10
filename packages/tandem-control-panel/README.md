# Tandem Control Panel

Full web control center for Tandem Engine (non-desktop entry point).

This package remains the control-panel add-on during migration. The canonical
master CLI now lives in `@frumu/tandem` as `tandem`, and this package keeps
`tandem-setup` / `tandem-control-panel` as compatibility shims.

## Fastest Ways To Start

- Web control panel: `npm i -g @frumu/tandem && tandem install panel && tandem panel init`
- Raw local engine: `npm i -g @frumu/tandem && tandem-engine serve --hostname 127.0.0.1 --port 39731`
- Terminal UI: `npm i -g @frumu/tandem-tui && tandem-tui`
- SDK usage: `npm install @frumu/tandem-client` or `pip install tandem-client`

If you are not sure which path to take, start with the web control panel. It gives you the engine, panel, token setup, and service install flow in one place.

## Install

```bash
npm i -g @frumu/tandem-panel
```

## Docker

If you want a containerized install, there is a ready-to-run compose setup in [`DOCKER.md`](./DOCKER.md).

It installs the engine from npm and the panel from the checked-in package source, runs them on the same Docker network, and keeps the engine token in `./secrets/tandem_api_token` so the browser can log in with the same key the engine uses.

By default the Docker panel listens on `39734` so it can sit next to the host engine and any older panel instance.

From this folder, the one-command start is:

```bash
npm run docker:up
```

To print the current engine token after the first boot:

```bash
npm run docker:token
```

## Editable App Scaffold

```bash
npm create tandem-panel@latest my-panel
```

Use the global install when you want the official ready-to-run panel.

Use the scaffold when you want the actual app source in your own folder so you can edit routes, pages, themes, styles, and runtime behavior without customizing files inside `node_modules`.

## Official Bootstrap

```bash
npm i -g @frumu/tandem
tandem install panel
tandem panel init
```

This creates a canonical env file, bootstraps engine state, and installs services on Linux/macOS when run with the privileges needed for service registration.

Useful follow-up commands:

```bash
tandem doctor
tandem status
tandem service install
tandem service status
tandem service restart
tandem panel doctor
tandem panel open
```

## Run Foreground

```bash
tandem-control-panel
```

Or:

```bash
tandem-setup run
```

## Service Management

```bash
tandem-setup service install
tandem-setup service status
tandem-setup service restart
tandem-setup service logs
```

Legacy flag mode is still supported for compatibility, but new installs should
prefer `tandem` and the panel add-on commands:

`tandem-control-panel --init`, `--install-services`, and `--service-op=...`

## Features

- Token-gated web portal
- Dashboard + health overview
- Coding workflows dashboard for multi-project run visibility
- Chat + session management
- Routines/automations
- Channels (Telegram/Discord/Slack)
- MCP server management
- Node-based swarm orchestration + live flow visualization
- Memory browsing/search/delete
- Agent teams + mission approvals
- Global live event feed
- Provider settings

## Environment

Copy and customize if needed:

```bash
cp .env.example .env
```

Variables:

- `TANDEM_CONTROL_PANEL_PORT` (default `39732`)
- `TANDEM_CONTROL_PANEL_HOST` (default `127.0.0.1`)
- `TANDEM_CONTROL_PANEL_PUBLIC_URL` (optional future pairing / gateway URL)
- `TANDEM_ENGINE_URL` (default `http://127.0.0.1:39731`)
- `TANDEM_ENGINE_HOST` + `TANDEM_ENGINE_PORT` fallback
- `TANDEM_STATE_DIR` (canonical engine state dir for official installs)
- `TANDEM_CONTROL_PANEL_STATE_DIR` (control-panel state dir for official installs)
- `TANDEM_CONTROL_PANEL_AUTO_START_ENGINE` (`1`/`0`)
- `TANDEM_CONTROL_PANEL_ENGINE_TOKEN` (token injected when panel auto-starts engine)
- `TANDEM_API_TOKEN` (backward-compatible alias for engine token)
- `TANDEM_CONTROL_PANEL_SESSION_TTL_MINUTES` (default `1440`)
- `TANDEM_SEARCH_BACKEND` (`auto`, `tandem`, `brave`, `exa`, `searxng`, or `none`; default official installs use `auto`)
- `TANDEM_SEARCH_URL` (hosted Tandem search endpoint or compatible router URL)
- `TANDEM_SEARCH_TIMEOUT_MS` (default `10000`)
- `TANDEM_BRAVE_SEARCH_API_KEY` (optional direct Brave override when `TANDEM_SEARCH_BACKEND=brave`)
- `TANDEM_EXA_API_KEY` (optional direct Exa override when `TANDEM_SEARCH_BACKEND=exa`)
- `TANDEM_SEARXNG_URL` (optional self-hosted override when `TANDEM_SEARCH_BACKEND=searxng`)

The desktop app now exposes these search settings directly in Settings, and the control panel exposes them under Settings -> Web Search when it is connected to a local engine host.

- `TANDEM_DISABLE_TOOL_GUARD_BUDGETS` (`1` disables per-run guard budgets; default in installer/service env is `0`)
- `TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS` (default `5000`)
- `TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS` (default `30000`)
- `TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS` (default `90000`)
- `TANDEM_BASH_TIMEOUT_MS` (default `30000`)
- `TANDEM_TOOL_BUDGET_DEFAULT`, `TANDEM_TOOL_BUDGET_BATCH`, `TANDEM_TOOL_BUDGET_WEBSEARCH`,
  `TANDEM_TOOL_BUDGET_READ`, `TANDEM_TOOL_BUDGET_SEARCH`, `TANDEM_TOOL_BUDGET_GLOB` (used when guards are enabled)

## Token Behavior

- If the panel connects to an already-running engine, use that engine's API token to sign in.
- If the panel auto-starts an engine, it now always starts with a known token:
  - `TANDEM_CONTROL_PANEL_ENGINE_TOKEN` (preferred), or
  - `TANDEM_API_TOKEN` (alias), or
  - auto-generated at startup (printed in panel logs).

## Tool Guard Budgets

Default installs now enforce guard budgets:

```bash
TANDEM_DISABLE_TOOL_GUARD_BUDGETS=0
```

To tune caps, set:

```bash
TANDEM_DISABLE_TOOL_GUARD_BUDGETS=0
TANDEM_TOOL_BUDGET_DEFAULT=10
TANDEM_TOOL_BUDGET_BATCH=10
TANDEM_TOOL_BUDGET_WEBSEARCH=8
TANDEM_TOOL_BUDGET_READ=8
TANDEM_TOOL_BUDGET_SEARCH=6
TANDEM_TOOL_BUDGET_GLOB=4
```

Notes:

- Unknown tools use `TANDEM_TOOL_BUDGET_DEFAULT`.
- `0|none|unlimited|infinite|inf` for a budget key means no cap for that key.

## Setup Flow

1. Run `tandem panel init` or `tandem-setup init`.
2. Verify with `tandem panel doctor` or `tandem-setup doctor`.
3. If running foreground, start `tandem-control-panel`.
4. Sign in with the printed `TANDEM_CONTROL_PANEL_ENGINE_TOKEN`.

## Development

```bash
cd packages/tandem-control-panel
npm install
npm run dev
npm run build
```

See [`CODING_WORKFLOWS_PLAN.md`](./CODING_WORKFLOWS_PLAN.md) for the current roadmap behind the coding-workflows section.

### Repo Source Workflow (No Global npm Install)

If you run from the repo root, use:

```bash
node packages/tandem-control-panel/bin/cli.js init --no-service
node packages/tandem-control-panel/bin/cli.js run
```

If you are already inside `packages/tandem-control-panel`, use:

```bash
node bin/cli.js init --no-service
node bin/cli.js run
```

Service install/ops from source from the repo root:

```bash
sudo node packages/tandem-control-panel/bin/cli.js service install
node packages/tandem-control-panel/bin/cli.js service status
sudo node packages/tandem-control-panel/bin/cli.js service restart
```

Service install/ops from inside `packages/tandem-control-panel`:

```bash
sudo node bin/cli.js service install
node bin/cli.js service status
sudo node bin/cli.js service restart
```
