import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  AnimatedPage,
  Badge,
  DetailDrawer,
  MotionNumber,
  PanelCard,
  SplitView,
  StaggerGroup,
  StatusPulse,
  Toolbar,
} from "../ui/index.tsx";
import { EmptyState, formatJson } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function formatCompactNumber(value: number) {
  return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(
    Number(value || 0)
  );
}

export function DashboardPage(props: AppPageProps) {
  const { api, client, navigate, providerStatus } = props;
  const [selectedWorkflowContextRunId, setSelectedWorkflowContextRunId] = useState("");
  const visibleContextRunTypes = new Set(["workflow", "failure_reporter_triage"]);

  const health = useQuery({
    queryKey: ["dashboard", "health"],
    queryFn: () => api("/api/system/health"),
    refetchInterval: 15000,
  });
  const sessions = useQuery({
    queryKey: ["dashboard", "sessions"],
    queryFn: () => client.sessions.list({ pageSize: 8 }).catch(() => []),
    refetchInterval: 15000,
  });
  const routines = useQuery({
    queryKey: ["dashboard", "routines"],
    queryFn: () => client.routines.list().catch(() => ({ routines: [] })),
    refetchInterval: 20000,
  });
  const swarm = useQuery({
    queryKey: ["dashboard", "swarm"],
    queryFn: () => api("/api/swarm/status").catch(() => ({ status: "unknown", activeRuns: 0 })),
    refetchInterval: 6000,
  });
  const workflowContexts = useQuery({
    queryKey: ["dashboard", "workflow-context-runs"],
    queryFn: () => api("/api/engine/context/runs?limit=12").catch(() => ({ runs: [] })),
    refetchInterval: 6000,
  });
  const workflowContextDetail = useQuery({
    queryKey: ["dashboard", "workflow-context-run", selectedWorkflowContextRunId],
    enabled: !!selectedWorkflowContextRunId,
    queryFn: () =>
      api(`/api/engine/context/runs/${encodeURIComponent(selectedWorkflowContextRunId)}`).catch(
        () => ({ run: null })
      ),
  });
  const workflowContextBlackboard = useQuery({
    queryKey: ["dashboard", "workflow-context-blackboard", selectedWorkflowContextRunId],
    enabled: !!selectedWorkflowContextRunId,
    queryFn: () =>
      api(
        `/api/engine/context/runs/${encodeURIComponent(selectedWorkflowContextRunId)}/blackboard`
      ).catch(() => ({ blackboard: null })),
  });

  const sessionRows = toArray(sessions.data, "sessions");
  const routineRows = toArray(routines.data, "routines");
  const workflowContextRows = toArray(workflowContexts.data, "runs").filter((run: any) =>
    visibleContextRunTypes.has(
      String(run?.run_type || "")
        .trim()
        .toLowerCase()
    )
  );
  const healthy = !!(health.data?.engine?.ready || health.data?.engine?.healthy);
  const swarmStatus = String(swarm.data?.status || "unknown");
  const swarmRunning = ["planning", "awaiting_approval", "running", "executing"].includes(
    swarmStatus.toLowerCase()
  );
  const activeWorkflowContexts = workflowContextRows.filter((run: any) =>
    ["queued", "planning", "running", "awaiting_approval"].includes(
      String(run?.status || "")
        .trim()
        .toLowerCase()
    )
  );

  const overviewStats = useMemo(
    () => [
      {
        label: "Recent sessions",
        value: sessionRows.length,
        tone: "info" as const,
        helper: "Latest active conversation surfaces",
      },
      {
        label: "Automations",
        value: routineRows.length,
        tone: "ok" as const,
        helper: "Configured routines and schedules",
      },
      {
        label: "Swarm status",
        value: swarmRunning ? 1 : 0,
        tone: swarmRunning ? ("warn" as const) : ("ghost" as const),
        helper: swarmStatus,
      },
      {
        label: "Provider ready",
        value: providerStatus.ready ? 1 : 0,
        tone: providerStatus.ready ? ("ok" as const) : ("warn" as const),
        helper: providerStatus.ready ? providerStatus.defaultModel : "Needs setup",
      },
      {
        label: "Workflow contexts",
        label: "Context runs",
        value: workflowContextRows.length,
        tone: activeWorkflowContexts.length ? ("warn" as const) : ("info" as const),
        helper: activeWorkflowContexts.length
          ? `${activeWorkflowContexts.length} active`
          : "Workflow + triage visibility",
      },
    ],
    [
      activeWorkflowContexts.length,
      providerStatus.defaultModel,
      providerStatus.ready,
      routineRows.length,
      sessionRows.length,
      swarmRunning,
      swarmStatus,
      workflowContextRows.length,
    ]
  );

  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard className="overflow-hidden">
        <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
          <div className="min-w-0">
            <div className="tcp-page-eyebrow">Overview</div>
            <h1 className="tcp-page-title">Command center</h1>
            <p className="tcp-subtle mt-2 max-w-3xl">
              A higher-signal home screen with animated state, quick entry points, and a clearer
              read on what the system is doing right now.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Badge tone={healthy ? "ok" : "warn"}>
                {healthy ? "Engine healthy" : "Engine checking"}
              </Badge>
              <Badge tone={providerStatus.ready ? "ok" : "warn"}>
                {providerStatus.ready ? providerStatus.defaultProvider : "Provider setup required"}
              </Badge>
              {swarmRunning ? (
                <StatusPulse tone="live" text={`Swarm ${swarmStatus}`} />
              ) : (
                <Badge tone="ghost">Swarm {swarmStatus}</Badge>
              )}
            </div>
          </div>
          <div className="grid gap-3">
            <Toolbar className="justify-start">
              <button className="tcp-btn-primary w-full sm:w-auto" onClick={() => navigate("chat")}>
                <i data-lucide="message-square"></i>
                Chat session
              </button>
              <button className="tcp-btn w-full sm:w-auto" onClick={() => navigate("orchestrator")}>
                <i data-lucide="sparkles"></i>
                Plan work
              </button>
              <button className="tcp-btn w-full sm:w-auto" onClick={() => navigate("automations")}>
                <i data-lucide="bot"></i>
                Automations
              </button>
              <button className="tcp-btn w-full sm:w-auto" onClick={() => navigate("settings")}>
                <i data-lucide="settings"></i>
                Configure runtime
              </button>
            </Toolbar>
          </div>
        </div>

        <div className="mt-5">
          <div className="dashboard-kpis">
            {overviewStats.map((stat) => (
              <div key={stat.label}>
                <div className="flex items-start justify-between gap-3">
                  <span className="dashboard-kpi-label">{stat.label}</span>
                  <Badge tone={stat.tone}>{stat.helper}</Badge>
                </div>
                <strong>
                  <MotionNumber value={stat.value} format={formatCompactNumber} />
                </strong>
              </div>
            ))}
          </div>

          <div className="mt-4 dashboard-bars">
            <div className="dashboard-bar-row">
              <div className="dashboard-bar-meta">
                <span>Engine readiness</span>
                <span className="dashboard-bar-count">{healthy ? "100%" : "45%"}</span>
              </div>
              <div className="dashboard-bar-track">
                <span
                  className="dashboard-bar-fill completed"
                  style={{ width: healthy ? "100%" : "45%" }}
                ></span>
              </div>
            </div>
            <div className="dashboard-bar-row">
              <div className="dashboard-bar-meta">
                <span>Provider confidence</span>
                <span className="dashboard-bar-count">{providerStatus.ready ? "100%" : "30%"}</span>
              </div>
              <div className="dashboard-bar-track">
                <span
                  className={`dashboard-bar-fill ${providerStatus.ready ? "running" : "queued"}`}
                  style={{ width: providerStatus.ready ? "100%" : "30%" }}
                ></span>
              </div>
            </div>
            <div className="dashboard-bar-row">
              <div className="dashboard-bar-meta">
                <span>Automation activity</span>
                <span className="dashboard-bar-count">{routineRows.length} routines</span>
              </div>
              <div className="dashboard-bar-track">
                <span
                  className={`dashboard-bar-fill ${routineRows.length ? "scheduled" : "manual"}`}
                  style={{ width: `${Math.min(100, Math.max(12, routineRows.length * 12))}%` }}
                ></span>
              </div>
            </div>
          </div>
        </div>
      </PanelCard>

      <SplitView
        main={
          <PanelCard title="Recent sessions" subtitle="Latest conversations, ready to reopen.">
            <div className="grid gap-2">
              {sessionRows.length ? (
                sessionRows.map((session: any) => (
                  <button
                    key={String(session.id || session.session_id || Math.random())}
                    className="tcp-list-item text-left"
                    onClick={() => navigate("chat")}
                  >
                    <div className="font-medium inline-flex items-center gap-2">
                      <i data-lucide="messages-square"></i>
                      {String(session.title || session.id || "Session")}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {String(session.id || session.session_id || "")}
                    </div>
                  </button>
                ))
              ) : (
                <EmptyState text="Start a conversation to populate recent sessions here." />
              )}
            </div>
          </PanelCard>
        }
        aside={
          <div className="grid gap-4">
            <PanelCard
              title="Context visibility"
              subtitle="Recent workflow and failure-triage context runs with their current projection state."
            >
              <Toolbar className="mb-3">
                <Badge tone={activeWorkflowContexts.length ? "warn" : "info"}>
                  {workflowContextRows.length} recent
                </Badge>
                <button className="tcp-btn" onClick={() => navigate("orchestrator")}>
                  <i data-lucide="workflow"></i>
                  Open orchestrator
                </button>
              </Toolbar>
              <div className="grid gap-2">
                {workflowContextRows.length ? (
                  workflowContextRows.slice(0, 5).map((run: any) => (
                    <button
                      key={String(run?.run_id || Math.random())}
                      className="tcp-list-item text-left"
                      onClick={() =>
                        setSelectedWorkflowContextRunId(String(run?.run_id || "").trim())
                      }
                    >
                      <div className="mb-1 flex items-center justify-between gap-2">
                        <div className="font-medium">
                          {String(run?.objective || run?.run_id || "Context run")}
                        </div>
                        <div className="flex flex-wrap items-center gap-2">
                          <Badge tone="ghost">{String(run?.run_type || "context")}</Badge>
                          <Badge
                            tone={
                              ["failed", "cancelled"].includes(
                                String(run?.status || "").toLowerCase()
                              )
                                ? "err"
                                : ["running", "queued", "planning"].includes(
                                      String(run?.status || "").toLowerCase()
                                    )
                                  ? "warn"
                                  : "ok"
                            }
                          >
                            {String(run?.status || "unknown")}
                          </Badge>
                        </div>
                      </div>
                      <div className="tcp-subtle text-xs">{String(run?.run_id || "")}</div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {Array.isArray(run?.tasks) ? run.tasks.length : 0} tasks ·{" "}
                        {Array.isArray(run?.steps) ? run.steps.length : 0} steps
                      </div>
                    </button>
                  ))
                ) : (
                  <EmptyState text="Workflow and failure-triage context runs will appear here once they execute." />
                )}
              </div>
            </PanelCard>

            <PanelCard title="Automation snapshot" subtitle="Schedules and run-ready routines.">
              <Toolbar className="mb-3">
                <Badge tone="info">{routineRows.length} loaded</Badge>
                <button className="tcp-btn" onClick={() => navigate("automations")}>
                  <i data-lucide="bot"></i>
                  Manage
                </button>
              </Toolbar>
              <div className="grid gap-2">
                {routineRows.length ? (
                  routineRows.slice(0, 4).map((routine: any) => (
                    <div
                      key={String(routine.id || routine.routine_id || Math.random())}
                      className="tcp-list-item"
                    >
                      <div className="font-medium">
                        {String(routine.name || routine.id || "Routine")}
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {String(routine.schedule || routine.status || "manual")}
                      </div>
                    </div>
                  ))
                ) : (
                  <EmptyState text="No routines are configured yet." />
                )}
              </div>
            </PanelCard>

            <PanelCard title="Health notes" subtitle="Direct links for the next likely action.">
              <div className="grid gap-2">
                <div className="tcp-list-item">
                  <div className="font-medium inline-flex items-center gap-2">
                    <i data-lucide="activity"></i>
                    Engine endpoint
                  </div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {String(health.data?.engineUrl || "Unavailable")}
                  </div>
                </div>
                <div className="tcp-list-item">
                  <div className="font-medium inline-flex items-center gap-2">
                    <i data-lucide="radio"></i>
                    Swarm
                  </div>
                  <div className="tcp-subtle mt-1 text-xs">Status: {swarmStatus}</div>
                </div>
                <div className="tcp-list-item">
                  <div className="font-medium inline-flex items-center gap-2">
                    <i data-lucide="database"></i>
                    Memory & feed
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button className="tcp-btn h-8 px-3 text-xs" onClick={() => navigate("memory")}>
                      Open memory
                    </button>
                    <button className="tcp-btn h-8 px-3 text-xs" onClick={() => navigate("feed")}>
                      Open live feed
                    </button>
                  </div>
                </div>
              </div>
            </PanelCard>
          </div>
        }
      />
      <DetailDrawer
        open={!!selectedWorkflowContextRunId}
        onClose={() => setSelectedWorkflowContextRunId("")}
        title={selectedWorkflowContextRunId || "Workflow context run"}
      >
        {selectedWorkflowContextRunId ? (
          <div className="grid gap-3">
            <div className="tcp-list-item">
              <div className="mb-1 flex items-center justify-between gap-2">
                <strong>
                  {String(
                    workflowContextDetail.data?.run?.objective || selectedWorkflowContextRunId
                  )}
                </strong>
                <Badge tone="info">
                  {String(workflowContextDetail.data?.run?.status || "unknown")}
                </Badge>
              </div>
              <div className="tcp-subtle text-xs">
                type: {String(workflowContextDetail.data?.run?.run_type || "workflow")}
              </div>
            </div>
            <div className="tcp-list-item">
              <div className="font-medium mb-2">Projected blackboard</div>
              <div className="tcp-subtle text-xs">
                tasks:{" "}
                {Array.isArray(workflowContextBlackboard.data?.blackboard?.tasks)
                  ? workflowContextBlackboard.data.blackboard.tasks.length
                  : 0}
                {" · "}artifacts:{" "}
                {Array.isArray(workflowContextBlackboard.data?.blackboard?.artifacts)
                  ? workflowContextBlackboard.data.blackboard.artifacts.length
                  : 0}
              </div>
            </div>
            <pre className="tcp-code">
              {formatJson({
                run: workflowContextDetail.data?.run || null,
                blackboard: workflowContextBlackboard.data?.blackboard || null,
              })}
            </pre>
          </div>
        ) : null}
      </DetailDrawer>
    </AnimatedPage>
  );
}
