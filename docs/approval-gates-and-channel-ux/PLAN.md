# Plan: Default Approval Gates + Rich Channel UX

> **Note on location.** Plan-mode constraints require this file to live at `~/.claude/plans/melodic-weaving-parnas.md`. The user asked for the plan to live under `docs/internal/`. **First step of execution after approval: copy this file to `docs/internal/approval-gates-and-channel-ux/PLAN.md` and create a companion `README.md` linking from `docs/internal/business/` and `docs/internal/demo/`.** All subsequent edits happen in the `docs/internal/` copy; this file becomes a frozen snapshot of approved scope.

---

## Context

Tandem's pitch is governed AI operations: agents own workflows, runtime governs. The runtime today has approval primitives in code (`HumanApprovalGate`, `AutomationPendingGate`, `awaiting_gate` checkpoints, `automations_v2_run_gate_decide`, coder approve flow, desktop supervised tool flow) but **workflows never pause** — `crates/tandem-server/src/workflows.rs:1209` hard-codes `approval_state: Some("executed")` and the run flows straight through. No shipped marketplace pack uses `HumanApprovalGate`. The user has never seen a workflow ask for permission.

The demo plan and the wedge audit both leaned on approval-aware execution as a headline capability. That overstated reality. Until approvals actually pause and surface, the agent-owned-workflows-under-runtime-governance thesis is a slogan.

This plan closes the gap with:

1. **Server-side**: pause-on-gate in workflow execution (reusing the working automation_v2 machinery), an action classifier, a compiler pass that auto-wraps external-action nodes in gates, and a unified pending-approval aggregator.
2. **Control panel**: a single Approvals Inbox page across runs, plus a per-step override toggle in `ScopeInspector.tsx` so users can tighten or loosen the default at scope-review time.
3. **Channels**: rich interactive approval cards in Slack (Block Kit), Discord (embeds + buttons + modals), and Telegram (inline keyboards + edit-in-place), with proper webhook signature verification and idempotency on button clicks.
4. **Agent prompt**: planner agent informed of the default-gate policy so generated workflows narrate coherently.

Intended outcome: every external action in a generated workflow is gated by default; gates surface in the Approvals Inbox and as native interactive cards in connected channels; humans approve/reject/rework with a durable receipt; the demo plan's claims about approvals become true.

---

## Design

### Core principle: enforcement by compiler, not by prompt

The agent doesn't strictly need to _insert_ gates — the compiler does, based on a server-side classifier that maps tool/action IDs to `RequiresApproval | NoApproval | UserConfigurable`. The agent is _informed_ via system prompt so its narration matches what the compiler will produce. If the agent forgets, the compiler adds. If the agent tries to remove, the compiler refuses.

Categories that gate by default:

- Outbound communications (email send; Slack/Discord/Telegram post to non-internal channels; SMS).
- CRM writes (create/update/delete contacts, deals, activities).
- Payment actions.
- File deletes outside the scratch directory.
- Public posts (LinkedIn, Twitter, blog publish).
- Calendar invites sent to non-internal addresses.
- Anything mutating a system of record.

### Channel UX principle: native, not bolted-on

Each channel renders the same `InteractiveCard` shape into its own native primitives. Slack gets Block Kit + thread per workflow run + App Home pinned approvals + modal for rework. Discord gets embeds + action rows + modals + threads. Telegram gets inline keyboards + `editMessageReplyMarkup` after decision + `force_reply` for rework. Reactions-as-approve is rejected (accidental click risk, no rework reason, fuzzy auth).

### Sequence by risk, not by layer

The Plan agent's strongest insight: ship one channel end-to-end before going wide so auth/race/idempotency surface while the data model is still soft. Slack first because it's the highest-ROI surface for B2B buyers and exercises every interaction primitive (signing, deferred response, modal, thread, in-place edit).

---

## Implementation Sequencing

### Week 1 — Foundation: pause/resume + minimal aggregator

**Goal:** workflows actually pause on a hand-coded gate; one HTTP endpoint can list pending approvals.

1. **Extract `pause_for_gate` helper** into a new module under `crates/tandem-server/src/app/state/automation/`. Today the gate-decide handler lives in `crates/tandem-server/src/http/routines_automations_parts/part02.rs:1563–1640`; the coder approval lives at `crates/tandem-server/src/http/coder_parts/part07.rs:386–450`; both implement the same idea differently. Extract the shared shape now or have three copies forever.
2. **Wire the workflow dispatcher to pause.** Replace the always-`"executed"` line at `crates/tandem-server/src/workflows.rs:1209` with: if the action's `ProjectedAutomationNode` carries a `gate`, set `awaiting_gate` on the run checkpoint, transition status to `AwaitingApproval`, and return without dispatching. Reuse `AutomationPendingGate` and the existing `AutomationRunCheckpoint::awaiting_gate: Option<AutomationPendingGate>` (`automation_v2/types.rs:1038`).
3. **Define `ApprovalRequest` shape** in `crates/tandem-types/src/`. Keep it scoped: `request_id`, `tenant_id`, `run_id`, `node_id`, `workflow_name`, `action_kind`, `action_preview_markdown`, `surface_payload: serde_json::Value` (each surface stamps its native ID here on send), `requested_at`, `expires_at`, `target_users: Vec<ActorId>`, plus the eventual `decided_by`, `decided_at`, `decision`, `rework_feedback`. Mirror the filter shape of `governance.rs:89` `has_pending_approval_request()` rather than inventing a new query.
4. **`GET /approvals/pending` aggregator endpoint** in `crates/tandem-server/src/http/`. Aggregates from automation_v2 pending gates, coder pending approvals, and workflow pending gates. New file: `routes_approvals.rs` + `approvals.rs`.
5. **Hand-code one gate** on a single CRM-write action in the `sales-research-outreach` demo bundle. End-to-end smoke: trigger workflow → workflow pauses → `/approvals/pending` returns the request → POST gate-decide approves → workflow resumes → audit shows decision.

**Acceptance:** running the demo workflow on a CRM-write step actually waits for an HTTP approve call before proceeding. No channel work yet.

### Week 2 — Slack vertical slice (the discovery slice)

**Goal:** end-to-end interactive approval card in Slack, with proper signing and idempotency. Discover every auth/race issue here so Discord and Telegram inherit the fix.

1. **Webhook signing module** at `crates/tandem-channels/src/signing/`. Slack uses HMAC-SHA256 (`x-slack-signature` + `x-slack-request-timestamp`); reject any timestamp older than 5 minutes (replay protection). Use the `hmac` + `sha2` crates already in workspace.
2. **`Channel::send_card(InteractiveCard)` trait method** added to `crates/tandem-channels/src/traits.rs` as a _separate method_, not an optional field on `SendMessage`. Default impl returns `Err(NotImplemented)` so the type system tells you which adapters are wired. Define the normalized `InteractiveCard` struct: title, body markdown, fields (key/value rows), primary/secondary/destructive button list, optional reason-prompt config, optional `thread_key: Option<String>`.
3. **Slack adapter renders Block Kit** in `crates/tandem-channels/src/slack.rs`. Header section (workflow name + step), context block (`run_id`, requested by), preview section (action body), divider, button row (Approve / Rework / Cancel), overflow menu with "View full payload" → opens a modal via `views.open`.
4. **Slack interaction endpoint** at `crates/tandem-server/src/http/channels/slack_interactions.rs` (new). Handles button clicks and modal submissions. Verify HMAC signature on every request. Acknowledge within 3 seconds (use `response_url` for deferred replies). Idempotency key derived from `(run_id, node_id, action_ts)`; dedupe at handler.
5. **In-place edit on decision.** Use `chat.update` with the original `ts` to replace the button row with "Approved by @alice at 14:32 — Reason: …".
6. **Race UX.** Extend the gate-decide endpoint at `routines_automations_parts/part02.rs:1576` so its 409-`AwaitingApproval`-precondition response carries the winner's identity in the body. Slack's deferred reply renders "already decided by @alice" instead of a raw error.
7. **Threaded updates.** Initial card posts to channel; subsequent run status (started, awaiting next gate, completed) posts as thread replies. Plumb `(run_id → message_ts)` mapping via a small server-side cache.
8. **Slack App Home tab pinned approvals.** New `app_home_opened` handler that calls the same aggregator and renders pending approvals as a list. High-leverage for power users.

**Acceptance:** founder runs the demo workflow in a real Slack workspace, sees a Block Kit card, clicks Approve, sees the card update in place, sees a thread reply confirming completion. Two concurrent clicks (one from inbox, one from Slack) result in exactly one winner with a clear "already decided" message on the loser. Signing rejects forged requests in tests.

### Week 3 — Classifier + compiler injection + ScopeInspector + Inbox UI

1. **Action classifier** at `crates/tandem-tools/src/approval_classifier.rs` (new) or as a method on the tool registry. Returns `RequiresApproval | NoApproval | UserConfigurable` for every tool/MCP-tool ID. Table-driven; default-deny for unknown external-network-touching tools.
2. **Compiler injection pass** in `crates/tandem-plan-compiler/src/mission_runtime.rs`. Slot in adjacent to line 212 where gates are already projected. After projection, walk the `ProjectedAutomationNode` graph; for any node whose action is `RequiresApproval`, attach a `ProjectedAutomationApprovalGate` if not already present. Reuse the existing `projected_gate()` helper at lines 318–333 to construct it.
3. **ScopeInspector per-step override UI.** In `packages/tandem-control-panel/src/features/automations/ScopeInspector.tsx`, render each gated step with a toggle: `Approve every run` (default) / `Auto-approve when [condition]` / `Skip approval`. Skip-approval requires a confirm modal ("you're removing the approval gate on outbound emails — confirm"). Persist choice into the workflow draft.
4. **Approvals Inbox page** at `packages/tandem-control-panel/src/pages/ApprovalsInboxPage.tsx`. Subscribes to `/approvals/pending` (poll every 5s for v1; SSE later), renders pending approvals as cards, click row → detail modal with full action preview + approve/reject/rework buttons + audit trail.
5. **Agent system prompt update.** In the planner agent's prompt template (search for the planner prompt under `crates/tandem-plan-compiler/` or `agent-templates/`), add: _"Workflows you generate are wrapped with human-approval gates on outbound communications, system-of-record writes, payments, and public posts. You don't add the gates yourself; describe the workflow as if approvals are present at those points so the human reviewing scope sees a coherent picture."_

### Week 4 — Discord + Telegram

1. **Discord adapter rich UX** in `crates/tandem-channels/src/discord.rs`. Embeds (title, description, fields, color, footer with workflow ID), action rows (Approve / Rework / Cancel), modals for rework feedback (`InteractionResponseType: 9`), threads per workflow run.
2. **Discord interaction endpoint** at `crates/tandem-server/src/http/channels/discord_interactions.rs`. Discord _requires_ Ed25519 signature verification on every interaction POST or it disables the endpoint. Use `ed25519-dalek` (add to workspace if not present). Mirror the Slack idempotency pattern.
3. **Telegram adapter rich UX** in `crates/tandem-channels/src/telegram.rs`. Inline keyboards via `InlineKeyboardMarkup`, `editMessageReplyMarkup` after decision, `force_reply: true` to capture rework reason as the user's next message (reuse the `channel_automation_drafts` state machine for this).
4. **Telegram callback_query handler.** Register handler in dispatcher; route to the same gate-decide path. Telegram has no signing per-request but uses a `secret_token` header on the webhook URL.

### Week 5 — Notification fan-out + slash commands + demo + tests

1. **Notification fan-out task** at `crates/tandem-server/src/app/tasks/approval_outbound.rs` (new). Subscribes to a new `approval.pending` event on the engine event bus. Fans out to channel adapters via `Channel::send_card`. **Critical correctness fix:** the existing event-bus pattern (`workflows.rs:628`, `app/tasks.rs:392`) drops on `Lagged(_)`. For approvals this is wrong — a missed notification means a stuck run. Implement the fan-out with a bounded mpsc + backpressure, or persist pending sends to a `pending_notifications` table that the task drains. Document this as a deliberate departure from the existing pattern.
2. **Slash command extensions** in `crates/tandem-channels/src/dispatcher_parts/part03.rs`. Add `/pending` (lists outstanding approvals scoped to channel/tenant), `/rework {id} {feedback}`, contextual `/approve` and `/reject` (work on most-recent pending in thread). Reuse the existing dispatch table.
3. **Authority chain resolver.** Add `approver_identity` resolver: surface user (Slack user ID / Discord user ID / Telegram user ID) → engine principal. New module `crates/tandem-server/src/app/state/principals/channel_identity.rs`. Without this, anyone in a channel's `allowed_users` can approve workflows started by other users — wrong for audit.
4. **Agent prompt finalization + demo workflow E2E pass.** Run the `sales-research-outreach` workflow on synthetic data with all gates active. Verify: every external action gates; cards appear in Slack/Discord/Telegram (whichever is connected); inbox aggregates them; race test (two concurrent approvals) handles cleanly; audit trail shows actor + executed_as for every decision.
5. **Race regression test.** Two concurrent gate-decide calls on the same run — assert exactly one wins, the other gets a structured 409 with the winner's identity. Mandatory before any rollout.

---

## Critical Files to Modify

| File                                                                                   | Change                                                                                                           |
| -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `crates/tandem-server/src/workflows.rs:1209`                                           | Replace always-`"executed"` with conditional pause-on-gate. The single source of the lie.                        |
| `crates/tandem-server/src/http/routines_automations_parts/part02.rs:1563–1640`         | Extract shared `pause_for_gate` / `decide_gate` helpers; add winner's identity to 409 response.                  |
| `crates/tandem-server/src/http/coder_parts/part07.rs:386–450`                          | Refactor to use the shared helpers.                                                                              |
| `crates/tandem-plan-compiler/src/mission_runtime.rs:212`                               | Add classifier-driven gate-injection pass adjacent to existing `projected_gate` projection (helpers at 318–333). |
| `crates/tandem-types/src/` (new module: `approvals.rs`)                                | `ApprovalRequest` shape; `ApprovalDecision` re-exported.                                                         |
| `crates/tandem-tools/src/approval_classifier.rs` (new)                                 | Action classifier.                                                                                               |
| `crates/tandem-server/src/http/routes_approvals.rs` + `approvals.rs` (new)             | Aggregator endpoint mirroring `governance.rs:89` filter pattern.                                                 |
| `crates/tandem-channels/src/traits.rs`                                                 | Add `send_card(InteractiveCard)` method; define `InteractiveCard`.                                               |
| `crates/tandem-channels/src/signing/` (new module)                                     | Slack HMAC + Discord Ed25519 + Telegram secret_token verification.                                               |
| `crates/tandem-channels/src/slack.rs`                                                  | Block Kit rendering; in-place edit; threaded updates; App Home tab.                                              |
| `crates/tandem-channels/src/discord.rs`                                                | Embeds + action rows + modals + threads.                                                                         |
| `crates/tandem-channels/src/telegram.rs`                                               | Inline keyboards + editMessageReplyMarkup + force_reply for rework.                                              |
| `crates/tandem-channels/src/dispatcher_parts/part03.rs` (relay_tool_decision near 750) | Extend slash command table with `/pending`, `/rework`.                                                           |
| `crates/tandem-server/src/http/channels/slack_interactions.rs` (new)                   | Slack interaction endpoint with HMAC verification + idempotency.                                                 |
| `crates/tandem-server/src/http/channels/discord_interactions.rs` (new)                 | Discord interaction endpoint with Ed25519 verification.                                                          |
| `crates/tandem-server/src/app/tasks/approval_outbound.rs` (new)                        | Fan-out task with bounded mpsc / persistent outbox.                                                              |
| `crates/tandem-server/src/app/state/principals/channel_identity.rs` (new)              | Surface user → engine principal resolver.                                                                        |
| `packages/tandem-control-panel/src/pages/ApprovalsInboxPage.tsx` (new)                 | Inbox UI.                                                                                                        |
| `packages/tandem-control-panel/src/features/automations/ScopeInspector.tsx`            | Per-step override toggles.                                                                                       |

---

## Reuse Map (do not rebuild)

- `HumanApprovalGate` and `ApprovalDecision` (`mission_builder.rs:135–144`).
- `AutomationPendingGate` + `AutomationRunCheckpoint::awaiting_gate` (`automation_v2/types.rs:1038`) — full pause/resume machinery.
- `automations_v2_run_gate_decide` decision logic (`routines_automations_parts/part02.rs:1563–1640`) — extract, don't duplicate.
- `projected_gate()` helper (`mission_runtime.rs:318–333`) — use for compiler injection.
- `governance.rs:89` `has_pending_approval_request` filter shape — mirror in aggregator.
- Dispatcher slash command table (`dispatcher_parts/part03.rs`) — extend, don't parallel.
- `relay_tool_decision()` (`dispatcher_parts/part03.rs:750`) — extend for gate decisions.
- `channel_automation_drafts` state machine — reuse for collecting rework feedback in Telegram (no modal available).
- Existing `is_user_allowed()`, `should_accept_message()`, `security_profile` (`config.rs:103`, `traits.rs:60`) — keep as outer permission filter; add `approver_identity` as inner authority resolver.
- Event bus subscribe pattern (`workflows.rs:628`, `app/tasks.rs:392`) — copy structurally but **do not copy the `Lagged(_)` drop semantics for approvals**.

---

## Risks and Mitigations

1. **Lagged broadcast drops missed notifications.** Existing pattern drops on lag. Approval fan-out must use bounded mpsc + backpressure, or a `pending_notifications` outbox table the task drains. Documented as deliberate departure.
2. **Discord interaction signature verification missing.** No Rust signature verification anywhere today; only TS reference under `docs/internal/openclaw/extensions/telegram/src/webhook.ts`. Discord disables the endpoint if even one interaction is unverified. Build the signing module Week 2 (Slack) so Discord (Week 4) inherits the pattern.
3. **Race between surfaces (Slack click + Inbox click + slash command).** Existing per-run mutation serializes (`part02.rs:1627`); second caller hits `AwaitingApproval` precondition and 409s. Today the 409 has no body. Extend it to return the winner's identity so deferred Slack/Discord responses render "already decided by @alice" instead of a raw error.
4. **Idempotency on retried button clicks.** Slack retries interactions if the 3-second ack is missed; Discord retries on transient failures. Per-click idempotency key from `(run_id, node_id, action_ts)`; dedupe at handler. Use Slack `response_url` for deferred replies.
5. **Authority chain confusion.** A Slack admin in `allowed_users` could approve workflows started by other users. Add `approver_identity` resolver (surface user → engine principal) before going wider than internal testing. Audit records `actor` (clicker) + `executed_as` (workflow owner) + `approval_chain`.
6. **Plan-compiler hot path.** Gate injection pass runs once per compile, not per execution. Verify with a benchmark before merging.
7. **`HumanApprovalGate.rework_targets` exists but channel UX may not surface them.** If skipped in MVP, document it. Discord can render as `StringSelect` inside the embed; Slack as a select menu in the modal.
8. **Channel API drift.** Slack changed Block Kit semantics in 2024; Discord deprecates rapidly. Nightly E2E harness against real test workspace catches this; mocks won't.

---

## Verification

Run all tests after each week's milestone. End-to-end demo run is the contract.

### Week 1 acceptance (foundation)

```bash
# Server-side
cargo test -p tandem-server --test workflows
cargo test -p tandem-server gate_decide
cargo test -p tandem-types approvals

# Manual smoke
cargo run -p tandem-engine -- serve --state-dir /tmp/tandem-test &
# Apply demo bundle with one hand-coded gate on CRM-write
curl -X POST http://localhost:39731/workflow-plans/apply -d @demo-bundle.json
# Trigger workflow
curl -X POST http://localhost:39731/automations/v2/{id}/run_now
# Confirm pause
curl http://localhost:39731/approvals/pending
# Approve
curl -X POST http://localhost:39731/automations/v2/runs/{run_id}/gate_decide -d '{"decision":"approve"}'
# Confirm completion in audit
```

### Week 2 acceptance (Slack vertical slice)

- HMAC verification: unit test with good and forged `x-slack-signature`.
- End-to-end in a real Slack test workspace: trigger workflow → Block Kit card appears → click Approve → card edits in place → thread reply confirms completion.
- Race test (mandatory before merge): two concurrent gate-decide calls on the same run → exactly one winner, structured 409 with winner's identity on the loser.
- App Home tab renders pending approvals.

### Week 3 acceptance (classifier + compiler + UI)

- Classifier table-driven unit tests cover every capability ID.
- Compiler injection: golden plan tests in `mission_runtime.rs` with classifier returning `RequiresApproval` for one known tool — assert `gate` is present on the projected node. Same test with `NoApproval` — assert no gate added.
- ScopeInspector override toggle: integration test in control panel.
- Approvals Inbox: poll every 5s, render at least one pending approval, decision flow round-trips.

### Week 4 acceptance (Discord + Telegram)

- Discord Ed25519 signature verification: unit test with good and forged keys.
- Discord modal flow: integration test in real Discord dev server.
- Telegram inline keyboard + `editMessageReplyMarkup` + `force_reply` rework: integration test against real Telegram bot.

### Week 5 acceptance (fan-out + slash commands + demo)

- Fan-out task: integration test that backpressure kicks in when subscriber slow; no notifications lost.
- Slash commands: `/pending`, `/rework`, contextual `/approve` round-trip in Slack and Discord.
- Authority chain: integration test where Slack user A clicks approve on a workflow owned by user B — audit record shows `actor=A`, `executed_as=B`.
- **Final demo E2E**: run `sales-research-outreach` on synthetic data with all gates active, all three channels connected. Every external action gates. Cards appear correctly in each channel. Inbox aggregates. Race test passes. Recorded for the demo video.

### Nightly harness (post-launch)

- Real Slack workspace + real Discord dev server + real Telegram bot, behind feature flag `TANDEM_NIGHTLY_E2E=1`.
- Run the demo workflow once per night. Failure pages on-call.

---

## Out of scope (explicit)

- Email-based approvals (magic-link approve/reject). Defer until ≥3 customers ask.
- Cross-tenant approval delegation. Single-tenant only for v1.
- Approval expiry workflows (auto-cancel after N hours). Set expiry, render countdown, but no automatic cancel logic in v1.
- Reactions-as-approve in Slack/Discord. Rejected: accidental approval risk, no rework reason, fuzzy auth.
- Discord application context-menu actions (right-click → "Send to Tandem"). Niche; defer until users ask.
- Mobile push notifications. Channels are the mobile surface for v1.
- SSE/WebSocket push for the Inbox page. Polling at 5s is fine for v1; switch to SSE if it becomes a UX bottleneck.

---

## First execution step

After approval, before any code change:

1. `mkdir -p docs/internal/approval-gates-and-channel-ux/`
2. `cp ~/.claude/plans/melodic-weaving-parnas.md docs/internal/approval-gates-and-channel-ux/PLAN.md`
3. Add `docs/internal/approval-gates-and-channel-ux/README.md` linking from `docs/internal/business/replit-for-operations-reality-map.md` (gap row in §7) and `docs/internal/demo/demo-analysis-plan.md` (Tier 0 [GAP] items).
4. Commit the plan to git so the team can reference and amend it.

All subsequent work happens against the `docs/internal/` copy.
