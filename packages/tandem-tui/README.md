# Tandem TUI (npm Wrapper)

Terminal user interface for Tandem. This npm package installs the `tandem-tui` CLI and downloads the matching release asset.

If you want to build from Rust source instead, use the crate docs in `crates/tandem-tui/README.md`.

## Install

```bash
npm install -g @frumu/tandem-tui
```

The installer downloads the release asset that matches this package version. Tags and package versions are expected to match (for example, `v0.3.3`).

## Quick Start

Start the engine in one terminal:

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

Start the TUI in another terminal:

```bash
tandem-tui
```

On first run, the setup wizard walks you through selecting a provider, entering an API key, and choosing a default model. Keys are stored in the system keystore.

## Key Features

- Multi-session chat UI
- Request Center for approvals and questions
- Slash commands for fast navigation and configuration
- Agent and mission workflows

## Core Keybindings

- `Ctrl+N`: New session
- `Ctrl+W`: Close active session
- `Ctrl+C`: Cancel active agent (press twice to quit)
- `Alt+R`: Request Center
- `F1`: Help
- `F2`: Open docs

## Common Slash Commands

- `/help`
- `/providers`
- `/provider <id>`
- `/models`
- `/model <id>`
- `/sessions`
- `/new`
- `/title <name>`
- `/requests`
- `/approve <id>`
- `/deny <id>`

## Troubleshooting

- If the TUI shows a connection error, ensure the engine is running.
- If port 39731 is in use, start the engine with `--port` and set `TANDEM_ENGINE_PORT` for the TUI.

## Documentation

- TUI guide and reference: https://tandem.frumu.ai/docs
- GitHub releases: https://github.com/frumu-ai/tandem/releases
