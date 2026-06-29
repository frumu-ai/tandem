import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_STATEFUL_RUN_FILTERS,
  buildStatefulRunRows,
  filterStatefulRunRows,
  formatRunTimestamp,
  normalizeStatefulRunFilters,
  summarizeStatefulRuns,
} from "../lib/runs/stateful-runs.js";

test("stateful run helpers project and dedupe workflow, automation, and context rows", () => {
  const rows = buildStatefulRunRows({
    workflowRuns: [
      {
        run_id: "run-1",
        automation_id: "workflow-1",
        trigger_type: "webhook",
        status: "running",
        created_at_ms: 1000,
        updated_at_ms: 3000,
        active_session_ids: ["session-1"],
        tenant_context: {
          org_id: "org-a",
          workspace_id: "finance",
          deployment_id: "prod",
          actor_id: "agent-a",
        },
        checkpoint: {
          lifecycle_history: [
            { event: "run_execution_claimed", recorded_at_ms: 2000 },
            { event: "node_started", recorded_at_ms: 3000, reason: "research" },
          ],
        },
        automation_snapshot: {
          name: "Quarter close",
          workspace_root: "/srv/finance",
        },
      },
    ],
    legacyRuns: [
      {
        run_id: "legacy-1",
        name: "Legacy daily brief",
        status: "completed",
        updated_at_ms: 2000,
      },
    ],
    contextRuns: [
      {
        run_id: "automation-v2-automation-v2-run-1",
        run_type: "workflow",
        status: "running",
        updated_at_ms: 4000,
        workspace: "/srv/finance/context",
      },
    ],
  });

  assert.equal(rows.length, 2);
  assert.equal(rows[0].id, "run-1");
  assert.equal(rows[0].source, "workflow");
  assert.equal(rows[0].title, "Quarter close");
  assert.equal(rows[0].tenantOrg, "org-a");
  assert.equal(rows[0].tenantWorkspace, "finance");
  assert.equal(rows[0].workspace, "/srv/finance");
  assert.equal(rows[0].lastEventLabel, "Node Started");
  assert.equal(rows[0].updatedAtMs, 4000);
  assert.equal(rows[1].source, "automation");
});

test("stateful run helpers classify waits, retries, and summary buckets", () => {
  const rows = buildStatefulRunRows({
    workflowRuns: [
      {
        run_id: "approval-1",
        automation_id: "workflow-approval",
        status: "awaiting_approval",
        updated_at_ms: 5000,
        checkpoint: {
          awaiting_gate: {
            node_id: "review",
            title: "Approve release note",
            requested_at_ms: 4500,
          },
        },
        automation_snapshot: { name: "Release publisher" },
      },
      {
        run_id: "failed-1",
        automation_id: "workflow-failed",
        status: "failed",
        updated_at_ms: 3000,
        checkpoint: {
          last_failure: {
            node_id: "deploy",
            reason: "CI failed",
            failed_at_ms: 2900,
          },
          node_attempts: { deploy: 3 },
        },
        automation_snapshot: { name: "Deploy" },
      },
      {
        run_id: "queued-1",
        automation_id: "workflow-queued",
        status: "queued",
        updated_at_ms: 1000,
        scheduler: { queue_reason: "workspace_lock", resource_key: "workspace:/repo" },
        automation_snapshot: { name: "Workspace job" },
      },
    ],
  });

  assert.equal(rows[0].statusGroup, "waiting");
  assert.equal(rows[0].phase, "Awaiting Approval");
  assert.equal(rows[0].currentWait, "Approve release note");
  assert.equal(rows[1].statusGroup, "failed");
  assert.equal(rows[1].retryState, "Last failure");
  assert.equal(rows[1].retryDetail, "deploy: CI failed");
  assert.equal(rows[2].statusGroup, "queued");
  assert.equal(rows[2].currentWait, "Queued: Workspace Lock");
  assert.deepEqual(summarizeStatefulRuns(rows), {
    total: 3,
    active: 0,
    waiting: 1,
    queued: 1,
    failed: 1,
    completed: 0,
    tenants: 1,
    workspaces: 0,
  });
});

test("stateful run helpers project canonical stateful runtime API rows", () => {
  const rows = buildStatefulRunRows({
    statefulRuns: [
      {
        run: {
          run_id: "run-stateful",
          kind: "automation_v2",
          automation_id: "automation-a",
          status: "awaiting_webhook",
          phase: "waiting_webhook",
          trigger_type: "webhook",
          updated_at_ms: 7000,
          scope: {
            tenant_context: {
              org_id: "org-a",
              workspace_id: "workspace-a",
              deployment_id: "prod",
            },
          },
        },
        current_wait: {
          wait_id: "wait-a",
          wait_kind: "webhook",
          status: "waiting",
          reason: "wait for provider callback",
        },
        latest_event: {
          event_id: "evt-a",
          event_type: "stateful_runtime.wait.webhook_registered",
          occurred_at_ms: 7100,
          phase_id: "phase-a",
        },
        latest_snapshot: {
          snapshot_id: "snapshot-a",
          seq: 3,
        },
      },
    ],
  });

  assert.equal(rows.length, 1);
  assert.equal(rows[0].id, "run-stateful");
  assert.equal(rows[0].source, "workflow");
  assert.equal(rows[0].statusGroup, "waiting");
  assert.equal(rows[0].phase, "Waiting Webhook");
  assert.equal(rows[0].currentWait, "wait for provider callback");
  assert.equal(rows[0].waitDetail, "wait-a · waiting");
  assert.equal(rows[0].tenantOrg, "org-a");
  assert.equal(rows[0].tenantWorkspace, "workspace-a");
  assert.equal(rows[0].lastEventLabel, "Stateful Runtime Wait Webhook Registered");
});

test("stateful run helpers filter by status source tenant workspace and phase wait text", () => {
  const rows = buildStatefulRunRows({
    workflowRuns: [
      {
        run_id: "run-a",
        automation_id: "a",
        status: "awaiting_approval",
        updated_at_ms: 1000,
        tenant_context: { org_id: "org-a", workspace_id: "hr" },
        checkpoint: { awaiting_gate: { node_id: "legal", title: "Legal review", requested_at_ms: 1 } },
        automation_snapshot: { name: "Hire packet", workspace_root: "/work/hr" },
      },
    ],
    contextRuns: [
      {
        run_id: "ctx-b",
        run_type: "workflow",
        status: "completed",
        updated_at_ms: 2000,
        tenant_context: { org_id: "org-b", workspace_id: "finance" },
        workspace: {
          workspace_id: "finance",
          canonical_path: "/work/finance",
        },
      },
    ],
  });

  assert.deepEqual(filterStatefulRunRows(rows, { status: "waiting" }).map((row) => row.id), ["run-a"]);
  assert.deepEqual(filterStatefulRunRows(rows, { source: "context" }).map((row) => row.id), ["ctx-b"]);
  assert.deepEqual(filterStatefulRunRows(rows, { tenant: "org-b finance" }).map((row) => row.id), [
    "ctx-b",
  ]);
  assert.deepEqual(filterStatefulRunRows(rows, { workspace: "hr" }).map((row) => row.id), ["run-a"]);
  assert.deepEqual(filterStatefulRunRows(rows, { workspace: "finance" }).map((row) => row.id), [
    "ctx-b",
  ]);
  assert.deepEqual(filterStatefulRunRows(rows, { wait: "legal" }).map((row) => row.id), ["run-a"]);
  assert.deepEqual(filterStatefulRunRows(rows, { query: "hire" }).map((row) => row.id), ["run-a"]);
});

test("stateful run helpers normalize filters and format timestamps", () => {
  assert.deepEqual(normalizeStatefulRunFilters({ status: "bogus", source: "bad", query: "  run " }), {
    ...DEFAULT_STATEFUL_RUN_FILTERS,
    query: "run",
  });

  const originalDateFormat = Intl.DateTimeFormat;
  Intl.DateTimeFormat = function (_locale, options) {
    assert.deepEqual(options, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
    return { format: () => "Jun 29, 12:34 PM" };
  };

  try {
    assert.equal(formatRunTimestamp(123), "Jun 29, 12:34 PM");
    assert.equal(formatRunTimestamp(0), "n/a");
  } finally {
    Intl.DateTimeFormat = originalDateFormat;
  }
});
