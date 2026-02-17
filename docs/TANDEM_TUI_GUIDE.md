# Tandem TUI Guide

The Tandem TUI (Terminal User Interface) provides a lightweight, keyboard-driven way to interact with the Tandem Engine.

## Running the TUI

To run the TUI, you must be in the `tandem` directory of the project.

```bash
cargo run -p tandem-tui
```

### Connectivity Options

The TUI can interact with the Tandem Engine in two ways:

1. **Existing Instance**: If an engine is already running (e.g., from the desktop app or another terminal), the TUI will attempt to connect to it.
2. **Auto-Spawn**: If no engine is detected, the TUI will attempt to bootstrap and spawn a local engine process automatically.

## Startup Experience

On startup, you will see a short Matrix-style animation. You can skip this by pressing `Enter`, `Esc`, or `Space` once the engine bootstrap is ready.

### PIN Unlock

The TUI requires a **User PIN** to decrypt your locally stored provider keys (e.g., OpenAI, Anthropic keys).

- Enter your PIN when prompted.
- The input is masked for security.
- Once decrypted, your provider credentials are loaded into memory and synced with the engine.

## Usage & Controls

### Main Menu

When not in an active session, you are in the Main Menu.

- **j / Down**: Highlight next session.
- **k / Up**: Highlight previous session.
- **Enter**: Open selected session.
- **n**: Create a brand new session.
- **q**: Quit.

### Chat Interface

The chat interface is where you interact with agents.

#### Navigation & Layout

- **Tab**: Switch to the next agent pane.
- **BackTab**: Switch to the previous agent pane.
- **Alt + 1..9**: Jump directly to an agent by its number.
- **Alt + G**: Toggle between **Focus Mode** (one agent) and **Grid Mode** (multi-agent view).
- **[ / ]**: Previous/Next page in Grid Mode.
- **Up / Down**: Scroll message history.
- **PageUp / PageDown**: Page through history.

#### Commands & Input

- **Enter**: Submit your prompt.
- **Shift + Enter**: Insert a newline in your draft.
- **Alt + Enter**: Insert a newline in your draft.
- **Left / Right**: Move cursor inside draft.
- **Home / End**: Jump to line start/end in draft.
- **Ctrl + Up / Ctrl + Down**: Move cursor between draft lines.
- **Delete / Backspace**: Delete next/previous character.
- **Paste**: Multi-line paste is inserted at cursor position.
- **Ctrl + N**: Add a new agent to the current session.
- **Ctrl + W**: Close the active agent.
- **Ctrl + C**: Cancel the currently running agent's operation.

#### Modals & Tools

- **F1**: Show Help Modal.
- **F2**: Open SDK Documentation.
- **Alt + R**: Open the **Request Center** (for pending permissions and questions).
- **Alt + M**: Cycle through Tandem Modes (Ask, Coder, Explore, etc.).

### Request Center (Permissions/Questions)

When an agent needs permission to run a tool or asks a question, use the Request Center (`Alt + R`).

- **Up / Down**: Navigate requests.
- **Space**: Toggle selection (for multi-choice).
- **Enter**: Confirm/Approve choice.
- **r / R**: Reject/Deny permission.
- **Esc**: Close Request Center.

### Markdown Style Legend

Assistant markdown in the transcript uses a visual style map:

- **Heading** lines (`#`, `##`, etc.) render in yellow.
- **Blockquote** lines (`>`) render in green.
- **List markers** (`-`, `*`, `1.`) render in blue.
- **Code fences and code lines** render in gray tones.

## Keyboard Shortcut Reference

| Key               | Action                        |
| :---------------- | :---------------------------- |
| **Ctrl + X**      | Quit TUI                      |
| **Ctrl + C**      | Cancel active run / Interrupt |
| **Ctrl + N**      | New Agent                     |
| **Ctrl + W**      | Close Active Agent            |
| **Tab / BackTab** | Switch Agents                 |
| **Alt + G**       | Toggle Grid View              |
| **Alt + R**       | Open Request Center           |
| **Alt + M**       | Cycle Mode                    |
| **F1**            | Help                          |
| **F2**            | Open Docs                     |

---

## SDK Documentation

The Tandem Engine SDK documentation can also be generated locally:

```bash
cd tandem
cargo doc --workspace --no-deps --open
```
