---
title: Agent Team API
---

## Endpoints

### `GET /agent-team/templates`

Returns loaded agent templates.

### `GET /agent-team/instances`

Query params:

- `missionID` (optional)
- `parentInstanceID` (optional)
- `status` (optional: `queued|running|completed|failed|cancelled`)

### `GET /agent-team/missions`

Returns mission-level rollups across instances:

- `instanceCount`
- `runningCount`
- `completedCount`
- `failedCount`
- `cancelledCount`
- `queuedCount`
- `tokenUsedTotal`
- `toolCallsUsedTotal`
- `stepsUsedTotal`
- `costUsedUsdTotal`

### `GET /agent-team/approvals`

Returns:

- `spawnApprovals`: queued spawn approvals (request-only edges / approval-required paths)
- `toolApprovals`: pending permission requests scoped to agent-team sessions

### `POST /agent-team/spawn`

Request body:

```json
{
  "missionID": "m1",
  "parentInstanceID": "ins_parent",
  "templateID": "worker-default",
  "role": "worker",
  "source": "ui_action",
  "justification": "Implement focused task",
  "budget_override": {
    "max_tokens": 8000,
    "max_steps": 4,
    "max_tool_calls": 10,
    "max_duration_ms": 240000,
    "max_cost_usd": 1.25
  }
}
```

Success response:

```json
{
  "ok": true,
  "missionID": "m1",
  "instanceID": "ins_...",
  "sessionID": "ses_...",
  "runID": null,
  "status": "running",
  "skillHash": "sha256:..."
}
```

Error response:

```json
{
  "ok": false,
  "code": "spawn_policy_missing",
  "error": "spawn policy file missing",
  "requiresUserApproval": false
}
```

If policy requires user approval, response shape is still `200` with:

```json
{
  "ok": false,
  "code": "spawn_requires_approval",
  "requiresUserApproval": true
}
```

## Tool-driven spawn (`spawn_agent`)

`spawn_agent` uses the same server-side policy/runtime gate as `POST /agent-team/spawn`.

Invoke it through a session prompt:

```text
/tool spawn_agent {"missionID":"m1","role":"worker","templateID":"worker-default","source":"tool_call","justification":"parallelize implementation"}
```

Tool response metadata shape:

```json
{
  "ok": true,
  "missionID": "m1",
  "instanceID": "ins_...",
  "sessionID": "ses_...",
  "runID": null,
  "status": "running",
  "skillHash": "sha256:..."
}
```

### `POST /agent-team/instance/{id}/cancel`

Request body:

```json
{
  "reason": "manual stop"
}
```

### `POST /agent-team/approvals/spawn/{id}/approve`

Request body:

```json
{
  "reason": "approved from command center"
}
```

Response:

```json
{
  "ok": true,
  "approvalID": "appr_...",
  "decision": "approved",
  "instanceID": "ins_...",
  "missionID": "m1",
  "status": "running"
}
```

### `POST /agent-team/approvals/spawn/{id}/deny`

Request body:

```json
{
  "reason": "not aligned with mission scope"
}
```

Response:

```json
{
  "ok": true,
  "approvalID": "appr_...",
  "decision": "denied",
  "reason": "not aligned with mission scope"
}
```

Response:

```json
{
  "ok": true,
  "instanceID": "ins_...",
  "sessionID": "ses_...",
  "status": "cancelled"
}
```

### `POST /agent-team/mission/{id}/cancel`

Request body:

```json
{
  "reason": "stop all mission agents"
}
```

Response:

```json
{
  "ok": true,
  "missionID": "m1",
  "cancelledInstances": 3
}
```

### `POST /mission/{id}/event` (orchestrator bridge fields)

When mission runtime bridge is active, mission event responses include additive fields:

- `orchestratorSpawns`: spawn results triggered by `mission_started`
- `orchestratorCancellations`: cancellation summary triggered by `mission_canceled`
