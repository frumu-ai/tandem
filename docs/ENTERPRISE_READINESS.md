# Tandem Enterprise Readiness

This document separates what Tandem can credibly show today from what is still in progress or planned. Tandem is not yet a complete enterprise platform with OIDC, SCIM, SIEM export, SOC2, and private sidecar enforcement. The current proof is the runtime authority foundation those enterprise features will attach to, including tenant-aware policy decisions, governed approvals, scoped memory, protected audit evidence, and initial intra-tenant authority modeling.

## Available Now

- **Engine-owned run state:** Workflows and automations execute as durable runtime records rather than chat transcripts.
- **Plan preview and apply flow:** Intent can be compiled through plan preview/apply paths into runtime-owned workflow bundles.
- **Tenant context foundations:** The OSS enterprise contract includes tenant context, local implicit tenant defaults, principals, authority chains, and secret references.
- **Public enterprise status:** `/enterprise/status` exposes a public-safe summary of enterprise mode, bridge state, capabilities, contract version, and tenant context.
- **Approval gates and inbox:** Automation runs can pause on human gates, and the control panel includes an Approvals Inbox backed by the pending approvals aggregator.
- **Approval channel fan-out:** Slack, Discord, and Telegram approval delivery exists, including authorization checks, callback deduplication, user capability tiers, rate limits, and lifecycle updates.
- **Durable protected audit records:** Protected events such as approvals, denials, pauses, provider secret changes, MCP activity, governance events, and coder transitions can be written to durable JSONL audit envelopes.
- **Tenant-scoped protected audit:** `/audit/stream` provides an admin-gated newline-delimited JSON feed for approval decisions, tool ledger events, and channel capability changes. Explicit tenant reads fail closed across tenant boundaries, and protected action/tool-effect events carry tenant context.
- **Protected audit ledger and evidence export:** Policy decisions, protected audit records, tool-effect ledger entries, and context-run journals can be tied together for governance evidence export.
- **MCP secret tenant checks:** MCP store secret references validate against tenant context before resolution.
- **Per-task tool and MCP policy:** Automation V2 nodes support step-level built-in tool and MCP connector scoping.
- **Runtime policy decision store:** Governed runtime decisions can be persisted and queried with tenant/run filtering through governance policy-decision surfaces.
- **Initial intra-tenant authority graph:** Direct grants, org-unit membership, role-domain nesting, inherited membership, explicit deny, and fail-closed no-grant decisions are modeled for runtime authorization.
- **Declarative approval gate matrix:** Risk tier and data class map to allow, deny, or approval-required outcomes with reviewer eligibility and approval TTL.
- **Cross-tenant grant contract:** Governed tenant-to-tenant sharing has signed grant envelopes, server/API surfaces, and positive sharing eval coverage; ordinary cross-tenant access remains denied unless an explicit grant applies.
- **Memory retrieval and encryption hardening:** Retrieval gateways govern memory/knowledge egress, memory promotion is policy-visible, local encrypted memory can store encryptable payloads as ciphertext, and hosted KMS mode fails closed until configured.
- **Action Firewall and egress preflight:** Protected actions and agent-team outbound effects can be evaluated before execution/egress.
- **Context hygiene guardrails:** Provider-facing context assembly emits budget telemetry, Full-context mode has fail-closed hard limits, and long-session evals assert provenance instead of only answer text.
- **Runtime artifacts and validation:** Runs can persist artifacts with validation metadata and expose them through runtime/debugging surfaces.
- **Evaluation framework:** The server includes AI failure taxonomy, eval datasets, an eval runner, regression thresholds, and quality-assurance documentation.

## In Progress

- **EnterpriseManager:** Runtime mode handling for `disabled`, `optional`, and `required` enterprise operation.
- **Fail-closed required mode:** Protected paths should block if enterprise is configured as required and the bridge is unavailable.
- **Bridge handshake:** Version negotiation, runtime instance identity, boot nonce, and sidecar capability discovery.
- **Capability negotiation:** Shared capability families for identity resolution, tenant resolution, policy authorization, token introspection, and audit append.
- **Status split:** Public-safe enterprise summary separate from admin-only diagnostics.
- **Tenant propagation audits:** Continued verification that sessions, automations, runs, artifacts, approvals, queues, memory, caches, logs, event streams, and exports are tenant-scoped.
- **Provider ACL sync and hosted identity integration:** Runtime grants currently model Tandem authority; upstream provider ACL sync and hosted identity lifecycle remain private control-plane work.

## Planned

- **Private enterprise sidecar:** Identity, tenancy, policy, audit, and governance implementation outside the OSS engine.
- **OIDC and SSO:** Enterprise identity integration in the private control plane/sidecar layer.
- **SCIM:** User and group provisioning for enterprise directories.
- **SIEM export:** Splunk, Elastic, Datadog, or compatible audit export paths.
- **Self-hosted enterprise license:** Commercial packaging around the public runtime plus private enterprise sidecar.
- **SOC2 and security package:** External audit, security one-pager, threat model, DPA, SLA, and procurement materials.
- **Fleet/control-plane separation:** Longer-term split between a runtime-local sidecar and enterprise admin/control-plane services.

## Current Enterprise Claim

Tandem can honestly claim a serious enterprise authority path today:

> The public runtime already carries the primitives enterprise AI work needs: durable runs, tenant-aware records, scoped tools, governed approval gates, initial policy decisions, intra-tenant authority modeling, explicit cross-tenant grant contracts, artifact validation, retrieval/egress controls, protected audit events, and a sidecar-ready contract. Full enterprise identity, hosted RBAC administration, OIDC, SCIM, SIEM export, private sidecar enforcement, and SOC2 are roadmap items, not shipped guarantees.

Approval gates are runtime control points, not a complete enterprise identity boundary by themselves. For regulated or customer-impacting actions, Tandem should fail closed unless the runtime can verify tenant, policy, approval, proposed-action identity, and capability evidence at the protected tool call. Tandem now has an approval-receipt verifier, a policy decision store, a gate matrix, and protected audit evidence for governed tool calls; enterprise required-mode sidecar enforcement remains roadmap work.

## Fintech Readiness Note

Fintech compliance and risk operations are a strong proof-sprint fit for Tandem because they need cited artifacts, scoped connectors, protected approvals, tenant-aware records, and replayable audit evidence. A credible first demo is a compliance/risk update brief that reads selected sources, drafts a cited artifact, flags limitations, and pauses before any external or customer-impacting action.

This is not a claim that Tandem is production-ready for regulated fintech deployment. `fintech_strict` is an internal runtime profile marker, not mandatory isolation on its own. Autonomous money movement, account freezes, customer approval, regulatory filings, credit decisions, and risk-rating changes require runtime-verified protected approval/policy evidence and stronger enterprise gates. Required enterprise mode, private sidecar enforcement, OIDC, SCIM, SIEM export, hosted RBAC administration, and SOC2 remain in progress or planned as described above.

## Related Docs

- [AI runtime infrastructure](AI_RUNTIME_INFRASTRUCTURE.md)
- [Enterprise proof walkthrough](ENTERPRISE_PROOF_WALKTHROUGH.md)
- [Runtime trust boundaries](RUNTIME_TRUST_BOUNDARIES.md)
- [Context assertion security](CONTEXT_ASSERTION_SECURITY.md)
- [Cross-tenant grants design](CROSS_TENANT_GRANTS_DESIGN.md)
- [Default DataBoundary enforcement design](DATA_BOUNDARY_ENFORCEMENT_DESIGN.md)
- [Memory ciphertext at rest](MEMORY_CIPHERTEXT_AT_REST.md)
- [Engine context assembly map](ENGINE_CONTEXT_ASSEMBLY_MAP.md)
- [Context evals](CONTEXT_EVALS.md)
- [Internal planning notes](internal/)
