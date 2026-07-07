# Department-Scoped Slack Agent — Requester Profile Seed (TAN-653)

Defines the five demo requester profiles as org-units + memberships + access
grants, keyed by Slack user id, so the same ACME question yields divergent
reachable memory/tools per department. Pinned to the real enterprise-contract
schema; the JSON authoring + loader wiring is the implementation slice.

## Schema (confirmed, `crates/tandem-enterprise-contract/src/lib.rs`)

- `OrganizationUnit` (`:621`): `unit_id`, `tenant_context`, `taxonomy_id`,
  `display_name`, `kind: OrganizationUnitKind`, `parent_unit_id?`, `state`,
  `labels`, `created_by: PrincipalRef`, `created_at_ms`, `updated_at_ms`.
- `OrganizationUnitMembership` (`:714`): `membership_id`, `tenant_context`,
  `unit: PrincipalRef`, `member: PrincipalRef`, `source`, `state`,
  `created_at_ms`, `expires_at_ms?`.
- `OrganizationUnitAccessGrant` (`:764`): `grant_id`, `tenant_context`,
  `unit: PrincipalRef`, `resource: ResourceRef`, `effect: AccessEffect`
  (`allow`/`deny`), `permissions: Vec<AccessPermission>`,
  `data_classes: Vec<DataClass>`, `tool_patterns: Vec<String>`, `state`,
  timestamps.
- `OrganizationUnitKind` (`:593`): `department`, `team`, `role_domain`,
  `contractor_group`, `executive_group`, `clinical_group`, `operational_group`,
  `custom`, `unspecified`.

Grant evaluation is `StrictTenantContext::evaluate_access(resource, permission,
data_class, now)` (`lib.rs:1528`): context-expiry → data-boundary → explicit
deny → deny grants → scope containment → allow grants → else `not_applicable`
(fail-safe: no allow).

Demo tenant: `org_id = "acme"`, `workspace_id = "hq"` (single workspace).

## The five profiles

| Slack user | Org-unit (`unit_id`) | `kind` | Membership |
|---|---|---|---|
| `U_SALES` | `sales` | `department` | member of `sales` |
| `U_ENG` | `engineering` | `department` | member of `engineering` |
| `U_FINANCE` | `finance` | `department` | member of `finance` |
| `U_LEADER` | `leadership` | `executive_group` | member of `leadership` (+ read-across) |
| `U_CONTRACTOR` | `contractor_acme_x` | `contractor_group` | member of `contractor_acme_x` only |

Principal for a Slack user: `PrincipalRef { kind: human_user, id:
"channel:slack:{user}", … }` (matches `build_principal`, TAN-652). The unit
principal: `PrincipalRef { kind: organization_unit | department, id: "{unit_id}" }`.

## Per-profile grants (resource + data-class intent)

Resources are ACME-scoped `ResourceRef`s under the demo tenant. Data classes use
the real `DataClass` set (`Internal`, `Confidential`, `Restricted`, `Credential`,
`FinancialRecord`, `Executive`, `Public`). Redaction of a class = a data-boundary
`Redact`/`Tokenize` action, not an `allow`.

- **Sales** — **allow** `acme/crm/*`, `acme/support/summaries/*`,
  `acme/risk/customer` at `Internal`/`Confidential`. **Deny** `FinancialRecord`
  (contract value, payment status) and raw repo.
- **Engineering** — **allow** `acme/github/*`, `acme/linear/*`,
  `acme/incidents/*` at `Internal`. **Deny** `FinancialRecord` (contract/payment)
  — explicit `deny` grant so it beats any inherited allow.
- **Finance** — **allow** `acme/invoices/*`, `acme/payments/*`,
  `acme/contracts/*` at `FinancialRecord`. **Deny** raw `acme/github/*`.
- **Leadership** — **allow** cross-functional read (`acme/*`) at
  `Internal`/`Confidential`, **but** `FinancialRecord` + `Credential` classes are
  **redacted** by the data boundary (summary, not raw). Modeled as broad allow +
  data-boundary redaction rules, not a raw-financial allow.
- **Contractor** — **allow** only `acme/projects/x/*` (assigned project). ACME
  customer/CRM/finance resources are **out of scope** → `evaluate_access` returns
  `not_applicable` → denied response.

## Per-profile tool grants (`tool_patterns` + risk tiers, TAN-655)

| Profile | Allowed tools | Notes |
|---|---|---|
| Sales | `mcp.crm.*` (read), `mcp.support.*` (read) | — |
| Engineering | `mcp.github.*`, `mcp.linear.*`, `mcp.incidents.*` (read) | no finance tools |
| Finance | `mcp.invoices.*`, `mcp.contracts.*` (read) | `FinancialRecordAccess` tier |
| Leadership | read-across of the above | financial detail redacted in output |
| Contractor | `mcp.projects.x.*` only | — |
| (any, gated) | `mcp.email.send` | `ExternalSend` tier → approval-gated |

## Implementation slice (remaining)

1. Author `org_units` / `org_unit_memberships` / `org_unit_access_grants` JSON in
   the on-disk format `EnterpriseState` loads (`enterprise_state.rs:38-43`) —
   confirm map-vs-array + the exact `ResourceRef` / `AccessPermission` /
   `DataClass` serde reps before writing, to avoid invalid fixtures.
2. A Slack-user → `unit_id` map (config) consumed by the TAN-652 resolver.
3. A per-profile `evaluate_access` test: assert each profile's allow/deny matrix
   for the ACME resources + tools above (positive + denial).

Pairs with **TAN-652** (ingress → verified context) and **TAN-655** (ACME dataset
+ tagged tools).
