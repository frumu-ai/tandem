import AxeBuilder from "@axe-core/playwright";
import { expect, test, waitForRoute } from "./fixtures/api";

const profiles = [
  ["U_ACME_SALES", "Sales", "sales.account_viewer", "Allow", "Sales pipeline is available."],
  ["U_ACME_ENGINEERING", "Engineering", "engineering.delivery_viewer", "Allow", "Delivery status is available."],
  ["U_ACME_FINANCE", "Finance", "finance.financial_record_viewer", "ApprovalRequired", "Finance details require approval."],
  ["U_ACME_LEADERSHIP", "Leadership", "leadership.cross_functional_viewer", "Allow", "Cross-functional summary is available."],
  ["U_ACME_CONTRACTOR_X", "Contractor ACME-X", "contractor.project_x_viewer", "Deny", "Restricted data was not disclosed."],
] as const;

function idFor(userId: string) {
  return `ctx-acme-${userId.toLowerCase()}`;
}

function runFor(userId: string) {
  const id = idFor(userId);
  return {
    run_id: id,
    run_type: "session",
    source_client: "channel:slack",
    status: "completed",
    objective: "Summarize what I can access",
    updated_at_ms: 1000 - profiles.findIndex(([candidate]) => candidate === userId),
    source_metadata: {
      slack_team_id: "T_ACME",
      slack_app_id: "A_TANDEM",
      slack_channel_id: "C_ACME_EXEC",
      slack_thread_ts: "1710000000.000100",
      user_id: userId,
      scope_id: "acme",
    },
  };
}

function receiptFor(profile: (typeof profiles)[number]) {
  const [userId, department, role, decision, response] = profile;
  const id = idFor(userId);
  const manifest = {
    offered: decision === "Deny" ? ["project_x.read"] : ["memory.search", "report.read"],
    used: decision === "Allow" ? ["memory.search"] : [],
    hidden_by_scope: decision === "Deny" ? ["finance.records.read"] : [],
    blocked_by_approval: decision === "ApprovalRequired" ? ["finance.records.read"] : [],
    used_subset_offered: true,
  };
  const evidence: any = {
    package_type: "tandem_run_governance_evidence",
    run: {
      context_run_id: id,
      goal: "Summarize what I can access",
      counts: { tool_calls: manifest.used.length, policy_decisions: 1, approval_records: decision === "ApprovalRequired" ? 1 : 0, memory_audit_records: 1 },
      tenant_context: { actor_id: `actor-${department.toLowerCase().replaceAll(" ", "-")}` },
      source_metadata: runFor(userId).source_metadata,
    },
    actors: {
      tenant_actor_id: `actor-${department.toLowerCase().replaceAll(" ", "-")}`,
      requester_org_units: [department],
      requester_roles: [role],
    },
    tool_manifest: manifest,
    policy_decisions: [{ decision_id: `decision-${userId}`, tool: "report.read", reason_code: decision.toLowerCase(), reason: `${department} scope`, decision }],
    approvals: {
      pending_gate: decision === "ApprovalRequired" ? { approval_id: "approval-finance" } : null,
      gate_history: [],
    },
    memory_audit: [{ audit_id: `memory-${userId}`, action: "memory.search", partition_key: department, status: decision === "Deny" ? "denied" : "allowed" }],
    audit: { protected_events: [{ event_id: `audit-${userId}` }] },
    artifacts: [],
    limitations: [],
    redaction_policy: { memory_content: "department_scoped", credentials: "never_surface" },
    final_outcome: { context_status: "completed", automation_status: "completed", slack_visible_response: response },
  };
  if (userId === "U_ACME_LEADERSHIP") {
    delete evidence.memory_audit;
    delete evidence.audit;
  }
  return { ledger: { records: [], summary: { record_count: 0 }, tool_manifest: manifest }, evidence_package: evidence };
}

async function expectAccessible(page: Parameters<typeof waitForRoute>[0]) {
  const results = await new AxeBuilder({ page }).analyze();
  expect(results.violations).toEqual([]);
}

test("five governed Slack profiles expose accurate receipts", async ({ page, apiFixture }) => {
  const runs = profiles.map(([userId]) => runFor(userId));
  apiFixture.mockResponse("/api/engine/context/runs", { runs });
  for (const profile of profiles) {
    const id = idFor(profile[0]);
    const receipt = receiptFor(profile);
    apiFixture.mockResponse(`/api/engine/context/runs/${id}/ledger`, receipt.ledger);
    apiFixture.mockResponse(`/api/engine/context/runs/${id}/governance-evidence`, { evidence_package: receipt.evidence_package });
  }

  await page.goto("/#/slack-receipts");
  await waitForRoute(page, "slack-receipts");
  const selector = page.getByLabel("Select Slack governance receipt");
  await expect(selector.locator("option")).toHaveCount(5);

  for (const [userId, department, role, decision, response] of profiles) {
    await selector.selectOption(idFor(userId));
    await expect(page.getByText(department, { exact: true })).toBeVisible();
    await expect(page.getByText(role, { exact: true })).toBeVisible();
    await expect(page.getByText(decision, { exact: true })).toBeVisible();
    await expect(page.getByText(response, { exact: true })).toBeVisible();
  }

  await selector.selectOption(idFor("U_ACME_FINANCE"));
  await expect(page.getByText("Approval required", { exact: true })).toBeVisible();
  await expectAccessible(page);
  await selector.selectOption(idFor("U_ACME_CONTRACTOR_X"));
  await expect(page.getByText("Restricted data was not disclosed.", { exact: true })).toBeVisible();
  await expectAccessible(page);
  await selector.selectOption(idFor("U_ACME_LEADERSHIP"));
  await expect(page.getByText("Partial evidence", { exact: true })).toBeVisible();
  await expect(page.getByText("memory_audit", { exact: true })).toBeVisible();
  await expect(page.getByText("protected_audit", { exact: true })).toBeVisible();
  await expectAccessible(page);
});

test("failed or stale receipt exports remain explicit", async ({ page, apiFixture }) => {
  const failed = { ...runFor("U_ACME_FINANCE"), status: "failed", last_error: "receipt store unavailable" };
  apiFixture.mockResponse("/api/engine/context/runs", { runs: [failed] });
  apiFixture.mockResponse(`/api/engine/context/runs/${failed.run_id}/ledger`, {
    records: [],
    summary: { record_count: 0 },
    tool_manifest: {},
  });
  apiFixture.mockResponse(`/api/engine/context/runs/${failed.run_id}/governance-evidence`, {
    error: "Governance evidence is unavailable for this stale run",
  });

  await page.goto("/#/slack-receipts");
  await waitForRoute(page, "slack-receipts");
  await expect(page.getByText("Governance export unavailable", { exact: true })).toBeVisible();
  await expect(page.getByText("Governance evidence is unavailable for this stale run", { exact: true })).toBeVisible();
  await expect(page.getByText("failed", { exact: true })).toBeVisible();
  await expect(page.getByText("No Slack response captured", { exact: true })).toBeVisible();
  await expectAccessible(page);
});
