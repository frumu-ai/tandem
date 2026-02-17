# Engine Protocol Matrix

Status legend: `implemented`, `partial`, `drift`

Last updated: 2026-02-13 (engine-backed workspace-first session scope + explicit attach + sandbox override)

## 1) Frontend <-> Tauri Commands

| Boundary                                                                             | Canonical contract                                                                       | Status      | Notes                                                                                      |
| ------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------ |
| Storage migration wizard -> `get_storage_migration_status` / `run_storage_migration` | Startup + settings-triggered migration control                                           | implemented | Auto-run at startup after vault unlock; manual rerun from Settings.                        |
| Plans panel -> `list_plans`                                                          | Returns plan metadata from `.tandem/plans` (plus legacy `.opencode/plans` read fallback) | implemented | Command registered in `src-tauri/src/lib.rs` invoke handler.                               |
| Plan content -> `read_plan_content`                                                  | Returns full plan markdown by path                                                       | implemented | Command registered and wired to `commands.rs`.                                             |
| Mode permissions -> `build_permission_rules`                                         | Rule names align with runtime tool names                                                 | partial     | Alias support present for `todowrite` + `todo_write`; broader alias policy still evolving. |
| Sessions sidebar scope                                                               | Show sessions for active workspace only                                                  | implemented | Canonical path matcher resolves `.`/missing directory against active workspace context.    |

## 2) Tauri <-> Sidecar HTTP

| Endpoint                                       | Canonical request/response shape                       | Status      | Notes                                                                               |
| ---------------------------------------------- | ------------------------------------------------------ | ----------- | ----------------------------------------------------------------------------------- |
| `GET /project`                                 | Array of project directories                           | partial     | Tolerated in adapter; additional variant fixtures still useful.                     |
| `GET /session`                                 | Array of wire sessions                                 | partial     | Parser supports core shape; nested/legacy variants still observed.                  |
| `GET /session?scope=workspace&workspace=<abs>` | Workspace-first scoped list (engine-enforced)          | implemented | Default Desktop/Tauri flow now uses scoped listing; no silent global fallback path. |
| `GET /session?scope=global`                    | Explicit cross-workspace list                          | implemented | Reserved for advanced/debug flows only.                                             |
| `POST /session`                                | Accepts `model.provider_id/model_id` and camel aliases | implemented | Added serde aliases + HTTP test coverage.                                           |
| `POST /session/{id}/attach`                    | Explicit cross-workspace attach with audit fields      | implemented | Persists attach metadata (`attachedFrom/To`, reason, timestamp, origin root).       |
| `POST /session/{id}/workspace/override`        | Temporary sandbox override (TTL, session-scoped)       | implemented | Emits `session.workspace_override.granted`; default remains strict sandbox deny.    |
| `GET /provider`                                | `{ all, connected, default }` with provider models map | partial     | Primary path works; fallback behavior and remote expansion still under refinement.  |

## 3) Sidecar SSE -> Tauri `StreamEvent`

| Event                                 | Canonical payload                                                      | Status      | Notes                                                                          |
| ------------------------------------- | ---------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------ |
| `storage-migration-progress`          | phase, percent, copied/skipped/errors, recovered counters              | implemented | Drives blocking startup migration overlay progress bar and counters.           |
| `storage-migration-complete`          | status, duration, copied/skipped/errors, repair totals                 | implemented | Drives completion summary + retry/details actions.                             |
| `message.part.updated` text           | `part.sessionID`, `part.messageID`, `part.type=text`, optional `delta` | implemented | Delta/no-delta assistant handling present.                                     |
| `message.part.updated` tool start/end | Consistent tool lifecycle with stable `id/state/tool`                  | partial     | Added structured-state tests; additional telemetry for dropped events pending. |
| `todo.updated`                        | `sessionID`, normalized todo items (`id/content/status`)               | implemented | Parser tolerant to malformed entries; emits normalized set.                    |
| `question.asked`                      | `id`, `sessionID`, `questions[]`, optional `tool.callID/messageID`     | implemented | Multi-question fixture covered in tests.                                       |

## 4) Tauri Emitted Events -> Frontend Consumers

| Consumer      | Required event contract                                             | Status      | Notes                                                                                           |
| ------------- | ------------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------------------- |
| Chat timeline | content/tool parts stream reliably to session transcript            | partial     | Core events present; some non-tool fallback paths still need parity checks.                     |
| Console tab   | tool lifecycle (`tool_start`/`tool_end`) visible for live + history | partial     | Structured-state mapping now tested; history reconstruction needs additional integration tests. |
| Tasks panel   | `todo.updated` drives pending/done UI state                         | implemented | Plan fallback emits synthetic todo tool lifecycle + todo update.                                |

## 5) Canonical Event Examples

### `GET /session` (workspace scope)

```http
GET /session?scope=workspace&workspace=C:\Users\evang\work\frumu
```

```json
[
  {
    "id": "ses_123",
    "title": "Chat",
    "directory": "C:\\Users\\evang\\work\\frumu",
    "workspaceRoot": "C:\\Users\\evang\\work\\frumu"
  }
]
```

### `POST /session/{id}/attach`

```json
{
  "target_workspace": "C:\\Users\\evang\\work\\tandem",
  "reason_tag": "manual_attach"
}
```

```json
{
  "id": "ses_123",
  "workspaceRoot": "C:\\Users\\evang\\work\\tandem",
  "originWorkspaceRoot": "C:\\Users\\evang\\work\\frumu",
  "attachedFromWorkspace": "C:\\Users\\evang\\work\\frumu",
  "attachedToWorkspace": "C:\\Users\\evang\\work\\tandem",
  "attachTimestampMs": 1770940000000,
  "attachReason": "manual_attach"
}
```

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

### `message.part.updated` (tool running)

```json
{
  "type": "message.part.updated",
  "properties": {
    "part": {
      "id": "call_1",
      "sessionID": "ses_123",
      "messageID": "msg_123",
      "type": "tool",
      "tool": "todo_write",
      "state": {
        "status": "running",
        "input": { "todos": [{ "content": "Audit contracts" }] }
      }
    }
  }
}
```

### `message.part.updated` (tool completed)

```json
{
  "type": "message.part.updated",
  "properties": {
    "part": {
      "id": "call_1",
      "sessionID": "ses_123",
      "messageID": "msg_123",
      "type": "tool",
      "tool": "todo_write",
      "state": {
        "status": "completed",
        "output": { "todos": [{ "id": "t1", "content": "Audit contracts", "status": "pending" }] }
      }
    }
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

## 6) JSON-First Orchestrator Contract (Planner + Validator)

| Area                     | Canonical contract                                                             | Status      | Notes                                                                                          |
| ------------------------ | ------------------------------------------------------------------------------ | ----------- | ---------------------------------------------------------------------------------------------- |
| Planner parse            | Strict JSON parse first (`tasks[]`/`plan[]`/`steps[]`/`items[]`/`task_list[]`) | implemented | Controlled by `OrchestratorConfig.strict_planner_json`.                                        |
| Planner prose fallback   | Optional markdown/prose fallback when strict parse fails                       | implemented | Guarded by `OrchestratorConfig.allow_prose_fallback`; emits contract warning telemetry/events. |
| Validator parse          | Strict JSON object required: `passed`, `feedback`, `suggested_fixes`           | implemented | Controlled by `OrchestratorConfig.strict_validator_json`.                                      |
| Validator prose fallback | Optional inference fallback when strict parse fails                            | implemented | Guarded by `OrchestratorConfig.allow_prose_fallback`; emits contract warning telemetry/events. |
| Contract observability   | Structured orchestrator events for degradation/failures                        | implemented | `contract_warning` and `contract_error` emitted on `orchestrator-event`.                       |
| Canonical state source   | UI state from typed events + persisted tool history, not prose                 | implemented | Console uses `tool_start/tool_end` + `tool_executions`; task state via task/todo events.       |

### Orchestrator contract warning event

```json
{
  "type": "contract_warning",
  "run_id": "run_123",
  "task_id": "task_4",
  "agent": "validator",
  "phase": "task_validation",
  "reason": "validator strict parse failed; prose fallback used",
  "fallback_used": true,
  "snippet": "Looks good overall, passed.",
  "timestamp": "2026-02-13T22:20:00Z"
}
```

### Orchestrator contract error event

```json
{
  "type": "contract_error",
  "run_id": "run_123",
  "task_id": null,
  "agent": "planner",
  "phase": "planning",
  "reason": "PLANNER_CONTRACT_PARSE_FAILED",
  "fallback_used": false,
  "snippet": "planner response did not contain valid JSON task payload",
  "timestamp": "2026-02-13T22:20:00Z"
}
```

## 7) Strict Contract Flags and Rollout

| Config field                | Type   | Default             | Effect                                                  |
| --------------------------- | ------ | ------------------- | ------------------------------------------------------- |
| `strict_planner_json`       | `bool` | feature-flag driven | Planner uses strict JSON parser first.                  |
| `strict_validator_json`     | `bool` | feature-flag driven | Validator uses strict JSON parser first.                |
| `allow_prose_fallback`      | `bool` | `true`              | Allows markdown/prose fallback when strict parse fails. |
| `contract_warnings_enabled` | `bool` | `true`              | Emits warning/error contract events and logs.           |

Feature flag:

- `TANDEM_ORCH_STRICT_CONTRACT=1`
- When enabled, orchestrator run creation forces strict planner/validator contract mode.

## 8) Canonical Planner and Validator Schemas

### Planner output (strict path)

```json
[
  {
    "id": "task_1",
    "title": "Analyze code structure",
    "description": "Review architecture and dependency boundaries.",
    "dependencies": [],
    "acceptance_criteria": ["Key files identified", "Architecture notes captured"]
  }
]
```

Validation rules in strict mode:

- Required task fields: `id`, `title`, `description`, `dependencies`, `acceptance_criteria`
- Empty title/description rejected
- Duplicate IDs deterministically de-duplicated (`task_1`, `task_1_2`, ...)

### Validator output (strict path)

```json
{
  "passed": false,
  "feedback": "Task misses retry handling.",
  "suggested_fixes": ["Add retry loop", "Return explicit error on final attempt"]
}
```

Validation rules in strict mode:

- Required keys: `passed`, `feedback`, `suggested_fixes`
- `passed` must be boolean
- `suggested_fixes` must be an array (can be empty)

## 9) Shared Storage Contract (Tauri + Engine + TUI)

| Boundary             | Canonical contract                                        | Status      | Notes                                                                   |
| -------------------- | --------------------------------------------------------- | ----------- | ----------------------------------------------------------------------- |
| Shared runtime root  | `%APPDATA%/tandem` (platform-equivalent data dir)         | implemented | Resolved via `tandem_core::resolve_shared_paths()`.                     |
| Legacy root          | `%APPDATA%/ai.frumu.tandem` is migration source only      | implemented | Non-destructive copy migration; legacy kept for rollback compatibility. |
| Startup migration    | Run once when canonical is empty and legacy exists        | implemented | `migrate_legacy_storage_if_needed()` writes `migration_report.json`.    |
| Sidecar state dir    | Tauri always passes explicit `--state-dir` to sidecar     | implemented | Prevents state drift caused by implicit cwd defaults.                   |
| Tool history DB path | Console history + live rows use canonical `memory.sqlite` | implemented | `tool_history.rs` now resolves DB path from shared storage root.        |
| Storage diagnostics  | UI/TUI-readable storage status command                    | implemented | `get_storage_status` command returns roots + marker/report status.      |

## 10) Workspace Namespace Contract

| Boundary              | Canonical contract                                                                                                   | Status      | Notes                                                                                                                          |
| --------------------- | -------------------------------------------------------------------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Workspace plans       | Write to `.tandem/plans`; read `.tandem/plans` then `.opencode/plans`                                                | implemented | `start_plan_session` writes canonical paths; `list_plans` and watcher include legacy-read fallback.                            |
| Workspace skills      | Canonical project root `.tandem/skill` + global root `%APPDATA%/tandem/skills`; engine `/skills*` is source of truth | implemented | Tauri + TUI now delegate install/list/load/import/delete/template flows to engine endpoints/tooling (no `.opencode` fallback). |
| Python workspace venv | Canonical `.tandem/.venv` with legacy `.opencode/.venv` compatibility                                                | implemented | Policy checks and status resolve canonical first; legacy venv still accepted during migration window.                          |
| Starter packs default | Install into `<workspace>/workspace-packs` when workspace is active                                                  | implemented | Falls back to previous global default only when no active workspace exists.                                                    |

## 9) Authoritative State Sources (Web + TUI)

| UI concern            | Authoritative source                        | Prohibited source        |
| --------------------- | ------------------------------------------- | ------------------------ |
| Task lifecycle        | Orchestrator task state + `todo.updated`    | Assistant prose          |
| Console tool timeline | `tool_start`/`tool_end` + `tool_executions` | Assistant prose          |
| Run status/errors     | Typed `orchestrator-event` + run snapshot   | Free-form assistant text |

Contract rule:

- Assistant prose is display-only; it is never authoritative for control-state transitions.
