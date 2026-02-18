---
title: Agent Teams Rollout Plan
---

## MVP Plan Summary

### User Flows

- Desktop/TUI/UI spawn:
  - Client calls `POST /agent-team/spawn`.
  - Server applies `SpawnPolicy`, skill validation, budget inheritance, capability defaults.
  - On allow, engine creates child session + instance and emits spawn lifecycle events.
- Tool-driven spawn:
  - LLM calls `spawn_agent`.
  - Engine hook routes to the same runtime gate as UI spawn.
  - Denials are machine-readable (`code`, `requiresUserApproval`).
- Orchestrator runtime spawn:
  - `POST /mission/{id}/event` with `mission_started` triggers role-mapped spawns.
  - `mission_canceled` propagates cancellation to mission instances.
- Approval surfaces:
  - `GET /agent-team/approvals` returns pending spawn-approval records and pending tool approvals for agent-team sessions.

### Definition Of Done

- Spawn gate cannot be bypassed:
  - `spawn_agent`, orchestrator bridge, and UI spawn all pass one server-side policy evaluator.
- Safe defaults enforced:
  - Missing policy denies spawn.
  - Network disabled unless explicitly enabled.
  - Git push disabled unless enabled; optional approval requirement asks through permission flow.
  - Secrets hidden unless alias is listed in `secretsScopes`.
- Budgets enforced:
  - Per-instance limits enforced for tokens, steps, tool calls, wall time, and optional cost.
  - Child budgets capped from parent remaining limits.
  - Optional mission total budget enforced and emits mission exhaustion event.
- Skills enforced:
  - Required skills enforced per role.
  - Skill source policy and pinned hashes enforced.
  - `skillHash` included in spawn responses, instance metadata, events, and audit rows.
- Visibility:
  - SSE includes spawn, capability denials, budget usage/exhaustion, completion/failure/cancellation.
  - HTTP exposes templates, instances, mission summaries, approvals, spawn, and cancellation APIs.
- Tests:
  - Unit + integration coverage for gating, budgets, skill validation, and mission bridge behavior.

## Phase Breakdown

### P0: Safe Core Gate

- Deliver:
  - Spawn policy and template registry loading.
  - Unified spawn gate used by UI/tool/orchestrator.
  - Spawn lifecycle SSE + audit log rows with `skillHash`.
  - Base endpoints (`templates`, `instances`, `spawn`, `cancel`).
- Acceptance:
  - Spawn denied when policy missing.
  - Spawn denied on role-edge/required-skill violations.
  - Tool and UI paths produce equivalent deny/allow decisions.

### P1: Runtime Enforcement

- Deliver:
  - Budget supervisor (tokens/steps/tool calls/time/cost).
  - Mission total budget and parent-remaining inheritance capping.
  - Capability runtime checks (tools/fs/net/secrets/git).
  - Approval handoff for push when `push_requires_approval=true`.
  - Dedicated approvals view endpoint.
  - Completion/failure lifecycle events.
- Acceptance:
  - Budget exhaustion triggers cancel-first behavior and events.
  - Mission budget exhaustion cancels mission instances and emits mission event.
  - Disallowed fs/net/git/secrets operations are blocked with machine-readable reasons.

### P2: Command Center Surfaces

- Deliver:
  - Desktop/TUI surfaces for instances, mission rollups, and approvals.
  - Live event-based status and budget telemetry.
  - Filtering by mission, status, parent/child lineage.
- Acceptance:
  - Operator can observe who spawned whom, active status, budget burn, denials, and failures.
  - Operator can cancel instance/mission and resolve approvals from dedicated UI workflows.
