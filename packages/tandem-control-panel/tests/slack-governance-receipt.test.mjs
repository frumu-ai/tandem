import assert from "node:assert/strict";
import test from "node:test";

import {
  evidenceCompleteness,
  receiptCorrelation,
  receiptOptionLabel,
  slackIdentityOf,
  slackReceiptRuns,
} from "../src/pages/slackGovernanceReceiptModel.mjs";

// Captured shape of a run persisted by the tandem-server production path
// (ensure_session_context_run for a governed Slack session) — see the
// acme_slack_demo_e2e_persists_selectable_slack_receipts test in
// crates/tandem-server.
function persistedSlackRun(userId, overrides = {}) {
  return {
    run_id: `session-ses-${userId.toLowerCase()}`,
    run_type: "session",
    source_client: "channel:slack",
    source_metadata: {
      channel: "slack",
      user_id: userId,
      scope_kind: "thread",
      scope_id: `T_ACME_HQ:A_ACME_TANDEM:C_ACME_DEMO:1800000300.000001`,
      slack_team_id: "T_ACME_HQ",
      slack_app_id: "A_ACME_TANDEM",
      slack_channel_id: "C_ACME_DEMO",
      slack_thread_ts: "1800000300.000001",
    },
    tenant_context: { org_id: "acme", workspace_id: "hq" },
    status: "completed",
    objective: `Interactive session: slack - ${userId} - thread`,
    updated_at_ms: 1_800_000_300_000,
    ...overrides,
  };
}

test("only Slack-originated session runs qualify as receipts", () => {
  const slackRun = persistedSlackRun("U_SALES");
  const rows = slackReceiptRuns([
    slackRun,
    { run_id: "session-other", run_type: "session", source_client: "session_api" },
    { run_id: "automation-v2-x", run_type: "automation", source_client: "automation" },
    { run_id: "session-broken", source_client: "channel:slack" }, // no identity metadata
    null,
  ]);
  assert.deepEqual(
    rows.map((run) => run.run_id),
    [slackRun.run_id]
  );
});

test("slack identity is read from persisted source metadata", () => {
  const identity = slackIdentityOf(persistedSlackRun("U_FINANCE"));
  assert.equal(identity.userId, "U_FINANCE");
  assert.equal(identity.channelId, "C_ACME_DEMO");
  assert.equal(identity.teamId, "T_ACME_HQ");
  assert.equal(identity.threadTs, "1800000300.000001");
  assert.equal(slackIdentityOf({ run_id: "session-x" }), null);
});

test("selector labels lead with requester and channel, never a bare id", () => {
  const label = receiptOptionLabel(persistedSlackRun("U_CONTRACTOR"));
  assert.match(label, /^U_CONTRACTOR · C_ACME_DEMO · completed · session-/);
  // Runs without identity fall back to the id rather than an empty label.
  assert.equal(receiptOptionLabel({ run_id: "session-x" }), "session-x");
});

test("evidence completeness reports missing sections, not zeros", () => {
  // Full package: nothing missing even when the arrays are legitimately empty.
  const full = {
    actors: {},
    tool_manifest: {},
    policy_decisions: [],
    approvals: { gate_history: [] },
    memory_audit: [],
    audit: { protected_events: [] },
    final_outcome: { context_status: "completed" },
  };
  assert.deepEqual(evidenceCompleteness(full), { available: true, missing: [] });

  // Partial package (e.g. audit store unavailable): the absent sections are
  // named explicitly.
  const partial = { ...full };
  delete partial.memory_audit;
  delete partial.audit;
  const report = evidenceCompleteness(partial);
  assert.equal(report.available, true);
  assert.deepEqual(report.missing.sort(), ["memory_audit", "protected_audit"]);

  // No package at all (export denied/unavailable).
  const unavailable = evidenceCompleteness(null);
  assert.equal(unavailable.available, false);
  assert.ok(unavailable.missing.length >= 7);
});

test("receipt correlation cross-checks run and evidence identities", () => {
  const run = persistedSlackRun("U_ENG");
  const consistent = receiptCorrelation(run, {
    run: {
      context_run_id: run.run_id,
      source_metadata: run.source_metadata,
    },
  });
  assert.equal(consistent.identityConsistent, true);
  assert.equal(consistent.packageContextRunId, run.run_id);

  const inconsistent = receiptCorrelation(run, {
    run: {
      context_run_id: run.run_id,
      source_metadata: { ...run.source_metadata, user_id: "U_SOMEONE_ELSE" },
    },
  });
  assert.equal(inconsistent.identityConsistent, false);

  // Evidence not exported yet: correlation still renders from the run alone.
  const pending = receiptCorrelation(run, null);
  assert.equal(pending.identityConsistent, true);
  assert.equal(pending.packageContextRunId, "");
});
