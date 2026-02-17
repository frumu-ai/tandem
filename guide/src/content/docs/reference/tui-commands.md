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

| Key                         | Action                        |
| --------------------------- | ----------------------------- |
| `Esc`                       | Initial "back" or dismiss     |
| `Enter`                     | Submit command / Send message |
| `Shift+Enter` / `Alt+Enter` | Insert Newline                |
| `Tab`                       | Switch to Next Agent          |
| `BackTab`                   | Switch to Previous Agent      |
| `Alt+1..9`                  | Select Agent by Number        |
| `Alt+M`                     | Cycle mode                    |
| `Alt+G`                     | Toggle UI Mode                |
| `Alt+R`                     | Open Request Center           |
| `Alt+S`                     | Start Demo Stream (dev)       |
| `Alt+B`                     | Spawn Background Demo (dev)   |
| `[` / `]`                   | Navigate Grid Pages           |
| `Up` / `Down`               | Scroll History                |
| `F1`                        | Open help modal               |
| `F2`                        | Open docs                     |

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
- **/routines**: List scheduled routines
- **/routine_create**: Create interval routine
- **/routine_edit**: Edit routine interval
- **/routine_pause**: Pause a routine
- **/routine_resume**: Resume a routine
- **/routine_run_now**: Trigger a routine now
- **/routine_delete**: Delete a routine
- **/routine_history**: Show routine execution history
- **/missions**: List engine missions
- **/mission_create**: Create an engine mission
- **/mission_get**: Get mission details
- **/mission_event**: Apply mission event JSON
- **/mission_start**: Apply mission started event
- **/mission_review_ok**: Approve review gate
- **/mission_test_ok**: Approve test gate
- **/mission_review_no**: Deny review gate
- **/config**: Show configuration
