---
title: Agent Team Events
---

Agent Team events are emitted on the standard SSE stream (`/event`, `/global/event`).

## Event Names

- `agent_team.spawn.requested`
- `agent_team.spawn.denied`
- `agent_team.spawn.approved`
- `agent_team.instance.started`
- `agent_team.budget.usage`
- `agent_team.budget.exhausted`
- `agent_team.instance.cancelled`
- `agent_team.instance.completed`
- `agent_team.instance.failed`
- `agent_team.mission.budget.exhausted`
- `agent_team.capability.denied`

## Common Fields

- `sessionID`
- `messageID`
- `runID`
- `missionID`
- `instanceID`
- `parentInstanceID`
- `timestampMs`
- `source` (`ui_action`, `tool_call`, or `orchestrator_runtime` on spawn lifecycle events)

When spawn originates from `spawn_agent`, `sessionID` and `messageID` are populated with the originating tool-call context.

## Example: `agent_team.spawn.denied`

```json
{
  "type": "agent_team.spawn.denied",
  "properties": {
    "missionID": "m1",
    "requestedRole": "worker",
    "code": "spawn_policy_missing",
    "error": "spawn policy file missing",
    "timestampMs": 1739850000000
  }
}
```

## Example: `agent_team.instance.started`

```json
{
  "type": "agent_team.instance.started",
  "properties": {
    "sessionID": "ses_123",
    "missionID": "m1",
    "instanceID": "ins_123",
    "role": "worker",
    "status": "running",
    "budgetLimit": {
      "max_tokens": 8000
    },
    "skillHash": "sha256:...",
    "timestampMs": 1739850000123
  }
}
```

## Example: `agent_team.budget.exhausted`

```json
{
  "type": "agent_team.budget.exhausted",
  "properties": {
    "sessionID": "ses_123",
    "missionID": "m1",
    "instanceID": "ins_123",
    "exhaustedBy": "max_tool_calls",
    "tokensUsed": 1200,
    "stepsUsed": 3,
    "toolCallsUsed": 10,
    "costUsedUsd": 0.42,
    "elapsedMs": 42000,
    "timestampMs": 1739850002222
  }
}
```

## Example: `agent_team.mission.budget.exhausted`

```json
{
  "type": "agent_team.mission.budget.exhausted",
  "properties": {
    "missionID": "m1",
    "exhaustedBy": "mission_max_tokens",
    "tokensUsed": 40000,
    "stepsUsed": 22,
    "toolCallsUsed": 61,
    "costUsedUsd": 5.61,
    "timestampMs": 1739850003333
  }
}
```

## Example: `agent_team.capability.denied`

```json
{
  "type": "agent_team.capability.denied",
  "properties": {
    "sessionID": "ses_123",
    "messageID": "msg_123",
    "missionID": "m1",
    "instanceID": "ins_123",
    "tool": "websearch",
    "reason": "network disabled for this agent instance",
    "timestampMs": 1739850003333
  }
}
```
