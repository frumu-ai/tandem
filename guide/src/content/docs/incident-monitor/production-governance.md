---
title: Incident Monitor Production Governance
description: Map Incident Monitor evidence, deployment cards, posture checks, and destination routing to production governance controls.
---

Use this page when an operator, auditor, or agent needs to answer whether an Incident Monitor deployment is ready for production governance review.

Incident Monitor is not a compliance certification engine. It records what Tandem observed, which authority surfaces exist, which checks ran, which route or approval decision was made, and where evidence was exported. A deployer still owns the policy mapping, retention schedule, reviewer assignments, incident reporting obligations, and external system of record.

## Production Governance Map

| Governance stage                   | Tandem surface                                                                                                      | Evidence artifact                                                                                                                                     | Operator-owned decision                                                                                         |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| Intended purpose and ownership     | Deployment cards for agents, workflows, sources, and Tandem self-monitoring                                         | JSON/Markdown deployment cards with purpose, owner, accountable team, approval protocol, escalation protocol, data classification, and review cadence | Confirm the purpose is allowed, name accountable owners, and set review cadence                                 |
| Data readiness and source lineage  | Monitored source identity, route tags, source bindings, scoped intake keys, readiness status                        | Source/project IDs, event schema version, tenant/workspace context, allowed/default destination IDs, redaction and retention profiles                 | Decide which sources may report, which data classes are allowed, and which sources require quarantine or review |
| Runtime authority inventory        | `GET /incident-monitor/security/authority-inventory`                                                                | Read-only inventory of workflows, agents, tool/MCP policy, destinations, routes, approvals, policy decisions, and recent external publish surfaces    | Decide whether the observed authority matches business intent                                                   |
| Deterministic posture review       | `GET /incident-monitor/security/posture-checks`                                                                     | Findings with severity, affected authority surface, evidence refs, mitigation guidance, and draft conversion payloads                                 | Map findings to customer policy and assign owners                                                               |
| Controlled governance checks       | `POST /incident-monitor/security/assessment-probes`                                                                 | Dry-run probe evidence for approval gates, scoped intake limits, route readiness, MCP allowlists, and webhook URL policy                              | Authorize probes, define sandbox boundaries, and decide which failures block production                         |
| Route and destination control      | Route preview, destination readiness, approval policy, publish receipts                                             | Matched routes, effective destination IDs, blocked reasons, receipt status, external URL/ID, evidence digest                                          | Decide which incident classes can reach GitHub, Linear, webhook, telemetry, memory, or MCP destinations         |
| Assessment and compliance packet   | `POST /incident-monitor/security/assessment-report`                                                                 | Redacted JSON/Markdown report with inventory, findings, probes, incidents, receipts, protected audit summaries, and export preview                    | Decide who reviews the packet and where it is retained                                                          |
| External evidence custody          | Protected audit stream or ledger export plus customer-owned destination                                             | Audit export summaries, ledger/export manifests, route receipts, and destination receipts                                                             | Configure retention, access control, SIEM/object-store/database custody, and legal hold policy                  |
| Incident response and drift review | Incident Monitor incidents, route failures, approval waits, deployment-card review dates, repeated posture findings | Open incidents, failed publish receipts, approval denials, stale cards, recurring findings, and follow-up issues                                      | Define escalation paths, incident response timing, and drift review thresholds                                  |

## What Tandem Can Prove

Tandem can provide evidence that:

- a source, workflow, agent, destination, route, approval, policy decision, or publish receipt existed in Tandem's governed runtime
- an assessment report, posture check, or controlled probe was generated from a redacted authority inventory
- a publish attempt used the destination router rather than a direct external mutation path
- scoped intake credentials were report-only and rejected privileged routes when checked
- a deployment card had or lacked required owner, purpose, escalation, approval, data-classification, and review metadata
- protected audit evidence was available for customer-owned review or export

## What Tandem Cannot Prove Alone

Tandem does not independently prove that:

- every customer system, credential, shadow agent, or external integration is connected to Tandem
- the customer's policy mapping is complete or legally sufficient
- a destination such as GitHub, Linear, SIEM, object storage, or a webhook receiver retained evidence correctly after export
- third-party systems are free of vulnerabilities
- all model-level failure modes, prompt-injection attacks, or data-quality issues were detected
- Tandem is safe merely because Tandem monitored itself

Use Tandem evidence as runtime governance input, then reconcile it with customer-owned identity, access, data, compliance, and incident-response controls.

## Production Readiness Checklist

Before using Incident Monitor for production governance:

1. Generate deployment cards for Tandem self-monitoring, monitored sources, high-authority agents, and workflows that can mutate external systems.
2. Fill required owner, accountable team, intended purpose, autonomy level, data classification, approval protocol, escalation protocol, and review cadence metadata.
3. Run authority inventory and deterministic posture checks with full admin context.
4. Run only authorized dry-run probes, and record sandbox boundaries for each probe.
5. Preview routes for representative high-risk, external, legal/regulatory, and low-risk incidents.
6. Confirm destination readiness for every configured publish target.
7. Confirm high-risk or sensitive destinations require approval and redaction.
8. Configure retention/export policy for reports, receipts, protected audit evidence, and customer-owned records.
9. Assign finding owners and escalation paths before enabling external publish routes.
10. Schedule periodic drift review for stale deployment cards, repeated failures, approval waits, and route changes.

## Compliance Mapping Notes

| Compliance question                         | Useful Tandem evidence                                                                                    | Still deployer-owned                                                                          |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| Who is accountable for this agent or route? | Deployment card owner/accountable team fields and required-field findings                                 | Organizational assignment and reviewer qualification                                          |
| What data can enter this monitor?           | Source identity, scoped intake keys, source bindings, data classification metadata, redaction profiles    | Data-class policy, minimization, lawful basis, and source-system authorization                |
| Can the agent mutate external systems?      | Destination inventory, route preview, MCP allowlists, approval policy, publish receipts                   | Business approval policy and connected-system authorization                                   |
| Was a risky action reviewed?                | Approval coverage, approval decisions, protected audit summaries, assessment report evidence refs         | Reviewer competence, dual-control policy, and legal/regulatory escalation                     |
| Where is evidence retained?                 | Report artifacts, protected audit export summaries, post receipts, delivery IDs, evidence digests         | SIEM/object-store/database custody, retention period, access review, legal hold               |
| How are incidents escalated?                | Incident records, failed receipts, posture findings, route suggestions, deployment-card escalation fields | Incident response playbooks, reporting thresholds, customer notification, regulator reporting |

## Edge Cases To Call Out

- If a source has no allowed/default destination policy, route preview can reveal surprising fallback behavior before publish.
- If `retention_days` is unset, Tandem can still record receipts and reports, but production use needs customer-owned retention policy.
- If a generic MCP destination is enabled, treat it as high risk until the server/tool allowlist, payload mapping, approval policy, and receipt behavior are reviewed.
- If Tandem monitors itself, export evidence outside Tandem before relying on it for audit or incident response.
- If a posture finding depends on customer policy that Tandem cannot infer, keep it as an open question rather than silently passing the deployment.

## Related

- [Incident Monitor Agent Runtime Guide](./agent-runtime-guide/)
- [Incident Monitor Security Posture Mode](./security-posture/)
- [Incident Monitor Setup Checklist](./setup-checklist/)
- [Incident Monitor Reference](../reference/incident-monitor/)
- [Governance Reference](../reference/governance/)
