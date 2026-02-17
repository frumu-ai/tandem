---
title: TUI Guide
---

The Tandem TUI provides a terminal-based interface for interacting with AI agents, managing sessions, and running tools.

## Workflows

### Setup Wizard

On your first run, Tandem will guide you through the initial configuration:

1. **Welcome**: Introduction to the tool.
2. **Select Provider**: Choose your AI provider (e.g., Anthropic, OpenAI, Ollama).
3. **Enter API Key**: Securely input your credential (stored in the system keystore).
4. **Select Model**: Choose the default model for your sessions.

### Request Center

Tandem prioritizes security and user control. When an agent wants to perform a sensitive action (like running a shell command or writing to a file) or needs your input, it initiates a **Request**.

- **Permission Requests**: Approve or Deny tool usage. You can approve "Once", "Always for this session", or "Always for this project".
- **Question Requests**: The agent may ask clarifying questions which appear here.
- Access the Request Center via the specific keybinding (default `Alt+R`) or slash command `/requests`.

### Pin Prompt

If you have encrypted your local storage, you will be prompted to enter your **PIN** at startup to unlock your semantic memory and session history.

## Key Features

- **Chat Interface**: Interact with agents in real-time.
- **Session Management**: Create, switch, and rename sessions.
- **Tool Integration**: Execute file operations, web searches, and more.
- **Slash Commands**: Quick access to features via `/`.

## Reference

For a complete list of keyboard shortcuts and slash commands, see the [TUI Commands Reference](./reference/tui-commands/).
