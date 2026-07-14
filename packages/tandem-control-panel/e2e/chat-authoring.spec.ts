import { expect, test, waitForRoute } from "./fixtures/api";

test("setup guidance stays advisory and product authoring reaches the model", async ({
  page,
  apiFixture,
}) => {
  const prompt = "Create an automation that summarizes support tickets every morning";
  apiFixture.mockResponse(
    "/api/engine/setup/understand",
    {
      decision: "intercept",
      intent_kind: "automation_create",
      clarifier: null,
      slots: {
        provider_ids: [],
        model_ids: [],
        integration_targets: [],
        channel_targets: [],
        goal: prompt,
      },
      proposed_action: {
        type: "automation_create",
        payload: { prompt },
      },
    },
    "POST"
  );
  apiFixture.mockResponse("/api/engine/session", { id: "chat-authoring-session" }, "POST");
  const modelRequest = apiFixture.holdNext(
    "/api/engine/session/chat-authoring-session/prompt_async",
    "POST"
  );

  await page.goto("/#/chat");
  await waitForRoute(page, "chat");
  const composer = page.getByPlaceholder(
    "Ask anything... (Enter to send, Shift+Enter newline)"
  );
  await composer.fill(prompt);
  await page.getByRole("button", { name: "Send", exact: true }).click();

  await modelRequest.waitUntilRequested();
  await expect(page.getByText("Automation setup", { exact: true })).toBeVisible();
  await expect(page.getByText(prompt, { exact: true }).first()).toBeVisible();
  await expect(composer).toHaveValue("");
  expect(apiFixture.requests).toContain("POST /api/engine/setup/understand");
  expect(apiFixture.requests).toContain(
    "POST /api/engine/session/chat-authoring-session/prompt_async?return=run"
  );

  modelRequest.release();
});

test("setup-only prompts stop before model execution", async ({ page, apiFixture }) => {
  apiFixture.mockResponse(
    "/api/engine/setup/understand",
    {
      decision: "intercept",
      intent_kind: "provider_setup",
      clarifier: null,
      slots: {
        provider_ids: ["openai"],
        model_ids: [],
        integration_targets: [],
        channel_targets: [],
        goal: null,
      },
      proposed_action: { type: "provider_setup", payload: { provider_id: "openai" } },
    },
    "POST"
  );

  await page.goto("/#/chat");
  await waitForRoute(page, "chat");
  const composer = page.getByPlaceholder(
    "Ask anything... (Enter to send, Shift+Enter newline)"
  );
  await composer.fill("Help me configure OpenAI");
  await page.getByRole("button", { name: "Send", exact: true }).click();

  await expect(page.getByText("Provider setup", { exact: true })).toBeVisible();
  await expect(composer).toHaveValue("");
  expect(apiFixture.requests.some((request) => request.includes("/prompt_async"))).toBe(false);
});

test("linked workflow artifacts render parallel stages and conversational actions", async ({
  page,
  apiFixture,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("tcp.chat.session", "artifact-chat");
  });
  apiFixture.mockResponse(
    /\/api\/engine\/session(?:\?|$)/,
    { sessions: [{ id: "artifact-chat", title: "Artifact chat", source_kind: "chat" }] },
    "GET"
  );
  apiFixture.mockResponse(
    /\/api\/engine\/workflow-plans\/sessions\?linked_chat_session_id=artifact-chat/,
    {
      sessions: [
        {
          session_id: "wfplan-edited-but-not-active",
          linked_chat_session_id: "artifact-chat",
          title: "Edited background draft",
          project_slug: "chat-authoring",
          workspace_root: "/workspace",
          current_plan_id: "plan-background",
          plan_revision: 1,
          created_at_ms: 20,
          updated_at_ms: 100,
        },
        {
          session_id: "wfplan-artifact",
          linked_chat_session_id: "artifact-chat",
          title: "Support triage",
          project_slug: "chat-authoring",
          workspace_root: "/workspace",
          current_plan_id: "plan-artifact",
          plan_revision: 3,
          last_referenced_at_ms: 30,
          created_at_ms: 10,
          updated_at_ms: 30,
        },
      ],
      count: 1,
    },
    "GET"
  );
  apiFixture.mockResponse(
    "/api/engine/workflow-plans/sessions/wfplan-artifact",
    {
      session: {
        session_id: "wfplan-artifact",
        linked_chat_session_id: "artifact-chat",
        project_slug: "chat-authoring",
        title: "Support triage",
        workspace_root: "/workspace",
        current_plan_id: "plan-artifact",
        goal: "Triage incoming support requests and draft a response",
        created_at_ms: 10,
        updated_at_ms: 30,
        artifact_links: [
          {
            link_id: "planner-link",
            kind: "planner_session",
            resource_id: "wfplan-artifact",
            resource_url: "/#/planner?session_id=wfplan-artifact",
            revision: 3,
            linked_at_ms: 30,
          },
          {
            link_id: "stale-automation-link",
            kind: "automation",
            resource_id: "automation-from-revision-2",
            resource_url: "/#/automations?automation_id=automation-from-revision-2",
            revision: 2,
            linked_at_ms: 29,
          },
        ],
        planning: {
          validation_status: "blocked",
          approval_status: "required",
          missing_requirements: ["Slack destination"],
        },
        draft: {
          plan_revision: 3,
          initial_plan: {},
          conversation: { messages: [] },
          current_plan: {
            plan_id: "plan-artifact",
            title: "Support triage",
            description: "Classify requests in parallel, then prepare the response.",
            schedule: { type: "cron", expression: "0 8 * * 1-5" },
            execution_target: "server",
            allowed_mcp_servers: ["slack"],
            metadata: { assumptions: ["Requests arrive in the support queue"] },
            steps: [
              {
                step_id: "classify",
                kind: "analysis",
                objective: "Classify urgency",
                depends_on: [],
                agent_role: "triage",
                output_contract: { kind: "classification" },
              },
              {
                step_id: "research",
                kind: "research",
                objective: "Find relevant account context",
                depends_on: [],
                agent_role: "researcher",
                output_contract: { kind: "account_context" },
              },
              {
                step_id: "respond",
                kind: "generation",
                objective: "Draft the support response",
                depends_on: ["classify", "research"],
                agent_role: "writer",
                output_contract: { kind: "draft_reply" },
              },
            ],
          },
          review: {
            required_capabilities: ["slack.messages.write"],
            blocked_capabilities: ["slack.messages.write"],
            validation_status: "blocked",
            approval_status: "required",
            preview_payload: {
              validation: {
                issues: [
                  {
                    blocking: false,
                    code: "review_delivery_channel",
                    message: "Confirm the destination channel before publishing.",
                  },
                ],
              },
            },
          },
        },
      },
    },
    "GET"
  );

  await page.goto("/#/chat");
  await waitForRoute(page, "chat");

  const artifact = page.getByTestId("chat-workflow-artifact");
  await expect(artifact).toHaveCount(1);
  await expect(artifact.getByText("Support triage", { exact: true })).toBeVisible();
  await expect(artifact.getByText("2 parallel", { exact: true })).toBeVisible();
  await expect(artifact.getByText("Classify urgency", { exact: true })).toBeVisible();
  await expect(artifact.getByText("Find relevant account context", { exact: true })).toBeVisible();
  await expect(artifact.getByText("Connection required: slack.messages.write")).toBeVisible();
  await expect(
    artifact.getByText("Confirm the destination channel before publishing.")
  ).toBeVisible();
  await expect(artifact.getByRole("button", { name: "Create draft" })).toBeVisible();
  await expect(artifact.getByRole("button", { name: "Publish" })).toHaveCount(0);
  await expect(artifact.getByRole("button", { name: "Enable" })).toHaveCount(0);

  const viewport = page.viewportSize();
  const box = await artifact.boundingBox();
  expect(box).not.toBeNull();
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual((viewport?.width ?? 0) + 1);

  await artifact.getByRole("button", { name: "Revise" }).click();
  await expect(
    page.getByPlaceholder("Ask anything... (Enter to send, Shift+Enter newline)")
  ).toHaveValue('Revise workflow "Support triage" (revision 3): ');
  await expect(artifact).toHaveCount(1);
  await artifact.getByRole("button", { name: "Open canvas" }).click();
  await expect(page).toHaveURL(/#\/planner\?session_id=wfplan-artifact$/);
});

test("failed planner operations remain visible without remounting the artifact", async ({
  page,
  apiFixture,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem("tcp.chat.session", "failed-artifact-chat");
  });
  apiFixture.mockResponse(
    /\/api\/engine\/session(?:\?|$)/,
    {
      sessions: [
        { id: "failed-artifact-chat", title: "Failed artifact chat", source_kind: "chat" },
      ],
    },
    "GET"
  );
  apiFixture.mockResponse(
    /\/api\/engine\/workflow-plans\/sessions\?linked_chat_session_id=failed-artifact-chat/,
    {
      sessions: [
        {
          session_id: "wfplan-failed",
          linked_chat_session_id: "failed-artifact-chat",
          title: "Billing follow-up",
          project_slug: "chat-authoring",
          workspace_root: "/workspace",
          last_referenced_at_ms: 20,
          created_at_ms: 10,
          updated_at_ms: 20,
        },
      ],
      count: 1,
    },
    "GET"
  );
  apiFixture.mockResponse(
    "/api/engine/workflow-plans/sessions/wfplan-failed",
    {
      session: {
        session_id: "wfplan-failed",
        linked_chat_session_id: "failed-artifact-chat",
        project_slug: "chat-authoring",
        title: "Billing follow-up",
        workspace_root: "/workspace",
        goal: "Follow up on overdue invoices",
        created_at_ms: 10,
        updated_at_ms: 20,
        artifact_links: [
          {
            link_id: "failed-automation-link",
            kind: "automation",
            resource_id: "billing-follow-up-draft",
            resource_url: "/#/automations?automation_id=billing-follow-up-draft",
            linked_at_ms: 19,
          },
        ],
        operation: {
          request_id: "planner-request-failed",
          kind: "revise",
          status: "failed",
          started_at_ms: 18,
          finished_at_ms: 20,
          error: "The provider rejected the planner request.",
        },
      },
    },
    "GET"
  );

  await page.goto("/#/chat");
  await waitForRoute(page, "chat");

  const artifact = page.getByTestId("chat-workflow-artifact");
  await expect(artifact).toHaveCount(1);
  await expect(artifact.getByText("The provider rejected the planner request.")).toBeVisible();
  await expect(artifact.getByText("Workflow structure is being prepared.")).toBeVisible();
  await expect(artifact.getByRole("button", { name: "Validate" })).toBeEnabled();
  await expect(artifact.getByRole("button", { name: "Publish" })).toHaveCount(0);
  await expect(artifact.getByRole("button", { name: "Enable" })).toHaveCount(0);
});
