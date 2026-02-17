---
title: TUI Commands
---

## Global Keybindings

| Key      | Action                                              |
| -------- | --------------------------------------------------- |
| `Ctrl+C` | Cancel active agent (in chat) or Quit (press twice) |
| `Ctrl+X` | Quit Tandem TUI                                     |
| `Ctrl+N` | Start a **New Agent** session                       |
| `Ctrl+W` | Close the **Active Agent**                          |
| `Ctrl+U` | Page Up                                             |
| `Ctrl+D` | Page Down                                           |

## Main Menu

| Key          | Action                 |
| ------------ | ---------------------- |
| `q`          | Quit                   |
| `n`          | Create **New Session** |
| `j` / `Down` | Next Session           |
| `k` / `Up`   | Previous Session       |
| `Enter`      | Select Session         |

## Chat Mode

| Key           | Action                        |
| ------------- | ----------------------------- |
| `Esc`         | Initial "back" or dismiss     |
| `Enter`       | Submit command / Send message |
| `Shift+Enter` | Insert Newline                |
| `Tab`         | Switch to Next Agent          |
| `BackTab`     | Switch to Previous Agent      |
| `Alt+[0-9]`   | Select Agent by Number        |
| `Alt+G`       | Toggle UI Mode                |
| `Alt+R`       | Open Request Center           |
| `Alt+S`       | Start Demo Stream             |
| `Alt+B`       | Spawn Background Demo         |
| `[` / `]`     | Navigate Grid Pages           |
| `Up` / `Down` | Scroll History                |

## Slash Commands

Type `/` in the chat input to see autocomplete.

- **/help**: Show available commands
- **/engine**: Check engine status / restart
- **/sessions**: List all sessions
- **/new**: Create new session
- **/agent**: Manage in-chat agents
- **/use**: Switch to session by ID
- **/title**: Rename current session
- **/prompt**: Send prompt to session
- **/cancel**: Cancel current operation
- **/last_error**: Show last prompt/system error
- **/messages**: Show message history
- **/modes**: List available modes
- **/mode**: Set or show current mode
- **/providers**: List available providers
- **/provider**: Set current provider
- **/models**: List models for provider
- **/model**: Set current model
- **/keys**: Show configured API keys
- **/key**: Manage provider API keys
- **/approve**: Approve a pending request
- **/deny**: Deny a pending request
- **/answer**: Answer a question (from a tool)
- **/requests**: Open pending request center
- **/config**: Show configuration
