---
title: Incident Monitor Security Posture Mode
description: Position Incident Monitor security posture assessment without overclaiming or encouraging unsafe probes.
---

Incident Monitor security posture mode helps teams see where AI agents, workflows, tools, destinations, tenants, and approvals create more authority than the business intended.

It is not a general vulnerability scanner. Tandem does not claim to find every weakness in an enterprise system, replace a SIEM, or prove that its own runtime is safe. Its defensible scope is narrower: governed runtime security for AI agents that use real tools, publish to external systems, operate across tenant/workspace context, and need approval and evidence trails.

## Positioning statement

Tandem is AI Agent Security Posture Monitoring for governed workflows, tools, tenants, approvals, and incident routing.

Use this wording when precision matters:

- Tandem shows where AI agents have more authority than the business intended.
- Tandem identifies governed AI-agent workflow and tool-authority gaps.
- Tandem records evidence for route, approval, destination, and publish decisions.
- Tandem can run authorized checks against its own governance controls.

Avoid:

- claims that Tandem finds every vulnerability
- claims that Tandem replaces SAST, DAST, SIEM, CSPM, or EDR
- uncontrolled probing of customer systems
- publishing sensitive incident payloads without redaction and approval
- implying Tandem can independently prove Tandem itself is safe

## Buyer personas

- CTO: wants AI automation in production without unmanaged tool sprawl.
- CISO: wants visibility into agent authority, external mutation paths, and approval gaps.
- Platform engineering: wants tenant/workspace boundaries, MCP tools, workflows, and destinations to be inventoryable and governable.
- AI engineering: wants safe runtime patterns for agents that call tools, route findings, and produce remediation work.
- Compliance and risk: wants evidence-backed reports for approvals, routes, publish attempts, receipts, and exception handling.

## Product layers

Security posture mode should be described as a layer on top of the existing Tandem runtime:

- Governed runtime: workflows, agents, tool policies, MCP policy, tenant/workspace context, and approval gates.
- Incident Monitor: passive intake, triage, routing, destination readiness, approvals, receipts, and remediation routing.
- Security posture assessment: read-only authority inventory, posture rules, controlled checks, evidence reports, and remediation issues.

The first posture primitive is the read-only authority inventory at `GET /incident-monitor/security/authority-inventory`. It summarizes workflows, Automation V2 specs, agent/tool/MCP policy, Incident Monitor destinations and routes, monitored sources, scoped intake keys, approvals, policy decisions, and recent external publish surfaces without exposing raw secrets or credentials.

## What Tandem detects

Tandem can detect and report gaps that are visible from governed runtime configuration, runtime decisions, and Incident Monitor evidence:

- agents with write-capable tools and no clear approval policy
- MCP servers or tools exposed without a narrow allowlist
- external mutation destinations that do not require approval
- workflows, sources, or automations missing tenant/workspace context
- monitored sources that can report incidents without an allowed destination policy
- publish destinations without clear receipt, audit, redaction, or retention posture
- broad tool policies where a narrower scope is expected
- scoped intake keys that should remain report-only
- route decisions that send high-risk incidents to the wrong destination class
- repeated policy denials, approval waits, or destination failures that indicate governance drift

## What Tandem does not detect

Tandem should not be positioned as detecting:

- arbitrary code vulnerabilities in application source
- network exposure or infrastructure misconfiguration outside Tandem's configured evidence
- malware, endpoint compromise, or host-level behavior
- every prompt injection or model-level failure mode
- vulnerabilities in third-party systems reached by configured tools
- unobserved shadow agents, credentials, or integrations not connected to Tandem
- compliance completeness without customer policy mapping and external evidence review

## Authority inventory

Before identifying gaps, Tandem needs a queryable inventory of what authority exists.

Inventory should answer:

- which workflows, automations, and agents exist
- which tools and MCP servers agents can use
- which destinations can publish externally
- which sources can report and where they can route
- which approvals guard privileged actions
- which tenant/workspace context applies
- which recent external-action surfaces exist for evidence

Sensitive values are summarized, not returned. Inventory includes identifiers and field-presence signals so later findings can reference evidence without leaking intake keys, token values, HMAC secrets, auth headers, webhook secret refs, action receipts, or arbitrary metadata values.

## Controlled probes

Controlled probes must be authorized, bounded, dry-run or sandboxed where possible, and auditable. They should verify Tandem controls, not attack third-party systems.

Safe probe categories:

- approval gate enforcement
- route preview and destination readiness
- scoped intake key limits
- tool policy enforcement
- tenant/workspace context boundaries
- redaction and retention policy behavior

Unsafe probe framing:

- attacking customer production systems
- brute forcing credentials or auth headers
- sending synthetic sensitive data to external destinations without approval
- executing arbitrary MCP tools outside a configured dry-run or sandbox boundary

## Demo storyline

1. Passive monitoring collects failures, incidents, route decisions, approval waits, and publish receipts.
2. Authority inventory shows agents, workflows, MCP tools, destinations, sources, scoped intake keys, and tenant/workspace context.
3. Posture rules identify a concrete gap, such as a write-capable MCP tool with no approval policy or an external destination that can publish without approval.
4. A controlled dry-run probe verifies the Tandem governance control, such as route preview or scoped intake-key denial.
5. Tandem produces an evidence report with inventory references, route/approval evidence, redaction notes, and remediation guidance.
6. Incident Monitor routes remediation to the right destination, such as Linear, GitHub, webhook, telemetry, memory, or an MCP tool destination.

## Assessment report outline

- Executive summary: risk themes, scope, and no-overclaim boundary.
- Inventory scope: tenants, workspaces, workflows, automations, MCP servers, tools, sources, routes, and destinations assessed.
- Findings: title, severity, affected authority surface, evidence ids, policy expectation, observed state, and remediation.
- Approval and routing posture: where actions require approval, where they do not, and where default routing could surprise operators.
- Data handling: redaction, retention, evidence refs, receipt policy, and external export path.
- Probe results: authorized checks performed, dry-run/sandbox boundaries, and outcomes.
- Open questions: customer policy assumptions that Tandem cannot infer.
- Remediation plan: issues, owners, target destinations, and acceptance checks.

## Packaging implications

Security posture mode should be packaged as an enterprise governance capability, not as generic vulnerability scanning.

- Core Incident Monitor: intake, triage, routing, approvals, receipts, and destination readiness.
- Security Posture Monitoring: authority inventory, posture rules, evidence reports, controlled checks, and remediation routing.
- Enterprise Governance: tenant/workspace policy, audit export, compliance mapping, retention profiles, and external evidence destinations.

The package boundary should track governance value: who can see authority, who can assess it, who can approve controlled checks, and where reports can be exported.

For production rollout, pair this page with [Production Governance](./production-governance/). That page maps authority inventory, posture checks, probes, assessment reports, deployment cards, receipts, and protected audit evidence to the operator-owned policy decisions needed before external publish routes are trusted.

## Comparison

| Category            | What it covers                                        | Tandem relationship                                                                                                                   |
| ------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| SAST                | Static source-code vulnerability patterns             | Complementary. Tandem focuses on runtime agent authority, routes, approvals, and tool/destination surfaces.                           |
| DAST                | Runtime web/app security testing                      | Complementary. Tandem should not probe customer apps unless checks are explicitly authorized and bounded.                             |
| SIEM                | Centralized security event collection and correlation | Complementary. Tandem can export evidence and incidents, but does not replace the customer's SIEM.                                    |
| CSPM                | Cloud posture and infrastructure configuration        | Complementary. Tandem assesses AI-agent governance surfaces, not full cloud account posture.                                          |
| EDR                 | Endpoint detection and response                       | Complementary. Tandem does not claim host-level malware or endpoint compromise detection.                                             |
| Workflow automation | Task execution and integrations                       | Tandem adds governed runtime controls, authority inventory, approvals, evidence, and incident routing for AI agents using real tools. |

## Self-monitoring boundary

Tandem can monitor its own runtime, but it is the runtime and control layer, not the model and not an external auditor.

Self-monitoring should focus on Tandem incidents, route decisions, publish attempts, approval decisions, monitor config changes, policy decisions, and audit export. Enterprise deployments should keep a customer-owned external audit export path so Tandem's own incidents are not trapped inside the system being assessed.

Do not state that Tandem self-monitoring proves Tandem is safe. State that Tandem records evidence about its own governance controls and can export that evidence for customer-owned review.

## Related

- [Production Governance](./production-governance/)
- [Agent Runtime Guide](./agent-runtime-guide/)
- [Setup Checklist](./setup-checklist/)
