import assert from "node:assert/strict";
import test from "node:test";
import {
  buildApprovalWaitRows,
  buildRecoveryQueueRows,
  buildWebhookInboxRows,
  filterStatefulQueueRows,
  summarizeApprovalWaitRows,
  summarizeRecoveryQueueRows,
  summarizeWebhookInboxRows,
} from "../lib/runs/stateful-queues.js";

test("webhook inbox rows expose accepted, duplicate, rejected, dead-lettered, and redacted states", () => {
  const rows = buildWebhookInboxRows({
    events: [
      {
        event_id: "evt-accepted",
        provider: "github",
        status: "accepted",
        queued_run_id: "run-new",
        dedupe_result: "accepted",
        payload_available: true,
        payload_ref: "payload://1",
        payload_bytes: 640,
      },
      {
        event_id: "evt-duplicate",
        provider: "github",
        status: "duplicate",
        dedupe_result: "duplicate",
        duplicate_of_run_id: "run-new",
        idempotency_key: "idem-1",
      },
      {
        event_id: "evt-rejected",
        provider: "stripe",
        status: "rejected",
        rejection_reason_code: "bad_signature",
      },
      {
        event_id: "evt-dead-letter",
        provider: "slack",
        status: "failed",
        correlation: { outcome: "dead_letter" },
      },
      {
        event_id: "evt-redacted",
        provider: "linear",
        status: "accepted",
        headers_redacted: { authorization: "[redacted]" },
        payload_available: false,
        payload_bytes: 2048,
      },
    ],
  });

  assert.equal(rows.length, 5);
  assert.equal(rows[0].verificationLabel, "Verified and accepted");
  assert.equal(rows[0].correlationLabel, "New Run - run-new");
  assert.equal(rows[1].statusGroup, "duplicate");
  assert.equal(rows[1].dedupeLabel, "Duplicate - key idem-1");
  assert.equal(rows[2].verificationLabel, "Rejected: Bad Signature");
  assert.equal(rows[3].deadLettered, true);
  assert.equal(rows[4].payloadLabel, "Payload expired - 2 KB - headers redacted");
  assert.deepEqual(summarizeWebhookInboxRows(rows), {
    total: 5,
    accepted: 2,
    duplicate: 1,
    rejected: 1,
    failed: 1,
    redacted: 1,
    deadLetters: 1,
  });
});

test("stateful queue filters apply tenant, workspace, policy, knowledge, and wait terms", () => {
  const rows = [
    ...buildWebhookInboxRows({
      events: [
        {
          event_id: "evt-prod",
          provider: "github",
          status: "accepted",
          queued_run_id: "run-prod",
          tenant_context: { org_id: "org-prod", workspace_id: "workspace-prod" },
          workspace_root: "/srv/prod",
          enterprise_scope: {
            owning_org_unit_id: "platform",
            resource_kind: "repository",
            resource_id: "repo-prod",
            policy_version_id: "policy-prod",
            data_classes: ["source"],
            visible_knowledge_sources: [{ binding_id: "kb-prod", source_type: "runbook" }],
          },
        },
        {
          event_id: "evt-sandbox",
          provider: "linear",
          status: "accepted",
          queued_run_id: "run-sandbox",
          tenant_context: { org_id: "org-sandbox", workspace_id: "workspace-sandbox" },
          workspace_root: "/srv/sandbox",
        },
      ],
    }),
    ...buildApprovalWaitRows(
      {
        approvals: [
          {
            request_id: "approval-prod",
            run_id: "run-prod",
            status: "pending",
            phase_id: "approval",
            approval_wait: { transition_id: "ship" },
            tenant_context: { org_id: "org-prod", workspace_id: "workspace-prod" },
          },
        ],
      },
      { now: 1000 }
    ),
  ];

  assert.deepEqual(
    filterStatefulQueueRows(rows, { tenant: "org-prod" }).map((row) => row.id),
    ["evt-prod", "approval-prod"]
  );
  assert.deepEqual(
    filterStatefulQueueRows(rows, { workspace: "/srv/prod" }).map((row) => row.id),
    ["evt-prod"]
  );
  assert.deepEqual(
    filterStatefulQueueRows(rows, { orgUnit: "platform", resource: "repo-prod", policy: "policy-prod" }).map(
      (row) => row.id
    ),
    ["evt-prod"]
  );
  assert.deepEqual(
    filterStatefulQueueRows(rows, { dataClass: "source", knowledge: "kb-prod" }).map((row) => row.id),
    ["evt-prod"]
  );
  assert.deepEqual(
    filterStatefulQueueRows(rows, { status: "waiting", wait: "ship" }).map((row) => row.id),
    ["approval-prod"]
  );
  assert.deepEqual(
    filterStatefulQueueRows(rows, { source: "automation" }).map((row) => row.id),
    ["evt-prod", "evt-sandbox", "approval-prod"]
  );
});

test("stateful queue source filter classifies webhook and approval row shapes before label fallback", () => {
  const rows = [
    {
      id: "raw-webhook",
      source: "github",
      provider: "github",
      raw: {
        provider_event_kind: "push",
        delivery_id: "delivery-1",
      },
    },
    {
      id: "raw-approval",
      source: "stateful",
      raw: {
        approval_wait: { transition_id: "ship" },
      },
    },
    {
      id: "context-row",
      source: "context",
    },
  ];

  assert.deepEqual(
    filterStatefulQueueRows(rows, { source: "automation" }).map((row) => row.id),
    ["raw-webhook", "raw-approval"]
  );
});

test("approval wait rows expose timeout, escalation, transition, and decision history", () => {
  const rows = buildApprovalWaitRows(
    {
      approvals: [
        {
          request_id: "approval-pending",
          run_id: "run-1",
          workflow_name: "Release",
          requested_at_ms: 1000,
          expires_at_ms: 61_000,
          approval_wait: { transition_id: "ship" },
          decisions: ["approve", "cancel"],
        },
        {
          request_id: "approval-expired",
          run_id: "run-2",
          expires_at_ms: 900,
        },
        {
          request_id: "approval-escalated",
          run_id: "run-3",
          escalation_state: "escalated",
        },
        {
          request_id: "approval-approved",
          run_id: "run-4",
          status: "approved",
          decision_history: [
            {
              decision_id: "decision-1",
              actor_id: "operator-a",
              decision: "approved",
              decided_at_ms: 5000,
              transition_id: "ship",
            },
          ],
        },
        { request_id: "approval-rejected", run_id: "run-5", status: "rejected" },
        { request_id: "approval-cancelled", run_id: "run-6", status: "cancelled" },
      ],
    },
    { now: 1000 }
  );

  assert.equal(rows[0].status, "pending");
  assert.equal(rows[0].timeoutLabel, "expires in 1m");
  assert.equal(rows[0].transitionId, "ship");
  assert.equal(rows[0].decisionHistory[0].available, true);
  assert.equal(rows[1].status, "expired");
  assert.equal(rows[2].status, "escalated");
  assert.equal(rows[3].decisionHistory[0].actor, "operator-a");
  assert.equal(rows[3].decisionHistory[0].transition, "ship");
  assert.deepEqual(summarizeApprovalWaitRows(rows), {
    total: 6,
    pending: 1,
    expired: 1,
    escalated: 1,
    decided: 3,
  });
});

test("recovery queue rows separate retryable, backoff, dead-lettered, and manual states", () => {
  const rows = buildRecoveryQueueRows({
    outbox: [
      {
        outbox_id: "outbox-pending",
        status: "pending",
        operation: "post_comment",
        run_id: "run-wait",
        attempts: 1,
        updated_at_ms: 1000,
      },
      {
        outbox_id: "outbox-failed",
        status: "failed",
        operation: "post_comment",
        run_id: "run-retry",
        attempts: 3,
        updated_at_ms: 2000,
      },
    ],
    dead_letters: [
      {
        dead_letter_id: "dead-open",
        source_type: "webhook",
        source_id: "evt-dead",
        status: "open",
        run_id: "run-dead",
        reason: "exhausted retries",
        recovery_options: ["retry", "ignore"],
        updated_at_ms: 3000,
      },
      {
        dead_letter_id: "dead-resolved",
        source_type: "tool_effect",
        source_id: "effect-1",
        status: "resolved",
        run_id: "run-manual",
        reason: "operator resumed",
        updated_at_ms: 4000,
      },
    ],
  });

  const byId = Object.fromEntries(rows.map((row) => [row.id, row]));
  assert.equal(byId["outbox-pending"].category, "waiting_backoff");
  assert.equal(byId["outbox-failed"].category, "retryable");
  assert.equal(byId["dead-open"].category, "dead_lettered");
  assert.deepEqual(byId["dead-open"].recoveryOptions, ["retry", "ignore"]);
  assert.equal(byId["dead-resolved"].category, "manually_blocked");
  assert.deepEqual(summarizeRecoveryQueueRows(rows), {
    total: 4,
    retryable: 1,
    waitingBackoff: 1,
    deadLettered: 1,
    manuallyBlocked: 1,
  });
});
