# Enterprise MCP Identity And Delegation

Design owner: TAN-348

Document status: implemented contract with remaining hosted-control-plane work.

Implementation review: 2026-07-14 against `origin/main` at `24440520`.
Repository behavior does not prove that a particular hosted deployment is running
the reviewed build; deployment verification remains an operator responsibility.

This document defines the enterprise MCP identity model for multi-employee
Tandem instances. The runtime now separates server definitions, user-owned
connections, service-principal connections, delegated execution, and audit
identity. The legacy global server representation remains as a local compatibility
surface, not as enterprise authority.

## Implementation Status

The principal-scoped MCP account model is implemented in the runtime and control
panel:

- `McpRegistry` retains provider/server rows for compatibility and separately
  exposes `McpServerDefinition` and `McpConnection` records.
- `McpConnection` owns tenant/principal identity, credential references,
  connection state, OAuth metadata, MCP session state, and authenticated tool
  cache.
- MCP OAuth sessions are bound to tenant context, acting principal,
  `connection_id`, provider, redirect URI, and OAuth state.
- Tool calls resolve run-as identity before readiness, policy evaluation, or
  execution. Wrong-tenant, wrong-actor, disabled, missing, or unsupported shared
  connections fail closed and emit protected denial evidence.
- Automation MCP policy can pin connection grants and service/shared run-as
  modes. Missing phase tool authority fails closed.
- Internal coder and other reviewed server MCP calls use the governed bridge,
  carrying verified tenant context, run-as identity, and phase-tool authority.
  Connector `allowed_tools` is rechecked immediately before the remote call.
- Saved automation grants pin both `connection_id` and an opaque connection
  generation. Connector removal, identity replacement, and credential changes
  rotate or remove that generation so stale or same-name replacement grants do
  not execute.
- Governed MCP decisions and executions write required protected denial/effect
  evidence; receipt persistence failure remains an execution error.
- The control panel lists actor-scoped, shared, and service connections and lets
  workflow/automation policy select explicit connection grants.

The following work is not a shipped universal guarantee:

- Hosted OAuth custody and long-lived secret storage still need a fully wired
  control-plane/KMS resolver for every provider. Local compatibility paths may
  retain runtime-owned OAuth state.
- Shared/service connection creation and grant administration are not yet a
  complete hosted identity-administration product.
- Tool schemas are not universally filtered before discovery for every possible
  provider/policy path; execution-time enforcement remains the hard boundary.
- The event taxonomy below is only partially implemented and should not be
  treated as a complete exported observability contract.

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

| Caller and target                          | Authentication boundary                                                                    | Credential visible to the model                     |
| ------------------------------------------ | ------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| Control Panel chat to Tandem product tools | Existing browser/desktop session plus `VerifiedTenantContext`                              | None                                                |
| External agent to the Tandem API           | Supported API token or OAuth entry credential plus hosted context assertion where required | Never; the agent host owns transport authentication |
| Tandem to a third-party MCP/service        | Principal-scoped connection or governed service credential                                 | Only an opaque connection reference                 |
| Tandem Docs MCP                            | Public read-only endpoint; catalog contract is `requires_auth = false`                     | None                                                |

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

Implemented core fields:

```rust
pub struct McpServerDefinition {
    pub server_id: String,
    pub name: String,
    pub transport: String,
    pub auth_kind: String,
    pub enabled: bool,
    pub purpose: String,
    pub grounding_required: bool,
    pub allowed_tools: Option<Vec<String>>,
}
```

`server_id` serves as the stable internal id even when it is initially derived
from a legacy server name. Display names may remain user-editable, but policy
and connection records should use ids.

### McpConnection

An MCP connection is a principal-scoped account/credential binding for one
server definition.

Implemented identity fields are shown below. The runtime record also carries
secret-header references, OAuth configuration, connection/auth state, MCP
session state, pending challenges, and the per-connection tool cache.

```rust
pub struct McpConnection {
    pub connection_id: String,
    pub server_id: String,
    pub tenant_context: TenantContext,
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
refresh metadata are stored on the connection or in state keyed by
`connection_id`.

### McpPrincipalRef

Supported acting principals:

```rust
pub enum McpPrincipalRef {
    HumanActor { actor_id: String },
    ServicePrincipal { principal_id: String },
    AutomationPrincipal { automation_id: String },
    SharedConnection { grant_id: String },
    LocalImplicit,
}
```

`LocalImplicit` is the explicit compatibility identity for local single-user
mode; it is not enterprise authority. Human-owned connections are usable by
that actor unless an admin policy further
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

The class records the intended policy ceiling. A user-owned connection must not
silently become a shared workflow credential, and a shared read-only connection
must not execute write tools merely because a server exposes them. The current
runtime persists and audits the class, but complete class-to-effect enforcement
is still required before those semantics can be claimed universally.

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

The target hosted/enterprise OAuth boundary follows the TAN-38 connector
decision:

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

Every MCP tool call resolves an internal run-as record before policy evaluation
or `tools/call`. The effective record includes the following authority data
(the concrete type is private to the HTTP module):

```rust
pub struct McpRunAsResolution {
    pub args: Value,
    pub requested_tenant_context: TenantContext,
    pub effective_tenant_context: TenantContext,
    pub connection_id: String,
    pub principal: McpPrincipalRef,
    pub connection_class: Option<String>,
    pub upstream_account: Option<Value>,
    pub requested_connection_id: Option<String>,
}
```

Resolution rules:

1. Interactive user session defaults to that human actor's own connection.
2. Interactive session may choose a shared/service connection only when a grant
   permits the actor to use it for the requested tool class.
3. Scheduled automation must name a service-principal or shared connection
   grant. It cannot inherit the last editor's user-owned connection.
4. Workflow tasks may narrow the connection/tool set but cannot widen beyond the
   run's approved connection grant and resolved `McpRunAsResolution`. Saved
   grants must still match the live connection id and generation.
5. Missing, disabled, wrong-tenant, wrong-actor, or wrong-grant connections fail
   before tool execution. Connection-aware readiness also uses the resolved
   identity; complete pre-discovery schema filtering remains open work.

## Authorization Checks

At tool execution, the runtime checks:

- The request has verified tenant context in hosted/enterprise mode.
- The requested connection belongs to the tenant.
- The acting principal owns or is granted the connection.
- Server-level allowed tools and workflow/session allowlists permit the tool.
- A saved workflow grant still matches the selected connection's current
  generation.
- Context-assertion and phase-tool authority permit the call where those
  policies apply.

Connection class is persisted and exposed for policy/audit use, but complete
effect-class semantics for `SharedReadOnly`, `SharedReadWrite`, and
`AdminManaged` are not established as a universal execution check. Discovery
paths should apply the same authority before exposing schemas; until all paths
do so, execution-time enforcement is the hard boundary.

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

Implemented protected event names include:

- `mcp.connection.oauth_started`
- `mcp.connection.oauth_completed`
- `mcp.connection.oauth_denied`
- `mcp.run_as_denied`

The following names remain the intended complete taxonomy; do not rely on every
name being emitted on every path yet:

- `mcp.connection.connected`
- `mcp.connection.refreshed`
- `mcp.connection.discovery_denied`
- `mcp.tool.call_started`
- `mcp.tool.call_denied`
- `mcp.tool.call_completed`

## Control Panel Behavior And Remaining Work

The control panel now shows provider/server definitions separately from visible
actor-scoped, shared, and service-principal connections. Workflow and automation
surfaces persist explicit connection grants and run-as configuration rather than
only a server name.

The intended hosted administration model still distinguishes four concepts:

- Provider/server catalog entry.
- My connection.
- Shared/admin-managed connection.
- Service-principal connection.

Self-service revocation and complete admin management of shared/service
connections and grants remain product-level work even where the underlying
runtime types and policy checks exist.

## Implementation History

The original implementation sequence has substantially landed:

1. TAN-349: scoped connection records and local compatibility migration — implemented.
2. TAN-350: tenant/principal/connection-scoped OAuth sessions and callbacks — implemented.
3. TAN-351: connection-aware connect, readiness, and execution state — implemented.
4. TAN-352: run-as/delegation enforcement for sessions and automations — implemented,
   with hosted grant administration still incomplete.
5. TAN-354 plus TAN-734/TAN-737/TAN-738: required protected denial and execution
   evidence is implemented for governed server MCP paths; the complete named
   event taxonomy remains open.
6. TAN-353: connection-aware control-panel surfaces — implemented, with hosted
   administration UX still incomplete.

## Remaining Decisions

- Whether tool caches should be per connection only, or whether unauthenticated
  catalog schemas can be shared by server definition.
- Whether local desktop should expose principal terminology at all, or keep a
  single-user "Connected account" facade over the compatibility connection.
- Which secret resolver provider name is canonical for hosted MCP OAuth refs.
- Which shared/service grant lifecycle and revocation API is authoritative in
  hosted deployments.
- Which discovery paths must pre-filter schemas in addition to the existing
  execution-time fail-closed checks.

These decisions are implementation-level and do not block the model: enterprise
behavior must remain scoped by tenant, acting principal, connection, and grant.
