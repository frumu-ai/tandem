---
title: Spawn Policy Reference
---

`SpawnPolicy` is the server-enforced gate for all Agent Team spawns.

## File

`.tandem/agent-team/spawn-policy.yaml`

## Minimal Example

```yaml
enabled: true
require_justification: true
max_agents: 24
max_concurrent: 6
child_budget_percent_of_parent_remaining: 40
cost_per_1k_tokens_usd: 0.03
mission_total_budget:
  max_tokens: 40000
  max_tool_calls: 80
  max_cost_usd: 6.0

spawn_edges:
  orchestrator:
    behavior: allow
    can_spawn: [delegator, worker, watcher, reviewer, tester, committer]
  delegator:
    behavior: allow
    can_spawn: [worker, watcher, tester]
  worker:
    behavior: request_only
    can_spawn: []
  watcher:
    behavior: deny
    can_spawn: []
  reviewer:
    behavior: deny
    can_spawn: []
  tester:
    behavior: deny
    can_spawn: []
  committer:
    behavior: deny
    can_spawn: []

required_skills:
  worker:
    - id: rust-editing

skill_sources:
  mode: allowlist
  allowlist_ids: [rust-editing, rust-testing]
  allowlist_paths: [".tandem/skills/worker/SKILL.md"]
  pinned_hashes:
    id:rust-editing: "sha256:abc123..."
    path:.tandem/skills/worker/SKILL.md: "sha256:def456..."
```

## Deny Codes

- `spawn_policy_missing`
- `spawn_policy_disabled`
- `spawn_denied_edge`
- `spawn_justification_required`
- `spawn_max_agents_exceeded`
- `spawn_max_concurrent_exceeded`
- `spawn_mission_budget_exceeded`
- `spawn_requires_approval`
- `spawn_required_skill_missing`
- `spawn_skill_source_denied`
- `spawn_skill_hash_mismatch`
