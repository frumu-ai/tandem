---
title: Control Panel (Web Admin)
description: Run the official Tandem web control panel or scaffold an editable app from npm.
---

Use the control panel when you want a browser UI for chat, orchestrator, automations, memory, live feed, packs, and runtime ops.

The canonical operator flow now starts from `@frumu/tandem` and its master
`tandem` CLI. The panel is an add-on that you install when you want the web UI.

## Choose a path

### Official ready-to-run package

Use this when you want the supported packaged control panel with bootstrap and service helpers.

```bash
npm i -g @frumu/tandem
tandem install panel
tandem panel init
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
tandem panel init
```

This creates/updates `.env` and ensures an engine token is available.

## Official package: run

```bash
tandem panel open
```

Open:

- `http://127.0.0.1:39732`

Aliases:

- `tandem`
- `tandem-setup`
- `tandem-control-panel`
- `tandem-control-panel-init` (init only)

## Official package: optional service install (Linux)

```bash
sudo tandem service install
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

- `Create`
- `Calendar`
- `List`
- `Tasks`
- `Optimize`
- `Active Teams`
- `AI Composer` when the feature flag is enabled

A built-in walkthrough wizard can be launched from the page header and also auto-opens for first-time empty workspaces.

The optional composer tab is designed for prompt-first workflow authoring. It reuses the existing planner chat transport, so the UI can draft an automation, ask a clarification question, preview the resulting `automationsV2` payload, and then create or run it directly.

Enable it with `?composer=1` or `#composer=true` when the workspace has the feature flag turned on.

Legacy `#/agents` links continue to redirect for backwards compatibility.

### AI Composer: Agent-Creation Test Mode

The **AI Composer** includes a test switch for agents:

- send create/run requests with `x-tandem-agent-test-mode: 1`
- set `x-tandem-request-source: agent`
- set `x-tandem-agent-id` to a synthetic id you control

Use this mode only for validation:

- to confirm your target agent id and capability set
- to verify `AUTOMATION_V2_*` governance failures during development
- to rehearse approval and review behavior before shipping a real external workflow creator

Create calls still use the panel engine token and the same local engine instance; only the actor
classification and resulting governance checks are switched to agent mode.

Deep-link query state is supported on `#/automations`:

- `tab`
- `wizard`
- `flow` (`routine` or `advanced`)
- `step`
- `composer` (`1`, `true`, `on`, or `yes`)

## Studio Workflow Builder

The **Studio** page (`#/studio`) provides a template-first workflow builder for creating multi-agent workflows.

Key features:

- **Starter templates** with editable per-agent role prompts, stage/dependency editing
- **Saved Studio workflow cards** with latest run status and stability snapshots
- **Shared workspace browser** for selecting workflow root folders
- **Direct save/run flows** into `automation_v2`

Studio templates compile research-heavy workflows into explicit stages:

- `discover` — File/content discovery
- `local-source` — Local file research
- `external-research` — Web research
- `finalize` — Artifact writing

Planner previews also surface project-scoped knowledge defaults and rollout guardrails so operators can see when Tandem will reuse promoted knowledge before recomputing.

When you are authoring staged workflows or missions, use [Prompting Workflows And Missions](./prompting-workflows-and-missions/) as the recommended guide for turning human intent into reliable Tandem prompts and handoffs.

If you want the prompt-first automation builder specifically, start with [Build an Automation With the AI Assistant](./automation-composer-workflows/).

If an external agent needs to create or run those missions through the engine APIs, also use [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/) and [Engine Authentication For Agents](./engine-authentication-for-agents/).

For a copy-paste, proof-style runbook for this exact path, see [Automation Examples For Teams](./automation-examples-for-teams/).

Saved workflows auto-migrate to `workflow_structure_version = 2` while preserving automation IDs and original research node IDs used by downstream nodes.

## Edit Workflow Automation Dialog (Handoffs Tab)

The **Edit workflow automation** dialog (Automations page → three-dot menu → Edit) now includes a **Handoffs** tab that exposes three connected-agent fields on V2 automations:

### Handoff config

Controls the directory layout for staged handoff artifacts, relative to the automation's workspace root.

- **Auto-approve toggle** — when on (default), artifacts move directly to the approved directory; when off, they wait in the inbox for a manual review step
- **Inbox directory** — where agents write new output artifacts (default: `shared/handoffs/inbox`)
- **Approved directory** — where promoted artifacts land (default: `shared/handoffs/approved`)
- **Archived directory** — where old artifacts are retired (default: `shared/handoffs/archived`)
- **Reset** — restores all fields to system defaults

### Scope policy

Defines the filesystem sandbox for all agents in the automation. Empty = open access.

- **Readable paths** — agents may read files at or under these paths
- **Writable paths** — agents may write to these paths (should be a subset of readable)
- **Denied paths** — always blocked, takes priority over readable/writable
- **Watch paths** — paths the watch evaluator may scan; defaults to readable paths if empty
- **Clear** — removes all restrictions (reverts to open policy)

Path entries use one path per line, prefix matching relative to workspace root.

### Watch conditions

Array of filesystem conditions that the automation evaluator checks. Each condition has a `path` and a `condition` type (`any_file_present`, `modified_since_last_run`, `empty`).

See [Connected-Agent Handoffs](./connected-agent-handoffs/) for the full reference including API shapes, HTTP examples, and WorkflowEditDraft type annotations.

## Optimize Tab (AutoResearch)

The **Optimize** tab (`#/automations?tab=optimize`) provides a UI for workflow prompt/objective optimization campaigns.

Features:

- **Campaign creation** with workflow selection and artifact references
- **Campaign detail** view with experiment listing and status
- **Experiment inspection** for reviewing candidate evaluation results
- **Approve/Reject/Apply controls** for promotion workflow

Campaigns generate bounded candidate prompts, evaluate them against baseline runs, and apply approved winners back to the live workflow.

Available campaign actions:

- `queue_replay` — Queue a baseline replay run to re-establish metrics
- `generate_candidate` — Generate the next bounded candidate for evaluation
- `approve` / `reject` — Mark an experiment as approved or rejected
- `apply` — Apply an approved winner to the live workflow with drift checks and audit metadata

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

This panel is for browser host diagnostics. Actual browser automation still runs through the engine tool system using tools such as `browser_open`, `browser_click`, and `browser_screenshot`.

## Provider Settings in Settings

The **Providers** section in Settings opens expanded by default so provider setup is visible immediately when you land there.

The control panel only asks the engine for live provider catalog data when a section actually needs it. Providers that require an API key are skipped if no key is configured, which keeps the Settings page from waiting on discovery work that cannot succeed yet.

OpenAI Codex still shows its starter catalog even before live discovery is available, so you can set defaults and connect the account without waiting for a remote model scan.

## Verify Engine + Panel

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

## See Also

- [Headless Service](https://docs.tandem.ac/headless-service/)
- [Channel Integrations](https://docs.tandem.ac/channel-integrations/)
- [Configuration](https://docs.tandem.ac/configuration/)
- [Build from Source](https://docs.tandem.ac/build-from-source/)
