---
title: Control Panel (Web Admin)
description: Run the official Tandem web control panel or scaffold an editable app from npm.
---

Use the control panel when you want a browser UI for chat, orchestrator, automations, memory, live feed, packs, and runtime ops.

## Choose a path

### Official ready-to-run package

Use this when you want the supported packaged control panel with bootstrap and service helpers.

```bash
npm i -g @frumu/tandem-panel
```

### Editable app scaffold

Use this when you want the actual app source in your own folder so you can customize routes, pages, themes, styles, and runtime behavior.

```bash
npm create tandem-panel@latest my-panel
cd my-panel
npm install
npm run dev
```

The generated app includes editable `src/`, `server/`, local runtime scripts, and local config files.

## Official package: initialize environment

```bash
tandem-control-panel --init
```

This creates/updates `.env` and ensures an engine token is available.

## Official package: run

```bash
tandem-control-panel
```

Open:

- `http://127.0.0.1:39732`

Aliases:

- `tandem-setup`
- `tandem-control-panel-init` (init only)

## Official package: optional service install (Linux)

```bash
sudo tandem-setup service install
```

Useful options:

- `--service-mode=both|engine|panel` (default `both`)
- `--service-user=<linux-user>`

## Editable scaffold: customize

After scaffolding:

- edit `src/pages/` and `src/app/` for routes and views
- edit `src/app/themeContract.ts` and `src/app/themes.js` for themes
- edit `server/` and `bin/setup.js` for runtime behavior
- run `npm run build` for a production bundle
- run `npm run start` to serve the built app locally

## Core Environment Variables

- `TANDEM_CONTROL_PANEL_PORT` (default `39732`)
- `TANDEM_ENGINE_URL` (default `http://127.0.0.1:39731`)
- `TANDEM_CONTROL_PANEL_AUTO_START_ENGINE` (`1` or `0`)
- `TANDEM_CONTROL_PANEL_ENGINE_TOKEN` (engine API token)
- `TANDEM_SEARCH_BACKEND` (`tandem`, `brave`, `exa`, `searxng`, or `none`)
- `TANDEM_SEARCH_URL` (default hosted Tandem search endpoint for official installs)
- `TANDEM_BRAVE_SEARCH_API_KEY` / `TANDEM_EXA_API_KEY` (optional direct-provider overrides in `engine.env`)

## Automations + Cost (Dashboard)

The main dashboard includes a first-class **Automations + Cost** section that aggregates:

- Token usage (`24h`, `7d`) from run telemetry.
- Estimated USD cost (`24h`, `7d`).
- Top automation/routine IDs by estimated cost, token volume, and run count.

This includes legacy automations/routines and advanced multi-agent automation runs.

Cost estimation uses the engine rate:

- `TANDEM_TOKEN_COST_PER_1K_USD` (USD per 1,000 tokens, default `0`).

## Control Panel Shell

The control panel now uses a shell with:

- an icon rail for primary navigation
- a context rail for system status and actions
- a main workspace with animated route transitions and page headers

The web app intentionally pushes motion a bit further than the Tauri app while keeping the same overall visual language.

## Automations Workspace (Tabbed + Wizard)

The left nav `Automations` page (`#/automations`) now uses task-focused tabs:

- `Overview`
- `Routines`
- `Automations`
- `Templates`
- `Runs & Approvals`

A built-in walkthrough wizard can be launched from the page header and also auto-opens for first-time empty workspaces.

Legacy `#/agents` links continue to redirect for backwards compatibility.

Deep-link query state is supported on `#/automations`:

- `tab`
- `wizard`
- `flow` (`routine` or `advanced`)
- `step`

## Workflow Operations (Packs)

The Packs area includes workflow operations so operators can validate workflow packs without leaving the control panel.

From the workflow operations view you can:

- inspect loaded workflows and hooks
- toggle workflow hooks on or off
- run workflow validation/reload
- simulate workflow events
- run workflows directly
- stream workflow events live while testing

This UI maps to engine workflow endpoints including:

- `GET /workflows`
- `GET /workflows/{id}`
- `POST /workflows/validate`
- `POST /workflows/simulate`
- `POST /workflows/{id}/run`
- `GET /workflows/events`
- `GET /workflow-hooks`
- `PATCH /workflow-hooks/{id}`

## Orchestrator Event Streaming

The orchestrator UI supports multiplex run event streaming so multiple run timelines can stay live at once.

- prefers `GET /context/runs/events/stream` when available on the connected engine
- exposes control-panel stream health via `/api/orchestrator/events/health`
- falls back to per-run event bridging if multiplex streaming is unavailable

## Browser Diagnostics in Settings

Settings includes a Browser Diagnostics panel for operator checks.

You can use it to:

- read current readiness from `GET /browser/status`
- trigger sidecar install with `POST /browser/install`
- run an end-to-end browser check with `POST /browser/smoke-test`

## Verify Engine + Panel

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

## See Also

- [Headless Service](./headless-service/)
- [Channel Integrations](./channel-integrations/)
- [Configuration](./configuration/)
- [Build from Source](./build-from-source/)
