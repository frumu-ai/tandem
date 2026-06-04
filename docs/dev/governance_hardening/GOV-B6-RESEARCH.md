# GOV-B6 — Research: governance recheck on run launch / resume

Status: **research / needs decision** (not implemented)
Owner: Engine / Runtime Authority
Related: `docs/dev/governance_hardening/KANBAN.md` (GOV-B6)

## Problem

A run's governance is evaluated at **create** time and at **gate** time, but the
internal transitions that move a run into execution do **not** re-check governance:

- **Launch** — `claim_specific_automation_v2_run`
  (`crates/tandem-server/src/app/state/app_state_impl_parts/part05.rs:573-665`)
  transitions `Queued → Running` checking only that the runtime context is present
  (`:599-643`); the actual state change is at `:645-664`. No agent-pause / spend-pause
  / capability check.
- **Stale auto-resume** — `auto_resume_stale_reaped_runs`
  (`part05.rs:356-...`) re-queues a `StaleReaped`-paused run based on the resume
  window, an auto-resume attempt cap, and whether repairable nodes remain — but **no**
  governance recheck.
- **Scheduler admission** — `automation/scheduler.rs` `can_admit`/`admit_run` consider
  rate-limit / workspace-lock / capacity only.

**Consequence:** an agent that is paused, spend-capped, or whose capability approval
expired/was revoked *after* its run was queued (or after it was stale-reaped) will still
launch/resume. The spend cap only re-pauses *reactively*, after more spend is recorded.

## What already exists (the model to copy)

`auto_resume_guardrail_stopped_runs` (`part05.rs:459-517`) is the reference pattern: it
collects the agent ids from `automation.creator_id` + `automation.agents[].agent_id`,
reads the governance snapshot once, and only resumes when
`has_approved_agent_quota_override(agent_id)` holds. A launch/resume recheck should
mirror this shape.

### Available governance primitives
On the governance snapshot (`automation_governance.read().await`), via
`crates/tandem-enterprise-contract/src/governance.rs:780-820`:

- `is_agent_paused(agent_id) -> bool` — membership in `paused_agents`.
- `is_agent_spend_paused(agent_id) -> bool` — membership in `spend_paused_agents`.
- `has_approved_agent_quota_override(agent_id, now_ms) -> bool` — approved+unexpired override.
- `has_approved_agent_capability(agent_id, capability_key, now_ms) -> bool` — approved+unexpired capability.

## Key safety property (local / non-enterprise is never blocked)

`is_agent_paused`/`is_agent_spend_paused` are membership checks on pause **sets that are
empty in the OSS/local engine** (those sets are only populated by the premium
`DefaultGovernanceEngine`'s spend/pause evaluation). Absence ⇒ not paused. So a launch-time
recheck built on these accessors is a **no-op for local single-user / non-enterprise**
operation — consistent with the GOV-B10 constraint. This must be preserved and tested.

## Open questions (why this needs a decision before coding)

1. **Hold vs fail on a failed recheck.** If launch finds the agent paused, should the run
   be (a) held in a resumable `Paused` state, (b) re-queued and retried later, or
   (c) failed? Holding is least destructive and matches the existing guardrail-pause model,
   but needs a resume path for each reason (below).
2. **Which `stop_kind` / resume path per reason?**
   - *Spend-paused without override* → set `Paused` + `stop_kind = GuardrailStopped`, which
     the **existing** `auto_resume_guardrail_stopped_runs` already resumes once an override
     is approved. This reuses machinery and is the cleanest.
   - *Agent paused* (`paused_agents`) → there is **no existing auto-resume arm** for "agent
     unpaused". Options: add one, or hold and require manual recovery. Decision needed.
   - *Capability missing/revoked* → overlaps GOV-D1 (no runtime capability-grant flow).
     Likely block terminal or hold; needs the D1 decision.
3. **Capability recheck scope.** A run requires the automation's declared capabilities;
   rechecking means enumerating required capability keys and calling
   `has_approved_agent_capability` for each. Need to confirm where the per-run required keys
   live (`AutomationDeclaredCapabilities` on the spec/metadata) and whether non-escalated
   capabilities even require approval (they may not, in which case this check is narrow).
4. **Which transitions to cover.** Launch (`claim_specific_automation_v2_run`) and
   stale-resume (`auto_resume_stale_reaped_runs`) clearly. Scheduler `can_admit` is a softer
   layer (admission/capacity) — probably leave governance to the claim step to avoid double
   logic. Decision: single chokepoint at claim vs also at admission.
5. **Mid-flight revocation is out of scope.** This item is about the *transition into*
   execution. A capability revoked while a run is already `Running` is a separate concern.
6. **Premium-only behavior + tests.** Because the pause sets are empty in OSS, any
   integration test asserting a *hold* must run under `--features premium-governance` and
   first drive the agent into a paused/spend-capped state; the OSS no-op path needs its own
   test. Mirror the GOV-B10 test split.

## Proposed design (for review — not yet implemented)

Add a single read-only helper:

```rust
enum RunLaunchDecision {
    Launch,
    HoldGuardrail,   // spend-capped, no override -> Paused + GuardrailStopped (existing resume)
    HoldAgentPaused, // agent paused -> Paused + (new) AgentPaused stop_kind
    Block(String),   // capability missing/revoked -> terminal or hold (gated on D1)
}

// reads the governance snapshot once; collects creator_id + agents[].agent_id
fn governance_run_launch_decision(&self, automation: &AutomationV2Spec) -> RunLaunchDecision
```

- Call it in `claim_specific_automation_v2_run` immediately before the `Queued → Running`
  transition (`part05.rs:645`), where `automation_for_context` (with the agent ids) is
  already in scope, and in `auto_resume_stale_reaped_runs` before re-queueing.
- `Launch` ⇒ proceed. Non-`Launch` ⇒ write the appropriate `Paused`/`stop_kind` +
  lifecycle event (and protected audit per GOV-B8) instead of running.
- Reuse `auto_resume_guardrail_stopped_runs` for the spend case; decide the agent-paused
  resume arm; gate the capability case on GOV-D1.

## Recommendation

Split GOV-B6 into:
- **B6a (low-risk, high-value):** spend-pause recheck at launch/stale-resume → `Paused +
  GuardrailStopped`, reusing the existing override auto-resume. No new resume machinery,
  no local impact, closes the "spend-capped agent still launches" gap.
- **B6b:** agent-pause recheck + a matching auto-resume arm (needs the hold-vs-fail and
  resume-trigger decision).
- **B6c:** capability recheck — fold into / sequence after GOV-D1.

---

## Analysis & decision (added after reviewing how each pause set is populated)

A closer read of *how the two pause sets are written* resolves most of the open questions
and points to a single best route.

### The two pause sets have different semantics

- `spend_paused_agents` — populated by the premium spend evaluation
  (`crates/tandem-governance-engine/src/lib.rs:879`,
  `crates/tandem-server/src/app/state/governance.rs:1669`). This is a genuine
  **execution/spend** stop: the agent has exhausted its budget, so launching a queued run
  would burn more. Rechecking this at launch is **correct and well-defined**.
- `paused_agents` — populated **only** by `pause_automation_creation_for_agent`
  (`crates/tandem-server/src/app/state/governance.rs:497-510`), driven by the
  creation-review threshold. Despite the generic name, `is_agent_paused` means **"this agent
  may not CREATE new automations"** — a creation-time gate consumed on the create path,
  **not** a run-execution stop. Halting an already-queued run on it would be a semantic
  mismatch: the automation already exists and was already cleared to run.
- There is **no `AgentPaused` `AutomationStopKind`** (variants are `Cancelled`,
  `OperatorStopped`, `GuardrailStopped`, `Panic`, `Shutdown`, `ServerRestart`,
  `StaleReaped`) and **no unpause→resume reaction** anywhere, so blocking on `paused_agents`
  would also require inventing both a stop-kind and a resume sweep.

### Conclusion — best route

**Implement B6a only; scope `paused_agents` (creation pause) OUT of the launch recheck;
defer capability to D1.**

- **B6a — spend-pause recheck (recommended, implement now).** A shared read-only helper
  called from `claim_specific_automation_v2_run` (immediately before the `Queued → Running`
  transition at `part05.rs:645`) and from `auto_resume_stale_reaped_runs` (before
  re-queueing): collect the agent ids; if any is `is_agent_spend_paused` **and** lacks an
  approved `has_approved_agent_quota_override`, hold the run as `Paused + GuardrailStopped`
  (with a `pause_reason`/`detail` distinguishing "held at launch" from "stopped mid-run")
  plus a protected audit event. This **reuses** the existing
  `auto_resume_guardrail_stopped_runs` sweep verbatim — **no new `stop_kind`, no new resume
  arm** — and is a **no-op in OSS/local** (the set is empty there), satisfying the
  local-safety constraint. This answers Q1/Q2 for the spend case: **hold**, via the existing
  `GuardrailStopped` path.
- **Creation pause (`paused_agents`) — explicitly out of scope for B6.** It is a creation
  gate; execution of an already-created automation is governed by spend and capability. (If
  product later decides creation-review should be a *hard stop on all agent activity*, that
  is a separate policy item with its own stop-kind + resume sweep — not a hardening fix, and
  it should not ride in under B6.)
- **B6c — capability recheck — defer to GOV-D1.** Most declared capabilities have no
  approval record, so a naive `has_approved_agent_capability` recheck would false-positive;
  it needs D1's "which capabilities require a runtime grant + which keys a run requires"
  model first.

### Why hold (not fail) for spend

The run has not started, so `Paused` is the accurate state, and an approved quota override
then resumes it automatically through machinery that **already exists**. Failing would
discard queued work and force a manual re-trigger for a transient, operator-resolvable
condition.

### Test plan for B6a

- **Premium** (`--features premium-governance`): drive an agent into `spend_paused_agents`,
  enqueue a run, claim it → assert it does **not** reach `Running` but `Paused +
  GuardrailStopped`; approve a quota override → assert the existing sweep resumes it and a
  subsequent claim launches it.
- **OSS** (default build): with no governance state, assert a queued run claims to `Running`
  normally (the recheck is a no-op) — guarding the local-safety invariant.

### Residual risk / notes

- Minor TOCTOU: the governance snapshot is read under `automation_governance.read()` and the
  transition takes `automation_v2_runs.write()` separately; an agent paused in that window is
  caught on the next sweep/claim. Acceptable.
- Keep the recheck independent of `TANDEM_DISABLE_STALE_AUTO_RESUME` (that switch governs
  stale resume only, not governance).
- Open product question (does **not** block B6a): should a creation-review pause also halt
  in-flight/queued execution? Default answer here is **no** (separate item if reversed).

Implement B6a first once the hold-state decision in Q1/Q2 is confirmed; B6b/B6c after the
D1 direction is set.
