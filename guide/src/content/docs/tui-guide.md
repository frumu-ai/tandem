---
title: TUI Guide
---

The Tandem TUI provides a terminal-based interface for interacting with AI agents, managing sessions, and running tools.

## Running the TUI

Launch the TUI from your terminal:

```bash
tandem-tui
```

The TUI attempts to attach to an existing engine first. If none is available, it downloads and bootstraps a local engine automatically.

## Workflows

### Setup Wizard

On your first run, Tandem will guide you through the initial configuration:

1. **Welcome**: Introduction to the tool.
2. **Select Provider**: Choose your AI provider (e.g., Anthropic, OpenAI, Ollama).
3. **Enter API Key**: Securely input your credential (stored in the system keystore).
4. **Select Model**: Choose the default model for your sessions.

### Startup Experience

On startup, the TUI shows a brief loading animation and waits for the engine to be ready. You can skip the animation with `Enter`, `Esc`, or `Space`.

### Request Center

Tandem prioritizes security and user control. When an agent wants to perform a sensitive action (like running a shell command or writing to a file) or needs your input, it initiates a **Request**.

- **Permission Requests**: Approve or Deny tool usage. You can approve "Once", "Always for this session", or "Always for this project".
- **Question Requests**: The agent may ask clarifying questions which appear here.
- Access the Request Center via the specific keybinding (default `Alt+R`) or slash command `/requests`.
- Use `Up` / `Down` to navigate, `Space` to toggle selections, `Enter` to confirm, and `r` / `R` to deny.

### Agent Teams (Fanout)

Use fanout to run coordinated multi-agent execution directly in TUI:

```text
/agent fanout 4 Build and ship the VPS web portal stress-lab improvements
```

- Creates/ensures 4 panes, switches to Grid, and dispatches worker tasks.
- If mode is `plan`, TUI auto-switches to `orchestrate` for fanout runs.
- Use `/agent-team` commands for mission/instance/approval visibility.

### Pin Prompt

If you have encrypted your local storage, you will be prompted to enter your **PIN** at startup to unlock your semantic memory and session history.

## Core Navigation

- Use `Tab` / `BackTab` to switch between agents in a session.
- Use `Alt+1..9` to jump directly to a numbered agent.
- Use `Alt+M` to cycle modes, or `/modes` and `/mode` to list/set them explicitly.

## Key Features

- **Chat Interface**: Interact with agents in real-time.
- **Session Management**: Create, switch, and rename sessions.
- **Tool Integration**: Execute file operations, web searches, and more.
- **Slash Commands**: Quick access to features via `/`.

## Environment Options

- `TANDEM_ENGINE_URL`: Explicit engine base URL for remote or forwarded setups.
- `TANDEM_ENGINE_PORT`: Override the shared engine port.
- `TANDEM_TUI_IME_CURSOR_MARKER`: Control IME cursor anchoring (`auto`, `on`, `off`).

## Reference

For a complete list of keyboard shortcuts and slash commands, see the [TUI Commands Reference](./reference/tui-commands/).
