---
title: Enterprise Data Governance
description: Configure enterprise MCP governance, org units, source bindings, Google Drive ingestion, quarantine review, and connector incident response.
---

Tandem's enterprise governance surface lets hosted admins decide which company
data may become memory before the model can retrieve it. The model is not the
access-control boundary; the runtime stamps ingested data with tenant,
resource, data-class, source-object, and connector metadata, then filters
source-bound memory before ranking, prompt assembly, citations, and cache reuse.

Agent search terms: enterprise MCP governance, enterprise setup, tenant scoped
memory, source bindings, connector credentials, org units, data classes,
quarantine review, Google Drive import, connector incident response, governed
MCP setup.

This page covers the current operator workflow in the control panel. The
enterprise admin route is intentionally separate from ordinary chat and
automation setup. For ordered client rollout steps, use the
[Enterprise Client Onboarding Runbook](./enterprise-client-onboarding-runbook/).

## Hosted control-plane boundary

The public Tandem runtime starts after hosted provisioning has established the
customer tenant, workspace, and verified principal context. The private hosted
control plane owns account provisioning, workspace creation, user invites,
SSO/OIDC, SCIM, billing, and hosted end-user lifecycle. Public runtime docs
should not imply public invite or user-provisioning endpoints.

## What agents can help set up

Agents are useful enterprise setup assistants when they operate through Tandem's
governance surfaces and leave authority with the operator. They can help:

- translate departments, groups, service accounts, and agent workers into
  proposed org units
- draft resource and data-class grants for admin review
- propose connector lifecycle records and source bindings before ingestion
- prepare read-only credential-reference checklists without handling raw secrets
- inspect MCP connector availability with `mcp_list` and `mcp_list_catalog`
- request missing capabilities through the approval queue instead of connecting
  tools themselves
- generate narrow automation policies that separate research, drafting,
  approval, and post-approval external actions
- write incident-response runbooks for connector revoke, rotation, quarantine,
  re-scope, and cache invalidation workflows

Agents should not self-grant enterprise roles, paste credential material into
records, bypass quarantine review, or treat an MCP catalog entry as permission
to execute tools. For connector execution policy and tool allowlists, pair this
page with [MCP Automated Agents](./mcp-automated-agents/) and
[MCP Capability Discovery And Request Flow](./mcp-capability-discovery-and-request-flow/).

## Open the enterprise admin page

Start the control panel, then open:

```text
http://127.0.0.1:39732/#/enterprise-admin
```

The page shows the active tenant, request principal, and enterprise bridge
state at the top. In hosted or enterprise modes, mutations require a verified
admin-style role such as `admin`, `owner`, `reconfigure`, or
`enterprise:admin`. Local/default installs can still use the local operator
token for development.

## Governance model

Enterprise data enters memory through a source binding:

1. Create organization units that match the customer's taxonomy.
2. Assign users, groups, agents, or service accounts to those units.
3. Grant an org unit access to a resource and data class.
4. Create a connector lifecycle record.
5. Attach secret-reference-only credentials to the connector.
6. Create a source binding from an external source root to a Tandem resource.
7. Import or reindex data through the binding.
8. Review quarantined output before it becomes searchable when review is
   required.

Each source-bound chunk is filtered by tenant, `ResourceRef`, and `DataClass`
before retrieval ranking. Principals without a matching strict `Read` grant do
not see the chunk, its source-object identifiers, or citation labels.

For agents, the setup order matters. Discover or request the connector first,
then draft source bindings and grants, then run preflight checks, and only then
import or reindex data. If an MCP or enterprise connector is cataloged but not
connected, the safe next step is an operator request, not a fallback connector
or direct `mcp_debug` call.

## Organization units

Use **Create org unit** to define customer-specific business domains. These are
generic taxonomy entries, not hardcoded Tandem roles.

Common examples include:

- `hr`
- `finance`
- `legal`
- `platform_oncall`
- `board_members`
- `claims_adjusters`

Then use **Org-unit membership** to bind a member to a unit. Members can be
humans, groups, departments, agent workers, or service accounts. Memberships
can come from direct admin entry now and later from hosted control-plane,
SCIM, Google Workspace, or Okta sync.

Use **Org-unit access grants** to grant `view`, `read`, `edit`, `execute`,
`delegate`, or `admin` over a resource and data class. Executive or
all-company access should be represented as an explicit high-scope grant, not
as an implicit bypass.

## Connectors and credentials

Use **Create connector** for the connector lifecycle gate. Supported states are:

- `active`
- `paused`
- `revoked`
- `quarantined`

Paused, revoked, or quarantined connectors cannot ingest. State changes also
invalidate source-bound response-cache entries for affected scopes.

Credential management uses secret references only. Use **Credential refs** to
attach or rotate metadata such as:

- credential id
- secret provider
- secret id
- credential class, usually `read_only`
- source-bound resource and data class

Do not enter raw credential material into connector records. The runtime rejects
raw Google Drive write/admin credential refs for the constrained Drive import
flow.

When an agent is helping, it should produce the credential-reference metadata
that an admin needs to fill in and should explicitly mark unknown secret IDs as
operator-supplied. It should not ask the model to remember, transform, or echo
the underlying token.

## Source bindings

Use **Create source binding** to bind an external source root to a Tandem
resource and data class. A binding includes:

- connector id
- binding id
- source root id and label
- resource kind and resource id
- data class
- lifecycle state: `enabled`, `disabled`, or `quarantined`
- ingestion policy, including review requirement

Hosted memory imports require a source binding so uploaded or connector data is
scoped before indexing. Local/default imports can remain unbound for legacy
behavior, or use the generated `local_manual_upload` binding to stamp local
manual uploads with source-object lifecycle metadata.

## Google Drive import

Google Drive is the first constrained external connector. The current flow is
admin-triggered and read-only:

1. Create an active Google Drive connector.
2. Attach a source-bound, read-only credential reference.
3. Create an enabled source binding for the Drive folder/root.
4. Select the binding in the Enterprise admin page.
5. Run **Preflight** to verify the connector, binding, credential reference,
   resolver-backed folder listing, and source scope.
6. Run **Import** to fetch supported Drive documents into the binding namespace.
7. Run **Reindex** to re-fetch the binding or a specific source object.

The import writes ingestion-job audit records, source-object lifecycle rows,
and source-bound memory chunks. If the binding requires review, newly indexed
chunks are purged before the import returns and a quarantine record is created.

OAuth ownership, provider ACL sync, background scheduling, and additional
providers are follow-up work. For now, source roots are admin-labeled and
Tandem grants are the authority that decides who can retrieve the indexed
content.

## Quarantine review

The **Quarantine** panel lists review-required ingestion output. Admins can:

- `release` output after review
- `delete` quarantined output
- `reindex` quarantined output

Quarantined output is not searchable until released or reindexed through an
approved path. Review actions update ingestion-job/source-object views and
invalidate affected source-bound cache entries.

## Source-object lifecycle

The **Source objects** panel shows indexed or quarantined objects for the
selected binding. Admins can:

- request reindex
- hard-delete indexed content and the lifecycle row
- re-scope an object to a different resource or data class

Re-scope purges old indexed chunks before moving lifecycle metadata so stale
resource grants cannot keep retrieving prior prompt context.

## Connector impact

Use **Connector impact** when responding to a revoke, rotation, quarantine, or
suspected compromise. The report summarizes:

- affected source bindings
- affected source objects
- ingestion jobs
- quarantine records
- compromise-window timing
- whether cache invalidation is needed
- recommended response actions

The current workflow makes the affected scope visible for review. Automatic
bulk destructive remediation should remain an explicit, reviewed follow-up.

## API reference

The control panel proxies these engine endpoints:

- `GET /enterprise/connector-providers`
- `GET /enterprise/org-units`
- `POST /enterprise/org-units`
- `GET /enterprise/org-unit-memberships`
- `POST /enterprise/org-unit-memberships`
- `PATCH /enterprise/org-unit-memberships/{membership_id}`
- `GET /enterprise/org-unit-access-grants`
- `POST /enterprise/org-unit-access-grants`
- `GET /enterprise/org-unit-access-grants/effective`
- `PATCH /enterprise/org-unit-access-grants/{grant_id}`
- `GET /enterprise/connectors`
- `POST /enterprise/connectors`
- `PATCH /enterprise/connectors/{connector_id}`
- `GET /enterprise/connectors/{connector_id}/impact`
- `POST /enterprise/connectors/{connector_id}/credential-refs`
- `PATCH /enterprise/connectors/{connector_id}/credential-refs/{credential_id}/rotate`
- `GET /enterprise/source-bindings`
- `POST /enterprise/source-bindings`
- `PATCH /enterprise/source-bindings/{binding_id}`
- `POST /enterprise/source-bindings/{binding_id}/google-drive/preflight`
- `POST /enterprise/source-bindings/{binding_id}/google-drive/import`
- `POST /enterprise/source-bindings/{binding_id}/google-drive/reindex`
- `GET /enterprise/source-bindings/{binding_id}/source-objects`
- `POST /enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}/reindex`
- `DELETE /enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}`
- `PATCH /enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}/scope`
- `GET /enterprise/ingestion-jobs`
- `GET /enterprise/ingestion-quarantines`
- `PATCH /enterprise/ingestion-quarantines/{quarantine_id}/review`
