# Tandem Documentation

This folder contains public technical references.

For end-user onboarding journeys (install, first run, desktop/CLI paths), use:

- `../guide/src/content/docs/`

## User Guides

- [Ollama Guide](./OLLAMA_GUIDE.md) - Provider-specific setup notes.

## Technical Documentation

- [Design System](./DESIGN_SYSTEM.md) - Detailed style/system notes.
- [EU AI Act Readiness](./EU_AI_ACT_COMPLIANCE.md) - CISO-facing control mapping, current Tandem coverage, and roadmap gaps.
- [Compliance Starter Pack](./compliance/README.md) - Public EU AI Act starter docs for deployers, CISOs, and compliance teams.
- [Enterprise Readiness](./ENTERPRISE_READINESS.md) - Current enterprise capabilities, in-progress work, and roadmap boundaries.
- [Runtime Trust Boundaries](./RUNTIME_TRUST_BOUNDARIES.md) - Hosted vs self-hosted trust boundaries, auth modes, and protected audit evidence.
- [Stateful Runtime Enterprise Threat Model](./STATEFUL_RUNTIME_ENTERPRISE_THREAT_MODEL.md) - Threat model for scoped runs, webhooks, delegation grants, MCP run-as, receipts, and replay.
- [Runtime Events](./RUNTIME_EVENTS.md) - Canonical event schema, durable replay log, and tenant-scoped query contract.
- [Stateful Runtime Durable Kernel](./STATEFUL_RUNTIME_DURABLE_KERNEL.md) - File-backed runtime constraints, tactical guardrails, and embedded-store migration direction.
- [PostgreSQL Stateful Storage](./POSTGRES_STATEFUL_STORAGE.md) - Backend selection, verified transfer runbook, retention, and backups for stateful orchestration data.
- [Context Assertion Security](./CONTEXT_ASSERTION_SECURITY.md) - Signed tenant assertion keysets, replay behavior, clock skew, and rotation runbook.
- [Cross-Tenant Grants Design](./CROSS_TENANT_GRANTS_DESIGN.md) - Signed grant envelope, inbound lookup, trust root, and enforcement design for governed tenant-to-tenant sharing.
- [Default DataBoundary Enforcement Design](./DATA_BOUNDARY_ENFORCEMENT_DESIGN.md) - Default data-class boundary policy and trigger for governed reads.
- [Engine Protocol Matrix](./ENGINE_PROTOCOL_MATRIX.md) - Wire contracts and status.
- [Engine Context Assembly Map](./ENGINE_CONTEXT_ASSEMBLY_MAP.md) - Provider-facing context assembly paths, context-budget telemetry, and Full-context guardrails.
- [Repo Intelligence Architecture](./repo-intelligence/architecture.md) - Source-derived repo graph, agent workflow, confidence rules, and context-bundle debugging.
- [Context Graph Taxonomy](./repo-intelligence/context-graph-taxonomy.md) - Shared graph node/edge taxonomy, trust semantics, and versioning rules.
- [Hybrid GraphRAG Follow-Up](./repo-intelligence/hybrid-graphrag.md) - Chunk-level retrieval, optional vector hooks, trace provenance, and merge rules.
- [Repo Graph And Governance Memory Semantics](./repo-intelligence/governance-memory-semantics.md) - Scope, provenance, freshness, visibility, and redaction contract across repo graph and memory evidence.
- [Context Evals](./CONTEXT_EVALS.md) - Long-session context regression evals with provenance assertions.
- [AI Runtime Infrastructure](./AI_RUNTIME_INFRASTRUCTURE.md) - Engine source-of-truth runtime for long-running context, replay, and guardrails.
- [Memory Ciphertext At Rest](./MEMORY_CIPHERTEXT_AT_REST.md) - Memory crypto modes, encrypted columns, search-required plaintext residuals, and backup implications.
- [Memory Scope Model](./MEMORY_SCOPE_MODEL.md) - The scope dimensions each memory store enforces (tenant, org-unit, subject, tier), their semantics, and the fail-closed rules.
- [MCP Improvements](./MCP_IMPROVEMENTS.md) - Connector tools, MCP discovery, and allowlist design.
- [Enterprise MCP Identity And Delegation](./ENTERPRISE_MCP_IDENTITY_AND_DELEGATION.md) - Principal-scoped MCP connections, OAuth ownership, delegation, and audit model for multi-employee enterprise runtimes.
- [GitHub Projects via MCP](./MCP_IMPROVEMENTS.md#implementation-note-github-projects-via-mcp) - Tandem auto-registers the official GitHub MCP server when a PAT is available, so GitHub Projects work without manual `mcp add`.
- [Workflow Automation Runtime](./WORKFLOW_RUNTIME.md) - How Tandem's workflow runtime produces verifiable, trustworthy artifacts across multi-stage AI pipelines.
- [Workflow Bug Replay Guide](./WORKFLOW_BUG_REPLAY.md) - How to turn live workflow failures into deterministic replay regressions and release gates.
- [Workflow Generated Variation Coverage](./WORKFLOW_GENERATED_VARIATIONS.md) - Constrained generator design and nightly workflow-variation coverage.
- [Channel Lifecycle and Diagnostics](./CHANNELS_LIFECYCLE_AND_DIAGNOSTICS.md) - Registry-based channel runtime lifecycle, endpoint behavior, and required config keys.

## Incident Monitor & Automation Webhooks

- [Adversarial Scenario Packs](./incident-monitor-adversarial-scenario-packs.md) - Versioned dry-run abuse scenarios exercised against live routing, approval, and readiness logic.
- [Governance Maturity Metrics](./incident-monitor-governance-maturity-metrics.md) - Redacted governance metrics, operator thresholds, and behavioral drift detection.
- [Continuous Reassessment](./incident-monitor-continuous-reassessment.md) - Scheduled and change-triggered governance reassessment with versioned, fingerprint-deduplicated results.
- [External Log Intake Demo](./incident-monitor-external-log-intake-demo.md) - End-to-end external log source intake walkthrough.
- [Using Notion Webhooks with Tandem](./automation-v2-notion-webhooks.md) - Notion provider setup, verification-token flow, and signed-event handling for Automation V2 webhooks.
- [Using Linear Webhooks with Tandem](./automation-v2-linear-webhooks.md) - Linear provider setup, signing-secret import flow, project/label guard, and troubleshooting for Automation V2 webhooks.

## Meta-Harness

- [Optimizer Loop](./meta-harness/optimizer-loop.md) - Design contract for turning scored traces into candidate proposals.
- [Candidate Scoring And Promotion](./meta-harness/candidate-scoring-promotion.md) - Scored version summaries, candidate ranking, and promotion states.
- [Approval Surfaces](./meta-harness/approval-surfaces.md) - Human review surfaces for comparing, approving, rejecting, and promoting candidates.

## SDK & Development

- [Engine CLI Guide](./ENGINE_CLI.md)
- [Engine Testing](./ENGINE_TESTING.md)

## Release Notes

- Canonical: [Release Notes](../RELEASE_NOTES.md)
- Compatibility pointer: `./RELEASE_NOTES.md`
