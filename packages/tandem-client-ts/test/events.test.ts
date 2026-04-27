import { readFileSync } from "node:fs";
import { describe, it, expect } from "vitest";
import { TandemClient } from "../src/client.js";
import { EngineEventSchema } from "../src/normalize/index.js";

const CONTRACT_PATH = "../../contracts/events.json";

describe("EngineEventSchema Contracts", () => {
  let eventsContract: Array<{ type: string; required: string[] }>;

  try {
    const raw = readFileSync(CONTRACT_PATH, "utf-8");
    eventsContract = JSON.parse(raw);
  } catch (err) {
    throw new Error(`Failed to load events.json from ${CONTRACT_PATH}: ${err}`);
  }

  it("has loaded contract definitions", () => {
    expect(eventsContract.length).toBeGreaterThan(0);
  });

  eventsContract.forEach((def) => {
    it(`validates and normalizes '${def.type}' events correctly`, () => {
      // Mock a tolerant wire payload
      const mockWirePayload: Record<string, unknown> = {
        type: def.type,
        timestamp: "2024-01-01T00:00:00Z",
        properties: { custom: "data" },
      };

      // Populate wire-specific required ID fields
      if (def.required.includes("sessionId")) {
        mockWirePayload.sessionID = "s_123"; // Wire name
      }
      if (def.required.includes("runId")) {
        mockWirePayload.run_id = "r_456"; // Wire name
      }

      // Parse through boundary normalized schema
      const result = EngineEventSchema.safeParse(mockWirePayload);
      expect(result.success).toBe(true);

      if (!result.success) return; // For TS narrow
      const event = result.data;

      // Assert canonical structure guarantees
      expect(event.type).toBe(def.type);
      expect(event.properties).toEqual({ custom: "data" });

      if (def.required.includes("sessionId")) {
        expect(event.sessionId).toBe("s_123");
      }
      if (def.required.includes("runId")) {
        expect(event.runId).toBe("r_456");
      }
    });
  });
});

describe("Coder SDK coverage", () => {
  it("posts memory path imports with canonical server payload", async () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });

    const originalFetch = globalThis.fetch;
    let requestedUrl = "";
    let requestInit: RequestInit | undefined;
    globalThis.fetch = (async (input, init) => {
      requestedUrl = String(input);
      requestInit = init;
      return new Response(
        JSON.stringify({
          ok: true,
          source: { kind: "path", path: "/srv/tandem/imports/company-docs" },
          format: "directory",
          tier: "project",
          project_id: "company-brain-demo",
          session_id: null,
          sync_deletes: true,
          discovered_files: 42,
          files_processed: 42,
          indexed_files: 39,
          skipped_files: 3,
          deleted_files: 0,
          chunks_created: 312,
          errors: 0,
        }),
        { status: 200, headers: { "Content-Type": "application/json" } }
      );
    }) as typeof fetch;

    try {
      const result = await client.memory.importPath({
        path: "/srv/tandem/imports/company-docs",
        format: "directory",
        tier: "project",
        projectId: "company-brain-demo",
        syncDeletes: true,
      });
      const body = JSON.parse(String(requestInit?.body || "{}"));
      expect(requestedUrl).toContain("/memory/import");
      expect(requestInit?.method).toBe("POST");
      expect(body).toEqual({
        source: { kind: "path", path: "/srv/tandem/imports/company-docs" },
        format: "directory",
        tier: "project",
        project_id: "company-brain-demo",
        sync_deletes: true,
      });
      expect(result.indexed_files).toBe(39);
      expect(result.chunks_created).toBe(312);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("lists coder runs with normalized query parameters", async () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });

    const originalFetch = globalThis.fetch;
    let requestedUrl = "";
    globalThis.fetch = (async (input) => {
      requestedUrl = String(input);
      return new Response(JSON.stringify({ runs: [{ coder_run_id: "coder-1" }] }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const result = await client.coder.listRuns({
        limit: 10,
        workflowMode: "issue_triage",
        repoSlug: "user123/tandem",
      });
      expect(requestedUrl).toContain("/coder/runs?");
      expect(requestedUrl).toContain("limit=10");
      expect(requestedUrl).toContain("workflow_mode=issue_triage");
      expect(requestedUrl).toContain("repo_slug=user123%2Ftandem");
      expect(result.runs[0]?.coder_run_id).toBe("coder-1");
      expect(result.count).toBe(1);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("posts approve run requests to the coder endpoint", async () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });

    const originalFetch = globalThis.fetch;
    let requestInit: RequestInit | undefined;
    globalThis.fetch = (async (_input, init) => {
      requestInit = init;
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const result = await client.coder.approveRun("coder-2", "ship it");
      expect(requestInit?.method).toBe("POST");
      expect(String(requestInit?.body)).toContain("ship it");
      expect(result.ok).toBe(true);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});

describe("Workflow planning SDK coverage", () => {
  it("routes preview, apply, and import bundle calls", async () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });

    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string }> = [];
    globalThis.fetch = (async (input, init) => {
      calls.push({ url: String(input), method: String(init?.method ?? "GET") });
      const url = String(input);
      if (url.endsWith("/workflow-plans/import/preview")) {
        return new Response(
          JSON.stringify({
            ok: true,
            bundle: { bundle: "import" },
            import_validation: { compatible: true },
            plan_package_preview: { plan_id: "plan-1" },
            derived_scope_snapshot: { plan_id: "plan-1" },
            summary: { plan_id: "plan-1" },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }
      if (url.endsWith("/workflow-plans/import")) {
        return new Response(
          JSON.stringify({
            ok: true,
            bundle: { bundle: "import" },
            import_validation: { compatible: true },
            plan_package_preview: { plan_id: "plan-1" },
            derived_scope_snapshot: { plan_id: "plan-1" },
            summary: { plan_id: "plan-1" },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }
      return new Response(
        JSON.stringify({
          plan: {
            plan_id: "plan-1",
            title: "Release checklist",
            schedule: { type: "manual" },
            steps: [{ step_id: "step-1", kind: "task", objective: "Review changelog" }],
          },
          plan_package_bundle: { bundle: "preview" },
          plan_package_validation: { compatible: true },
        }),
        { status: 200, headers: { "Content-Type": "application/json" } }
      );
    }) as typeof fetch;

    try {
      const preview = await client.workflowPlans.preview({ prompt: "Create a release checklist" });
      const applied = await client.workflowPlans.apply({ planId: preview.plan.plan_id });
      const importedPreview = await client.workflowPlans.importPreview({ bundle: { bundle: "x" } });
      const imported = await client.workflowPlans.importPlan({ bundle: { bundle: "x" } });

      expect(preview.plan.plan_id).toBe("plan-1");
      expect(applied.plan_package_bundle).toEqual({ bundle: "preview" });
      expect(importedPreview.import_validation).toEqual({ compatible: true });
      expect(imported.summary).toEqual({ plan_id: "plan-1" });
      expect(calls[0]?.url).toContain("/workflow-plans/preview");
      expect(calls[1]?.url).toContain("/workflow-plans/apply");
      expect(calls[2]?.url).toContain("/workflow-plans/import/preview");
      expect(calls[3]?.url).toContain("/workflow-plans/import");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});

describe("Workflow planner session SDK coverage", () => {
  it("routes session CRUD and chat methods to the new planner-session endpoints", async () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });

    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string; body?: string }> = [];
    let activeOperation: {
      kind: "start" | "message";
      requestId: string;
      updatedAtMs: number;
      messages: Array<{ role: string; text: string }>;
    } | null = null;
    globalThis.fetch = (async (input, init) => {
      const url = String(input);
      const body = typeof init?.body === "string" ? init.body : String(init?.body ?? "");
      calls.push({ url, method: String(init?.method ?? "GET"), body });

      if (url.endsWith("/workflow-plans/sessions") && String(init?.method ?? "GET") === "POST") {
        return new Response(
          JSON.stringify({
            session: {
              session_id: "wfplan-session-1",
              project_slug: "planner-project",
              title: "Planner session",
              workspace_root: "/workspace/repos/demo",
              current_plan_id: "wfplan-1",
              draft: {
                initial_plan: {
                  plan_id: "wfplan-1",
                  title: "Planner session",
                  schedule: { type: "manual" },
                  steps: [],
                },
                current_plan: {
                  plan_id: "wfplan-1",
                  title: "Planner session",
                  schedule: { type: "manual" },
                  steps: [],
                },
                conversation: { messages: [] },
              },
              created_at_ms: 1,
              updated_at_ms: 2,
            },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      if (url.includes("/workflow-plans/sessions?project_slug=")) {
        return new Response(
          JSON.stringify({
            sessions: [{ session_id: "wfplan-session-1", title: "Planner session" }],
            count: 1,
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      if (url.endsWith("/workflow-plans/sessions/wfplan-session-1")) {
        if (String(init?.method ?? "GET") === "GET" && activeOperation) {
          const responsePayload =
            activeOperation.kind === "start"
              ? {
                  session: {
                    session_id: "wfplan-session-1",
                    project_slug: "planner-project",
                    title: "Planner session",
                    workspace_root: "/workspace/repos/demo",
                    current_plan_id: "wfplan-3",
                    created_at_ms: 1,
                    updated_at_ms: activeOperation.updatedAtMs,
                    operation: {
                      request_id: activeOperation.requestId,
                      kind: activeOperation.kind,
                      status: "completed",
                      started_at_ms: 5,
                      finished_at_ms: activeOperation.updatedAtMs,
                      response: {
                        session: {
                          session_id: "wfplan-session-1",
                          project_slug: "planner-project",
                          title: "Planner session",
                          workspace_root: "/workspace/repos/demo",
                          current_plan_id: "wfplan-3",
                          created_at_ms: 1,
                          updated_at_ms: activeOperation.updatedAtMs,
                        },
                        plan: {
                          plan_id: "wfplan-3",
                          title: "Planner session",
                          schedule: { type: "manual" },
                          steps: [],
                        },
                        conversation: { messages: [] },
                        change_summary: [],
                        planner_diagnostics: { mode: "start" },
                        clarifier: { status: "none" },
                      },
                      error: null,
                    },
                  },
                }
              : {
                  session: {
                    session_id: "wfplan-session-1",
                    project_slug: "planner-project",
                    title: "Planner session",
                    workspace_root: "/workspace/repos/demo",
                    current_plan_id: "wfplan-3",
                    created_at_ms: 1,
                    updated_at_ms: activeOperation.updatedAtMs,
                    operation: {
                      request_id: activeOperation.requestId,
                      kind: activeOperation.kind,
                      status: "completed",
                      started_at_ms: 6,
                      finished_at_ms: activeOperation.updatedAtMs,
                      response: {
                        session: {
                          session_id: "wfplan-session-1",
                          project_slug: "planner-project",
                          title: "Planner session",
                          workspace_root: "/workspace/repos/demo",
                          current_plan_id: "wfplan-3",
                          created_at_ms: 1,
                          updated_at_ms: activeOperation.updatedAtMs,
                        },
                        plan: {
                          plan_id: "wfplan-3",
                          title: "Planner session",
                          schedule: { type: "manual" },
                          steps: [],
                        },
                        conversation: { messages: activeOperation.messages },
                        change_summary: ["updated"],
                        planner_diagnostics: { mode: "message" },
                        clarifier: { status: "none" },
                      },
                      error: null,
                    },
                  },
                };
          return new Response(JSON.stringify(responsePayload), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          });
        }
        if (String(init?.method ?? "GET") === "PATCH") {
          return new Response(
            JSON.stringify({
              session: {
                session_id: "wfplan-session-1",
                project_slug: "planner-project",
                title: "Planner session renamed",
                workspace_root: "/workspace/repos/demo",
                current_plan_id: "wfplan-1",
                created_at_ms: 1,
                updated_at_ms: 3,
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } }
          );
        }
        if (String(init?.method ?? "GET") === "DELETE") {
          return new Response(
            JSON.stringify({
              ok: true,
              session: {
                session_id: "wfplan-session-1",
                project_slug: "planner-project",
              },
            }),
            { status: 200, headers: { "Content-Type": "application/json" } }
          );
        }
        return new Response(
          JSON.stringify({
            session: {
              session_id: "wfplan-session-1",
              project_slug: "planner-project",
              title: "Planner session",
              workspace_root: "/workspace/repos/demo",
              current_plan_id: "wfplan-1",
              created_at_ms: 1,
              updated_at_ms: 2,
            },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      if (url.includes("/workflow-plans/sessions/wfplan-session-1/duplicate")) {
        return new Response(
          JSON.stringify({
            session: {
              session_id: "wfplan-session-2",
              project_slug: "planner-project",
              title: "Copy of Planner session",
              workspace_root: "/workspace/repos/demo",
              current_plan_id: "wfplan-2",
              created_at_ms: 4,
              updated_at_ms: 4,
            },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      if (url.includes("/workflow-plans/sessions/wfplan-session-1/start-async")) {
        activeOperation = {
          kind: "start",
          requestId: "wfplan-op-start-1",
          updatedAtMs: 5,
          messages: [],
        };
        return new Response(
          JSON.stringify({
            session: {
              session_id: "wfplan-session-1",
              project_slug: "planner-project",
              title: "Planner session",
              workspace_root: "/workspace/repos/demo",
              current_plan_id: "wfplan-3",
              created_at_ms: 1,
              updated_at_ms: 5,
              operation: {
                request_id: activeOperation.requestId,
                kind: activeOperation.kind,
                status: "running",
                started_at_ms: 5,
                finished_at_ms: null,
                response: null,
                error: null,
              },
            },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      if (url.includes("/workflow-plans/sessions/wfplan-session-1/message-async")) {
        activeOperation = {
          kind: "message",
          requestId: "wfplan-op-message-1",
          updatedAtMs: 6,
          messages: [{ role: "assistant", text: "ok" }],
        };
        return new Response(
          JSON.stringify({
            session: {
              session_id: "wfplan-session-1",
              project_slug: "planner-project",
              title: "Planner session",
              workspace_root: "/workspace/repos/demo",
              current_plan_id: "wfplan-3",
              created_at_ms: 1,
              updated_at_ms: 6,
              operation: {
                request_id: activeOperation.requestId,
                kind: activeOperation.kind,
                status: "running",
                started_at_ms: 6,
                finished_at_ms: null,
                response: null,
                error: null,
              },
            },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      if (url.includes("/workflow-plans/sessions/wfplan-session-1/reset")) {
        return new Response(
          JSON.stringify({
            session: {
              session_id: "wfplan-session-1",
              project_slug: "planner-project",
              title: "Planner session",
              workspace_root: "/workspace/repos/demo",
              current_plan_id: "wfplan-3",
              created_at_ms: 1,
              updated_at_ms: 7,
            },
            plan: {
              plan_id: "wfplan-3",
              title: "Planner session",
              schedule: { type: "manual" },
              steps: [],
            },
            conversation: { messages: [] },
            planner_diagnostics: { mode: "reset" },
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      }

      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const listed = await client.workflowPlannerSessions.list({ project_slug: "planner-project" });
      const created = await client.workflowPlannerSessions.create({
        project_slug: "planner-project",
        title: "Planner session",
        workspace_root: "/workspace/repos/demo",
        goal: "Ship a planner session",
        notes: "seeded by test",
        plan_source: "coding_task_planning",
      });
      const fetched = await client.workflowPlannerSessions.get("wfplan-session-1");
      const patched = await client.workflowPlannerSessions.patch("wfplan-session-1", {
        title: "Planner session renamed",
      });
      const duplicated = await client.workflowPlannerSessions.duplicate("wfplan-session-1", {
        title: "Copy of Planner session",
      });
      const started = await client.workflowPlannerSessions.start("wfplan-session-1", {
        prompt: "Create a planner session",
        workspace_root: "/workspace/repos/demo",
        plan_source: "coding_task_planning",
      });
      const messaged = await client.workflowPlannerSessions.message("wfplan-session-1", {
        message: "Revise the plan",
      });
      const reset = await client.workflowPlannerSessions.reset("wfplan-session-1");
      const deleted = await client.workflowPlannerSessions.delete("wfplan-session-1");

      expect(listed.sessions.length).toBe(1);
      expect(created.session.session_id).toBe("wfplan-session-1");
      expect(fetched.session.current_plan_id).toBe("wfplan-1");
      expect(patched.session.title).toBe("Planner session renamed");
      expect(duplicated.session.session_id).toBe("wfplan-session-2");
      expect(started.session?.current_plan_id).toBe("wfplan-3");
      expect(messaged.session?.updated_at_ms).toBe(6);
      expect(reset.planner_diagnostics).toEqual({ mode: "reset" });
      expect(deleted.ok).toBe(true);
      expect(calls[0]?.url).toContain("/workflow-plans/sessions?project_slug=planner-project");
      expect(calls[1]?.method).toBe("POST");
      expect(calls[2]?.method).toBe("GET");
      expect(calls[3]?.method).toBe("PATCH");
      expect(calls[4]?.method).toBe("POST");
      expect(calls.some((call) => call.url.includes("/start-async"))).toBe(true);
      expect(calls.some((call) => call.url.includes("/message-async"))).toBe(true);
      expect(calls.some((call) => call.url.includes("/reset"))).toBe(true);
      expect(calls[calls.length - 1]?.method).toBe("DELETE");
      expect(String(calls[1]?.body || "")).toContain("planner-project");
      expect(String(calls.find((call) => call.url.includes("/start-async"))?.body || "")).toContain(
        "Create a planner session"
      );
      expect(
        String(calls.find((call) => call.url.includes("/message-async"))?.body || "")
      ).toContain("Revise the plan");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});

describe("High-value parity coverage", () => {
  it("exposes new top-level namespaces", () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });

    expect(client.browser).toBeTruthy();
    expect(client.workflows).toBeTruthy();
    expect(client.bugMonitor).toBeTruthy();
  });

  it("hits browser status and install endpoints", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const urls: string[] = [];
    globalThis.fetch = (async (input) => {
      urls.push(String(input));
      return new Response(JSON.stringify({ ok: true, runnable: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      await client.browser.status();
      await client.browser.install();
      await client.browser.smokeTest({ url: "https://example.com" });
      expect(urls[0]).toContain("/browser/status");
      expect(urls[1]).toContain("/browser/install");
      expect(urls[2]).toContain("/browser/smoke-test");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("hits workflow list, run, and patch hook endpoints", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string }> = [];
    globalThis.fetch = (async (input, init) => {
      calls.push({ url: String(input), method: String(init?.method ?? "GET") });
      return new Response(JSON.stringify({ workflows: [], count: 0, run: {}, hook: {} }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      await client.workflows.list();
      await client.workflows.run("wf-1");
      await client.workflows.patchHook("hook-1", { enabled: false });
      expect(calls[0]?.url).toContain("/workflows");
      expect(calls[1]?.url).toContain("/workflows/wf-1/run");
      expect(calls[1]?.method).toBe("POST");
      expect(calls[2]?.url).toContain("/workflow-hooks/hook-1");
      expect(calls[2]?.method).toBe("PATCH");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("uses canonical bug monitor routes", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const urls: string[] = [];
    globalThis.fetch = (async (input) => {
      urls.push(String(input));
      return new Response(JSON.stringify({ status: {}, drafts: [], count: 0, ok: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      await client.bugMonitor.getStatus();
      await client.bugMonitor.listDrafts({ limit: 3 });
      await client.bugMonitor.approveDraft("draft-1", "yes");
      expect(urls[0]).toContain("/bug-monitor/status");
      expect(urls[1]).toContain("/bug-monitor/drafts?limit=3");
      expect(urls[2]).toContain("/bug-monitor/drafts/draft-1/approve");
      expect(urls.every((url) => !url.includes("/failure-reporter/"))).toBe(true);
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("returns raw TOML from the MCP catalog endpoint", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    globalThis.fetch = (async () =>
      new Response("name = 'demo'\n", {
        status: 200,
        headers: { "Content-Type": "text/plain" },
      })) as typeof fetch;

    try {
      const toml = await client.mcp.catalogToml("demo");
      expect(toml).toContain("name = 'demo'");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("encodes resource keys for get and patch", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const urls: string[] = [];
    globalThis.fetch = (async (input) => {
      urls.push(String(input));
      return new Response(JSON.stringify({ key: "a/b", ok: true, rev: 1, value: {} }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      await client.resources.get("a/b");
      await client.resources.patchKey("a/b", { value: { ok: true } });
      expect(urls[0]).toContain("/resource/a%2Fb");
      expect(urls[1]).toContain("/resource/a%2Fb");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("posts routine and automation artifacts to artifact endpoints", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: string[] = [];
    globalThis.fetch = (async (input) => {
      calls.push(String(input));
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      await client.routines.addArtifact("run-r", { uri: "file://a", kind: "report" });
      await client.automations.addArtifact("run-a", { uri: "file://b", kind: "report" });
      expect(calls[0]).toContain("/routines/runs/run-r/artifacts");
      expect(calls[1]).toContain("/automations/runs/run-a/artifacts");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
