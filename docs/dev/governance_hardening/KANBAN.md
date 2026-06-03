# Governance Enforcement Hardening Kanban

Last updated: 2026-06-02
Owner: Engine / Runtime Authority
Goal: `docs/dev/governance_hardening/GOAL.md`
Source plan: `docs/dev/governance_hardening/GOVERNANCE_ENFORCEMENT_HARDENING_PLAN.md`

## Status Legend
- `todo`
- `in_progress`
- `research` (investigated; needs a decision before implementation)
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
  - Status: `done`
  - Priority: P0
  - Scope: `coder_run_create`/`execute_all`/`approve`/`cancel` resolved no actor and called no governance; an agent blocked on V2 create/run could use the coder endpoints instead.
  - Acceptance: agent-context coder create/execute/approve/cancel over HTTP is refused; internal system-initiated follow-on runs are unaffected.
  - Progress: added `ensure_coder_human_actor` (resolves the governance actor from the verified principal; rejects non-human callers with `FORBIDDEN`) and applied it to the `coder_run_create` HTTP handler and `coder_run_execute_all`/`approve`/`cancel`. The two internal auto-spawn/follow-on callers in `part05.rs` were repointed from the (now-gated) `coder_run_create` HTTP handler to `coder_run_create_inner`, so system-initiated follow-on runs within an already-governed parent run keep working without a human gate. Rationale matches B2b: coder runs have no per-run agent-governance record, so the HTTP path is human-only; agents needing governed autonomous work use Automations V2.
  - Files: `crates/tandem-server/src/http/coder_parts/part06.rs` (`ensure_coder_human_actor`, `coder_run_create`); `part07.rs` (`coder_run_execute_all`/`approve`/`cancel`); `part05.rs` (two internal callers → `coder_run_create_inner`).
  - Verification: `cargo test -p tandem-server coder_run_create_rejects_agent_context` (pass) + `coder_merge_recommendation_execute_all_runs_to_completion` (pass, exercises the rerouted auto-spawn path); full `coder::` module = 101 passed / 2 failed, where the 2 failures (`issue_fix_handoff_commits_and_pushes_worker_branch`, `coder_issue_fix_worker_uses_managed_worktree_for_git_repo`) are PRE-EXISTING environmental git-push/worktree failures — confirmed identical on a stashed clean tree (origin/main), unrelated to this change.

- `GOV-B2b` Routines + v1 wrappers through governance
  - Status: `done`
  - Priority: P0
  - Scope: `routines_create` (`// TODO: SECURITY`), `routines_run_now`, decision handlers, and v1 `automations_run_*` resolved no actor and consulted no governance; approval flags + creator fields were client-set.
  - Acceptance: create/run/approve/deny/pause/resume require a verified human; agent-context routine work is refused; actions audited with the resolved actor.
  - Progress: added `ensure_routine_human_actor` (resolves the governance actor from the verified principal and rejects non-human callers with `ROUTINE_REQUIRES_HUMAN`). Wired it into `routines_create` (also derives `creator_id`/`creator_type` from the actor instead of the body, replacing the `TODO: SECURITY`), `routines_run_now`, and `routines_run_approve/deny/pause/resume`; threaded the resolved actor + real tenant into their protected audit events and added a previously-missing audit event for resume. The five v1 wrappers (`automations_run_now/approve/deny/pause/resume`) now extract and forward `RequestPrincipal`/tenant/headers. Rationale: routines have no per-routine governance/approval record, so agent-authored routine work is fail-closed — agents needing governed autonomous work must use Automations V2 (which carries the capability/approval flow).
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part01.rs` (`ensure_routine_human_actor`, `routines_create`, `routines_run_now`, `routines_run_approve/deny/pause/resume`); `part02.rs` (the five `automations_run_*` wrappers).
  - Verification: `cargo test -p tandem-server routines_reject_agent_context_create_and_run`; full `routines::` suite (state + http, incl. operator-wrapper tests) green / 0 failed.

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
  - Status: `done`
  - Priority: P1
  - Scope: an explicit `x-tandem-agent-id` could be overridden to Human by a forged `x-tandem-request-source: control_panel`, laundering an agent past every human/agent gate (worsened by the default source resolving to `local_control_panel`).
  - Acceptance: an explicit agent identity always classifies as Agent regardless of source; a genuine control-panel request (no agent identity) is still Human; hosted/enterprise JWS path unchanged.
  - Progress: reordered `resolve_governance_actor` and `resolve_governance_provenance` so the `x-tandem-agent-id` header is honored **before** the `control_panel`→human shortcut — an explicit agent identity is now authoritative and cannot be upgraded to a human by any (forgeable) source string. Rewrote the unit test that codified the forgery (`control_panel_request_source_is_human_even_with_agent_header` → `agent_header_is_not_overridden_by_control_panel_source`, asserting Agent) and added `control_panel_source_without_agent_header_is_human`. Updated the integration test `automations_v2_create_and_run_now_treat_control_panel_source_as_human` to drop the spoofed agent header so it tests the genuine human control-panel path.
  - Files: `crates/tandem-server/src/http/governance.rs` (`resolve_governance_actor`, `resolve_governance_provenance`, unit tests); `crates/tandem-server/src/http/tests/governance.rs` (integration test).
  - Verification: new unit tests + agent-rejection regression across gate/routines/coder/channel = 6 passed / 0 failed; `governance::` module with `--features premium-governance` = 15 passed / 0 failed. (The non-premium governance lifecycle tests require `--features premium-governance` to run at all — unrelated to this change.)
  - Residual (deferred): the remaining gap — an anonymous local caller with no agent header claiming `control_panel` — is the inherent `LocalSingleTenant` trust model. Fully closing it requires binding the `control_panel`→human classification to a trusted local transport token (loopback + token), which is a deployment-posture decision tracked under GOV-B10 / the auth-mode hardening rather than this item.

- `GOV-B10` OSS ownership check (+ enterprise admin header fallback deferred)
  - Status: `done` (part a — OSS ownership); part b (enterprise admin fallback) deferred — see note
  - Priority: P1
  - Scope: OSS "Human ⇒ allow" had no owner check, so any human could mutate any automation.
  - Acceptance: a distinct identified human cannot mutate another's automation; **non-enterprise local single-user operation is never blocked** (per the explicit constraint).
  - Progress: `UnavailableGovernanceEngine::authorize_mutation` (the OSS/non-premium engine only) now, for a human actor, denies (`AUTOMATION_V2_NOT_OWNER`) when the record has a DISTINCT IDENTIFIED human owner — i.e. both the record's creator and the acting actor carry actor_ids and they differ. It is intentionally a **no-op for local single-user**: the local control-panel user is anonymous (no actor_id) and locally-created records carry no identified owner, so there is nothing to compare and the mutation is allowed. The check only engages when two distinct human identities are present. Premium (`DefaultGovernanceEngine`) is unaffected (it has its own ownership/grant logic).
  - Files: `crates/tandem-server/src/app/state/governance.rs` (`UnavailableGovernanceEngine::authorize_mutation`).
  - Verification (non-premium build): `oss_mutation_denied_for_distinct_identified_non_owner` (bob cannot share alice's automation → 403 `AUTOMATION_V2_NOT_OWNER`; alice can), `oss_local_anonymous_mutation_is_allowed` (local anonymous create+share → 200). Premium `governance::` module unregressed = 15 passed / 0 failed; local v2 lifecycle/recover/repair/run-task suite unaffected.
  - Deferred (part b — enterprise admin header fallback): `enterprise_admin_allowed_for_mutation` grants admin from a `request_principal.source` string when no verified context is present. This is reachable only in `LocalSingleTenant` (hosted/enterprise modes always carry a verified JWS context), and it is the affordance that lets a local-enterprise dev operator act as admin without configuring JWS — so removing it risks breaking legitimate local-enterprise operation. Closing the agent-forgery angle cleanly requires threading agent-context into 22 `require_enterprise_admin` call sites; deferred to avoid destabilizing local operation, tracked as a follow-up. Non-enterprise local users never reach these routes.

- `GOV-B6` Governance recheck on launch + stale auto-resume
  - Status: `done` (B6a — spend-pause recheck implemented & tested; creation-pause out of scope; capability recheck → GOV-D1)
  - Priority: P1
  - Scope: launch (`claim_specific_automation_v2_run`) and stale auto-resume transition runs to Running without rechecking agent spend pause / capability approval.
  - Acceptance: a spend-capped agent's queued/reaped run does not launch/resume; the hold is recorded; resumes on override approval; **non-enterprise local single-user is never blocked**.
  - Research: full findings + decided route in `docs/dev/governance_hardening/GOV-B6-RESEARCH.md`. Decisive finding: the two pause sets have **different semantics** — `spend_paused_agents` is a real execution/spend stop, while `paused_agents` is populated **only** by `pause_automation_creation_for_agent` (a creation-review gate, *not* a run-execution stop). There is no `AgentPaused` stop-kind and no unpause→resume reaction.
  - **Decided best route — implement B6a only:** recheck `is_agent_spend_paused` (minus `has_approved_agent_quota_override`) at launch (before the `Queued→Running` transition at `part05.rs:645`) and at `auto_resume_stale_reaped_runs`; on a hit, **hold** the run as `Paused + GuardrailStopped`, which the existing `auto_resume_guardrail_stopped_runs` sweep already resumes on override approval (no new stop-kind, no new resume arm). No-op in OSS/local (the set is empty). Creation-pause (`paused_agents`) is **out of scope** (creation gate, not execution); capability recheck deferred to **GOV-D1**.
  - Files (B6a): `crates/tandem-server/src/app/state/app_state_impl_parts/part05.rs` — new `run_launch_blocked_by_spend_pause` helper; called in `claim_specific_automation_v2_run` (before the `Queued→Running` transition; on a hit holds the run as `Paused + GuardrailStopped` + `run_launch_held` lifecycle event) and in `auto_resume_stale_reaped_runs` (skips re-queueing a spend-paused run).
  - Implemented: launch + stale-resume now recheck `is_agent_spend_paused` (minus `has_approved_agent_quota_override`); a held run is resumed by the existing `auto_resume_guardrail_stopped_runs` sweep once an override is approved. No new stop-kind / resume arm. No-op in OSS (`spend_paused_agents` empty).
  - Verification: premium `spend_capped_agent_run_is_held_at_launch_and_resumes_after_override` (queued run held at claim, then launches after override) and OSS `run_launches_normally_without_governance_state` (no-op) both pass; premium `governance::` suite 16/0; default run-lifecycle (`run_recover`/`run_repair`/`run_task`) unregressed.
  - Out of scope / follow-ups: creation-pause (`paused_agents`) is a creation gate, not an execution stop (excluded by design); capability recheck deferred to GOV-D1.

- `GOV-B7` Govern + audit `automations_v2_share`
  - Status: `done`
  - Priority: P1
  - Scope: visibility/share mutation had no governance call and no audit; could flip private → org-wide.
  - Acceptance: non-authorized/agent actor cannot widen visibility; change is audited.
  - Progress: `automations_v2_share` now takes `Extension<RequestPrincipal>` + `HeaderMap`, resolves the governance actor, bootstraps the governance record via `get_or_bootstrap_automation_governance`, and runs `can_mutate_automation` (which rejects agent-context callers) before applying the share metadata — mirroring the `automations_v2_delete` governance pattern. After persisting it writes an `automation.governance.shared` protected audit event recording the actor and resulting visibility.
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part02.rs` (`automations_v2_share`).
  - Verification: `cargo test -p tandem-server automation_v2_share_is_governed` (human owner widens to org → 200; agent-context share → rejected). No prior test exercised the v2 share endpoint, so no regression surface.

## Backlog

### P2 - Evidence / Attribution
- `GOV-B8` Protected audit on deny paths + internal sweeps with system actor
  - Status: `done` (B8a — deny-path audit; B8b internal-sweep system-actor audit deferred — see note)
  - Priority: P2
  - Scope: V2 deny paths emitted no audit; internal sweeps write only run-local lifecycle history with no system-actor attribution.
  - Acceptance: every deny of a consequential mutation produces an attributable protected audit record.
  - Progress (B8a): new `enforce_mutation_or_audit` helper in `http/governance.rs` wraps a `can_mutate_automation` result and, on denial, writes an attributed `automation.governance.denied` protected audit event (actor, automation id, code, detail) before returning the HTTP error. Applied to all six consequential `can_mutate_automation` deny sites (patch / run_now / share / delete / pause / resume-class handlers) in `routines_automations_parts/part02.rs`.
  - Files: `crates/tandem-server/src/http/governance.rs` (`enforce_mutation_or_audit`); `crates/tandem-server/src/http/routines_automations_parts/part02.rs` (six call sites).
  - Verification: `governance_denial_writes_protected_audit` (agent share denied → `automation.governance.denied` in the protected audit log with actor + automation id); allow paths unregressed — premium `governance::` suite 16/0, share/run_now/X1 green.
  - Deferred (B8b): system-actor protected audit for internal sweeps (reaper, auto-resume, recover, shutdown-fail) — these already write run-local lifecycle history; adding a parallel system-actor protected-audit stream is a larger, lower-security-value change (internal infra transitions, not external actor decisions). The create-path deny (`authorize_create`, part02.rs:1084) can also adopt `enforce_*`-style auditing in the same follow-up. Tracked as B8b.

### P2 - Channel Authority
- `GOV-B5` Channel Approve-by-default fallback + button step-up + tenant binding
  - Status: `in_progress` (B5a done — approve-by-default closed; B5b step-up + B5c tenant binding remain)
  - Priority: P2
  - Decision (D-B5.1, from owner): approval is granular per-identity, separate from the comms allowlist. A hand-picked (non-wildcard) allowlist is a deliberate identity-trust decision, so those users keep approval; a wildcard-open (`*`) channel does NOT confer approval — that requires an explicit per-identity grant. Never lock out solo/hand-picked setups.
  - Progress (B5a): `channel_user_can_approve` now (1) honors an explicit per-identity capability grant as authoritative (including a deliberate downgrade below Approve), and (2) for an ungranted user, falls back to the channel security profile ONLY on a non-open channel — on a wildcard-open channel it returns false. New `channel_is_open_to_all` helper detects `*` in `allowed_users`; threaded into all four interaction approve checks (slack/telegram/discord). Behavior changes ONLY for `*`-open Operator-profile channels (the actual hole); hand-picked allowlists, solo operators, and other profiles are unaffected.
  - Files (B5a): `crates/tandem-server/src/app/state/principals/channel_identity.rs` (`channel_is_open_to_all`); `channel_user_capabilities.rs` (`channel_user_can_approve`); `slack_interactions.rs`, `telegram_interactions.rs`, `discord_interactions.rs`.
  - Verification (B5a): `open_channel_denies_approval_without_explicit_grant`, `explicit_grant_approves_even_on_open_channel`; existing channel-cap + 16 interaction-handler tests green.
  - Scope: unenrolled allowlisted users default to `Operator` → `Reconfigure` ≥ `Approve`; PIN step-up is a global env var and only on slash commands, not approve buttons; channels not tenant-bound.
  - Acceptance: allowlisted-but-unenrolled user cannot approve; button approvals require per-user expiring step-up; channel actions tenant-scoped and attributed.
  - Research: full findings + options + decisions in `docs/dev/governance_hardening/GOV-B5-RESEARCH.md`. Confirmed: `command_tier_for_profile(Operator) = Reconfigure` (the max tier) and `Operator` is the default profile, so any unenrolled user on a default-profile channel can approve/reconfigure; the step-up PIN is a global env var (not per-user), slash-only (buttons bypass it), and `Reconfigure`-only; channel capability records key on `{channel,user}` with no tenant.
  - Blocking decisions: **D-B5.1** unenrolled fallback tier + solo-operator bootstrap (changing it risks breaking single-operator channels — same local-safety constraint as B10/B6); **D-B5.2** per-user expiring step-up token model + apply to buttons + which tiers; **D-B5.3** channel→tenant binding model + migration.
  - Recommended phasing: **B5a** cap unenrolled fallback below Approve (with solo bootstrap) + protected audit on every channel Approve/Reconfigure; **B5b** per-user expiring step-up token applied to buttons; **B5c** tenant binding + migration.
  - Files (for implementation): `crates/tandem-server/src/app/state/channel_user_capabilities.rs`; `crates/tandem-channels/src/dispatcher_parts/part03.rs`, `*_interactions.rs`, `channel_registry.rs`, `config.rs`.

### P2 - Authorization Altitude
- `GOV-B9` `run_now` / `gate_decide` require owner/admin (not read-visibility)
  - Status: `done`
  - Priority: P2
  - Scope: both gated on read-level `visible_to_context`, letting a view-only user execute/approve a run they cannot edit.
  - Acceptance: a view-only (`org`-visibility) user cannot execute or approve.
  - Progress: `automations_v2_run_now` now calls `ensure_automation_v2_owner_or_admin` instead of `ensure_automation_v2_visible_to_context`, matching patch/delete/share. `gate_decide` (`automations_v2_run_gate_decide_inner`) was already switched to `ensure_automation_v2_run_owner_or_admin` under GOV-B1, so no further change there.
  - Files: `crates/tandem-server/src/http/routines_automations_parts/part02.rs` (`automations_v2_run_now`).
  - Verification: `run_now_allowed_for_local_human_and_refused_for_agent` (local human runs → 200; agent-context → refused). Note: the owner/admin **denial** branch requires a verified (signed-assertion) context that the HTTP integration suite has no infrastructure for — no handler has an `AUTOMATION_V2_ACCESS_DENIED` test repo-wide. `run_now` now shares the identical, already-trusted helper as patch/delete/share, so the denial behavior is covered transitively; the local-safety invariant (no verified context ⇒ owner/admin is a no-op) is tested directly.

- `GOV-B2d` Attribution + audit for `abort_session` / `cancel_run_by_id`
  - Status: `done`
  - Priority: P2
  - Scope: session/run cancel enforced same-tenant but resolved no actor and wrote no cancel audit.
  - Acceptance: cancellations are attributed and audited.
  - Progress: both `abort_session` and `cancel_run_by_id` now take `Extension<RequestPrincipal>` + `HeaderMap`, resolve the governance actor, and write a protected audit event (`session.aborted` / `session.run.cancelled`) attributing the actor. No human-only gate is imposed — cancellation is a de-escalation/stop, and blocking an agent from stopping its own run would be harmful — but it is now attributed and tamper-evidently recorded.
  - Files: `crates/tandem-server/src/http/sessions.rs` (`abort_session`, `cancel_run_by_id`).
  - Verification: `abort_session_writes_attributed_protected_audit` (abort with `x-tandem-actor-id` → `session.aborted` event in the protected audit log carrying the actor + session id).

### Cross-cutting
- `GOV-X1` Consequential-route regression guard
  - Status: `done`
  - Priority: P1
  - Scope: one integration test asserting that consequential mutation routes refuse a forged agent-context request.
  - Acceptance: adding a new mutation route without governance fails this test.
  - Progress: `consequential_routes_refuse_agent_context` table-drives an agent-context (`x-tandem-request-source: agent` + `x-tandem-agent-id`) request at create / run_now / share / patch / delete and asserts each is non-success. Gated to the OSS build, where the `UnavailableGovernanceEngine` uniformly refuses agent mutations, so the guard is deterministic. A new ungoverned mutation route added to this list (or that bypasses governance) fails the test.
  - Files: `crates/tandem-server/src/http/tests/global_parts/part03.rs`.
  - Verification: `cargo test -p tandem-server consequential_routes_refuse_agent_context` passes (all five routes refuse).
  - Follow-up: extend the case list as new consequential routes are added (gate_decide/routines/coder/channel-draft already have dedicated agent-rejection tests under B1/B2).
