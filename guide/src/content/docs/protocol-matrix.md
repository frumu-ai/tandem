---
title: Protocol Matrix
---

This matrix summarizes the engine-backed contracts between Desktop/TUI clients and the engine runtime.

## Frontend <-> Tauri Commands

| Command                                                  | Contract                                                      |
| -------------------------------------------------------- | ------------------------------------------------------------- |
| `get_storage_migration_status` / `run_storage_migration` | Controls startup + settings-triggered migration flow.         |
| `list_plans`                                             | Reads `.tandem/plans` with legacy `.opencode/plans` fallback. |
| `read_plan_content`                                      | Reads a plan markdown file by path.                           |
| `build_permission_rules`                                 | Returns rule names aligned with runtime tool names.           |
| Session scope                                            | Session list is scoped to the active workspace.               |

## Tauri <-> Engine HTTP

| Endpoint                                       | Contract                                                |
| ---------------------------------------------- | ------------------------------------------------------- |
| `GET /session?scope=workspace&workspace=<abs>` | Workspace-scoped session list (engine-enforced).        |
| `GET /session?scope=global`                    | Explicit cross-workspace list for advanced/debug flows. |
| `POST /session/{id}/attach`                    | Explicit attach across workspaces with audit fields.    |
| `POST /session/{id}/workspace/override`        | Temporary sandbox override with TTL.                    |
| `GET /provider`                                | Provider catalog with default/connected metadata.       |
| `POST /mission` / `POST /routines`             | Mission/routine lifecycle endpoints.                    |

## Engine SSE -> Client Events

| Event                        | Contract                                                                    |
| ---------------------------- | --------------------------------------------------------------------------- |
| `message.part.updated`       | Text and tool part streaming for chat timeline + console.                   |
| `todo.updated`               | Normalized todo state (`pending`, `in_progress`, `completed`, `cancelled`). |
| `question.asked`             | Questions with `tool.callID` for correlation.                               |
| `storage-migration-progress` | Migration phase/progress/counters.                                          |
| `storage-migration-complete` | Migration completion status and summary.                                    |

## Canonical Event Examples

### `message.part.updated` (text)

```json
{
  "type": "message.part.updated",
  "properties": {
    "part": {
      "id": "part_123",
      "sessionID": "ses_123",
      "messageID": "msg_123",
      "type": "text",
      "text": "Hello"
    },
    "delta": "Hello"
  }
}
```

### `todo.updated`

```json
{
  "type": "todo.updated",
  "properties": {
    "sessionID": "ses_123",
    "todos": [{ "id": "t1", "content": "Audit contracts", "status": "pending" }]
  }
}
```

### `question.asked`

```json
{
  "type": "question.asked",
  "properties": {
    "id": "q_123",
    "sessionID": "ses_123",
    "messageID": "msg_123",
    "questions": [
      {
        "header": "Scope",
        "question": "Pick one",
        "options": [{ "label": "A", "description": "..." }]
      }
    ],
    "tool": { "callID": "call_2", "messageID": "msg_123" }
  }
}
```

## JSON-First Orchestrator Contract

Tandem validates planner + validator responses as strict JSON first. The strict mode can be enabled with:

```bash
TANDEM_ORCH_STRICT_CONTRACT=1
```

Strict mode emits `contract_warning` or `contract_error` events when the JSON contract is degraded.
