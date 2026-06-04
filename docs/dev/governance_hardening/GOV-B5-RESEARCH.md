# GOV-B5 — Research: channel authority (approve-by-default, button step-up, tenant binding)

Status: **research / needs decisions** (not implemented)
Owner: Channels / Authority
Related: `docs/dev/governance_hardening/KANBAN.md` (GOV-B5)

GOV-B5 is not a single hardening fix — it is three distinct problems across the
`tandem-server` and `tandem-channels` crates, each with a product decision and real risk
of breaking legitimate single-operator channel use. This note lays out the findings,
options, and the decisions needed before coding.

## Problem 1 — Approve/Reconfigure by default for unenrolled users

`channel_user_capability_tier`
(`crates/tandem-server/src/app/state/channel_user_capabilities.rs:185-198`) returns an
enrolled user's `max_tier`, else falls back to `command_tier_for_profile(fallback_profile)`.

- `CommandTier` order: `Read < Act < Approve < Reconfigure`.
- `command_tier_for_profile(Operator) = Reconfigure` (the **maximum** tier).
- `ChannelSecurityProfile::Operator` is the **`#[default]`**.

⇒ Any unenrolled user on a default-profile channel is treated as `Reconfigure`, so
`channel_user_can_approve` (`:200-209`) is true — they can approve gates and reconfigure
**without ever enrolling**. Enrollment becomes optional for full power.

### Why this can't just be flipped
Setting the unenrolled fallback to `Read`/`Act` would secure shared channels but **break the
single-operator case**: a solo operator using their own Slack/CLI channel never formally
"enrolls" and today expects full control. This is the same local-safety constraint that
shaped GOV-B10/B6 — a blind change here regresses legitimate solo use.

### Options
1. **Profile-as-ceiling, enrollment-for-Approve+ (recommended).** Keep the profile tier as
   the channel **ceiling**, but cap the *unenrolled* fallback at `< Approve` (e.g. `Act`),
   so consequential approve/reconfigure requires an explicit per-user enrollment grant. Add
   a deployment switch (or auto-enroll the first/owner operator) so a solo operator is not
   locked out of their own channel.
2. **Default-deny unenrolled (`Read`).** Strongest, but most disruptive; needs an explicit
   bootstrap/enrollment step for every channel before it is useful.
3. **Status quo + audit only.** Record every unenrolled Approve as a protected audit event
   but keep allowing it. Weakest; only buys detection.

### Decision needed
**D-B5.1:** unenrolled fallback tier, and how a solo operator bootstraps Approve+ (auto-
enroll owner vs explicit enroll vs deployment flag).

## Problem 2 — Step-up is a global env PIN, slash-only, Reconfigure-only

`reconfigure_step_up_satisfied`
(`crates/tandem-channels/src/dispatcher_parts/part03.rs:1060-1083`):

- The expected PIN is a **global env var** `TANDEM_CHANNEL_STEP_UP_PIN` (+ a global issued-at
  env), so it is **not bound to a user** — anyone who learns it can step up as anyone, and it
  cannot express "this user, this action."
- `step_up_required_reason` (`:1048-1058`) is only invoked for **slash commands**, so
  **approve buttons / interactive components bypass step-up entirely**.
- Step-up only triggers for `CommandTier::Reconfigure`; `Approve` actions never step up.

### Options
- Replace the global env PIN with a **per-user, per-action, expiring step-up token** issued by
  the desktop/control-panel and verified server-side (stored in state, TTL, single-use,
  bound to `{channel,user,action}`).
- Invoke the step-up check in the **button/interaction path** (`*_interactions.rs`), not just
  the slash path.
- Decide whether `Approve` (not only `Reconfigure`) requires step-up for consequential gates.

### Decision needed
**D-B5.2:** step-up model (per-user expiring token issuance + verification surface), which
tiers require it, and whether buttons require step-up for Approve.

## Problem 3 — Channels not tenant-bound

Channel capability records key on `{channel,user}` only
(`channel_user_capability_key`), with no tenant binding, and channel actions are not
scoped/attributed to a tenant. In multi-tenant/hosted deployments this allows cross-tenant
capability bleed and unattributed channel actions.

### Options
- Add a tenant dimension to the capability key and channel config, and resolve+stamp the
  tenant on every channel-originated action (attribution + scope checks).
- Define the binding source (channel config → tenant mapping) and migration for existing
  records.

### Decision needed
**D-B5.3:** the channel→tenant binding model and how existing unkeyed records migrate.

## Recommended phasing

- **B5a (lowest-risk, do first once D-B5.1 is set):** cap the unenrolled fallback below
  `Approve` (Option 1) with a solo-operator bootstrap, **plus** a protected audit event on
  every channel Approve/Reconfigure (attribution now, regardless of the tier decision). This
  closes the "approve without enrollment" hole while preserving solo use.
- **B5b:** per-user expiring step-up token + apply the check to buttons (D-B5.2). A feature,
  not a one-liner.
- **B5c:** channel tenant binding + migration (D-B5.3). A data-model change.

## Local-safety invariant (must hold for every phase)

A single-operator local deployment must keep working without ceremony. Whatever default and
step-up model is chosen, the owner/first operator of a channel must not be locked out of
their own channel, mirroring the OSS-safe posture used in GOV-B10/B6a.
