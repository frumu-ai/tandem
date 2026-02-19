---
title: Running Tandem
---

Tandem consists of two main components that work together: the **Engine** (server) and the **TUI** (client).

## 1. Start the Engine

The engine must be running for clients to connect.

```bash
# Start the engine server
tandem-engine
```

By default, the engine listens on `http://127.0.0.1:39731`. You can configure the port and other settings via environment variables (see [Configuration](./configuration/)).

## 2. Start the TUI

Open a new terminal window and start the Terminal User Interface:

```mermaid
graph LR
    A[User] -- TUI --> B[tandem-tui]
    B -- HTTP --> C[tandem-engine]
```

```bash
# Start the TUI
tandem-tui
```

The TUI will attempt to connect to the local engine. If the engine is not running, the TUI will display a connection error or waiting status.

When a shared engine is already running, TUI now checks engine version before attaching. If the running engine is older than the TUI-required version, TUI defaults to replacing it with a fresh managed engine.

## Headless Service Mode

If you want to run Tandem as a service (without launching TUI), see:

- [Headless Service](./headless-service/)

## Troubleshooting

- **Connection Refused**: Ensure `tandem-engine` is running in a separate terminal.
- **Port Conflicts**: If port 39731 is in use, change the engine's port via `TANDEM_ENGINE_PORT`.
- **Stale Shared Engine**: Control startup behavior with `TANDEM_ENGINE_STALE_POLICY`:
  - `auto_replace` (default): replace stale engine automatically.
  - `fail`: stop with upgrade instructions.
  - `warn`: continue and attach anyway.
