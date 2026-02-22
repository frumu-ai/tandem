---
title: Agent Teams
---

Agent Teams add policy-gated multi-agent spawning to `tandem-engine`.

## What Is Implemented

- Server-side spawn gate (`SpawnPolicy`) shared by all spawn callers.
- Spawn callers supported:
  - UI/API (`POST /agent-team/spawn`)
  - tool call (`/tool spawn_agent ...`)
  - orchestrator runtime bridge (`POST /mission/{id}/event` with `mission_started` + assigned agents)
  - orchestrator runtime cancellation bridge (`POST /mission/{id}/event` with `mission_canceled`)
- Agent template registry (`.tandem/agent-team/templates/*.yaml`).
- Spawn API endpoints:
  - `GET /agent-team/templates`
  - `GET /agent-team/instances`
  - `GET /agent-team/missions`
  - `GET /agent-team/approvals`
  - `POST /agent-team/spawn`
  - `POST /agent-team/approvals/spawn/{id}/approve`
  - `POST /agent-team/approvals/spawn/{id}/deny`
  - `POST /agent-team/instance/{id}/cancel`
  - `POST /agent-team/mission/{id}/cancel`
- Desktop operator surface:
  - `Agent Automation` page includes an `Agent Ops` tab for spawn, approvals, and mission/instance control.
- Structured events:
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

## Safe Defaults

- Spawn is denied when no policy file is present.
- Justification is required when policy requires it.
- Spawn edges are enforced by role mapping in policy.
- Required skills per role are enforced.
- Skill hash (`skillHash`) is recorded in spawn response and events.
- Budget exhaustion behavior is cancel-first: instance is cancelled and child session run is cancelled.
- Mission total budgets are supported and can trigger mission-wide cancellation.
- Cost tracking is supported with optional cost limits (`max_cost_usd`).
- Runtime capability enforcement blocks disallowed tool use and emits `agent_team.capability.denied`.
- `git push` can require approval handoff via permission queue (`push_requires_approval`).
- Skill source policy supports `project_only`, `allowlist`, and optional pinned hashes.
- Token accounting prefers provider-reported usage when emitted (`provider.usage`) and falls back to stream text estimation.

## Config Locations

- Policy: `.tandem/agent-team/spawn-policy.yaml`
- Templates: `.tandem/agent-team/templates/*.yaml`

See:

- [Spawn Policy Reference](./reference/spawn-policy/)
- [Agent Team API](./reference/agent-team-api/)
- [Agent Team Events](./reference/agent-team-events/)
- [Agent Command Center](./agent-command-center/)
- [Agent Teams Rollout Plan](./agent-teams-rollout/)
