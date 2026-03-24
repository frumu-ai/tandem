import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { AnimatedPage, Badge, PanelCard, StatusPulse } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import { useCapabilities } from "../features/system/queries.ts";
import type { AppPageProps } from "./pageTypes";

type CodingTab = "overview" | "board" | "manual" | "integrations";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function normalizeServers(raw: any) {
  const rows = Array.isArray(raw)
    ? raw
    : Array.isArray(raw?.servers)
      ? raw.servers
      : raw && typeof raw === "object"
        ? Object.entries(raw).map(([name, row]) => ({ name, ...(row as any) }))
        : [];
  return rows
    .map((row: any) => ({
      name: String(row?.name || "").trim(),
      connected: !!row?.connected,
      enabled: row?.enabled !== false,
      transport: String(row?.transport || "").trim(),
      lastError: String(row?.last_error || row?.lastError || "").trim(),
    }))
    .filter((row: any) => row.name)
    .sort((a: any, b: any) => a.name.localeCompare(b.name));
}

function normalizeTools(raw: any) {
  const rows = Array.isArray(raw) ? raw : Array.isArray(raw?.tools) ? raw.tools : [];
  return rows
    .map((tool: any) => {
      if (typeof tool === "string") return tool;
      return String(tool?.namespaced_name || tool?.namespacedName || tool?.id || "").trim();
    })
    .filter(Boolean);
}

function runId(run: any, index: number) {
  return String(run?.run_id || run?.runId || run?.id || `run-${index}`).trim();
}

function runTitle(run: any) {
  return String(
    run?.objective ||
      run?.title ||
      run?.summary ||
      run?.workflow_id ||
      run?.workflowId ||
      run?.run_type ||
      run?.runType ||
      run?.run_id ||
      run?.runId ||
      "Untitled run"
  ).trim();
}

function runStatus(run: any) {
  return String(run?.status || "unknown")
    .trim()
    .toLowerCase();
}

function formatStatus(status: string) {
  return String(status || "unknown")
    .replace(/_/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

function runIsActive(run: any) {
  return !["completed", "done", "failed", "cancelled"].includes(runStatus(run));
}

function groupRuns(runs: any[]) {
  const lanes: Array<{
    id: string;
    label: string;
    hint: string;
    statuses: string[];
    items: any[];
  }> = [
    {
      id: "queue",
      label: "Queue",
      hint: "Ready or waiting to be claimed",
      statuses: ["queued", "pending", "idle"],
      items: [],
    },
    {
      id: "planning",
      label: "Planning",
      hint: "Task decomposition and board shaping",
      statuses: ["planning", "preflight", "triage"],
      items: [],
    },
    {
      id: "active",
      label: "Active",
      hint: "Currently being executed",
      statuses: ["running", "executing", "in_progress", "active", "working"],
      items: [],
    },
    {
      id: "waiting",
      label: "Waiting",
      hint: "Blocked, paused, or awaiting approval",
      statuses: ["blocked", "awaiting_approval", "paused", "needs_info", "waiting"],
      items: [],
    },
    {
      id: "done",
      label: "Done",
      hint: "Finished or archived",
      statuses: ["completed", "done", "cancelled", "failed", "archived"],
      items: [],
    },
    { id: "other", label: "Other", hint: "Unclassified run states", statuses: [], items: [] },
  ];

  runs.forEach((run) => {
    const status = runStatus(run);
    const bucket = lanes.find((lane) => lane.statuses.includes(status)) || lanes[lanes.length - 1];
    bucket.items.push(run);
  });

  return lanes;
}

function Metric({
  label,
  value,
  helper,
  tone = "info",
}: {
  label: string;
  value: string | number;
  helper: string;
  tone?: "info" | "ok" | "warn" | "ghost";
}) {
  return (
    <div className="rounded-2xl border border-white/10 bg-black/20 p-4 shadow-[0_12px_36px_rgba(0,0,0,0.12)]">
      <div className="flex items-start justify-between gap-3">
        <div className="tcp-kpi-label text-sm">{label}</div>
        <Badge tone={tone}>{helper}</Badge>
      </div>
      <div className="mt-3 text-2xl font-semibold tracking-tight">{value}</div>
    </div>
  );
}

export function CodingWorkflowsPage({ api, client }: AppPageProps) {
  const [tab, setTab] = useState<CodingTab>("overview");

  const caps = useCapabilities();
  const acaAvailable = caps.data?.aca_integration === true;
  const engineAvailable = caps.data?.engine_healthy === true;
  const codingAvailable = caps.data?.coding_workflows === true;

  const health = useQuery({
    queryKey: ["coding-workflows", "health"],
    queryFn: () => api("/api/system/health"),
    refetchInterval: codingAvailable ? 15000 : false,
    enabled: codingAvailable,
  });
  const swarm = useQuery({
    queryKey: ["coding-workflows", "swarm"],
    queryFn: () => api("/api/swarm/status").catch(() => ({ status: "unknown", activeRuns: 0 })),
    refetchInterval: codingAvailable ? 6000 : false,
    enabled: codingAvailable,
  });
  const workflowContexts = useQuery({
    queryKey: ["coding-workflows", "workflow-context-runs"],
    queryFn: () => api("/api/engine/context/runs?limit=16").catch(() => ({ runs: [] })),
    refetchInterval: codingAvailable ? 6000 : false,
    enabled: codingAvailable,
  });
  const mcpServersQuery = useQuery({
    queryKey: ["coding-workflows", "mcp-servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: codingAvailable ? 10000 : false,
    enabled: codingAvailable,
  });
  const mcpToolsQuery = useQuery({
    queryKey: ["coding-workflows", "mcp-tools"],
    queryFn: () => client.mcp.listTools().catch(() => []),
    refetchInterval: codingAvailable ? 15000 : false,
    enabled: codingAvailable,
  });

  const mcpServers = useMemo(() => normalizeServers(mcpServersQuery.data), [mcpServersQuery.data]);
  const mcpTools = useMemo(() => normalizeTools(mcpToolsQuery.data), [mcpToolsQuery.data]);

  const workflowRuns = useMemo(() => {
    const rows = toArray(workflowContexts.data, "runs").filter((run: any) =>
      ["workflow", "bug_monitor_triage", "coding", "coding_workflow", "task"].includes(
        String(run?.run_type || run?.runType || "")
          .trim()
          .toLowerCase()
      )
    );
    return rows;
  }, [workflowContexts.data]);

  const activeRuns = workflowRuns.filter(runIsActive);
  const lanes = useMemo(() => groupRuns(workflowRuns), [workflowRuns]);

  const healthy = !!(health.data?.engine?.ready || health.data?.engine?.healthy);
  const swarmStatus = String(swarm.data?.status || "unknown");
  const githubConnected = mcpServers.some((server) => server.name.toLowerCase().includes("github"));

  const tabs: Array<{ id: CodingTab; label: string; icon: string }> = [
    { id: "overview", label: "Overview", icon: "layout-dashboard" },
    { id: "board", label: "Board", icon: "kanban-square" },
    { id: "manual", label: "Manual tasks", icon: "code" },
    { id: "integrations", label: "Integrations", icon: "plug-zap" },
  ];

  if (!codingAvailable) {
    return (
      <AnimatedPage className="grid gap-4">
        <PanelCard className="overflow-hidden">
          <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
            <div className="min-w-0">
              <div className="tcp-page-eyebrow">Coding workflows</div>
              <h1 className="tcp-page-title">Autonomous Coding</h1>
              <p className="tcp-subtle mt-2 max-w-3xl">
                {acaAvailable
                  ? "External ACA  is connected. Engine-native autonomous coding features are being loaded."
                  : "External ACA  not detected. Using engine-native autonomous coding."}
              </p>
              {!engineAvailable && (
                <div className="mt-3 rounded-2xl border border-yellow-500/20 bg-yellow-500/10 p-4">
                  <p className="text-sm text-yellow-200">
                    <strong>Engine not detected.</strong> Configure and start the Tandem engine to
                    enable autonomous coding features. Use the{" "}
                    <a href="/settings" className="underline">
                      Settings
                    </a>{" "}
                    page to configure the engine connection.
                  </p>
                </div>
              )}
            </div>
          </div>
        </PanelCard>
      </AnimatedPage>
    );
  }

  return (
    <AnimatedPage className="grid gap-4">
      <PanelCard className="overflow-hidden">
        <div className="grid gap-5 xl:grid-cols-[minmax(0,1.3fr)_minmax(320px,0.9fr)] xl:items-start">
          <div className="min-w-0">
            <div className="tcp-page-eyebrow">Coding workflows</div>
            <h1 className="tcp-page-title">Internal Kanban and task launch pad</h1>
            <p className="tcp-subtle mt-2 max-w-3xl">
              A dedicated home for repo-bound coding runs, manual task launches, worker swarms, and
              integration status. This page is intentionally small now so we can layer in extra tabs
              later without changing the mental model.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Badge tone={healthy ? "ok" : "warn"}>
                {healthy ? "Engine healthy" : "Engine checking"}
              </Badge>
              <Badge tone={swarmStatus === "unknown" ? "ghost" : "info"}>
                Swarm {formatStatus(swarmStatus)}
              </Badge>
              <Badge tone={githubConnected ? "ok" : "warn"}>
                {githubConnected ? "GitHub MCP connected" : "GitHub MCP pending"}
              </Badge>
              <StatusPulse
                tone={activeRuns.length ? "live" : "info"}
                text={`${activeRuns.length} active runs`}
              />
            </div>
          </div>
        </div>
      </PanelCard>

      <div className="tcp-settings-tabs">
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            className={`tcp-settings-tab tcp-settings-tab-underline ${tab === item.id ? "active" : ""}`}
            onClick={() => setTab(item.id)}
          >
            <i data-lucide={item.icon}></i>
            {item.label}
          </button>
        ))}
      </div>

      {tab === "overview" ? (
        <div className="grid gap-4 xl:grid-cols-2">
          <Metric
            label="Workflow runs"
            value={workflowRuns.length}
            helper={activeRuns.length ? `${activeRuns.length} active` : "Idle"}
            tone={activeRuns.length ? "warn" : "ok"}
          />
          <Metric
            label="Connected MCP servers"
            value={mcpServers.length}
            helper={githubConnected ? "GitHub available" : "MCP pending"}
            tone={githubConnected ? "ok" : "warn"}
          />
          <Metric
            label="Registered tools"
            value={mcpTools.length}
            helper="Tool surface"
            tone={mcpTools.length ? "info" : "ghost"}
          />
          <Metric
            label="Engine mode"
            value={String(health.data?.engine?.mode || "unknown")}
            helper={String(health.data?.engine?.version || "n/a")}
            tone={healthy ? "ok" : "warn"}
          />
        </div>
      ) : null}

      {tab === "board" ? (
        <div className="grid gap-4 xl:grid-cols-2 2xl:grid-cols-3">
          {lanes.map((lane) => (
            <PanelCard
              key={lane.id}
              title={`${lane.label} (${lane.items.length})`}
              subtitle={lane.hint}
              className="h-full"
            >
              {lane.items.length ? (
                <div className="grid gap-2">
                  {lane.items.map((run: any, index: number) => {
                    const status = runStatus(run);
                    return (
                      <div
                        key={runId(run, index)}
                        className="rounded-2xl border border-white/10 bg-black/20 p-3"
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="truncate text-sm font-semibold">{runTitle(run)}</div>
                            <div className="tcp-subtle mt-1 text-xs">
                              {String(run?.workspaceRoot || run?.workspace_root || "workspace")}
                            </div>
                          </div>
                          <Badge
                            tone={
                              lane.id === "done" ? "ok" : lane.id === "waiting" ? "warn" : "info"
                            }
                          >
                            {formatStatus(status)}
                          </Badge>
                        </div>
                        <div className="mt-2 flex flex-wrap gap-2 text-xs text-slate-300">
                          <span className="rounded-full border border-white/10 px-2 py-1">
                            {String(run?.run_type || run?.runType || "workflow")}
                          </span>
                          {String(run?.run_id || run?.runId || "").trim() ? (
                            <span className="rounded-full border border-white/10 px-2 py-1">
                              {String(run?.run_id || run?.runId).slice(0, 12)}
                            </span>
                          ) : null}
                        </div>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <EmptyState text="No runs in this lane yet." />
              )}
            </PanelCard>
          ))}
        </div>
      ) : null}

      {tab === "manual" ? (
        <PanelCard
          title="Manual task launcher"
          subtitle="Reserved for explicit coding tasks, branch-bound work, and user-triggered actions."
        >
          <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(280px,0.75fr)] xl:items-start">
            <div className="grid gap-3">
              <p className="tcp-subtle">
                This is where we can later add buttons for claim, start, branch selection, worker
                fan-out, and task-typing controls. For now it gives us a dedicated place to hang the
                workflow UI without inventing another page.
              </p>
              <div className="flex flex-wrap gap-2">
                <Badge tone="info">Future tab: claim task</Badge>
                <Badge tone="info">Future tab: worker swarm</Badge>
                <Badge tone="info">Future tab: branch control</Badge>
              </div>
            </div>
          </div>
        </PanelCard>
      ) : null}

      {tab === "integrations" ? (
        <div className="grid gap-4 xl:grid-cols-2">
          <PanelCard
            title="Connected MCP servers"
            subtitle="The integration layer this view depends on."
          >
            {mcpServers.length ? (
              <div className="grid gap-2">
                {mcpServers.map((server) => (
                  <div
                    key={server.name}
                    className="flex items-center justify-between gap-3 rounded-2xl border border-white/10 bg-black/20 px-3 py-2"
                  >
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold">{server.name}</div>
                      <div className="tcp-subtle text-xs">
                        {server.transport || "transport pending"}
                        {server.lastError ? ` · ${server.lastError}` : ""}
                      </div>
                    </div>
                    <Badge tone={server.connected ? "ok" : server.enabled ? "warn" : "ghost"}>
                      {server.connected ? "Connected" : server.enabled ? "Configured" : "Disabled"}
                    </Badge>
                  </div>
                ))}
              </div>
            ) : (
              <EmptyState text="No MCP servers detected yet." />
            )}
          </PanelCard>

          <PanelCard
            title="Tool surface"
            subtitle="Useful for GitHub, repo, and orchestration actions."
          >
            {mcpTools.length ? (
              <div className="flex flex-wrap gap-2">
                {mcpTools.slice(0, 20).map((tool) => (
                  <Badge key={tool} tone="info">
                    {tool}
                  </Badge>
                ))}
                {mcpTools.length > 20 ? (
                  <Badge tone="ghost">+{mcpTools.length - 20} more</Badge>
                ) : null}
              </div>
            ) : (
              <EmptyState text="No MCP tools discovered yet." />
            )}
          </PanelCard>
        </div>
      ) : null}
    </AnimatedPage>
  );
}
