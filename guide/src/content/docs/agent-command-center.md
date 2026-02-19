---
title: Agent Command Center
---

The Agent Command Center is the desktop control surface for Agent Teams.

## What You Can Do

- Spawn agents with policy-logged justification.
- View mission rollups and instance status in real time.
- Approve or deny queued spawn requests.
- Cancel individual instances or full missions.
- Track pending tool approvals for agent-team sessions.

## Where It Lives

- Desktop: `Orchestrator` panel now includes an `Agent Command Center` section.
- Backend source of truth remains `tandem-server` (`/agent-team/*`).

## UX Goals

- Non-developer friendly defaults:
  - role presets
  - template auto-selection fallback
  - clear mission and instance counters
- Fast feedback:
  - periodic refresh for missions, instances, and approvals
  - inline error messages on denied/failed actions

## Safety Notes

- Spawn policy is enforced server-side for every action.
- Approval decisions are auditable and require justification when policy requires it.
- Mission/instance cancellation uses the same runtime safety gates as API callers.
