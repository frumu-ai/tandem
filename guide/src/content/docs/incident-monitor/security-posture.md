---
title: Incident Monitor Security Posture Mode
description: Position Incident Monitor security posture assessment without overclaiming or encouraging unsafe probes.
---

Incident Monitor can become an AI-agent security posture layer, but it must be positioned precisely.

Tandem should not claim to find every vulnerability in an enterprise system. Its scope is narrower and more defensible: identify and govern security gaps around AI agents, tool access, workflow automation, external integrations, tenant/workspace context, approvals, and incident routing.

## Direction

Security posture mode should connect:

- authority inventory for agents, workflows, tools, MCP servers, destinations, sources, and approvals
- deterministic posture rules and baseline checks
- controlled assessment probes
- incidents and route decisions
- evidence-backed reports and export packs

## Authority inventory

Before identifying gaps, Tandem needs a queryable inventory of what authority exists:

- which workflows and agents can call which tools
- which destinations can receive which incidents
- which sources can report and where they can route
- which approvals guard privileged actions
- which tenant/workspace context applies

## Controlled probes

Controlled probes must be authorized, bounded, dry-run or sandboxed where possible, and auditable. They should verify Tandem controls, not attack third-party systems.

Examples of safe probe categories:

- approval gate enforcement
- route preview and destination readiness
- scoped intake key limits
- tool policy enforcement
- tenant/workspace context boundaries

## Self-monitoring boundary

Tandem can monitor its own runtime, but it is the runtime/control layer, not the model. Self-monitoring should focus on Tandem incidents, route decisions, publish attempts, approval decisions, monitor config changes, and audit export.

Enterprise deployments should have a customer-owned external audit export path so Tandem's own incidents are not trapped inside the system being assessed.

## Report posture carefully

Good wording:

- "Tandem identifies governed AI-agent workflow and tool-authority gaps."
- "Tandem records evidence for route, approval, and destination decisions."
- "Tandem can run authorized checks against its own governance controls."

Avoid:

- claims that Tandem finds every vulnerability
- uncontrolled probing of customer systems
- publishing sensitive incident payloads without redaction and approval
