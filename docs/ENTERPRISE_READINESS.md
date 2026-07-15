# Tandem Enterprise Readiness

Document status: implementation inventory and explicit roadmap boundary.

Implementation review: 2026-07-15 against `origin/main` at `bbcf274d`.
“Available now” means implemented in the reviewed repository. It does not prove
that a particular hosted deployment runs that commit or that an external auditor
has accepted the control.

This document separates what Tandem can credibly show today from what is still in progress or planned. Tandem is not yet a complete enterprise platform with OIDC, SCIM, turnkey SIEM connector integrations, SOC2, and private sidecar enforcement. The current proof is the runtime authority foundation those enterprise features will attach to, including tenant-aware policy decisions, governed approvals, scoped memory, protected audit evidence, and initial intra-tenant authority modeling.

## Available Now

- **Engine-owned run state:** Workflows and automations execute as durable runtime records rather than chat transcripts.
- **Plan preview and apply flow:** Intent can be compiled through plan preview/apply paths into runtime-owned workflow bundles.
- **Tenant context foundations:** The OSS enterprise contract includes tenant context, local implicit tenant defaults, principals, authority chains, and secret references.
- **Public OSS enterprise status contract:** `/enterprise/status` exposes the public status schema, but the current public route is backed by `NoopEnterpriseBridge` and reports the OSS/local-implicit state (`disabled` / `absent`). It is not a live hosted bridge diagnostic.
- **Admin readiness summary:** The enterprise server exposes admin-gated `/enterprise/readiness` checks and onboarding-plan preview for configured organization units, grants, connectors, source bindings, quarantines, and approvals.
- **Strict hosted/enterprise HTTP ingress for non-public routes:** Hosted and `enterprise_required` auth modes require a transport token and signed context assertion on non-public engine/API routes, reject caller-supplied raw tenant headers there, and fail startup configuration checks when verifier keys or the transport token are absent. OAuth callbacks, `POST /webhooks/automations/*`, and `POST /channels/slack/events` intentionally bypass the global transport/context gate so external providers can reach them; those routes depend on their own OAuth-state or webhook-signature controls. This is implemented route-aware ingress enforcement, not universal transport authentication or private-sidecar availability enforcement.
- **Central fail-closed tool dispatch:** Reviewed production server paths route registered native, batch, engine/spawned-agent, global HTTP, server-backed CLI, and bridged MCP calls through one dispatcher. Server contexts deny unmatched tools by default, caller scopes can narrow but not widen authority, and startup rejects an allow-all policy or no-op ledger before the runtime is marked ready. Explicit local/evaluation helpers remain outside this server guarantee.
- **Approval gates and inbox:** Automation runs can pause on human gates, publish deadlines, reject late decisions after expiry, and process reminder or escalation policies while keeping the gate decidable. A missing or zero timeout receives a finite default that cancels by default; resume-on-timeout requires explicit configuration and emits a warning. The control panel includes an Approvals Inbox backed by the pending approvals aggregator.
- **Approval channel fan-out:** Slack, Discord, and Telegram approval delivery exists, including authorization checks, callback deduplication, user capability tiers, rate limits, and lifecycle updates.
- **Required dispatch receipts and durable protected audit:** The central dispatcher writes a correlated policy-decision receipt before returning an allow or denial, then records execution-started and execution-completed/failed phases. Receipt failure blocks the next transition; denial persistence failure remains blocked and returns an explicit error. Consequential external dispatch also reserves a durable pre-send outbox record. Protected events such as approvals, provider secret changes, MCP activity, governance events, and coder transitions use sequence-numbered, hash-chained, fsynced JSONL audit envelopes. These operational receipts are distinct from the separate cryptographically signed approval-receipt contract, and the repository does not establish transactional atomicity between every external/business effect and its final audit append.
- **Tenant-scoped protected audit:** `/audit/stream` provides an admin-gated newline-delimited JSON feed for approval decisions, tool ledger events, and channel capability changes. Explicit tenant reads fail closed across tenant boundaries, and protected action/tool-effect events carry tenant context.
- **Protected audit ledger and evidence export:** `/audit/ledger/manifest` and `/audit/ledger/export` expose a verifiable hash-chain manifest and deterministic NDJSON export. Run-governance evidence packages tie policy decisions, protected audit records, tool calls, approval history, memory audit, artifacts, and context-run journals together. These are generic exports, not turnkey SIEM integrations or WORM retention.
- **MCP secret tenant checks:** MCP store secret references validate against tenant context before resolution.
- **Principal-scoped MCP connections and run-as:** MCP definitions are separated from tenant/principal-owned connections. OAuth sessions bind tenant, principal, provider, and connection identity; interactive and scheduled calls routed through the governed MCP bridge resolve run-as authority and reject wrong-tenant, wrong-actor, missing-authority, and unsupported shared/service use. Connector `allowed_tools` is rechecked immediately before execution, and saved workflow grants pin a connection generation so removal or credential/identity replacement invalidates stale grants. Some coder GitHub Project and Incident Monitor compatibility callers still invoke the MCP registry directly and are outside the bridge run-as, phase-authority, and central dispatch-receipt guarantee. The control panel exposes connection-aware policy selection.
- **Per-task tool and MCP policy:** Automation V2 nodes support step-level built-in tools, MCP connector scoping, explicit connection grants, and service/shared run-as configuration.
- **Runtime policy decision store:** Governed runtime decisions can be persisted and queried with tenant/run filtering through governance policy-decision surfaces.
- **Parameter-aware inherited policy enforcement:** Published `EnterprisePolicyRule` records can apply bounded `permission_predicates/v1` expressions to exact tool arguments before inherited allow, deny, or approval-required resolution. The evaluator supports typed scalar, host/domain, fixed-precision amount, path, repository, existence, and array-length conditions with three-valued fail-closed behavior. Runtime decisions retain the effective policy snapshot and write protected enforcement evidence.
- **Tenant-scoped no-code policy authoring:** Enterprise admins can use the Control Panel Policy Studio and enterprise APIs to create and edit drafts, validate field-level errors, preview the effective inherited winner, publish, disable, and supersede tenant-scoped policies. Mutation persistence and protected audit writes are required together, with rollback on audit failure.
- **Versioned starter policies:** The enterprise contract, APIs, and Policy Studio ship CRM, finance, and coding starter templates. Deployments can instantiate a version, preview the effective result, apply only declared override fields, upgrade, and roll back while non-overridable denies remain authoritative.
- **Initial intra-tenant authority graph:** Direct grants, org-unit membership, role-domain nesting, inherited membership, explicit deny, and fail-closed no-grant decisions are modeled for runtime authorization.
- **Declarative approval gate matrix:** Risk tier and data class map to allow, deny, or approval-required outcomes with reviewer eligibility and approval TTL.
- **Cross-tenant grant contract:** Governed tenant-to-tenant sharing has signed grant envelopes, server/API surfaces, and positive sharing eval coverage; ordinary cross-tenant access remains denied unless an explicit grant applies.
- **Memory retrieval and encryption hardening:** Retrieval gateways govern memory/knowledge egress, memory promotion is policy-visible, local encrypted memory can store encryptable payloads as ciphertext, hosted KMS mode fails closed until configured, and hosted/enterprise or verifier-key-configured runtimes activate strict tenant memory/context policy before prompt assembly.
- **PostgreSQL storage options:** Enterprise builds can use PostgreSQL/pgvector for the memory store and PostgreSQL for the stateful orchestration storage backend. SQLite remains the local default. Backend availability is not equivalent to a managed migration or hosted operations guarantee.
- **Enterprise ingestion control plane:** Google Drive provides the implemented reference path for tenant-scoped connector credentials, source bindings, read-only ingestion, admission policy, high-risk quarantine/review, and source-object lifecycle tracking. Notion, GitHub, Slack, and Gmail enterprise ingestion remain plans.
- **Action Firewall and egress preflight:** Protected actions and agent-team outbound effects are evaluated before execution/egress on governed paths. Egress-DLP approvals bind the exact action and scope, are atomically consumed before dispatch, and cannot be reused after success, expiry, denial, mismatch, or concurrent replay.
- **Context hygiene guardrails:** Provider-facing context assembly emits budget telemetry, Full-context mode has fail-closed hard limits, and long-session evals assert provenance instead of only answer text.
- **Runtime artifacts and validation:** Runs can persist artifacts with validation metadata and expose them through runtime/debugging surfaces.
- **Evaluation framework:** The server includes AI failure taxonomy, eval datasets, an eval runner, regression thresholds, and quality-assurance documentation.

## In Progress

- **Dynamic EnterpriseManager:** A runtime manager that reports and coordinates `disabled`, `optional`, and `required` private-bridge operation is not implemented. The current public status route always reports the no-op OSS bridge.
- **Private-bridge fail-closed required mode:** Non-public engine/API routes already fail closed on missing transport/context authentication in strict modes. Deliberately public OAuth callback and webhook routes use separate route-specific controls, and blocking protected paths specifically because a required private enterprise bridge is unavailable remains unimplemented.
- **Bridge handshake:** Version negotiation, runtime instance identity, boot nonce, and sidecar capability discovery.
- **Capability negotiation:** Shared capability families for identity resolution, tenant resolution, policy authorization, token introspection, and audit append.
- **Status split and live bridge diagnostics:** Public no-op status and admin readiness routes exist, but the public endpoint is not yet backed by a dynamic bridge manager and there is no complete admin bridge-health diagnostic.
- **Tenant propagation audits:** Continued verification that sessions, automations, runs, artifacts, approvals, queues, memory, caches, logs, event streams, and exports are tenant-scoped.
- **Provider ACL sync and hosted identity integration:** Runtime grants and admin-labeled fallback currently model Tandem authority; no provider has proven synchronized per-object ACLs. Hosted identity lifecycle remains private control-plane work.
- **Hosted MCP and connector secret custody:** Runtime reference types and tenant checks exist, while universal control-plane OAuth custody and KMS/secret-manager resolution remain incomplete.
- **Enterprise/global policy-authoring edge cases:** Tenant-scoped authoring is available, but the current create route always stamps the request tenant and therefore cannot create a tenantless global rule. The supersede target lookup also uses raw tenant-ID equality instead of the canonical matcher used by runtime enforcement and the other authoring routes. TAN-744 and TAN-745 track these gaps.
- **Complete predicate dispatcher and receipt contract:** The current runtime can enforce authored allow, deny, and approval-required rules through its action-gate adapter, but the central dispatcher still exposes only allowed/denied outcomes, and predicate decision metadata carries a redacted marker rather than the RFC's structured deployment-HMAC evidence. TAN-746 and TAN-747 track first-class approval-required dispatch and privacy-preserving condition evidence.

## Planned

- **Private enterprise sidecar:** Identity, tenancy, policy, audit, and governance implementation outside the OSS engine.
- **OIDC and SSO:** Enterprise identity integration in the private control plane/sidecar layer.
- **SCIM:** User and group provisioning for enterprise directories.
- **Turnkey SIEM integrations:** Splunk, Elastic, Datadog, or compatible managed audit export paths with connector-level retry/backpressure behavior.
- **Self-hosted enterprise license:** Commercial packaging around the public runtime plus private enterprise sidecar.
- **SOC2 and security package:** External audit, security one-pager, threat model, DPA, SLA, and procurement materials.
- **Fleet/control-plane separation:** Longer-term split between a runtime-local sidecar and enterprise admin/control-plane services.

## Current Enterprise Claim

Tandem can honestly claim a serious enterprise authority path today:

> The reviewed runtime carries substantial enterprise authority primitives: durable stateful runs, tenant-aware records, centrally dispatched and receipted server tools, scoped and principal-bound MCP connections, governed approval gates, tenant-scoped parameter-aware policy authoring, versioned starter policies, intra-tenant authority modeling, explicit cross-tenant grant contracts, artifact validation, retrieval/egress controls, hash-chained protected audit records, verifiable generic evidence exports, PostgreSQL storage options, and a reference enterprise ingestion path. These are repository capabilities, not proof of hosted deployment, non-bypassability, external control acceptance, or operational maturity. Tenantless global-policy creation, normalization-safe supersession, first-class approval-required dispatcher outcomes, and the RFC's structured predicate evidence remain open. Full enterprise identity, hosted RBAC administration, OIDC, SCIM, turnkey SIEM integrations, private-sidecar enforcement, managed KMS custody across every signing/connector lane, and SOC2 remain roadmap items or unverified operational claims.

Approval gates are runtime control points, not a complete enterprise identity boundary by themselves. For regulated or customer-impacting actions, Tandem should fail closed unless the runtime can verify tenant, policy, approval, proposed-action identity, and capability evidence at the protected tool call. Every reviewed production server dispatch now requires correlated durable policy and execution receipts, and the egress-DLP path atomically consumes an exact-action approval once. Tandem also has a cryptographic approval-receipt verifier contract and a fintech path that matches approved gate history to tenant, tool, exact action hash, and expiry. These are different receipt layers: the repository does not establish that every protected tool consumes a cryptographically signed approval receipt. Universal signed-receipt consumption and enterprise required-mode private-sidecar enforcement remain open.

## Fintech Readiness Note

Fintech compliance and risk operations are a strong proof-sprint fit for Tandem because they need cited artifacts, scoped connectors, protected approvals, tenant-aware records, and replayable audit evidence. A credible first demo is a compliance/risk update brief that reads selected sources, drafts a cited artifact, flags limitations, and pauses before any external or customer-impacting action.

This is not a claim that Tandem is production-ready for regulated fintech deployment. `fintech_strict` is an internal runtime profile marker, not mandatory isolation on its own. Autonomous money movement, account freezes, customer approval, regulatory filings, credit decisions, and risk-rating changes require runtime-verified protected approval/policy evidence and stronger enterprise gates. Required enterprise mode, private sidecar enforcement, OIDC, SCIM, turnkey SIEM integrations, hosted RBAC administration, and SOC2 remain in progress or planned as described above.

## Related Docs

- [AI runtime infrastructure](AI_RUNTIME_INFRASTRUCTURE.md)
- [Enterprise proof walkthrough](ENTERPRISE_PROOF_WALKTHROUGH.md)
- [Runtime trust boundaries](RUNTIME_TRUST_BOUNDARIES.md)
- [Stateful runtime enterprise threat model](STATEFUL_RUNTIME_ENTERPRISE_THREAT_MODEL.md)
- [Context assertion security](CONTEXT_ASSERTION_SECURITY.md)
- [Cross-tenant grants design](CROSS_TENANT_GRANTS_DESIGN.md)
- [Parameter-aware permission predicate RFC](rfcs/parameter-aware-permission-predicates.md)
- [Default DataBoundary enforcement design](DATA_BOUNDARY_ENFORCEMENT_DESIGN.md)
- [Memory ciphertext at rest](MEMORY_CIPHERTEXT_AT_REST.md)
- [Engine context assembly map](ENGINE_CONTEXT_ASSEMBLY_MAP.md)
- [Context evals](CONTEXT_EVALS.md)
- [Internal planning notes](internal/)
