---
title: Automation Governance Lifecycle
description: Concrete state transitions for Tandem automation reviews, approvals, grants, pauses, retirement, and recovery.
---

Use this page when you need the runtime truth behind governance: what records exist, which transitions are allowed, and what approval actually changes.

If you only need the high-level concepts, start with [Governance Reference](./governance/). If you need to operate the system as an LLM, read [Self-Operator Playbook](../self-operator-playbook/).

## Core Records

The governance model centers on these records:

- `AutomationGovernanceRecord`
- `GovernanceApprovalRequest`
- `AutomationGrantRecord`
- `AgentCreationReviewSummary`
- `AgentSpendSummary`
- `AutomationLifecycleFinding`

These records are appendable and auditable. They are not just UI state.

## Approval Queue Types

All approval flows use the same approval queue surface. The request type tells Tandem and the operator what is being reviewed.

| Request type          | Typical use                                                                                          |
| --------------------- | ---------------------------------------------------------------------------------------------------- |
| `capability_request`  | MCP capability gaps and approved capability grants                                                   |
| `external_post`       | External side effects that need explicit review                                                      |
| `quota_override`      | Temporary spend or creation quota overrides                                                          |
| `lifecycle_review`    | Creation quota acknowledgment, drift review, dependency revocation, expiration, or retirement review |
| `elevated_capability` | Broader capability grant beyond the default envelope                                                 |
| `depth_override`      | Rare exception to the recursion-depth limit                                                          |
| `retirement_action`   | Extend, retire, or acknowledge an expiring automation                                                |

The canonical write surface is `POST /governance/approvals`.

## Governance State Fields

The automation governance record stores the fields that matter for enforcement:

- `provenance`
- `declared_capabilities`
- `modify_grants`
- `capability_grants`
- `published_externally`
- `creation_paused`
- `review_required`
- `review_kind`
- `review_requested_at_ms`
- `review_request_id`
- `last_reviewed_at_ms`
- `runs_since_review`
- `expires_at_ms`
- `expired_at_ms`
- `retired_at_ms`
- `retire_reason`
- `paused_for_lifecycle`
- `health_last_checked_at_ms`
- `health_findings`
- `deleted_at_ms`
- `delete_retention_until_ms`

Use those fields to determine whether the automation is mutable, paused, awaiting review, or expired.

## State Transitions

### 1. Creation

When an automation is created, Tandem records provenance and initializes governance state.

If the creator is an agent, Tandem also applies the creation quota, depth limit, spend policy, and capability escalation checks.

The key creation failure codes are:

- `AUTOMATION_V2_CREATION_DISABLED`
- `AUTOMATION_V2_AGENT_ID_REQUIRED`
- `AUTOMATION_V2_AGENT_CREATION_PAUSED`
- `AUTOMATION_V2_AGENT_SPEND_CAP_EXCEEDED`
- `AUTOMATION_V2_AGENT_REVIEW_REQUIRED`
- `AUTOMATION_V2_LINEAGE_DEPTH_EXCEEDED`
- `AUTOMATION_V2_AGENT_DAILY_QUOTA_EXCEEDED`
- `AUTOMATION_V2_AGENT_CAP_EXCEEDED`
- `AUTOMATION_V2_CAPABILITY_ESCALATION_FORBIDDEN`

If the agent wants `creates_agents` or `modifies_grants`, the escalation must already be approved. Otherwise the route rejects the request.

### 2. Capability Escalation

Declared capability escalation is enforced server-side.

The important rule is simple:

- an agent cannot create or patch an automation into a more powerful capability state unless it already has an approved capability request for that capability

That applies to:

- `creates_agents`
- `modifies_grants`

If the escalation is not approved yet, the engine returns `AUTOMATION_V2_CAPABILITY_ESCALATION_FORBIDDEN`.

### 3. Creation Quota Acknowledgment

The engine tracks how many automations an agent has created since its last review.

When the per-agent creation threshold is reached:

- `review_required` becomes `true`
- `review_kind` becomes `creation_quota`
- the engine opens a `lifecycle_review` approval
- the agent should stop creating more automations until the review is acknowledged

Approving the review clears the review flag and resets the counter.

### 4. Spend Guardrails

Spend is tracked per agent across daily, weekly, monthly, and lifetime windows.

When the weekly cap is reached:

- the agent is paused from creating more automations
- a `quota_override` request is opened
- the run is paused if it was active

The soft warning threshold fires before the hard stop so operators can react early.

### 5. Dependency Revocation

If a grant is revoked or an MCP policy is narrowed, Tandem pauses the affected automation and opens a lifecycle review.

The record is updated with:

- `paused_for_lifecycle = true`
- `review_required = true`
- `review_kind = dependency_revoked`

The approval queue entry is a `lifecycle_review`.

Approving that review clears the review record, but it does not erase the fact that the automation was paused. Resume or re-arm it through the normal automation control path after the dependency is restored.

### 6. Drift And Health Reviews

Tandem can mark an automation for review when:

- repeated runs fail or block
- completed runs produce empty output
- guardrails stop runs repeatedly

The health check updates `health_findings` and usually opens a `lifecycle_review`.

### 7. Expiration And Retirement

Agent-authored automations may carry an expiration timestamp.

When they approach expiration:

- `review_kind` becomes `expiration_soon`
- a `retirement_action` approval may be opened

When they expire:

- `expired_at_ms` is set
- `paused_for_lifecycle` becomes `true`
- `review_kind` becomes `expired`
- the automation is paused

Retiring an automation sets:

- `retired_at_ms`
- `retire_reason`
- `paused_for_lifecycle = true`
- `review_kind = retired`

### 8. Deletion And Restore

Deletion is soft by default.

When an automation is deleted:

- `deleted_at_ms` is set
- `delete_retention_until_ms` is set
- the deleted automation is retained for the restore window

Restoring the automation clears the delete markers and brings the record back.

## Approval Decision Semantics

`POST /governance/approvals/{approval_id}/approve` has two effects:

1. it marks the approval request as approved
2. for lifecycle review requests, it acknowledges the matching automation or agent review state

That means approval is not just a UI checkbox. It is the operator action that resolves the review ticket.

For dependency revocation, the approval clears the review record, but the automation may still need a normal resume or re-arm action.

## Governance Read Surfaces

These routes are the useful inspection surfaces:

- `GET /governance/approvals`
- `POST /governance/approvals`
- `POST /governance/approvals/{approval_id}/approve`
- `POST /governance/approvals/{approval_id}/deny`
- `GET /governance/reviews`
- `GET /automations/v2/{id}/governance`
- `GET /governance/spend`
- `GET /governance/agents/{agent_id}/spend`

Use `GET /automations/v2/{id}/governance` when you need the complete state for one automation.

## What Not To Do

- Do not assume approval means automatic resume.
- Do not assume `review_required = false` means the runtime policy is open.
- Do not treat `published_externally` as an invitation to post without approval.
- Do not override lineage depth or spend caps from the client side.
- Do not let an approval flow bypass the audit log.

## Related Docs

- [Governance Reference](./governance/)
- [Self-Operator Playbook](../self-operator-playbook/)
- [MCP Capability Discovery And Request Flow](../mcp-capability-discovery-and-request-flow/)
