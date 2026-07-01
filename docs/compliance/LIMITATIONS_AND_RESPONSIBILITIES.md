# Limitations And Responsibilities

This document separates current Tandem capabilities from deployer responsibilities and known gaps. It is intended to help security and compliance teams decide what Tandem can support today and what must be supplied by the deployer or future Tandem work.

## Current Tandem Capabilities

Tandem currently provides:

- Durable workflow and automation run records.
- Workflow graphs with dependencies and approval gates.
- Approval decisions with approve, rework, and cancel paths.
- Runtime artifacts and validation metadata.
- Step-level built-in tool and MCP connector scoping.
- Tenant context and authority-chain foundations.
- Protected audit envelopes and selected audit stream events.
- Protected audit ledger/export foundations for customer-owned evidence review.
- Incident Monitor authority inventory, posture checks, controlled probes, assessment reports, deployment cards, destination route previews, and destination receipts.
- Automation attempt receipts.
- Fintech strict-mode protected-action classification and fail-closed checks for matching approval receipts.
- Public documentation and source-available runtime implementation.

## Known Gaps

The following items are not yet complete platform guarantees:

- Immutable/WORM storage adapters.
- Full RBAC, OIDC, SCIM, and enterprise identity enforcement.
- Turnkey SIEM connector integrations with retry/backpressure management.
- Formal Article 15 accuracy and robustness metrics.
- Systematic Article 50 badges across all desktop and web generated-content surfaces.
- Dual-control approval policies.
- Complete Annex IV technical documentation package.
- SOC2 or equivalent external security audit package.
- Required enterprise-mode sidecar enforcement for every protected path.

## Responsibility Matrix

| Area                        | Tandem current responsibility                                                                                                                   | Deployer responsibility                                                                                       | Planned Tandem work                                                                                   |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Workflow execution          | Provide durable runs, workflow state, artifacts, and validation metadata.                                                                       | Configure workflows appropriate to the use case. Monitor execution and respond to failures.                   | Improve audit exports and compliance mode defaults.                                                   |
| Human oversight             | Provide approval gates, approval history, and rework/cancel paths.                                                                              | Assign qualified reviewers and define approval policy.                                                        | Add role-based assignment and dual-control policies.                                                  |
| Protected actions           | Classify selected fintech protected actions and fail closed when matching approval evidence is missing.                                         | Identify protected actions for the deployment and keep unsafe actions blocked.                                | Expand protected-action taxonomy and enterprise policy enforcement.                                   |
| Logging                     | Record run state, receipts, protected audit events, selected audit stream events, and protected audit ledger/export evidence.                   | Configure retention, access controls, export, and records management.                                         | Add immutable storage, turnkey SIEM connectors, per-tenant retention enforcement, and signed bundles. |
| Transparency                | Preserve runtime provenance for generated content.                                                                                              | Disclose AI-generated or AI-assisted content where required.                                                  | Add systematic `AI-Generated` badges and export provenance.                                           |
| Technical documentation     | Provide public architecture, readiness, runtime, logging, and QA docs.                                                                          | Complete deployment-specific documentation and use-case analysis.                                             | Provide Annex IV templates and system cards.                                                          |
| Accuracy and robustness     | Provide artifact contracts, validation metadata, repair loops, QA docs, and scoped execution controls.                                          | Define workflow-specific acceptance criteria, monitor quality, and review outputs.                            | Publish workflow metrics and adversarial regression suites.                                           |
| Security governance         | Provide source-available runtime controls and security roadmap documentation.                                                                   | Operate identity, access, incident response, and provider governance.                                         | Add enterprise identity integrations, policy sidecar enforcement, and security evidence package.      |
| Incident Monitor governance | Provide authority inventory, posture checks, controlled probes, assessment reports, deployment cards, route previews, and destination receipts. | Map findings to policy, assign owners, approve probes, configure evidence custody, and run incident response. | Add deeper production-readiness automation, maturity/drift metrics, and incident-response templates.  |

## Deployment Assumptions To Validate

Before production use in a regulated workflow, deployers should validate:

- The workflow's AI Act role and risk classification.
- Whether the workflow materially affects natural persons.
- Which data categories enter Tandem and connected providers.
- Which actions are protected and which are prohibited.
- Who can approve or cancel protected actions.
- Where logs and artifacts are retained.
- How generated content is labeled and reviewed.
- How incidents are detected, reported, and remediated.

## Practical Starting Point

For a first regulated deployment, use Tandem for evidence preparation, drafting, investigation, and review packets. Keep external mutations and system-of-record changes approval-gated or blocked until the deployer has completed workflow-specific oversight, logging, retention, identity, and incident-response controls.
