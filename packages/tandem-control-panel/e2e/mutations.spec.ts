import { blankIconDescriptions, expect, test, waitForRoute } from "./fixtures/api";

test("save icon survives the complete mutation loading lifecycle", async ({ page, apiFixture }) => {
  await page.goto("/#/settings");
  await waitForRoute(page, "settings");

  const save = page.getByRole("button", { name: "Save config" });
  await expect(save).toBeEnabled();
  await expect(save.locator("svg.lucide-save")).toHaveCount(1);
  await expect(save.locator("svg.lucide-save > *")).not.toHaveCount(0);

  const held = apiFixture.holdNext("/api/control-panel/config", "PATCH");
  await save.click();
  await held.waitUntilRequested();
  await expect(save).toBeDisabled();
  await expect(save.locator("svg.lucide-save > *")).not.toHaveCount(0);
  expect(await blankIconDescriptions(page)).toEqual([]);

  held.release();
  await expect(save).toBeEnabled();
  await expect(save.locator("svg.lucide-save > *")).not.toHaveCount(0);
  expect(await blankIconDescriptions(page)).toEqual([]);
});

test("Studio save exposes its loader until the workflow is persisted", async ({
  page,
  apiFixture,
}) => {
  apiFixture.mockResponse(
    "/api/system/health",
    {
      engine: { ready: true, healthy: true },
      engineUrl: "fixture://tandem",
      workspaceRoot: "/tmp/e2e",
    },
    "GET"
  );
  apiFixture.mockResponse(
    "/api/engine/automations/v2",
    {
      automations: [
        {
          automation_id: "studio-e2e",
          name: "E2E Studio Workflow",
          description: "A compact persisted Studio fixture.",
          status: "draft",
          workspace_root: "/tmp/e2e",
          metadata: {
            studio: {
              version: 2,
              created_from: "studio",
              workflow: { status: "draft", schedule_type: "manual", output_targets: [] },
              agent_drafts: [
                {
                  agentId: "writer",
                  displayName: "Writer",
                  role: "worker",
                  skills: [],
                  prompt: { mission: "Write the requested artifact." },
                  modelProvider: "openai",
                  modelId: "gpt-5-mini",
                  toolAllowlist: [],
                  toolDenylist: [],
                  mcpAllowedServers: [],
                  mcpAllowedTools: null,
                  mcpOtherAllowedTools: [],
                  mcpAllowedConnections: [],
                },
              ],
              node_drafts: [
                {
                  nodeId: "draft",
                  title: "Draft",
                  objective: "Write the requested artifact.",
                  agentId: "writer",
                  dependsOn: [],
                },
              ],
            },
          },
        },
      ],
    },
    "GET"
  );
  apiFixture.mockResponse(
    "/api/engine/automations/v2/studio-e2e",
    { automation: { automation_id: "studio-e2e" } },
    "PATCH"
  );
  await page.goto("/#/studio");
  await waitForRoute(page, "studio");

  const save = page.getByRole("button", { name: /^(Save Workflow|Saving\.\.\.)$/ });
  await page.getByText("E2E Studio Workflow", { exact: true }).click();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("textbox", { name: "Name", exact: true })).toHaveValue(
    "E2E Studio Workflow"
  );
  await expect(save).toBeEnabled();
  const held = apiFixture.holdNext("/api/engine/automations/v2/studio-e2e", "PATCH");
  await save.click();
  await held.waitUntilRequested();
  await expect(page.getByRole("button", { name: "Saving..." })).toBeDisabled();
  await expect(save.locator("svg.lucide-loader-circle")).toHaveCount(1);
  expect(await blankIconDescriptions(page)).toEqual([]);

  held.release();
  await expect(page.getByText("Studio workflow saved.", { exact: true })).toBeVisible();
  await expect(save).toBeEnabled();
  await expect(save.locator("svg.lucide-save")).toHaveCount(1);
});

test("Coder repository sync exposes its loader until refreshed repo data returns", async ({
  page,
  apiFixture,
}) => {
  apiFixture.mockResponse(
    "/api/capabilities",
    {
      aca_integration: true,
      coding_workflows: true,
      missions: true,
      agent_teams: true,
      coder: true,
      engine_healthy: true,
      control_panel_mode: "standalone",
      control_panel_config_ready: true,
      workspace_files_available: true,
      workspace_files_api_available: true,
    },
    "GET"
  );
  apiFixture.mockResponse(
    "/api/aca/projects",
    [
      {
        slug: "tandem-e2e",
        name: "Tandem E2E",
        repo: { path: "/tmp/tandem-e2e", default_branch: "main" },
        task_source: { type: "local_backlog", path: "/tmp/tandem-e2e/tasks.json" },
      },
    ],
    "GET"
  );
  apiFixture.mockResponse(
    "/api/aca/projects/tandem-e2e/repo/sync",
    { repo: { path: "/tmp/tandem-e2e", commit: "1234567890abcdef", dirty: false } },
    "POST"
  );
  await page.goto("/#/coding");
  await waitForRoute(page, "coding");

  const sync = page.getByRole("button", { name: /^(Sync repo|Syncing)$/ });
  await expect(sync).toBeEnabled();
  const held = apiFixture.holdNext("/api/aca/projects/tandem-e2e/repo/sync", "POST");
  await sync.click();
  await held.waitUntilRequested();
  await expect(page.getByRole("button", { name: "Syncing" })).toBeDisabled();
  await expect(sync.locator("svg.lucide-loader-circle")).toHaveCount(1);
  expect(await blankIconDescriptions(page)).toEqual([]);

  held.release();
  await expect(page.getByText(/Ready at \/tmp\/tandem-e2e.*1234567/)).toBeVisible();
  await expect(sync).toBeEnabled();
  await expect(sync.locator("svg.lucide-refresh-cw")).toHaveCount(1);
});
