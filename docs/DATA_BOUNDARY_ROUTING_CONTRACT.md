# Data Boundary Local/Private Routing Contract

Status: design contract for TAN-396 (Tandem Secure Data Boundary, Cycle 3).
`RouteToLocal` exists in the decision/event vocabulary and its *fallback*
semantics are implemented; actual re-routing to a local provider is
deliberately **not** implemented until provider capabilities are explicit.
Companion docs: `DATA_BOUNDARY_MODULE.md` (module overview),
`DATA_BOUNDARY_INTEGRATION_MAP.md` (egress call sites).

## What `RouteToLocal` means

Policy (`require_local_classes`, or
`TANDEM_DATA_BOUNDARY_EXTERNAL_RAW_POLICY=require_local`) declares that
payloads containing certain sensitive classes must only be processed by a
provider whose boundary class is internal (`Local` or `CustomerHosted`). When
such a payload is headed to any external provider, the decision engine returns
`DataBoundaryAction::RouteToLocal`: "this call is only allowed if it is
re-dispatched to an internal provider."

The decision intentionally does not name a target provider. Choosing one is a
routing concern that requires capability data the registry does not have yet
(model quality/context-window parity, tool support, tenant-specific
deployments).

## Behavior by mode (current, implemented)

| Mode | Behavior when `RouteToLocal` is decided |
| --- | --- |
| `off` | Not evaluated. |
| `audit` | Downgrade: dispatch proceeds unchanged; `data_boundary.evaluated` records the decided action and reason codes (`require_local_*`). |
| `enforce` | **Fail closed**: the dispatch is blocked with reason `route_to_local_unavailable` appended to the decision's reason codes, because no routing capability exists. Evidence records both the routing requirement and why it could not be satisfied. |

Fail-closed is the only safe fallback: silently continuing to the external
provider would defeat the policy, and silently substituting an arbitrary local
model could produce materially different results without operator awareness.

## Fallback semantics (contract for the future implementation)

When routing is implemented, `RouteToLocal` resolution must follow this
order, stopping at the first satisfiable step:

1. **Session/tenant-pinned internal provider** — a tenant- or
   deployment-configured private model endpoint, if one is registered and
   healthy.
2. **Policy-named internal provider** — an explicit
   `TANDEM_DATA_BOUNDARY_LOCAL_ROUTE_PROVIDER` (future config) naming the
   provider id to re-dispatch to.
3. **Registry default internal provider** — the first registered provider
   whose boundary class is `Local`/`CustomerHosted` and which serves the
   requested capability (chat/stream), only if policy opts into implicit
   selection (`allow_implicit_local_route`, future config, default off).
4. **Fail closed** — block with `route_to_local_unavailable` (today's
   behavior and the permanent last resort).

Re-dispatch requirements:

* The re-routed call must re-enter the boundary evaluation (the internal
  provider classification makes it pass), so evidence shows both decisions.
* Approval decisions and audit continuity must be preserved: the original
  decision id is carried as a `rerouted_from` evidence ref.
* The user-visible response must disclose the substitution (model id in the
  session's message metadata already does this).

## Provider registry changes needed (not yet made)

1. **Boundary metadata on providers.** Today provider identity is a bare
   `String` id; boundary classification lives outside the registry (data
   boundary classifier: env mapping + built-in loopback defaults). The
   registry should eventually expose
   `boundary_class(provider_id) -> ProviderBoundaryClass` sourced from
   provider definitions themselves (config-defined custom providers declaring
   `boundary_class`, builtin defaults for loopback hosts), so classification
   and routing use one source of truth.
2. **Capability flags.** Selecting a local substitute requires knowing which
   local providers can serve the request shape (streaming, tool calls, context
   window ≥ request size). `ModelInfo.context_window` exists; tool-call
   capability does not.
3. **Health/readiness.** Routing to a dead local endpoint converts a policy
   decision into an outage. The registry needs a cheap readiness probe before
   substitution (the cost-ordered fallback in `select_cheapest_provider_id`
   is not health-aware).

## Explicit TODOs

* TODO(TAN-396-follow-up): `TANDEM_DATA_BOUNDARY_LOCAL_ROUTE_PROVIDER` config
  + resolution step 2.
* TODO(TAN-396-follow-up): provider-declared `boundary_class` in provider
  config definitions; migrate the classifier's env mapping onto it.
* TODO(TAN-396-follow-up): capability/health metadata for internal providers
  (step 3 prerequisites).
* TODO(TAN-396-follow-up): re-dispatch mechanics in the engine loop
  (re-entering evaluation, `rerouted_from` evidence, message metadata
  disclosure).

## Non-goals

* No implicit model substitution before capability and health metadata exist.
* No cross-provider prompt rewriting; the re-routed request is the same
  payload (post any required transformation) sent to a different provider.
* No weakening of the fail-closed default: routing is an *optimization* over
  blocking, never a bypass.
