# Governance Enforcement Hardening Kanban

Last updated: 2026-06-02
Owner: Engine / Runtime Authority
Goal: `docs/internal/governance_hardening/GOAL.md`
Source plan: `docs/internal/governance_hardening/GOVERNANCE_ENFORCEMENT_HARDENING_PLAN.md`

## Status Legend
- `todo`
- `in_progress`
- `blocked`
- `done`

## Priority Legend
- P0: human-in-the-loop / governance bypassable in ANY deployment mode. Must land first.
- P1: defeats governance in the default runtime mode, or resumes governed work without recheck.
- P2: audit/attribution gaps, channel tier defaults, authorization-altitude.

---

## Now

- `GOV-B1` Gate-decision endpoint: require verified human, self-approval guard, audit decider
  - Status: `done`
  - Priority: P0
  - Scope: `automations_v2_run_gate_decide` resumed an `AwaitingApproval` run with no actor, no human requirement, no self-approval guard, no attributable audit.
  - Acceptance: agent-context approval of its own run is rejected; decider must be a verified Human (or channel-verified Approve tier) and not the run's executing agent; access guard is owner/admin; gate record + protected audit both carry `decided_by`.
  - Progress: split the axum handler into `automations_v2_run_gate_decide` (resolves the governance actor from the request principal) and a shared `automations_v2_run_gate_decide_inner`; the inner rejects any non-Human decider with `AUTOMATION_V2_GATE_REQUIRES_HUMAN` (this is the self-approval guard — a governed run executes as an agent, so requiring a human means the executing agent cannot approve itself), upgrades the access check from read-visibility to `owner_or_admin`, threads a `decided_by: GovernanceActorRef` into `AutomationGateDecisionRecord`, and writes a protected `automation.governance.gate_decided` audit event. The three channel interaction handlers (Slack/Discord/Telegram) now call the inner with a channel-verified human decider (they already enforce signature + allowlist + Approve tier upstream).
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part02.rs` (`automations_v2_run_gate_decide` + new `_inner`); `crates/tandem-server/src/app/state/automation/gates.rs` (`apply_automation_gate_decision` takes `decided_by`); `crates/tandem-server/src/automation_v2/types.rs` (`AutomationGateDecisionRecord.decided_by`); `crates/tandem-server/src/http/{slack,discord,telegram}_interactions.rs`.
  - Verification: `cargo test -p tandem-server gate_decision_rejects_agent_context_caller -- --nocapture` and `gate_decision_records_human_decider` (both pass); regression slice `automations_v2_gate gate_rework approvals_aggregator slack_ discord_ telegram_` = 35 passed / 0 failed.

- `GOV-B4` Enforce human-only + self-approval on approval pipeline
  - Status: `done` (governance approval pipeline); routine/coder human-only folded into GOV-B2a/b
  - Priority: P0
  - Scope: `governance_approval_approve`/`deny` omitted the `kind==Human` check; `decide_approval_request` had no self-approval guard.
  - Acceptance: agent-origin approval forbidden; an agent-filed request cannot be reviewed by the same agent identity.
  - Progress: added `ensure_governance_review_authorized` to both `governance_approval_approve` and `_deny` — rejects non-human reviewers (`GOVERNANCE_APPROVAL_REQUIRES_HUMAN`) and self-review of agent-filed requests (`GOVERNANCE_APPROVAL_SELF_REVIEW`). Added an authoritative self-review guard inside the engine `decide_approval_request` so no caller path can self-approve, plus a `get_governance_approval_request` accessor. The self-review guard is scoped to agent-filed requests so the legitimate human-operator "file-on-behalf-of-agent then approve" workflow is preserved. The routine/coder approve/deny/pause/cancel human-only enforcement is deferred into GOV-B2a/B2b, where those endpoints are routed through governance.
  - Files: `crates/tandem-server/src/http/routes_governance.rs` (`ensure_governance_review_authorized` + both handlers); `crates/tandem-governance-engine/src/lib.rs` (`decide_approval_request` self-review guard); `crates/tandem-server/src/app/state/governance.rs` (`get_governance_approval_request`).
  - Verification: `cargo test -p tandem-server --features premium-governance governance_approval_approve_rejects_agent_reviewer`, `governance_approval_rejects_agent_self_review` (both pass); full `governance::` module = 14 passed / 0 failed (operator create+approve workflow preserved).

- `GOV-B2a` Coder runs through governance
  - Status: `todo`
  - Priority: P0
  - Scope: `coder_run_create`/`execute_all`/`approve`/`cancel` resolve no actor and call no governance; an agent blocked on V2 create/run uses the coder endpoints instead.
  - Acceptance: agent-context coder create/run is gated identically to V2; approve/cancel human-only; allow+deny audited.
  - Files: `crates/tandem-server/src/http/coder_parts/part06.rs:1130`; `part07.rs:279,419,620`.
  - Verification: `cargo test -p tandem-server coder_run_governance -- --nocapture`.

- `GOV-B2b` Routines + v1 wrappers through governance
  - Status: `todo`
  - Priority: P0
  - Scope: `routines_create` (`// TODO: SECURITY`), `routines_run_now`, and v1 `automations_run_*` never consult governance; approval flags are client-set body fields.
  - Acceptance: create/run gated like V2; approve/deny/pause/resume human-only; audited.
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part01.rs:827,983,1242,1312,1382,1467`; `part02.rs:476,589`.
  - Verification: `cargo test -p tandem-server routines_governance -- --nocapture`.

- `GOV-B2c` Channel automation draft confirm through creation governance
  - Status: `done`
  - Priority: P0
  - Scope: `channel_automation_drafts_confirm` called `put_automation_v2` directly, skipping `can_create_automation_for_actor`; no principal.
  - Acceptance: channel-draft create runs creation governance with a resolved principal and is audited; agent-context confirm is rejected.
  - Progress: `channel_automation_drafts_confirm` now takes `Extension<RequestPrincipal>` + `HeaderMap`, resolves provenance via `resolve_governance_provenance`, derives declared capabilities from the built automation metadata, calls `can_create_automation_for_actor` (mapping denials through `governance_error_response`) before `put_automation_v2`, records governance provenance, and writes an `automation.governance.created` protected audit event tagged `origin: channel_draft`. An agent-context confirm is now refused instead of silently provisioning an automation.
  - Files: `crates/tandem-server/src/http/channel_automation_drafts.rs` (`channel_automation_drafts_confirm`).
  - Verification: `cargo test -p tandem-server channel_automation_draft_confirm_rejects_agent_context` (non-premium build: agent confirm returns NOT_IMPLEMENTED and the draft stays unapplied); existing `channel_automation_draft_*` suite = 5 passed / 0 failed.
  - Note: `start`/`answer` collect draft state only and do not create the automation, so creation governance is enforced at `confirm`; no separate gating needed there.

## Next

- `GOV-B3` Non-forgeable actor (human/agent) classification
  - Status: `todo`
  - Priority: P1
  - Scope: in `LocalSingleTenant`, Human-ness is derived from the unsigned `x-tandem-request-source` header, defeating all agent gates.
  - Acceptance: a caller cannot self-elevate to Human via headers alone in local mode; hosted/enterprise JWS path unchanged; the test `automations_v2_create_and_run_now_treat_control_panel_source_as_human` is updated to the secure behavior.
  - Files: `crates/tandem-server/src/http/governance.rs:27-126`; `crates/tandem-server/src/http/middleware.rs:392-411`.
  - Verification: `cargo test -p tandem-server governance_actor -- --nocapture` plus `control_panel_source_requires_local_token`.

- `GOV-B10` OSS ownership check + enterprise admin header fallback
  - Status: `todo`
  - Priority: P1
  - Scope: OSS "Human ⇒ allow" has no owner/grant check; latent enterprise-admin fallback grants admin from a header source absent verified context.
  - Acceptance: a human who is neither owner nor grant-holder cannot mutate another's automation; enterprise mutation under local mode is impossible or token+role gated.
  - Files: `crates/tandem-server/src/app/state/governance.rs:17-66,554-570`; `crates/tandem-enterprise-server/src/http/routes_enterprise.rs:1140`.
  - Verification: `cargo test -p tandem-server oss_human_ownership -- --nocapture`; `cargo test -p tandem-enterprise-server admin_fallback -- --nocapture`.

- `GOV-B6` Governance recheck on launch + stale auto-resume
  - Status: `todo`
  - Priority: P1
  - Scope: launch and stale auto-resume transition runs to Running without rechecking agent pause / spend pause / capability approval.
  - Acceptance: a paused or capability-revoked agent's queued/reaped run does not launch/resume; the block is recorded.
  - Files: `crates/tandem-server/src/app/state/app_state_impl_parts/part05.rs:573,356`; `crates/tandem-server/src/app/state/automation/scheduler.rs:150,203`.
  - Verification: `cargo test -p tandem-server launch_governance_recheck -- --nocapture`, `stale_resume_governance_recheck`.

- `GOV-B7` Govern + audit `automations_v2_share`
  - Status: `todo`
  - Priority: P1
  - Scope: visibility/share mutation has no governance call and no audit; can flip private → org-wide.
  - Acceptance: non-authorized/agent actor cannot widen visibility; change is audited.
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part02.rs:1300`.
  - Verification: `cargo test -p tandem-server automation_share_governance -- --nocapture`.

## Backlog

### P2 - Evidence / Attribution
- `GOV-B8` Protected audit on deny paths + internal sweeps with system actor
  - Status: `todo`
  - Priority: P2
  - Scope: V2 deny paths emit no audit; internal sweeps (reaper, auto-resume, recover, shutdown-fail) write only run-local lifecycle history with no system-actor attribution.
  - Acceptance: every allow AND deny of a consequential action, including internal transitions, produces an attributable protected audit record.
  - Files: `crates/tandem-server/src/app/state/automation/lifecycle.rs:7-16`; sweep sites in `app_state_impl_parts/part03.rs`, `part05.rs`, `automation_v2/executor.rs`; deny paths across `routines_automations_parts/part02.rs`.
  - Verification: `cargo test -p tandem-server governance_deny_audit -- --nocapture`, `internal_sweep_audit_attribution`.

### P2 - Channel Authority
- `GOV-B5` Channel Approve-by-default fallback + button step-up + tenant binding
  - Status: `todo`
  - Priority: P2
  - Scope: unenrolled allowlisted users default to `Operator` → `Reconfigure` ≥ `Approve`; PIN step-up is a global env var and only on slash commands, not approve buttons; channels not tenant-bound.
  - Acceptance: allowlisted-but-unenrolled user cannot approve; button approvals require per-user expiring step-up; channel actions tenant-scoped and attributed.
  - Files: `crates/tandem-server/src/app/state/channel_user_capabilities.rs:185-209`; `crates/tandem-channels/src/channel_registry.rs:119-124`, `config.rs:28`; `crates/tandem-channels/src/dispatcher_parts/part03.rs:1060-1083`; `*_interactions.rs`.
  - Verification: `cargo test -p tandem-server channel_approval_tier -- --nocapture`, `channel_button_step_up`.

### P2 - Authorization Altitude
- `GOV-B9` `run_now` / `gate_decide` require owner/admin (not read-visibility)
  - Status: `todo`
  - Priority: P2
  - Scope: both gate on read-level `visible_to_context`, letting a view-only user execute/approve a run they cannot edit.
  - Acceptance: a view-only (`org`-visibility) user cannot execute or approve.
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part02.rs:1393,2022`.
  - Verification: `cargo test -p tandem-server run_now_owner_or_admin -- --nocapture`.

- `GOV-B2d` Attribution + audit for `abort_session` / `cancel_run_by_id`
  - Status: `todo`
  - Priority: P2
  - Scope: session/run cancel enforces same-tenant but resolves no actor, enforces no human-only, writes no cancel audit.
  - Acceptance: cancellations are attributed and audited.
  - Files: `crates/tandem-server/src/http/sessions.rs:1825,1861`.
  - Verification: `cargo test -p tandem-server session_cancel_audit -- --nocapture`.

### Cross-cutting
- `GOV-X1` Consequential-route regression guard
  - Status: `todo`
  - Priority: P1
  - Scope: one integration test asserting that for every consequential route, a forged-source agent request is either blocked or produces a protected audit event with a verified non-self decider.
  - Acceptance: adding a new mutation route without governance fails this test.
  - Verification: `cargo test -p tandem-server consequential_routes_enforce_governance -- --nocapture`.
