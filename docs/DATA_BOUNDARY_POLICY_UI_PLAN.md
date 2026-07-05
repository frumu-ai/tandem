# Data Boundary Policy UI/API Plan (TAN-399)

Planning document for the operator/admin surfaces that sit on top of the
data-boundary engine contracts (TAN-385 through TAN-398). **No UI or write
API is implemented by this document** — the engine contracts stabilized in
Cycles 1–3 and the monitoring read model (TAN-398) shipped first precisely so
UI work can follow without touching enforcement code. This doc defines what
that follow-up work needs, so each piece can be scoped as its own issue.

## Current state (what the UI will sit on)

Policy today is env-only, read at dispatch time and validated at startup
(`EngineConfigReport::from_env`):

| Var | Controls |
| --- | --- |
| `TANDEM_DATA_BOUNDARY_MODE` | `off` (default) / `audit` / `enforce` |
| `TANDEM_DATA_BOUNDARY_STRICT` | fail closed on missing tenant / unknown provider class |
| `TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES` | `provider_id=boundary_class` mappings |
| `TANDEM_DATA_BOUNDARY_REDACT_CLASSES` | sensitive classes redacted before dispatch |
| `TANDEM_DATA_BOUNDARY_APPROVAL_CLASSES` | classes that raise a `data_boundary_egress` ask |
| `TANDEM_DATA_BOUNDARY_BLOCK_CLASSES` | classes blocked outright |
| `TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY` | default posture for raw payloads to external classes |
| `TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES` | oversized-payload guard |

Read surfaces that already exist:

- `GET /audit/data-boundary/monitoring` — tenant-scoped counts by action,
  provider, model, boundary class, sensitive class, source kind, policy
  fingerprint, tenant; payload-hash dedupe (TAN-398).
- `GET /audit/protected?event_type=data_boundary.*` — the raw ledger records.
- Every decision carries a `policy_fingerprint` (hash of the effective
  policy), so any future policy store can correlate decisions with the exact
  policy revision that produced them.

## Read/write API needs

A policy API should be a thin CRUD layer over a persisted policy document
that *compiles to* the same `DataBoundaryPolicy` struct the env vars build
today — enforcement code must not grow a second policy path.

- `GET /data-boundary/policy` — effective policy for the caller's tenant,
  with provenance per field (`env`, `stored`, `default`) mirroring how
  `config` layers report. Include the computed `policy_fingerprint`.
- `PUT /data-boundary/policy` — admin-gated (same `api_token` /
  `control_panel` principal check as `/audit/*`), full-document replace with
  server-side validation identical to startup validation. Writes append a
  `data_boundary.policy_changed` protected-audit record (old/new
  fingerprints only, never inline diffs of secrets-adjacent fields).
- `GET /data-boundary/policy/decisions?fingerprint=...` — convenience filter
  over the monitoring read model to answer "what did this revision do".
- Precedence when both exist: env vars win and the GET response must say so
  (`"overridden_by_env": [...]`) — otherwise operators will edit stored
  policy and see no behavior change.

## Control-panel surfaces

1. **Provider classification table** — one row per registered provider:
   boundary class dropdown (`local`, `customer_hosted`, `approved_external`,
   `unapproved_external`, `prohibited`), classification source badge
   (`env_mapping` / `stored` / `unclassified`), and a warning row for any
   provider left `unclassified` while strict mode is on. This is the surface
   that eventually absorbs endpoint-verified classification
   (docs/DATA_BOUNDARY_ROUTING_CONTRACT.md TODOs).
2. **Class-action matrix** — sensitive class (credential, secret, pii, …) ×
   action (allow raw / redact / tokenize / require approval / block), one
   matrix per provider boundary class. This is the visual form of
   `REDACT_CLASSES` / `APPROVAL_CLASSES` / `BLOCK_CLASSES` /
   `EXTERNAL_RAW_POLICY` and must round-trip to exactly those semantics.
3. **Mode & posture card** — off/audit/enforce toggle, strict toggle, max
   payload size; enforce + strict changes require a typed confirmation since
   they can stop traffic.
4. **Monitoring dashboard** — charts over the TAN-398 read model (blocks,
   redactions, approvals over time; top providers; top sensitive classes;
   repeat payload hashes as a leak-attempt signal). Read-only; links each
   count through to `/audit/protected` records.
5. **Approval queue** — `data_boundary_egress` asks already appear in the
   standard permission surfaces; the panel needs only a filtered view plus
   the class/count/hash evidence rendered honestly (no payload preview —
   there is deliberately nothing to preview).

## Sequencing and separation

UI/API work stays behind these gates, in order:

1. Policy document schema + validation shared with env parsing (no behavior
   change; env-only deployments unaffected).
2. Read API (`GET /data-boundary/policy`) + control-panel read-only views.
3. Write API + audit trail, still env-overridable.
4. Only then: interactive surfaces (matrix editing, approval queue polish).

Enforcement code changes for any of these must be zero except the policy
*loader* (env → env-or-stored).

## Migration / default behavior

- **Local (default)**: mode stays `off`; nothing appears in the panel except
  an explainer card and a "turn on audit mode" affordance. Turning on audit
  is safe (observe-only) and is the intended first step everywhere.
- **Hosted**: audit mode as the recommended default at workspace creation;
  stored policy is per-tenant; env vars remain the operator escape hatch and
  the UI must surface when they override stored policy.
- **Enterprise**: strict + enforce as the provisioning-time recommendation;
  a local-implicit tenant counts as missing tenant context and fails closed
  under strict (TAN-400), so enterprise onboarding must establish real
  tenancy before flipping strict on. Managed-config deployments can pin the
  policy document read-only, making the panel view-only.
- No migration is needed for existing deployments: absent stored policy, the
  compiled policy is exactly today's env-derived one, fingerprint included.

## Follow-up issues to file when this work is picked up

- Policy document schema + shared validation (engine crate).
- Policy read API + provenance reporting (server).
- Policy write API + `data_boundary.policy_changed` audit record (server).
- Control-panel provider classification + class-action matrix (UI).
- Monitoring dashboard over `/audit/data-boundary/monitoring` (UI).
