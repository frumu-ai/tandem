# Enterprise MCP Identity And Delegation

Design owner: TAN-348

This document defines the enterprise MCP identity model for multi-employee
Tandem instances. It turns the current single global MCP server registry into a
model where server definitions, user-owned connections, service-principal
connections, delegated execution, and audit identity are separate concepts.

## Current State

The existing generic MCP path is safe enough for local single-user use, and it
has useful tenant checks for store-backed secret headers, but it is not a
complete enterprise account model.

- `McpRegistry` stores servers in a `HashMap<String, McpServer>` keyed by server
  name.
- `McpServer` mixes provider definition, connection state, secret refs, auth
  challenge state, OAuth metadata, session id, allowed tools, and tool cache.
- MCP OAuth sessions are tied to server name and OAuth state, not tenant,
  actor, service principal, or connection id.
- Tool execution can carry `TenantContext`, but connect, refresh, readiness,
  OAuth refresh, and discovery paths still have tenantless/global behavior.
- The control panel presents MCP as a global server list with global
  connect/auth/delete actions.

The enterprise requirement is stricter: when Tandem performs an MCP tool call,
the call must act as a known principal and use only the credential that principal
is allowed to use.

## Design Goals

- Let multiple employees connect the same MCP provider in one Tandem workspace.
- Let admins create shared or service-principal connections without making them
  implicit global credentials.
- Make every enterprise MCP tool call resolve an acting principal before any
  external effect.
- Keep hosted OAuth and long-lived secret material outside the runtime, following
  the connector ownership precedent in
  `docs/ENTERPRISE_CONNECTOR_CONTROL_PLANE_DECISIONS.md`.
- Preserve a local single-user compatibility mode for existing desktop and local
  engine installs.
- Emit enough audit evidence to answer: who initiated the run, who the tool call
  acted as, which connection credential was used, and which upstream account was
  reached.

## First-Party Chat Authority Boundary

Control Panel chat is a first-party Tandem surface, not an external MCP client.
The runtime already knows the signed-in human through the verified session
context, so first-party product tools must execute with that delegated identity.
They must never ask the user to paste a Tandem API key into chat or expose the
runtime transport token to the model.

| Caller and target | Authentication boundary | Credential visible to the model |
| --- | --- | --- |
| Control Panel chat to Tandem product tools | Existing browser/desktop session plus `VerifiedTenantContext` | None |
| External agent to the Tandem API | Supported API token or OAuth entry credential plus hosted context assertion where required | Never; the agent host owns transport authentication |
| Tandem to a third-party MCP/service | Principal-scoped connection or governed service credential | Only an opaque connection reference |
| Tandem Docs MCP | Public read-only endpoint; catalog contract is `requires_auth = false` | None |

The desktop sidecar may use a generated local transport token internally. That
is an implementation detail of the trusted desktop boundary, not a credential
the user should configure for chat authoring.

First-party tool dispatch must:

- derive actor and tenant from server-injected verified context;
- ignore caller-supplied actor or creator fields for authenticated mutations;
- require a verified principal outside local implicit mode;
- keep raw cookies, API tokens, OAuth tokens, authorization headers, and secret
  values out of prompts, tool arguments, results, logs, and audit payloads;
- use the local `local-operator` identity only in local implicit mode;
- keep external MCP selection and credentials separate from first-party Tandem
  capability routing.

If the hosted Tandem Docs MCP deployment requests authentication, the deployment
is out of contract with the bundled catalog and must be repaired rather than
worked around by prompting an in-product user for an API key.

## Core Model

### McpServerDefinition

An MCP server definition is the provider or endpoint shape. It is not a
credential and does not imply that anyone is connected.

Suggested fields:

```rust
pub struct McpServerDefinition {
    pub server_id: String,
    pub display_name: String,
    pub transport: String,
    pub auth_kind: String,
    pub purpose: String,
    pub grounding_required: bool,
    pub allowed_tools_policy: Option<Vec<String>>,
    pub catalog_slug: Option<String>,
}
```

`server_id` replaces global server name as the stable internal id. Display names
may remain user-editable, but policy and connection records should use ids.

### McpConnection

An MCP connection is a principal-scoped account/credential binding for one
server definition.

Suggested fields:

```rust
pub struct McpConnection {
    pub connection_id: String,
    pub server_id: String,
    pub tenant: TenantScope,
    pub owner: McpPrincipalRef,
    pub credential_ref: Option<McpCredentialRef>,
    pub upstream_account: Option<McpUpstreamAccount>,
    pub connection_class: McpConnectionClass,
    pub enabled: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}
```

`McpConnection` owns authenticated runtime state. Connection status,
MCP-session id, authenticated tool cache, pending auth challenge, and OAuth
refresh metadata should move here or into state keyed by `connection_id`.

### McpPrincipalRef

Supported acting principals:

```rust
pub enum McpPrincipalRef {
    HumanActor { actor_id: String },
    ServicePrincipal { principal_id: String },
    AutomationPrincipal { automation_id: String },
    SharedConnection { grant_id: String },
}
```

Human-owned connections are usable by that actor unless an admin policy further
restricts them. Service and shared connections require explicit grants.
Automation principals must be backed by an approved service principal or a
specific delegated connection grant; an automation id alone is not authority.

### McpConnectionClass

```rust
pub enum McpConnectionClass {
    UserOwned,
    ServiceAccount,
    SharedReadOnly,
    SharedReadWrite,
    AdminManaged,
}
```

The class bounds what policy may permit. For example, a user-owned connection
cannot silently become a shared workflow credential; a shared read-only
connection cannot execute write tools even if a server exposes them.

### McpCredentialRef

Hosted and enterprise deployments should not persist raw OAuth tokens in the
runtime. They should store a reference resolved through the control plane or a
customer-managed secret resolver.

```rust
pub struct McpCredentialRef {
    pub provider: String,
    pub secret_id: String,
    pub credential_version: Option<String>,
    pub expires_at_ms: Option<u64>,
}
```

Local single-user mode may keep using the existing provider auth store as a
compatibility path, but the compatibility credential must be scoped to the local
implicit tenant and must not be accepted in hosted or enterprise modes.

### McpUpstreamAccount

This is safe display/audit metadata, never a secret:

```rust
pub struct McpUpstreamAccount {
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub provider_tenant_id: Option<String>,
}
```

All fields are optional because not every MCP/OAuth provider returns identity
metadata. When present, they must be redacted according to audit/export policy.

## Connection Identity Key

Enterprise connection identity is:

```text
org_id + workspace_id + optional deployment_id + server_id + owner principal
```

The runtime may also assign an opaque `connection_id` and use it as the primary
key. Policy checks still validate that the connection id belongs to the current
tenant and requested acting principal.

## OAuth Ownership

Hosted/enterprise OAuth follows the TAN-38 connector decision:

- The control plane starts and owns the OAuth flow.
- The control plane stores long-lived OAuth material in KMS or a customer
  secret manager.
- The runtime stores only `McpCredentialRef` and resolves short-lived access
  material by reference for the duration of a request.
- Runtime OAuth callback endpoints are local/dev compatibility only unless they
  can verify signed tenant context, actor identity, OAuth state, and intended
  `connection_id`.

Enterprise runtime-owned OAuth must fail closed unless all of these are present:

1. Verified tenant context.
2. Actor or service-principal identity.
3. Intended server id and connection id.
4. OAuth state bound to the same tenant/principal/connection.
5. Secret resolver configured for that tenant.

## Run-As Resolution

Every MCP tool call resolves an `McpActingContext` before readiness, discovery,
or `tools/call`:

```rust
pub struct McpActingContext {
    pub tenant: TenantContext,
    pub initiating_actor: Option<String>,
    pub acting_principal: McpPrincipalRef,
    pub connection_id: String,
    pub server_id: String,
    pub delegation_id: Option<String>,
}
```

Resolution rules:

1. Interactive user session defaults to that human actor's own connection.
2. Interactive session may choose a shared/service connection only when a grant
   permits the actor to use it for the requested tool class.
3. Scheduled automation must name a service-principal or shared connection
   grant. It cannot inherit the last editor's user-owned connection.
4. Workflow tasks may narrow the connection/tool set but cannot widen beyond the
   run's approved `McpActingContext`.
5. Missing, disabled, wrong-tenant, wrong-actor, or wrong-grant connections fail
   before discovery/readiness/tool execution.

## Authorization Checks

Before a tool schema is exposed to a model or a tool call is executed, the
runtime checks:

- The request has verified tenant context in hosted/enterprise mode.
- The requested connection belongs to the tenant.
- The acting principal owns or is granted the connection.
- The connection class permits the requested effect class.
- Server-level allowed tools and workflow/session allowlists permit the tool.
- Approval gates, if required, show the acting account and connection id.

No enterprise call path may fall back to local implicit headers or the global
server row when a scoped connection is missing.

## State Migration

Local single-user compatibility:

1. Existing `mcp_servers.json` rows remain readable.
2. On first write or explicit migration, create a server definition for each
   existing row.
3. If the row has local credentials or OAuth metadata, create one compatibility
   connection under the local implicit tenant with `UserOwned` class.
4. Preserve the current display name and enabled state.
5. Keep raw-token behavior local-only; hosted/enterprise modes reject migrated
   local implicit credentials.

Enterprise migration:

- Do not auto-promote global MCP rows into shared enterprise credentials.
- Admins must create explicit shared/service connections through the control
  plane or an enterprise setup flow.
- Existing global server definitions can become provider definitions only.

## Audit And Observability

MCP audit events should include:

- `tenant.org_id`, `tenant.workspace_id`, optional deployment id.
- Initiating actor id.
- Acting principal ref.
- Delegation/grant id, if any.
- Server id and connection id.
- Tool name and effect class.
- Upstream account metadata when safe.
- Decision result and denial reason.

Sensitive fields that must never be emitted:

- Access tokens, refresh tokens, API keys, authorization headers.
- OAuth authorization codes or PKCE verifiers.
- Raw provider payloads unless a product-specific audit policy explicitly
  allows a redacted summary.

Recommended event names:

- `mcp.connection.oauth_started`
- `mcp.connection.oauth_completed`
- `mcp.connection.oauth_denied`
- `mcp.connection.connected`
- `mcp.connection.refreshed`
- `mcp.connection.discovery_denied`
- `mcp.tool.call_started`
- `mcp.tool.call_denied`
- `mcp.tool.call_completed`

## Control Panel Implications

The control panel should show four concepts separately:

- Provider/server catalog entry.
- My connection.
- Shared/admin-managed connection.
- Service-principal connection.

Non-admin users can connect and revoke their own accounts. Admins can manage
shared/service connections and grants. Workflow and automation screens should
select the acting connection or delegation mode explicitly instead of selecting
only an MCP server name.

## Implementation Order

1. TAN-349: add scoped connection records and local migration.
2. TAN-350: scope OAuth sessions, callbacks, and refresh.
3. TAN-351: make connect, refresh, readiness, and discovery connection-aware.
4. TAN-352: enforce run-as/delegation policy in sessions and automations.
5. TAN-354: add audit, observability, and isolation tests.
6. TAN-353: update the control panel once backend contracts are stable.

## Open Decisions For Implementation

- Whether `connection_id` should be opaque UUID only or include a deterministic
  tenant/server/principal prefix for easier support debugging.
- Whether tool caches should be per connection only, or whether unauthenticated
  catalog schemas can be shared by server definition.
- Whether local desktop should expose principal terminology at all, or keep a
  single-user "Connected account" facade over the compatibility connection.
- Which secret resolver provider name is canonical for hosted MCP OAuth refs.

These decisions are implementation-level and do not block the model: enterprise
behavior must remain scoped by tenant, acting principal, connection, and grant.
