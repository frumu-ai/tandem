# Channel Message Ingress → Verified Principal — Design (TAN-652)

How an inbound Slack (or Discord/Telegram) **message** becomes a verified
principal + tenant context on the engine, so the governed memory/tool filters
actually engage for a channel-originated run. This is the "hard part" of the
Department-Scoped Slack Agent demo: the authority model, not the Slack transport.

## Current state (grounded)

- **Channels are an HTTP client of the engine.** The dispatcher posts to
  `POST /session/{id}/prompt_async` (`crates/tandem-channels/src/dispatcher_parts/part03.rs:312`)
  with:
  - `x-tandem-request-source: channel` (`dispatcher_parts/part01.rs:60`),
  - an optional signed `x-tandem-context-assertion` taken from
    `config.context_assertion` (`part01.rs:62-63,1199`),
  - `x-tandem-client-id: channel:{sender}` — a *memory-subject hint only*
    (`part03.rs:317`), and
  - the shared `TANDEM_API_TOKEN` bearer.
- **The engine verifies the assertion, not the sender.** Middleware
  (`crates/tandem-server/src/http/middleware.rs`) requires a signed
  `x-tandem-context-assertion` under hosted/enterprise auth modes, verifies it via
  `TenantContextAssertionVerifier` with replay protection (`middleware.rs:612-627`),
  and otherwise falls back to `local_request_source` (`:632-648`).
- **The channel sends one _static_ assertion.** `config.context_assertion` is a
  single workspace-level value — it does **not** carry the specific Slack sender's
  `actor_id`, org-units, or roles. So even when present, every sender shares one
  identity.
- **The real resolver runs only on the interaction webhook.**
  `resolve_channel_user` → `build_principal` (`app/state/principals/channel_identity.rs:52,147`)
  and `channel_bound_tenant` (`:110`) exist and are used by
  `slack_interactions.rs` (button clicks), which *is* the server. The **message**
  path never calls them, so a message run carries no verified principal and
  org/workspace default to `"local"`.

**Net gap:** message-ingress needs to produce, per sender, a `VerifiedTenantContext`
carrying `actor_id = channel:{kind}:{user}`, the channel-bound tenant, and the
sender's org-units — so `resolve_prompt_memory_access` resolves `Governed` (not
`LocalNoop`) and the strict tool projection engages.

## Trust model

The channel process authenticates the surface user on its side (Slack bot token +
the `channels.{kind}.allowed_users` allowlist) and is itself authenticated to the
engine (shared token). The question is how the **per-message sender identity**
crosses into the engine as a *verified* principal. Two options:

### Option A — per-sender signed context assertion (production-safe)

The channel mints (or fetches from a minting endpoint) a **signed context
assertion per sender**, carrying `actor_id = channel:{kind}:{user}`, the bound
tenant, and the sender's roles/org-units. The engine verifies it with the
existing `TenantContextAssertionVerifier` + replay policy — **no new trust path**.

- **Pros:** reuses the verified, replay-protected assertion pipeline; the engine
  trusts cryptographic proof, not a claimed header; correct for multi-tenant /
  hosted.
- **Cons:** the channel needs signing material or a minting service, and a source
  of the sender's org-units/roles (the enterprise directory). More moving parts.

### Option B — server-side channel resolution (demo-pragmatic)

On a `request-source: channel` prompt, the **engine** resolves the sender
(`x-tandem-client-id` / a dedicated `x-tandem-channel-user` header) via
`resolve_channel_user` against config, applies `channel_bound_tenant`, loads the
sender's org-unit memberships from `EnterpriseState`, and builds the
`VerifiedTenantContext` server-side. The trust boundary is: *the shared-token
channel process is trusted to assert the sender id it already authenticated.*

- **Pros:** no per-user signing; reuses the exact resolver the interaction webhook
  uses; smallest change; sufficient for the governed single-tenant demo.
- **Cons:** the engine trusts the channel's asserted sender id (acceptable only
  because the channel is a trusted, shared-secret component). Not safe if the
  channel-to-engine boundary is ever untrusted/multi-tenant.

## Decision

- **Demo:** **Option B.** The demo runs governed single-tenant with a trusted
  channel process; server-side resolution reuses `resolve_channel_user` /
  `channel_bound_tenant` and the TAN-653 profile memberships, and is the smallest
  correct change. Fail **closed** on `Denied` / `ChannelNotConfigured` (never run
  a channel prompt as an anonymous or local principal).
- **Production:** **Option A.** Move to per-sender signed assertions once the
  channel-to-engine boundary spans tenants; the engine-side resolution from
  Option B becomes the assertion *minting* logic, so the work is not wasted.

## Concrete wiring plan (Option B)

1. **Headers** — the channel dispatcher sends, alongside `x-tandem-request-source: channel`:
   - `x-tandem-channel-kind: slack|discord|telegram` — the **authoritative** channel
     kind the middleware needs for `resolve_channel_user(…, kind, …)` and
     `channel_bound_tenant(…, kind)`. Do **not** infer the kind from the
     `x-tandem-client-id` memory-subject hint (e.g. `telegram:123`,
     `dispatcher_parts/part03.rs:229-237`) — that is not an auth input, and inferring
     it risks resolving against the wrong allowlist/tenant in multi-channel deployments.
   - `x-tandem-channel-user: {surface_user_id}` — the raw sender id.
   - `x-tandem-channel-auth: {credential}` — a **channel-specific** credential,
     distinct from the shared API token (see Trust, below). (`dispatcher_parts/part01.rs` / `part03.rs`.)
2. **Middleware — server-side channel resolution takes _precedence_.** For a request
   carrying a valid `x-tandem-channel-auth` + `request-source: channel`, resolve the
   per-sender identity **before** honoring any static `config.context_assertion`.
   Otherwise a demo that leaves `TANDEM_CHANNEL_CONTEXT_ASSERTION` set (attached as
   `x-tandem-context-assertion`, `dispatcher_parts/part01.rs:60-63`) would satisfy the
   verified-assertion path first and run **every** sender under one static identity —
   the per-sender org-unit filters would never engage. Resolution:
   - validate `x-tandem-channel-auth` against the configured channel credential, else **deny**;
   - `resolve_channel_user(effective_config, kind, user)` → principal, else **deny**;
   - `channel_bound_tenant(effective_config, kind)` → `(org_id, workspace_id)`;
   - load org-unit memberships for `actor_id` from `EnterpriseState` (TAN-653);
   - assemble a `VerifiedTenantContext { human_actor, org_units, roles,
     strict_projection, … }`.
   (`crates/tandem-server/src/http/middleware.rs`, alongside
   `resolve_enterprise_request_context_for_mode`.) Alternatively, **forbid** the static
   channel assertion whenever per-sender channel resolution is enabled, so the two
   identity paths can't conflict.
3. **Auth mode:** the demo tenant runs `HostedSingleTenant` so the resolved
   context yields `GovernedStrict` reads (`memory/read_policy.rs`).
4. **Fail-closed:** `Denied` → 403; `ChannelNotConfigured` → refuse; never fall
   through to `local`.
5. **Open-channel caveat:** honor GOV-B5a — a `["*"]` channel grants *talk*, not
   approval authority; approvals still require an explicit per-identity capability
   (already enforced on the interaction path).

## Security considerations

- **Channel credential (non-spoofable channel identity) — REQUIRED.**
  `x-tandem-request-source` and `x-tandem-channel-user` are caller-controlled, and
  `auth_gate` today validates only the **shared** API token (`middleware.rs:343,1571-1600`)
  — the *same* token the channel dispatcher uses. So the shared token is **not**
  sufficient: any API-token client could set `request-source: channel` +
  `x-tandem-channel-user` to impersonate an allowlisted sender. Option B therefore
  requires a **channel-specific credential** (`x-tandem-channel-auth`) — a separate
  secret / HMAC (ideally over the sender id + timestamp, with a replay window) that
  the channel process holds and the middleware verifies **before** trusting the
  asserted sender. Division of labor: the shared API token gates the endpoint; the
  channel credential proves *the caller is the channel process*. Only after it
  verifies is `x-tandem-channel-user` trusted.
- **Replay / allowlist / bound-tenant:** unchanged from the interaction path;
  reuse `resolve_channel_user` (allowlist, deny-by-default) and `channel_bound_tenant`
  (prevents a channel acting on another tenant's run).

## Test plan

- Mapped sender (valid `x-tandem-channel-auth`, in allowlist) → run resolves
  `Governed`, principal `channel:slack:{user}`, bound tenant applied, org-units populated.
- **Impersonation:** a plain API-token client with **no** valid `x-tandem-channel-auth`
  setting `request-source: channel` + `x-tandem-channel-user` → **rejected** (proves
  the shared token alone cannot impersonate a channel sender).
- Valid channel credential + unmapped sender → 403.
- Missing/omitted `x-tandem-channel-kind` → rejected (no allowlist/tenant to resolve against).
- **Static-assertion precedence:** with `TANDEM_CHANNEL_CONTEXT_ASSERTION` set *and*
  a valid channel credential, each mapped sender resolves to its **own** principal
  (not the static assertion identity).
- Regression: local single-user (no channel source) path unchanged.
- Interaction webhook path unchanged (shared resolver).

## Sequencing / follow-ups

1. This design (TAN-652).
2. Option B wiring (header + middleware resolution + fail-closed) — depends on the
   TAN-653 org-unit memberships being loadable, and pairs with running the demo
   tenant governed.
3. Option A (per-sender signed assertions) — production follow-up.
