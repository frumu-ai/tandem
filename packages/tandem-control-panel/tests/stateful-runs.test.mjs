import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_STATEFUL_RUN_FILTERS,
  buildRunObservabilityDetail,
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
	    orgUnits: 0,
	    policyVersions: 0,
	    knowledgeSources: 0,
  });
});

test("stateful run helpers keep the persisted run id for observability requests", () => {
  const rows = buildStatefulRunRows({
    workflowRuns: [
      {
        run_id: "automation-v2-run-123",
        automation_id: "workflow-prefixed",
        status: "running",
        updated_at_ms: 1000,
      },
    ],
  });

  assert.equal(rows.length, 1);
  assert.equal(rows[0].id, "automation-v2-run-123");
  assert.equal(rows[0].canonicalId, "run-123");
  assert.equal(rows[0].observabilityRunId, "automation-v2-run-123");
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
	        enterprise_scope: {
	          owning_org_unit_id: "finance",
	          owning_org_unit: {
	            display_name: "Finance Ops",
	          },
	          owner_principal: {
	            kind: "automation",
	            id: "automation-a",
	          },
	          resource_kind: "repository",
	          resource_id: "repo-a",
	          data_classes: ["financial_record"],
	          policy_version_id: "policy-2026-06",
	          delegation_grant_ids: ["delegation-a"],
	          visible_knowledge_sources: [
	            {
	              binding_id: "binding-repo",
	              source_type: "github",
	              source_root_label: "Finance repo",
	              data_class: "financial_record",
	            },
	          ],
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
	  assert.equal(rows[0].orgUnitName, "Finance Ops");
	  assert.equal(rows[0].resourceLabel, "Repository repo-a");
	  assert.equal(rows[0].policyVersion, "policy-2026-06");
	  assert.deepEqual(rows[0].dataClasses, ["financial_record"]);
	  assert.deepEqual(rows[0].knowledgeSourceIds, ["binding-repo"]);
		});

test("stateful run helpers normalize observability detail sections", () => {
  const detail = buildRunObservabilityDetail({
    run_id: "run-observe",
    source: "stateful_runtime_observability",
    run: {
      run_id: "run-observe",
      kind: "automation_v2",
      status: "blocked",
      phase: "paused_attention_required",
      current_phase_id: "phase-review",
      allowed_next_phases: ["queued", "running_phase", "failed", "cancelled"],
      phase_history: [
        {
          event_id: "phase-a",
          from_phase: "running_phase",
          to_phase: "paused_attention_required",
          event_type: "stateful_runtime.phase.transition",
          phase_id: "phase-review",
          reason: "rehydrated blocked review",
          occurred_at_ms: 3200,
        },
      ],
      scope: {
        tenant_context: {
          org_id: "org-observe",
          workspace_id: "workspace-observe",
          deployment_id: "prod",
        },
        resource_scope: {
          root: {
            resource_kind: "repository",
            resource_id: "repo-observe",
          },
        },
        policy_version_id: "policy-observe",
        data_classes: ["confidential"],
      },
      workflow_definition_version: "v3",
      workflow_definition_snapshot_hash: "sha256:workflow",
    },
    current_wait: {
      wait_id: "wait-approval",
      wait_kind: "approval",
      status: "waiting",
      phase_id: "phase-review",
      reason: "awaiting operator",
      claimed_by: "scheduler-a",
      claim_expires_at_ms: 9000,
    },
    waits: [
      {
        wait_id: "wait-approval",
        wait_kind: "approval",
        status: "waiting",
        phase_id: "phase-review",
        reason: "awaiting operator",
      },
    ],
    policy_decisions: [
      {
        decision_id: "decision-a",
        decision: "approval_required",
        tool: "github.comment",
        reason_code: "external_effect",
        approval_id: "approval-a",
      },
    ],
    reliability: {
      outbox: [{ outbox_id: "outbox-a", operation: "github.comment", status: "pending", idempotency_key: "idem-a" }],
      tool_effects: [
        {
          effect_id: "effect-a",
          operation: "github.comment",
          status: "failed",
          error: "timeout",
          metadata: {
            run_as: {
              principal: { kind: "service_principal", id: "svc-a" },
              connectionId: "conn-a",
            },
          },
        },
      ],
      dead_letters: [{ dead_letter_id: "dead-a", source_type: "tool_effect", status: "open", reason: "timeout" }],
      compensations: [
        {
          compensation_id: "comp-a",
          compensation_type: "operator_review",
          status: "awaiting_approval",
          target_effect_id: "effect-a",
        },
      ],
    },
    audit: {
      payload_policy: "redacted_payload_digest_only",
      protected_events: [
        {
          event_id: "audit-a",
          event_type: "policy.decision.recorded",
          seq: 5,
          actor: "operator-a",
          payload_digest: "sha256:audit",
        },
      ],
    },
    events: {
      latest: { event_id: "event-4", event_type: "runtime.effect.failed", seq: 4, phase_id: "phase-review" },
      tail: [
        { event_id: "event-2", event_type: "runtime.wait.recorded", seq: 2, phase_id: "phase-review" },
        { event_id: "event-4", event_type: "runtime.effect.failed", seq: 4, phase_id: "phase-review" },
      ],
    },
    snapshots: {
      latest: {
        snapshot_id: "snapshot-4",
        seq: 4,
        status: "blocked",
        phase: "paused_attention_required",
        workflow_definition_version: "v3",
      },
      items: [
        {
          snapshot_id: "snapshot-2",
          seq: 2,
          status: "waiting",
          phase: "awaiting_approval",
          phase_id: "phase-draft",
          payload_digest: "sha256:before",
          checkpoint: {
            completed_nodes: ["draft"],
            pending_nodes: ["review"],
            blocked_nodes: [],
            node_attempts: { draft: 1 },
            execution_claim_epoch: 1,
          },
        },
        {
          snapshot_id: "snapshot-4",
          seq: 4,
          status: "blocked",
          phase: "paused_attention_required",
          phase_id: "phase-review",
          payload_digest: "sha256:after",
          allowed_next_phases: ["queued", "running_phase", "failed", "cancelled"],
          checkpoint: {
            completed_nodes: ["draft"],
            pending_nodes: [],
            blocked_nodes: ["review"],
            awaiting_gate: { node_id: "review", title: "Review output" },
            node_attempts: { draft: 1, review: 2 },
            resume_reason: "server_restart_rehydration",
            execution_claim: {
              claim_id: "claim-a",
              claimant_id: "worker-a",
              claim_expires_at_ms: 9500,
            },
            active_session_ids: ["session-a"],
          },
        },
      ],
    },
    operator_summary: {
      is_blocked: true,
      blocking_reasons: [{ kind: "active_wait", wait_id: "wait-approval", wait_kind: "approval", status: "waiting" }],
      allowed_actions: [{ action: "review_wait", wait_ids: ["wait-approval"] }],
    },
  });

  assert.equal(detail.runId, "run-observe");
  assert.equal(detail.statusLabel, "Blocked");
  assert.equal(detail.phase, "Phase Review");
  assert.equal(detail.runtimePhase, "Paused Attention Required");
  assert.equal(detail.workflowDefinitionVersion, "v3");
  assert.equal(detail.currentWait.label, "awaiting operator");
  assert.equal(detail.latestSnapshot.payloadDigest, "sha256:after");
  assert.equal(detail.snapshotDiffs.length, 1);
  assert.equal(detail.snapshotDiffs[0].status, "changed");
  assert.ok(detail.snapshotDiffs[0].changes.some((change) => change.key === "payloadDigest"));
  assert.ok(detail.snapshotDiffs[0].changes.some((change) => change.key === "awaitingGate"));
  assert.equal(detail.resumeReason, "server_restart_rehydration");
  assert.equal(detail.crashRecoverySnapshotDiff.label, "Crash recovery checkpoint");
  assert.ok(detail.allowedNextPhases.some((row) => row.label === "Running Phase"));
  assert.ok(detail.lockConstraints.some((row) => row.label === "Execution claim"));
  assert.ok(detail.lockConstraints.some((row) => row.label === "Tenant workspace"));
  assert.ok(detail.phaseHistory.some((row) => row.detail.includes("rehydrated blocked review")));
  assert.equal(detail.policyDecisions[0].detail, "github.comment · external_effect · approval-a");
  assert.equal(detail.toolEffects[0].status, "failed");
  assert.ok(detail.toolEffects[0].detail.includes("principal service_principal:svc-a"));
  assert.equal(detail.protectedAuditEvents[0].detail, "operator-a · sha256:audit");
  assert.equal(detail.counts.events, 2);
  assert.equal(detail.replay.firstSeq, 2);
  assert.equal(detail.replay.lastSeq, 4);
  assert.equal(detail.replay.isUnsafe, true);
  assert.ok(detail.replay.unsafeReasons.some((reason) => reason.includes("Outbox outbox-a is pending")));
  assert.equal(detail.payloadPolicy, "redacted_payload_digest_only");
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
	        enterprise_scope: {
	          owning_org_unit_id: "legal",
	          owning_org_unit: { display_name: "Legal Ops" },
	          resource_kind: "repository",
	          resource_id: "repo-hr",
	          policy_version_id: "policy-hr",
	          data_classes: ["confidential"],
	          visible_knowledge_sources: [{ binding_id: "source-hr", source_root_label: "HR Handbook" }],
	        },
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
	  assert.deepEqual(filterStatefulRunRows(rows, { orgUnit: "legal" }).map((row) => row.id), ["run-a"]);
	  assert.deepEqual(filterStatefulRunRows(rows, { resource: "repo-hr" }).map((row) => row.id), ["run-a"]);
	  assert.deepEqual(filterStatefulRunRows(rows, { policy: "policy-hr" }).map((row) => row.id), ["run-a"]);
	  assert.deepEqual(filterStatefulRunRows(rows, { dataClass: "confidential" }).map((row) => row.id), [
	    "run-a",
	  ]);
	  assert.deepEqual(filterStatefulRunRows(rows, { knowledge: "handbook" }).map((row) => row.id), ["run-a"]);
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
