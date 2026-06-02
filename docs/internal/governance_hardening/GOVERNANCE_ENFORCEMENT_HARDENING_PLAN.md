# Governance Enforcement Hardening Plan

Last updated: 2026-06-02
Owner: Engine / Runtime Authority
Goal: `docs/internal/governance_hardening/GOAL.md`
Kanban: `docs/internal/governance_hardening/KANBAN.md`

Derived from a five-surface governance-enforcement audit of the engine runtime
(`crates/tandem-server`, `crates/tandem-governance-engine`, `crates/tandem-enterprise-*`).
Line numbers are anchors against the analyzed tree (main @ 0.5.13) and should be
re-confirmed at implementation time.

---

## How to read this

Each finding card: **failure**, **root cause (file:line)**, **change**, **acceptance**,
**verification** (suggested test names), and **priority**.

Priority legend:
- **P0** — human-in-the-loop / governance can be bypassed *regardless of deployment mode*.
  Must land before the governance pitch is credible.
- **P1** — defeats governance in the default runtime mode, or resumes governed work
  without recheck.
- **P2** — evidence/attribution gaps, channel tier defaults, authorization-altitude.

---

## Top-line framing

The V2 CRUD handlers (`create/patch/delete/run_now/pause/resume`) are correctly wired:
resolve actor → call governance → persist → audit. That proves the right pattern exists.
The gaps are:

1. The **approval control itself** (gate-decision endpoint) has no actor, no human
   requirement, no self-approval guard, no audit decider. (B1)
2. Whole **side-door endpoints** (coder runs, routines, channel drafts) create/run
   governed work with no governance call at all. (B2)
3. The **actor "human vs agent" classification** is forgeable in the default
   `LocalSingleTenant` mode, neutralizing the otherwise-correct gates. (B3)
4. **Human-only / self-approval** is not enforced on several approval paths. (B4)
5. **Audit evidence** is missing on deny paths, gate decisions, and internal sweeps. (B8)

What is already solid (reference model, do not regress): the hosted/enterprise control
plane (`routes_enterprise*.rs`) — JWS-verified actor/tenant, `require_enterprise_admin`
on all mutations, cross-tenant lookups return `NOT_FOUND`; `has_approved_agent_capability`
/ `has_approved_agent_quota_override` are fail-closed; channel inbound signature
verification; GuardrailStopped auto-resume override checks; spend hard-stop audit.

---

## P0 — bypassable in any mode

### GOV-B1 — Gate-decision endpoint: agent can approve its own run
- **Failure:** the actual human-in-the-loop control resumes an `AwaitingApproval` run
  with no approver identity, no human requirement, no self-approval guard, and no
  attributable audit.
- **Root cause:** `automations_v2_run_gate_decide`
  (`crates/tandem-server/src/http/routines_automations_parts/part02.rs:2012`) takes no
  `HeaderMap`/`RequestPrincipal`, never calls `resolve_governance_actor`, guards only on
  `ensure_automation_v2_run_visible_to_context` (read-level), and persists via
  `apply_automation_gate_decision` (`crates/tandem-server/src/app/state/automation/gates.rs:26`)
  whose record `AutomationGateDecisionRecord`
  (`crates/tandem-server/src/automation_v2/types.rs:1067`) has no `decided_by`. The
  `approval.decision.recorded` event (part02.rs:2185) omits the decider and writes no
  protected audit.
- **Change:** add `HeaderMap` + `Extension<RequestPrincipal>`; resolve the actor; require
  a verified Human (or channel-verified `Approve` tier); add a self-approval guard
  (decider ≠ the run's executing agent/actor); switch the access guard to
  `ensure_automation_v2_run_owner_or_admin`; thread `decided_by` into the record and emit
  `append_protected_audit_event`.
- **Acceptance:** an agent-context request to approve its own run is rejected; the gate
  record and a protected audit event both carry the verified decider; a non-owner/
  non-admin cannot approve.
- **Verification:** `cargo test -p tandem-server gate_decide -- --nocapture`;
  add `gate_decision_requires_human_non_self_approver`,
  `gate_decision_writes_protected_audit_with_decider`.

### GOV-B2 — Side-door endpoints run governed work with no governance
Split into three sub-items because they live in different handlers.

- **GOV-B2a — Coder runs.** `coder_run_create` / `coder_run_execute_all`
  (`crates/tandem-server/src/http/coder_parts/part06.rs:1130`, `part07.rs:279`) and
  `coder_run_approve` / `coder_run_cancel` (`part07.rs:419/620`) resolve no actor and
  call no governance. `coder_execution_policy_block` (part06.rs:1030) is workflow-order
  only, not an authorization substitute.
- **GOV-B2b — Routines & v1 wrappers.** `routines_create` (`part01.rs:827`, carries a
  literal `// TODO: SECURITY`), `routines_run_now` (`part01.rs:983`), and the v1 wrappers
  `automations_run_now` / `automations_run_*` (`part02.rs:476/589+`) never consult
  governance; `requires_approval`/`external_integrations_allowed` are client-set body
  fields.
- **GOV-B2c — Channel automation draft confirm.** `channel_automation_drafts_confirm`
  (`crates/tandem-server/src/http/channel_automation_drafts.rs:329`) calls
  `put_automation_v2` directly, skipping `can_create_automation_for_actor` +
  `record_automation_creation`; no `RequestPrincipal`.
- **Change:** route each through the same `resolve_governance_actor` +
  `can_create_automation_for_actor` / `authorize_mutation` path as V2 (prefer delegating
  to the governed handler); restrict approve/cancel to Human; add allow+deny audit.
- **Acceptance:** an agent-context request blocked on the V2 create/run path is equally
  blocked on the coder/routine/channel-draft path; creation-governance and audit fire.
- **Verification:** `cargo test -p tandem-server coder_run_governance -- --nocapture`,
  `routines_governance`, `channel_draft_confirm_governance`.

### GOV-B4 — Human-only & self-approval not enforced on approval paths
- **Failure:** an agent can approve a pending governance approval (capability/quota
  escalation) and routines/coder approvals; a requester can approve its own request.
- **Root cause:** `governance_approval_approve`
  (`crates/tandem-server/src/http/routes_governance.rs:166`) omits the
  `if reviewer.kind != GovernanceActorKind::Human { FORBIDDEN }` check its sibling grant
  handlers have. `decide_approval_request`
  (`crates/tandem-governance-engine/src/lib.rs:310`;
  `crates/tandem-server/src/app/state/governance.rs:806`) never compares `reviewer` to
  `existing.requested_by` (the `requested_by_is_agent` helper is unused).
  `routines_run_approve/deny/pause` and `coder_run_approve/cancel` have no human gate.
- **Change:** add the `kind==Human` check to `governance_approval_approve` (and consider
  `deny`); add a self-approval guard in `decide_approval_request`; restrict routine/coder
  approve/deny/pause/cancel to Human actors.
- **Acceptance:** agent-origin approval is forbidden; reviewer == requester is rejected.
- **Verification:** `cargo test -p tandem-server governance_approval -- --nocapture`;
  add `approval_requires_human_reviewer`, `approval_rejects_self_approval`.

---

## P1 — defeats governance in default mode / resumes without recheck

### GOV-B3 — Forgeable actor classification in default runtime mode
- **Failure:** in `LocalSingleTenant` (the engine default), sending
  `x-tandem-request-source: control_panel` (or omitting headers) classifies any caller as
  Human, so all agent-authored gates pass.
- **Root cause:** `resolve_governance_actor` / `resolve_governance_provenance`
  (`crates/tandem-server/src/http/governance.rs:27-126`) treat `control_panel` source as
  human; `local_request_source` (`crates/tandem-server/src/http/middleware.rs:392-411`)
  derives that source from the raw, unsigned `x-tandem-request-source` header.
- **Change:** classify Human only from an authenticated signal — bind `control_panel`/
  `local_control_panel` to a trusted local transport token (or loopback + token), and
  stop letting a present `x-tandem-agent-id` be silently overridden by a forgeable source
  string.
- **Acceptance:** a caller cannot self-elevate to Human via headers alone in local mode;
  existing hosted/enterprise JWS path unchanged.
- **Verification:** `cargo test -p tandem-server governance_actor -- --nocapture`;
  add `control_panel_source_requires_local_token`.
- **Note:** the test `automations_v2_create_and_run_now_treat_control_panel_source_as_human`
  encodes the current (insecure) behavior and must be updated alongside this change.

### GOV-B10 — OSS human authorization has no ownership check; enterprise admin header fallback
- **Failure:** OSS `UnavailableGovernanceEngine` reduces to "Human ⇒ allow" with no
  owner/grant check; a latent enterprise-admin fallback grants admin from a header source
  when no verified context is present.
- **Root cause:** `UnavailableGovernanceEngine`
  (`crates/tandem-server/src/app/state/governance.rs:17-66`) and `can_mutate_automation`
  (`governance.rs:554-570`, only checks a record *exists*);
  `enterprise_admin_allowed_for_mutation`
  (`crates/tandem-enterprise-server/src/http/routes_enterprise.rs:1140`).
- **Change:** require the human actor to be owner or hold a modify-grant; gate/remove the
  header-source admin fallback, or assert enterprise routes never mount under
  `LocalSingleTenant`.
- **Acceptance:** a human who is neither owner nor grant-holder cannot mutate another's
  automation; enterprise mutation under local mode is impossible or token+role gated.
- **Verification:** `cargo test -p tandem-server oss_human_ownership -- --nocapture`;
  `cargo test -p tandem-enterprise-server admin_fallback -- --nocapture`.

### GOV-B6 — Internal launch / stale-resume skip governance recheck
- **Failure:** a paused or capability-revoked agent's queued/stale-reaped run still
  launches/resumes; spend cap only re-pauses after more spend is incurred.
- **Root cause:** `claim_specific_automation_v2_run`
  (`crates/tandem-server/src/app/state/app_state_impl_parts/part05.rs:573`) and
  `auto_resume_stale_reaped_runs` (`part05.rs:356`) check runtime-context/attempt-budget
  only; `can_admit`/`admit_run`
  (`crates/tandem-server/src/app/state/automation/scheduler.rs:150/203`) check
  rate-limit/lock/capacity only.
- **Change:** add a governance recheck (agent pause / spend pause / capability still
  approved) before transitioning Queued→Running and before stale auto-resume.
- **Acceptance:** a paused/revoked agent's run does not launch/resume; the block is
  recorded.
- **Verification:** `cargo test -p tandem-server launch_governance_recheck -- --nocapture`,
  `stale_resume_governance_recheck`.

### GOV-B7 — `automations_v2_share` mutates visibility with no governance/audit
- **Root cause:** `automations_v2_share`
  (`crates/tandem-server/src/http/routines_automations_parts/part02.rs:1300`) checks
  tenant + owner/admin but never calls `can_mutate_automation`; can flip a private
  automation to org-wide (`visibility:"org"`) with no audit event.
- **Change:** gate through `resolve_governance_actor` + `can_mutate_automation` (consider
  `can_escalate_declared_capabilities` for visibility widening); emit
  `automation.governance.shared` audit.
- **Acceptance:** an agent/non-authorized actor cannot widen visibility; the change is
  audited.
- **Verification:** `cargo test -p tandem-server automation_share_governance -- --nocapture`.

---

## P2 — evidence, channel tiers, authorization altitude

### GOV-B8 — Audit-evidence gaps (deny paths, gate decisions, internal sweeps)
- **Root cause:** V2 handlers emit no audit on the `governance_error_response` deny path;
  gate decisions record no `decided_by` (covered by B1);
  `record_automation_lifecycle_event`
  (`crates/tandem-server/src/app/state/automation/lifecycle.rs:7-16`) writes only run-local
  `lifecycle_history` — the reaper, auto-resume, recover, and shutdown-fail sweeps
  (`part03.rs`, `part05.rs`, `automation_v2/executor.rs`) leave no protected audit and no
  system-actor attribution.
- **Change:** emit `append_protected_audit_event` on every deny path; give internal
  sweeps an explicit **system/service actor** + protected audit.
- **Acceptance:** every allow and deny of a consequential action, including internal
  transitions, produces an attributable protected audit record.
- **Verification:** `cargo test -p tandem-server governance_deny_audit -- --nocapture`,
  `internal_sweep_audit_attribution`.

### GOV-B5 — Channel: Approve-by-default fallback + button step-up missing
- **Root cause:** unenrolled allowlisted users fall back to the default `Operator`
  profile → `CommandTier::Reconfigure` ≥ `Approve`
  (`crates/tandem-server/src/app/state/channel_user_capabilities.rs:185-209`;
  `crates/tandem-channels/src/channel_registry.rs:119-124`, `config.rs:28`). The PIN
  step-up (`crates/tandem-channels/src/dispatcher_parts/part03.rs:1060-1083`) is a single
  global env var and runs only on slash commands; approve/rework/cancel **buttons** never
  call it. Channels aren't bound to a tenant (interaction handlers derive tenant from the
  run, falling back to `local_implicit`).
- **Change:** require an explicit enrollment record for channel Approve/Reconfigure (drop
  the `Operator` fallback for consequential actions); enforce per-user, server-issued,
  expiring step-up on the approve/reconfigure buttons; introduce a channel→tenant binding;
  pass the resolved channel principal into the gate decision (ties to B1).
- **Acceptance:** an allowlisted-but-unenrolled user cannot approve; button approvals
  require step-up; channel actions are tenant-scoped and attributed.
- **Verification:** `cargo test -p tandem-server channel_approval_tier -- --nocapture`,
  `channel_button_step_up`.

### GOV-B9 — `run_now` / `gate_decide` authorization altitude
- **Root cause:** both gate on read-level `visible_to_context`
  (`routines_automations_parts/part02.rs:1393` and `:2022`), so in hosted mode a view-only
  (`org`-visibility) user can execute/approve a run they cannot edit.
- **Change:** use `ensure_automation_v2_(run_)owner_or_admin` (or a dedicated
  execute/approve permission + capability tier) for `run_now` and `gate_decide`.
- **Acceptance:** a view-only user cannot execute or approve.
- **Verification:** `cargo test -p tandem-server run_now_owner_or_admin -- --nocapture`.

### GOV-B2d — `abort_session` / `cancel_run_by_id` attribution
- **Root cause:** `abort_session` / `cancel_run_by_id`
  (`crates/tandem-server/src/http/sessions.rs:1825/1861`) enforce same-tenant but resolve
  no actor, enforce no human-only, and write no cancel audit.
- **Change:** resolve principal, enforce human-only where appropriate, add cancel audit.
- **Acceptance:** cancellations are attributed and audited.
- **Verification:** `cargo test -p tandem-server session_cancel_audit -- --nocapture`.

---

## Suggested sequencing

```
Phase 0 (P0): GOV-B1  →  GOV-B4  →  GOV-B2a/B2b/B2c   (close the open holes)
Phase 1 (P1): GOV-B3  →  GOV-B10 →  GOV-B6  →  GOV-B7  (fix the trust root + rechecks)
Phase 2 (P2): GOV-B8  →  GOV-B5  →  GOV-B9  →  GOV-B2d (evidence + channel + altitude)
```

B1 first: it is the human-in-the-loop control the product sells, and today an agent can
approve itself with no record. B3 underpins many others but is a behavior/test change, so
sequence it after the open-hole closures so its test churn lands on a stable base.

## Cross-cutting test to add

A single integration test that asserts: for every consequential route, an agent-context
request (forged-source attempt) is either blocked or produces a protected audit event with
a verified non-self decider. This guards against regressions reopening any side door.
