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
import { formatCompactNumber, formatUsd } from "../lib/format";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function normalizeStatusKey(value: any) {
  return String(value || "")
    .trim()
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[\s-]+/g, "_")
    .toLowerCase();
}

function providerAuthSummary(providerStatus: AppPageProps["providerStatus"]) {
  if (!providerStatus.ready) return "";
  if (providerStatus.defaultProvider !== "openai-codex") {
    if (providerStatus.defaultProviderAuthKind === "oauth") return "OAuth session active";
    if (providerStatus.defaultProviderSource === "env") return "Env-managed credentials";
    if (providerStatus.defaultProviderSource === "persisted") return "Stored credentials";
    return "";
  }
  if (providerStatus.defaultProviderManagedBy === "codex-upload") {
    return "Imported auth.json on hosted server";
  }
  if (providerStatus.defaultProviderManagedBy === "codex-cli") {
    return "Mirrored from local Codex CLI";
  }
  if (providerStatus.defaultProviderAuthKind === "oauth") {
    return "Codex account session active";
  }
  return "";
}

export function DashboardPage(props: AppPageProps) {
  const { api, client, navigate, providerStatus } = props;
  const [selectedWorkflowContextRunId, setSelectedWorkflowContextRunId] = useState("");
  const [tokenGranularity, setTokenGranularity] = useState<"day" | "week" | "month">("day");
  const visibleContextRunTypes = new Set(["workflow", "bug_monitor_triage"]);

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
  const automationRuns = useQuery({
    queryKey: ["dashboard", "automation-v2-runs"],
    queryFn: () => api("/api/engine/automations/v2/runs?limit=200").catch(() => ({ runs: [] })),
    refetchInterval: 15000,
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
  const automationRunRows = toArray(automationRuns.data, "runs");
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
  const liveAutomationRunRows = automationRunRows.filter((run: any) =>
    ["queued", "running", "pausing", "awaiting_approval", "blocked"].includes(
      normalizeStatusKey(run?.status)
    )
  );
  const trackedAutomationRunRows = automationRunRows.filter((run: any) => {
    const totalTokens = Number(run?.total_tokens || 0);
    const estimatedCostUsd = Number(run?.estimated_cost_usd || 0);
    return totalTokens > 0 || estimatedCostUsd > 0;
  });
  const automationTokenTotals = automationRunRows.reduce(
    (acc: { prompt: number; completion: number; total: number; cost: number }, run: any) => ({
      prompt: acc.prompt + Number(run?.prompt_tokens || 0),
      completion: acc.completion + Number(run?.completion_tokens || 0),
      total: acc.total + Number(run?.total_tokens || 0),
      cost: acc.cost + Number(run?.estimated_cost_usd || 0),
    }),
    { prompt: 0, completion: 0, total: 0, cost: 0 }
  );
  const activeWorkflowContexts = workflowContextRows.filter((run: any) =>
    ["queued", "planning", "running", "awaiting_approval"].includes(
      String(run?.status || "")
        .trim()
        .toLowerCase()
    )
  );
  const providerAuthDetail = providerAuthSummary(providerStatus);

  const tokenUsageBuckets = useMemo(() => {
    type Bucket = { label: string; runs: number; tokens: number; cost: number };
    const map = new Map<string, Bucket>();
    const now = Date.now();

    let windowMs: number;
    let bucketCount: number;
    let keyFn: (d: Date) => string;
    let labelFn: (key: string) => string;
    if (tokenGranularity === "day") {
      windowMs = 14 * 24 * 60 * 60 * 1000;
      bucketCount = 14;
      keyFn = (d) => d.toISOString().slice(0, 10);
      labelFn = (k) => {
        const [, m, day] = k.split("-");
        return `${m}/${day}`;
      };
    } else if (tokenGranularity === "week") {
      windowMs = 8 * 7 * 24 * 60 * 60 * 1000;
      bucketCount = 8;
      keyFn = (d) => {
        const monday = new Date(d);
        monday.setDate(d.getDate() - ((d.getDay() + 6) % 7));
        return monday.toISOString().slice(0, 10);
      };
      labelFn = (k) => {
        const [, m, day] = k.split("-");
        return `w/${m}/${day}`;
      };
    } else {
      windowMs = 6 * 30 * 24 * 60 * 60 * 1000;
      bucketCount = 6;
      keyFn = (d) => d.toISOString().slice(0, 7);
      labelFn = (k) => {
        const [y, m] = k.split("-");
        return `${m}/${y.slice(2)}`;
      };
    }

    const cutoff = now - windowMs;
    for (const run of automationRunRows) {
      const ts = Number(run?.created_at_ms || run?.updated_at_ms || 0);
      if (!ts || ts < cutoff) continue;
      const d = new Date(ts);
      const key = keyFn(d);
      const existing = map.get(key) ?? { label: labelFn(key), runs: 0, tokens: 0, cost: 0 };
      existing.runs += 1;
      existing.tokens += Number(run?.total_tokens || 0);
      existing.cost += Number(run?.estimated_cost_usd || 0);
      map.set(key, existing);
    }

    const sorted = [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    const maxTokens = Math.max(1, ...sorted.map(([, b]) => b.tokens));
    return { buckets: sorted.map(([, b]) => b), maxTokens, bucketCount };
  }, [automationRunRows, tokenGranularity]);

  const overviewStats = useMemo(
    () => [
      {
        label: "Recent sessions",
        value: sessionRows.length,
        tone: "info" as const,
        helper: "Latest active conversation surfaces",
      },
      {
        label: "Stored runs",
        value: automationRunRows.length,
        tone: "ok" as const,
        helper: "Persisted automation-v2 history",
      },
      {
        label: "Live runs",
        value: liveAutomationRunRows.length,
        tone: liveAutomationRunRows.length ? ("warn" as const) : ("ghost" as const),
        helper: "Queued, running, blocked, awaiting approval",
      },
      {
        label: "Tracked tokens",
        value: automationTokenTotals.total,
        tone: automationTokenTotals.total ? ("info" as const) : ("ghost" as const),
        helper: `${trackedAutomationRunRows.length} runs with usage`,
      },
      {
        label: "Estimated spend",
        value: automationTokenTotals.cost,
        tone: automationTokenTotals.cost ? ("ok" as const) : ("ghost" as const),
        helper: "Summed from stored run records",
        format: formatUsd,
      },
      {
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
      automationRunRows.length,
      automationTokenTotals.cost,
      automationTokenTotals.total,
      liveAutomationRunRows.length,
      sessionRows.length,
      workflowContextRows.length,
      trackedAutomationRunRows.length,
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
              A higher-signal home screen with animated state and a clearer read on what the system
              is doing right now.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Badge tone={healthy ? "ok" : "warn"}>
                {healthy ? "Engine healthy" : "Engine checking"}
              </Badge>
              <Badge tone={providerStatus.ready ? "ok" : "warn"}>
                {providerStatus.ready ? providerStatus.defaultProvider : "Provider setup required"}
              </Badge>
              {providerAuthDetail ? <Badge tone="info">{providerAuthDetail}</Badge> : null}
              {swarmRunning ? (
                <StatusPulse tone="live" text={`Swarm ${swarmStatus}`} />
              ) : (
                <Badge tone="ghost">Swarm {swarmStatus}</Badge>
              )}
            </div>
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
                  <MotionNumber value={stat.value} format={stat.format ?? formatCompactNumber} />
                </strong>
              </div>
            ))}
          </div>
          <p className="tcp-subtle mt-3 text-xs">
            Usage totals are aggregated from stored automation-v2 run history, so they reflect real
            executions instead of inferred activity.
          </p>

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
                <span className="dashboard-bar-count">{liveAutomationRunRows.length} live</span>
              </div>
              <div className="dashboard-bar-track">
                <span
                  className={`dashboard-bar-fill ${
                    liveAutomationRunRows.length ? "running" : "manual"
                  }`}
                  style={{
                    width: `${
                      automationRunRows.length
                        ? Math.min(
                            100,
                            Math.max(
                              12,
                              (liveAutomationRunRows.length / automationRunRows.length) * 100
                            )
                          )
                        : 12
                    }%`,
                  }}
                ></span>
              </div>
            </div>
          </div>
        </div>
      </PanelCard>

      <PanelCard>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <div>
            <div className="tcp-page-eyebrow">Usage</div>
            <h2 className="font-semibold">Token usage over time</h2>
          </div>
          <div className="flex gap-1">
            {(["day", "week", "month"] as const).map((g) => (
              <button
                key={g}
                type="button"
                className={`tcp-btn h-7 px-2 text-xs capitalize ${tokenGranularity === g ? "tcp-btn-active" : ""}`}
                onClick={() => setTokenGranularity(g)}
              >
                {g}
              </button>
            ))}
          </div>
        </div>
        {tokenUsageBuckets.buckets.length === 0 ? (
          <p className="tcp-subtle text-xs">No token data in this window yet.</p>
        ) : (
          <div className="grid gap-2">
            {tokenUsageBuckets.buckets.map((bucket) => (
              <div key={bucket.label} className="dashboard-bar-row">
                <div className="dashboard-bar-meta">
                  <span className="font-mono text-xs">{bucket.label}</span>
                  <span className="dashboard-bar-count">
                    {formatCompactNumber(bucket.tokens)} tok
                    {bucket.cost > 0 ? ` · ${formatUsd(bucket.cost)}` : ""}
                    {" · "}
                    <span className="tcp-subtle">
                      {bucket.runs} run{bucket.runs !== 1 ? "s" : ""}
                    </span>
                  </span>
                </div>
                <div className="dashboard-bar-track">
                  <span
                    className={`dashboard-bar-fill ${bucket.tokens > 0 ? "running" : "manual"}`}
                    style={{
                      width: `${Math.max(4, (bucket.tokens / tokenUsageBuckets.maxTokens) * 100)}%`,
                    }}
                  ></span>
                </div>
              </div>
            ))}
          </div>
        )}
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
                  Open task board
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
                    Memory & runs
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button className="tcp-btn h-8 px-3 text-xs" onClick={() => navigate("memory")}>
                      Open memory
                    </button>
                    <button className="tcp-btn h-8 px-3 text-xs" onClick={() => navigate("runs")}>
                      Open runs
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
