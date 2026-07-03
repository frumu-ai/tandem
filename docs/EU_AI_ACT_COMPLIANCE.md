# Tandem EU AI Act Readiness

Status: public readiness brief
Date: 2026-05-19
Audience: CISOs, security reviewers, compliance teams, platform engineering

This document describes how Tandem's current runtime architecture maps to selected Chapter III requirements and Article 50 transparency obligations in the EU Artificial Intelligence Act, Regulation (EU) 2024/1689, and what Tandem plans to implement next.

It is intended as a security and compliance orientation document. It is not legal advice, a conformity assessment, or a complete compliance statement for any particular deployment. Actual obligations depend on the customer use case, deployment model, role in the AI Act value chain, data processed, sector-specific law, and the customer's governance controls.

## Executive Summary

Tandem is built as governed AI runtime infrastructure rather than a model-only interface. Its current architecture already includes several primitives that are relevant to high-risk AI governance:

- Durable run state, workflow graphs, checkpoints, artifacts, validations, and receipts.
- Human approval gates that pause a run and support approve, rework, or cancel decisions.
- Tenant context, principals, authority chains, verified tenant context, and secret references in the enterprise contract.
- Step-level built-in tool and MCP connector scoping.
- Protected audit envelopes and an admin-gated audit stream.
- A hash-chained, fsync-durable protected audit ledger with verifiable export bundles (`/audit/ledger/manifest`, `/audit/ledger/export`) and governance evidence completeness checks that cross-reference protected actions, approvals, tool effects, and audit events.
- Runtime tool-policy enforcement for tenant-aware protected actions.
- Fintech strict-mode policy that classifies protected financial mutations and fails closed unless a matching tenant-scoped approval receipt is verified at tool execution time.
- Source-available Rust implementation that security and compliance teams can inspect.

Tandem's current coverage is strongest as a control-plane foundation for human oversight, execution traceability, scoped tool access, protected-action enforcement, and customer-reviewable audit evidence. A public compliance starter pack ships under `docs/compliance/` (Annex IV technical documentation template, control mapping one-pager, deployer instructions, and a limitations and responsibility matrix). The main remaining work is auditor-grade packaging and hardening: immutable storage integrations, signed evidence bundles where required, explicit retention controls, turnkey SIEM connectors, Article 50 AI-generated labeling, enterprise identity/RBAC, role-based oversight policy, formal accuracy/robustness metrics, and adversarial security testing.

## Scope And Assumptions

The official AI Act Explorer describes Regulation (EU) 2024/1689 by chapter, article, recital, annex, obligations, penalties, and application dates. Article 113 says the Regulation generally applies from 2 August 2026, with earlier and later exceptions. Article 6 classifies AI systems as high-risk when they meet product-safety criteria or fall within Annex III, subject to limited exceptions where the system does not materially influence high-impact decisions.

This brief also tracks Article 50 because its transparency obligations sit outside Chapter III and can apply beyond high-risk systems. Article 50 includes obligations to inform natural persons when they interact directly with an AI system, to mark AI-generated or manipulated audio, image, video, or text outputs in machine-readable form where applicable, and to provide disclosures clearly, distinguishably, accessibly, and by first interaction or exposure.

For regulated financial services, relevant Annex III categories can include:

- AI systems used to evaluate the creditworthiness of natural persons or establish a credit score, except systems used for detecting financial fraud.
- AI systems used for life and health insurance risk assessment and pricing in relation to natural persons.
- Certain biometric systems, where permitted by Union or national law.
- Adjacent domains such as employment, education, essential public services, law enforcement, migration, and critical infrastructure.

Tandem should be assessed per deployment. Depending on packaging and integration, Tandem may be part of a provider system, a deployer-operated runtime, or a component in a broader value chain. This document focuses on the runtime controls Tandem provides or plans to provide.

## Control Coverage Matrix

| AI Act area                                      | Current Tandem support                                                                                                                                                                                                                                                                                                                                                          | Current status    | Planned hardening                                                                                                                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Article 11 - Technical documentation             | Public docs describe runtime architecture, workflow execution, enterprise readiness, logging, QA, and regulated-operation boundaries. Source-available Rust modules expose the engine, automation gates, audit append path, tenant contract, and fintech policy. An Annex IV technical documentation template and deployer instructions ship under `docs/compliance/`.          | Partial           | System cards, model/provider inventory, intended-purpose statements, architecture diagrams, version history, and per-use-case limitations.                                                                                   |
| Article 12 - Record-keeping                      | Automation attempt receipts are stored as JSONL with sequence numbers. Protected audit envelopes capture durable required events in a hash-chained, fsync-durable ledger with verifiable export bundles and manifests. Audit stream emits approval, tool-effect, channel capability, and fintech protected-action events. Governance evidence exports include completeness checks and an explicit event taxonomy for customer-reviewable evidence packets. | Strong foundation | Immutable/WORM storage adapters, per-tenant retention policies, turnkey SIEM connector integrations, signed bundles, and clock/source metadata.                                                                              |
| Article 14 - Human oversight                     | Automation V2 supports approval gates with approve, rework, cancel, expiry, reminder, and escalation policy metadata. Runs enter `awaiting_approval`; decisions and expiry outcomes are recorded in gate history. Approvals inbox and channel approval surfaces route decisions through authoritative handlers.                                                                 | Strong foundation | Role-based assignment, dual-control approval policies, operator training/competence records, automation-bias UX controls, output interpretation aids, and enterprise policy checks on protected approvals.                   |
| Article 15 - Accuracy, robustness, cybersecurity | Artifact contracts, validation metadata, repair loops, evaluation framework, scoped tool policies, tenant-aware execution, secret references, and fintech strict-mode fail-closed policy support robustness.                                                                                                                                                                    | Partial           | Formal accuracy metrics, benchmark declarations, prompt-injection/adversarial regression suites, data/model poisoning controls where applicable, vulnerability management, incident drills, and lifecycle security evidence. |
| Article 26 - Deployer obligations                | Tandem can help deployers monitor operation, preserve logs under their control, assign oversight workflows, and suspend or cancel runs. Deployer instructions for use ship under `docs/compliance/`.                                                                                                                                                                            | Supportive        | Log retention configuration, incident reporting workflow, operator training evidence, and integration guidance for governance records.                                                                                      |
| Article 50 - Transparency for certain AI systems | Tandem already has runtime provenance for generated plans, artifacts, and AI-assisted outputs, but visible standardized UI labeling is not yet systematic across desktop and web surfaces.                                                                                                                                                                                      | Gap / planned     | Add visible `AI-Generated` badges to generated text, proposed plans, and artifacts; add accessible labels/tooltips; preserve provenance metadata in exports where practical.                                                 |

## Current Evidence In The Repository

Security and compliance reviewers can inspect the following surfaces:

- Runtime architecture: `docs/AI_RUNTIME_INFRASTRUCTURE.md`
- Enterprise status and boundaries: `docs/ENTERPRISE_READINESS.md`
- Workflow validation and artifact contracts: `docs/WORKFLOW_RUNTIME.md`
- Structured logging schema: `docs/LOGGING_SCHEMA.md`
- Enterprise proof walkthrough: `docs/ENTERPRISE_PROOF_WALKTHROUGH.md`
- Incident Monitor production governance map: `guide/src/content/docs/incident-monitor/production-governance.md`
- AI quality assurance: `docs/user/AI_QUALITY_ASSURANCE.md`
- Tenant, principal, authority chain, verified tenant context, and secret refs: `crates/tandem-enterprise-contract/src/lib.rs`
- Automation DAG nodes, dependencies, approval gates, run status, and gate history: `crates/tandem-server/src/automation_v2/types.rs`
- Gate pause and gate decision state transitions: `crates/tandem-server/src/app/state/automation/gates.rs`
- Approval aggregation API: `crates/tandem-server/src/http/approvals.rs`
- Control panel approvals inbox: `packages/tandem-control-panel/src/pages/ApprovalsInboxPage.tsx`
- Protected audit envelopes: `crates/tandem-server/src/audit.rs`
- Admin audit stream: `crates/tandem-server/src/http/audit_stream.rs`
- Automation attempt receipt ledger: `crates/tandem-server/src/app/state/automation/receipts.rs`
- Fintech strict classification and protected action hashing: `crates/tandem-core/src/fintech.rs`
- Runtime tool-policy enforcement for tenant match and approval-receipt checks: `crates/tandem-server/src/agent_teams_parts/part01.rs`

## Article 14: Human Oversight

### Current Implementation

Tandem represents sensitive work as a workflow graph where consequential steps can depend on review or approval nodes. The runtime can pause a run before a protected action and persist a gate with decisions such as approve, rework, or cancel.

Current implementation supports:

- `AutomationFlowNode.depends_on` for DAG dependencies.
- `AutomationApprovalGate` and `AutomationPendingGate` for human review gates.
- `AutomationRunStatus::AwaitingApproval` for paused execution.
- `apply_automation_gate_decision` for approve, rework, and cancel paths.
- Gate history records with decision, reason, timestamp, and metadata.
- A unified pending approvals API and control-panel inbox.
- Channel approval surfaces with authorization checks.
- Gate expiry policies that expose deadlines, reject late decisions after
  auto-cancel expiry, and can remind or escalate while keeping a gate decidable.
- Protected audit and runtime events for expiry, reminder, and escalation
  outcomes.

The fintech strict path adds a stronger enforcement pattern: approval-gate state is not treated as authorization by itself. For protected fintech tools, the runtime looks for a matching approval receipt with the same tenant, protected action category, tool, canonical action hash, and non-expired metadata. Without that match, execution fails closed.

### Planned Work

Tandem still needs enterprise-grade oversight controls:

- Configurable approval policies by action class, tenant, user, role, amount, and risk tier.
- Dual-control approval flows where required by customer policy or use case.
- Human operator assignment with competence or training evidence.
- UI controls that help operators interpret model output and mitigate automation bias.
- A global emergency stop that can halt runs, queued actions, and connector mutations by tenant or workspace.
- Required enterprise-mode policy enforcement, not only runtime-local checks.

## Article 12: Record-Keeping

### Current Implementation

Tandem currently records execution evidence through multiple layers:

- Engine-owned run identity, checkpoint state, lifecycle events, and artifacts.
- JSONL automation attempt receipts with run ID, node ID, attempt, session ID, sequence, timestamp, event type, and payload.
- Protected audit envelopes with event ID, durability marker, event type, tenant context, actor, payload, and timestamp.
- Admin-gated `/audit/stream` NDJSON feed for selected audit-relevant events.
- A hash-chained, fsync-durable protected audit ledger with verifiable NDJSON export bundles and manifests (`/audit/ledger/manifest`, `/audit/ledger/export`) plus governance evidence export completeness checks that cross-reference each protected action's policy decision, approval, tool effect, and audit event for customer-owned review.
- Tool-effect and fintech protected-action event mapping.
- Fintech audit package builder that groups run ID, tenant, actor, tool calls, connector proof, artifacts, approvals, policy decisions, and limitations.

This is a strong foundation for Article 12 traceability. It is not yet a complete compliance logging subsystem for every regulated deployment.

### Planned Work

Planned record-keeping hardening includes:

- Optional signing of protected audit bundles.
- Immutable storage adapters such as S3 Object Lock, Azure immutable blobs, GCS retention lock, or customer WORM targets.
- Turnkey SIEM connector integrations with retry and backpressure handling.
- Retention and deletion policy controls per tenant and use case.
- Stable schema versioning and migration notes for auditors.
- Redaction policy that preserves audit usefulness without logging secrets or protected personal data unnecessarily.

## Article 11: Technical Documentation

### Current Implementation

Tandem has a source-available runtime and public technical documentation. Security reviewers can inspect how the control plane works:

- How workflow graphs are represented.
- How approval gates pause, resume, rework, or cancel.
- How tenant context and authority chains are modeled.
- How protected audit events are appended.
- How tool scope and MCP scope are enforced.
- How fintech protected actions are classified and hashed.
- How missing or mismatched approval evidence fails closed.

The current documents are engineering-oriented. They provide useful evidence, but they are not yet a complete Annex IV technical documentation package.

### Planned Work

Planned documentation work includes:

- Annex IV technical documentation template.
- Product/system cards for Tandem runtime, automation planner, execution engine, and connector layer.
- Intended-purpose and prohibited-use statements.
- Instructions for deployers and human overseers.
- Model/provider inventory and change history.
- Accuracy, robustness, and cybersecurity declarations.
- Data flow diagrams and data retention notes.
- Human oversight assessment linked to Article 14.
- Known limitations and residual-risk register.
- Responsibility mapping that separates Tandem controls from customer controls.

## Article 15: Accuracy, Robustness, And Cybersecurity

### Current Implementation

Tandem's core Article 15-relevant security pattern is separation of model reasoning from execution authority. The model can propose a tool call, but the runtime decides whether the call is allowed. That decision can consider:

- Session-scoped allowed tools.
- Automation node tool/MCP policy.
- Tenant context and verified tenant assertions.
- Read-only source-of-truth file protections.
- Fintech strict protected-action classification.
- Matching approval receipts and protected action hashes.
- Fail-closed behavior for unknown external mutation tools.

This separation is important for prompt-injection resistance. A malicious prompt can ask the model to send money, change a risk rating, file a report, or email a customer, but the protected execution path can deny the tool call unless policy and approval evidence match.

Tandem also has reliability controls:

- Artifact contracts and validation metadata.
- Concrete path enforcement.
- Repair loops and retry accounting.
- QA docs and regression gates.
- Provider failure classification and fallback concepts.
- Secret redaction and secret-reference validation foundations.

### Planned Work

Tandem needs formal assurance artifacts before treating Article 15 readiness as mature:

- Published accuracy metrics by workflow class and model/provider.
- Robustness benchmarks and failure-mode thresholds.
- Prompt-injection and malicious-document regression suite.
- Connector-level adversarial tests for external mutation tools.
- Security threat model covering prompt injection, connector compromise, local sidecar exposure, XSS, secrets, and tenant isolation.
- Secure SDLC and vulnerability management process.
- Incident response runbooks and serious-incident escalation hooks.
- Evidence that protected paths fail closed in hosted and enterprise deployments.

## Article 50: Transparency Labeling

### Current Implementation

Tandem's runtime distinguishes model-generated plans, automation outputs, artifacts, and receipts in its execution state. That provenance is useful for debugging and audit review, but the desktop app and web control panel do not yet apply a consistent visible label across every generated-content surface.

The current gap is straightforward: users and reviewers should be able to identify AI-generated or AI-assisted content directly in the UI, without needing to inspect run metadata.

### Planned Work

Planned Article 50 transparency work includes:

- Add a reusable `AI-Generated` badge component for desktop and web panel surfaces.
- Apply the badge to generated text, proposed workflow plans, plan previews, artifacts, summaries, briefs, and generated handoff content.
- Add accessible label text and tooltip/help text explaining that the content was produced or materially transformed by an AI system.
- Preserve AI-generation provenance in exported artifacts where practical, for example with metadata fields or document headers.
- Add tests or visual checks for the main generated-content surfaces.
- Document when human-reviewed or editorially controlled content changes status from generated draft to approved artifact.

## Regulated Financial Services Use Cases

Tandem's safest current fit in regulated financial services is AI-assisted compliance, risk, and operations workflows around high-risk systems, not autonomous final decision-making.

Examples:

- Credit model change review packet.
- AI-assisted credit policy exception investigation.
- Regulatory change impact brief with cited sources.
- Vendor/model monitoring evidence packet.
- Risk rating recommendation with mandatory human approval before system-of-record update.
- Customer communication draft with approval and audit trail before delivery.
- Incident triage workflow that gathers evidence but cannot file reports or notify customers without verified approval.

Actions such as autonomous credit decisions, money movement, account freezes, adverse-action notices, regulatory filings, and risk-rating changes should be treated as protected actions. Tandem's planned enterprise posture is to require strict runtime policy, verified tenant context, and matching approval receipts before those actions can execute.

## Implementation Roadmap

### Phase 1: Compliance Evidence Pack

- Shipped: Annex IV documentation template under `docs/compliance/` (`ANNEX_IV_TECHNICAL_DOCUMENTATION_TEMPLATE.md`).
- Shipped: public EU AI Act control mapping one-pager (`AI_ACT_CONTROL_MAPPING.md`).
- Shipped: instructions for deployers and human overseers (`DEPLOYER_INSTRUCTIONS.md`).
- Shipped: public limitations and responsibility matrix (`LIMITATIONS_AND_RESPONSIBILITIES.md`).
- Implement Article 50 `AI-Generated` badges in the desktop app and web control panel.
- Create a regulated-finance demo workflow that produces an audit package without mutating external systems.

### Phase 2: Immutable Evidence Custody

- Harden audit exports with signed bundle metadata where required.
- Add immutable storage and turnkey SIEM connector adapters.
- Add retention configuration with a minimum six-month deployer baseline where applicable.
- Shipped: completeness checks for approvals, denials, and protected tool calls (governance evidence `audit_completeness` block).

### Phase 3: Enterprise Enforcement

- Finish required enterprise mode and fail-closed sidecar behavior.
- Add OIDC/SSO, SCIM, RBAC, and group-based approval policies.
- Add role and competence metadata for human overseers.
- Add dual-control approval policies for configured high-risk action classes.
- Add tenant propagation tests across sessions, automations, runs, artifacts, approvals, queues, memory, logs, streams, and exports.

### Phase 4: Article 15 Assurance

- Publish workflow-class accuracy and robustness metrics.
- Add adversarial/prompt-injection regression suites.
- Add protected tool red-team tests.
- Add incident response and post-market monitoring workflow templates.
- Add model/provider change management and rollback evidence.

## Open Review Questions

- Which Tandem deployment model is being assessed: local, self-hosted, hosted, or enterprise sidecar?
- Which party is acting as provider, deployer, distributor, importer, or component supplier for the use case?
- Which workflows materially influence decisions about natural persons under Annex III?
- Which logs contain personal data, and what retention/minimization policy is compatible with GDPR and sector-specific law?
- What customer systems of record should receive Tandem audit IDs or ledger root digests?
- Which actions require one approver, two approvers, compliance review, legal review, or board-level controls?
- What accuracy metrics are meaningful for each workflow class: classification accuracy, citation precision, artifact validation pass rate, tool-call denial rate, or human override rate?
- What external conformity assessment, harmonised standards, or internal governance frameworks are required by the deployment?

## Summary Assessment

Tandem has a credible architecture for EU AI Act readiness because it treats AI work as governed runtime execution rather than unstructured model output. Current strengths include human approval gates, durable execution records, tenant-aware control paths, scoped tools, artifact validation, and protected-action enforcement.

The most important implementation work ahead is to convert those primitives into auditor-ready controls: visible AI-generated labeling, immutable evidence custody, signed bundles where required, Annex IV documentation, instructions for use, enterprise identity and policy enforcement, and formal Article 15 assurance evidence.

## References

- Official AI Act Explorer: https://ai-act-service-desk.ec.europa.eu/en/ai-act-explorer
- Article 6, high-risk classification: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-6
- Annex III, high-risk use cases: https://ai-act-service-desk.ec.europa.eu/en/ai-act/annex-3
- Article 11, technical documentation: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-11
- Article 12, record-keeping: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-12
- Article 14, human oversight: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-14
- Article 15, accuracy, robustness, and cybersecurity: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-15
- Article 26, deployer obligations: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-26
- Article 50, transparency obligations for certain AI systems: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-50
- Annex IV, technical documentation contents: https://ai-act-service-desk.ec.europa.eu/en/ai-act/annex-4
- Article 113, entry into force and application: https://ai-act-service-desk.ec.europa.eu/en/ai-act/article-113
- EUR-Lex official text: https://eur-lex.europa.eu/eli/reg/2024/1689/oj
