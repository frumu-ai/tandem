import { blankIconDescriptions, expect, test, waitForRoute } from "./fixtures/api";

const automation = {
  automation_id: "parallel-release",
  name: "Parallel release workflow",
  description: "Plan once, execute three tasks together, then publish.",
  status: "active",
  schedule: { kind: "manual" },
  workspace_root: "/tmp/tandem",
  execution: { max_parallel_agents: 3 },
  metadata: {
    operator_preferences: {
      execution_mode: "swarm",
      max_parallel_agents: 3,
    },
  },
  flow: {
    nodes: [
      {
        node_id: "plan",
        title: "Plan",
        objective: "Prepare the release plan.",
        agent_id: "planner",
        depends_on: [],
      },
      {
        node_id: "research",
        title: "Research",
        objective: "Check release dependencies.",
        agent_id: "researcher",
        depends_on: ["plan"],
      },
      {
        node_id: "implement",
        title: "Implement",
        objective: "Build the release artifacts.",
        agent_id: "builder",
        depends_on: ["plan"],
      },
      {
        node_id: "verify",
        title: "Verify",
        objective: "Run release verification.",
        agent_id: "reviewer",
        depends_on: ["plan"],
      },
      {
        node_id: "publish",
        title: "Publish",
        objective: "Publish the verified release.",
        agent_id: "publisher",
        depends_on: ["research", "implement", "verify"],
      },
    ],
  },
};

const missionBlueprintAutomation = {
  ...automation,
  automation_id: "mission-blueprint",
  name: "Mission blueprint workflow",
  metadata: {
    builder_kind: "mission_blueprint",
  },
};

function mockParallelAutomation(apiFixture: any) {
  apiFixture.mockResponse(
    "/api/engine/automations/v2?view=summary",
    { automations: [automation, missionBlueprintAutomation], count: 2 },
    "GET"
  );
  apiFixture.mockResponse(
    `/api/engine/automations/v2/${automation.automation_id}`,
    { automation },
    "GET"
  );
}

test("workflow library opens parallel Flow and task configuration views", async ({
  page,
  apiFixture,
}) => {
  mockParallelAutomation(apiFixture);
  await page.goto("/#/automations");
  await waitForRoute(page, "automations");
  await page.getByRole("button", { name: "Library", exact: true }).click();

  await expect(page.getByText(automation.name, { exact: true })).toBeVisible();
  await expect(page.getByText(missionBlueprintAutomation.name, { exact: true })).toBeVisible();
  const workflowCard = page.locator(".tcp-card").filter({ hasText: automation.name });
  await expect(workflowCard).toHaveCount(1);
  await expect(page.getByRole("button", { name: "View workflow flow" })).toHaveCount(1);
  await workflowCard.getByRole("button", { name: "View workflow flow" }).click();

  const flowTab = page.getByRole("tab", { name: "Flow", exact: true });
  const configureTab = page.getByRole("tab", { name: "Configure", exact: true });
  await expect(flowTab).toHaveAttribute("aria-selected", "true");
  await expect(page.getByText("3 concurrent / 3 max", { exact: true })).toBeVisible();
  await expect(page.getByText("1 parallel stage", { exact: true })).toBeVisible();
  await expect(page.getByText("Parallel", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Configure Research" })).toBeVisible();

  await page.getByRole("button", { name: "Configure Research" }).click();
  await expect(configureTab).toHaveAttribute("aria-selected", "true");
  const researchEditor = page.locator("#workflow-node-editor-research");
  await expect(researchEditor).toBeVisible();
  await expect(researchEditor.locator("textarea").first()).toHaveValue(
    "Check release dependencies."
  );

  await page.getByRole("button", { name: "Close workflow editor" }).click();
  await workflowCard.getByRole("button", { name: "Edit workflow automation" }).click();
  await expect(configureTab).toHaveAttribute("aria-selected", "true");
  expect(await blankIconDescriptions(page)).toEqual([]);

  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
  expect(overflow).toBeLessThanOrEqual(1);
});
