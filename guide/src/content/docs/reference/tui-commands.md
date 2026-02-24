---
title: TUI Commands
---

This page explains what each Tandem TUI command does, when to use it, and the most common examples.

Tip: type `/` in chat to open command autocomplete.

## Command Syntax

- `<required>` means you must provide a value.
- `[optional]` means the value is optional.
- For JSON arguments, use valid JSON (double quotes around keys/strings).

## Global Keybindings

These work from most TUI screens.

| Key      | What it does                                                 |
| -------- | ------------------------------------------------------------ |
| `Ctrl+C` | In chat: cancel active run. Press again within 1.5s to quit. |
| `Ctrl+X` | Quit Tandem TUI immediately.                                 |
| `Ctrl+N` | Create a new agent pane in chat.                             |
| `Ctrl+W` | Close active agent pane.                                     |
| `Ctrl+U` | Page up in transcript/content.                               |
| `Ctrl+D` | Page down in transcript/content.                             |
| `Ctrl+Y` | Copy latest assistant response to clipboard.                 |

## Main Menu Keys (Sessions Screen)

| Key            | What it does             |
| -------------- | ------------------------ |
| `n`            | Create a new session.    |
| `j` / `Down`   | Select next session.     |
| `k` / `Up`     | Select previous session. |
| `d` / `Delete` | Delete selected session. |
| `Enter`        | Open selected session.   |
| `q`            | Quit TUI.                |

## Chat Keys

| Key                         | What it does                              |
| --------------------------- | ----------------------------------------- |
| `Enter`                     | Send current prompt.                      |
| `Shift+Enter` / `Alt+Enter` | Insert newline in composer.               |
| `Tab` / `Shift+Tab`         | Next/previous agent pane.                 |
| `Alt+1..9`                  | Jump directly to agent `A1..A9`.          |
| `Alt+M`                     | Cycle mode (`ask`, `plan`, `coder`, etc). |
| `Alt+G`                     | Toggle Focus/Grid view.                   |
| `[` / `]`                   | Previous/next grid page.                  |
| `Alt+R`                     | Open Request Center.                      |
| `Alt+I`                     | Queue steering interrupt message.         |
| `F1`                        | Help modal.                               |
| `F2`                        | Open docs.                                |

## Request Center Keys

When requests are pending:

| Key                     | What it does                                                                             |
| ----------------------- | ---------------------------------------------------------------------------------------- |
| `Enter`                 | Confirm current request choice/answer.                                                   |
| `r`                     | Reject current request.                                                                  |
| `Esc`                   | Close request center.                                                                    |
| `Space`                 | Toggle selected option (questions) or cycle permission choice.                           |
| `Left` / `Right`        | Move choice cursor (question options or permission choice).                              |
| `1..9`                  | Select option by number (when available).                                                |
| `Up` / `Down`           | For question requests: move option cursor. For permission requests: move request cursor. |
| `Ctrl+Up` / `Ctrl+Down` | Move previous/next request explicitly.                                                   |
| `Ctrl+E`                | Expand/collapse compact request panel.                                                   |

## Slash Commands

## Basics

### `/help`

Show built-in command help.

```text
/help
```

### `/engine status`

Show engine health, version, mode, endpoint, and connection source.

```text
/engine status
```

### `/engine restart`

Restart the engine process and reconnect.

```text
/engine restart
```

### `/engine token` and `/engine token show`

Show masked engine token or full token.

```text
/engine token
/engine token show
```

## Sessions and Chat

### `/sessions`

List available sessions with IDs.

```text
/sessions
```

### `/new [title...]`

Create a new session. If title is omitted, default title is used.

```text
/new
/new Release prep
```

### `/use <session_id>`

Switch current chat to an existing session.

```text
/use 1aa70178-...
```

### `/title <new title...>`

Rename current session.

```text
/title Plan for March launch
```

### `/prompt <text...>`

Send text to current session (same as typing normally, but explicit).

```text
/prompt Summarize this repository architecture.
```

### `/messages [limit]`

Show historical messages from current session.

```text
/messages
/messages 30
```

### `/cancel`

Cancel active run in current session.

```text
/cancel
```

### `/last_error`

Show most recent prompt/system error for current session.

```text
/last_error
```

### `/copy`

Copy latest assistant response to clipboard.

```text
/copy
```

## Agent Commands

### `/agent new`

Create additional agent pane (multi-agent chat view).

```text
/agent new
```

### `/agent list`

List agent panes, status, and bound session IDs.

```text
/agent list
```

### `/agent use <A#>`

Switch active agent pane.

```text
/agent use A2
```

### `/agent close`

Close active agent pane.

```text
/agent close
```

### `/agent fanout [n]`

Ensure `n` agents exist and switch to Grid view. Default is `4`.

```text
/agent fanout
/agent fanout 4
/agent fanout 6
```

## Modes

### `/modes`

List available modes.

```text
/modes
```

### `/mode`

Show current mode.

```text
/mode
```

### `/mode <name>`

Set mode (`ask`, `plan`, `coder`, `explore`, `immediate`, `orchestrate`).

```text
/mode plan
/mode coder
```

## Providers and Models

### `/providers`

List providers known by engine and whether configured.

```text
/providers
```

### `/provider <id>`

Set active provider for new requests.

```text
/provider openrouter
```

### `/models [provider]`

List models for active or specified provider.

```text
/models
/models openrouter
```

### `/model <model_id>`

Set active model.

```text
/model z-ai/glm-5
```

## Keys and Credentials

### `/keys`

Show configured provider auth/key status.

```text
/keys
```

### `/key set <provider> <api_key>`

Set provider key.

```text
/key set openrouter sk-or-...
```

### `/key remove <provider>`

Remove stored provider key.

```text
/key remove openrouter
```

### `/key test <provider>`

Test connectivity for a provider.

```text
/key test openrouter
```

## Request Approval and Questions

### `/requests`

Open request center for pending permission/question requests.

```text
/requests
```

### `/approve <request_id> [always]`

Approve tool permission request once or persistently.

```text
/approve req_123
/approve req_123 always
```

### `/approve all`

Approve all pending requests in current session.

```text
/approve all
```

### `/deny <request_id>`

Deny a permission request.

```text
/deny req_123
```

### `/answer <question_id> <reply>`

Send freeform answer text to a question request.

```text
/answer q_456 Proceed with option 2.
```

## Tools and Queue Control

### `/tool <name> <json_args>`

Pass-through tool call directly to engine.

```text
/tool webfetch {"url":"https://tandem.ai","return":"text"}
```

### `/steer <message>`

Queue steering message to redirect active run.

```text
/steer Focus only on tests that currently fail.
```

### `/followup <message>`

Queue follow-up message after active run completes.

```text
/followup Next, generate a rollout checklist.
```

### `/queue`

Show steering/follow-up queue state.

```text
/queue
```

### `/queue clear`

Clear queued steering and follow-up messages.

```text
/queue clear
```

## Task Commands (Local Task List Panel)

### `/task add <description...>`

Add local task item.

```text
/task add write smoke tests for auth flow
```

### `/task done <id>` `/task fail <id>` `/task work <id>` `/task pending <id>`

Update task status.

```text
/task work task-3
/task done task-3
```

### `/task pin <id>`

Pin/unpin task in list.

```text
/task pin task-2
```

### `/task list`

List current tasks and statuses.

```text
/task list
```

## Routines

### `/routines`

List routines.

```text
/routines
```

### `/routine_create <id> <interval_seconds> <entrypoint>`

Create interval routine.

```text
/routine_create nightly-summary 86400 mission.default
```

### `/routine_edit <id> <interval_seconds>`

Update routine schedule.

```text
/routine_edit nightly-summary 43200
```

### `/routine_pause <id>` `/routine_resume <id>`

Pause/resume routine execution.

```text
/routine_pause nightly-summary
/routine_resume nightly-summary
```

### `/routine_run_now <id> [count]`

Trigger routine immediately.

```text
/routine_run_now nightly-summary
/routine_run_now nightly-summary 3
```

### `/routine_delete <id>`

Delete routine.

```text
/routine_delete nightly-summary
```

### `/routine_history <id> [limit]`

Show routine run history.

```text
/routine_history nightly-summary
/routine_history nightly-summary 20
```

## Missions

### `/missions`

List missions.

```text
/missions
```

### `/mission_create <title> :: <goal> [:: work_item_title]`

Create mission quickly from inline text.

```text
/mission_create Release prep :: Ship v0.3.20 safely :: Final QA pass
```

### `/mission_get <mission_id>`

Show mission details.

```text
/mission_get mission_abc
```

### `/mission_event <mission_id> <event_json>`

Apply raw mission event JSON.

```text
/mission_event mission_abc {"type":"mission_started"}
```

### `/mission_start <mission_id>`

Shortcut mission started event.

```text
/mission_start mission_abc
```

### `/mission_review_ok <mission_id> <work_item_id> [approval_id]`

Approve review gate.

```text
/mission_review_ok mission_abc work_1
```

### `/mission_test_ok <mission_id> <work_item_id> [approval_id]`

Approve test gate.

```text
/mission_test_ok mission_abc work_1
```

### `/mission_review_no <mission_id> <work_item_id> [reason]`

Deny review gate with optional reason.

```text
/mission_review_no mission_abc work_1 "needs docs updates"
```

## Agent-Team (Orchestration)

### `/agent-team`

Show agent-team dashboard summary.

```text
/agent-team
```

### `/agent-team missions`

List mission rollups for agent-team subsystem.

```text
/agent-team missions
```

### `/agent-team instances [mission_id]`

List running instances, optionally scoped to one mission.

```text
/agent-team instances
/agent-team instances mission_abc
```

### `/agent-team approvals`

List pending agent-team approvals.

```text
/agent-team approvals
```

### `/agent-team approve spawn <approval_id> [reason]`

Approve spawn approval request.

```text
/agent-team approve spawn appr_123
```

### `/agent-team deny spawn <approval_id> [reason]`

Deny spawn approval request.

```text
/agent-team deny spawn appr_123 "resource limit"
```

### `/agent-team approve tool <request_id>`

Approve tool permission request from agent-team run.

```text
/agent-team approve tool req_987
```

### `/agent-team deny tool <request_id>`

Deny tool permission request from agent-team run.

```text
/agent-team deny tool req_987
```

## Config

### `/config`

Print current engine/TUI config snapshot.

```text
/config
```

## Practical Workflows

### Start a clean Plan-mode session

```text
/new plan website migration
/mode plan
/provider openrouter
/model z-ai/glm-5
```

### Force 4-agent execution layout

```text
/agent fanout 4
```

### Resolve pending requests quickly

```text
/requests
/approve all
```
