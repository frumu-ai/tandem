---
title: Control Panel (Web Admin)
description: Install and run the Tandem web control panel from npm.
---

Use the control panel when you want a browser UI for chat, routines, channels, memory, and ops.

## Install

```bash
npm i -g @frumu/tandem-panel
```

## Initialize Environment (Recommended)

```bash
tandem-control-panel --init
```

This creates/updates `.env` and ensures an engine token is available.

## Run

```bash
tandem-control-panel
```

Open:

- `http://127.0.0.1:39732`

Aliases:

- `tandem-setup`
- `tandem-control-panel-init` (init only)

## Optional: Install systemd Services (Linux)

```bash
sudo tandem-control-panel --install-services
```

Useful options:

- `--service-mode=both|engine|panel` (default `both`)
- `--service-user=<linux-user>`

## Core Environment Variables

- `TANDEM_CONTROL_PANEL_PORT` (default `39732`)
- `TANDEM_ENGINE_URL` (default `http://127.0.0.1:39731`)
- `TANDEM_CONTROL_PANEL_AUTO_START_ENGINE` (`1` or `0`)
- `TANDEM_CONTROL_PANEL_ENGINE_TOKEN` (engine API token)

## Verify Engine + Panel

```bash
curl -s http://127.0.0.1:39731/global/health \
  -H "X-Agent-Token: tk_your_token"
```

## See Also

- [Headless Service](./headless-service/)
- [Channel Integrations](./channel-integrations/)
- [Configuration](./configuration/)
