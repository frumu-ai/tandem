---
title: Governance Reference
description: Canonical reference for Tandem provenance chains, capability grants, approval queues, and audit events.
---

Tandem keeps governance explicit so agent-authored work stays auditable, scoped, and revocable.

If you are looking for the runtime token and transport layer, see [Engine Authentication For Agents](https://docs.tandem.ac/engine-authentication-for-agents/).
This page is about authorization scope, lineage, approvals, and audit.

## The short version

- Every agent-authored artifact carries provenance.
- Human-granted permissions are scoped and revocable.
- Sensitive or capability-expanding actions can pause in an approval queue.
- Audit events record who did what, when, and through which lineage.

## Provenance

The existing Tandem vocabulary already uses provenance fields such as:

- `creator_type`
- `creator_id`
- `creator_chain`
- `actor_type`
- `actor_id`
- `actor_chain`

The important part is not just identity, but lineage. `creator_chain` preserves the full ancestor chain so review can answer who created what and how the permission flowed.

Provenance is also what makes lineage depth enforceable. If an agent creates another agent that creates an automation, the chain gives the runtime and the operator a custody trail instead of an anonymous blob of work.

## Grants

Tandem's minimum grant model is intentionally narrow:

- modify grants let a human grant an agent permission to change a specific automation or workflow
- capability grants let the runtime or operator add a scoped capability rather than broad blanket access
- grants should be revocable, traceable, and tied to a concrete actor

The blog vocabulary for the current implementation uses `automation_grants` as the scoped grant table.

## Approval queue

Some actions should not execute immediately.

Use an approval queue when:

- a new capability needs human review
- a sensitive action is requested by a descendant agent
- the operator wants a deliberate pause before a mutation is applied

Agents should emit structured requests for review instead of self-approving.

For the concrete state machine behind these reviews, see [Automation Governance Lifecycle](./governance-lifecycle/).

## Audit events

Audit events make the governance model inspectable.

The current vocabulary uses `automation_audit_log` for the append-only trail, with fields such as:

- actor type
- actor id
- actor chain
- action
- diff
- timestamp

That trail is what lets Tandem answer:

- who changed this
- what changed
- when it changed
- what lineage produced the change

## Related docs

- [How Tandem Works Under the Hood](https://docs.tandem.ac/how-tandem-works/)
- [Creating And Running Workflows And Missions](https://docs.tandem.ac/creating-and-running-workflows-and-missions/)
- [Prompting Workflows And Missions](https://docs.tandem.ac/prompting-workflows-and-missions/)
- [Agent Workflow And Mission Quickstart](https://docs.tandem.ac/agent-workflow-mission-quickstart/)
- [Engine Authentication For Agents](https://docs.tandem.ac/engine-authentication-for-agents/)
- [Self-Operator Playbook](../self-operator-playbook/)
- [MCP Capability Discovery And Request Flow](../mcp-capability-discovery-and-request-flow/)
- [Automation Governance Lifecycle](./governance-lifecycle/)
