import { Badge, PanelCard } from "../ui/index.tsx";
import { EmptyState } from "./ui";

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

function formatStatus(status: string) {
  return String(status || "unknown")
    .replace(/_/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

function safeText(value: any, fallback = "unknown") {
  const text = String(value ?? "").trim();
  return text || fallback;
}

function asStringList(value: any) {
  return Array.isArray(value) ? value.map((item) => String(item || "").trim()).filter(Boolean) : [];
}

function formatOverviewTime(value: any) {
  const timestamp = Number(value || 0);
  if (!timestamp) return "not refreshed yet";
  return new Date(timestamp).toLocaleString();
}

export function CodingWorkflowsOverviewTab({
  projects,
  selectedProjectSlug,
  setSelectedProjectSlug,
  selectedProject,
  acaOverview,
  projectTasksQuery,
  refreshTaskPreview,
  taskPreviewRefreshAt,
  visibleRunsCount,
  activeRunsCount,
  connectedMcpServersCount,
  registeredToolsCount,
}: {
  projects: any[];
  selectedProjectSlug: string;
  setSelectedProjectSlug: (value: string) => void;
  selectedProject: any;
  acaOverview: any;
  projectTasksQuery: any;
  refreshTaskPreview: () => void;
  taskPreviewRefreshAt: number | null;
  visibleRunsCount: number;
  activeRunsCount: number;
  connectedMcpServersCount: number;
  registeredToolsCount: number;
}) {
  return (
    <>
      <div className="grid gap-4">
        <PanelCard title="Project selector" subtitle="Choose the repository context before intake">
          {projects.length ? (
            <div className="grid gap-3">
              <select
                className="tcp-input"
                value={selectedProjectSlug}
                onChange={(event) =>
                  setSelectedProjectSlug((event.target as HTMLSelectElement).value)
                }
              >
                {!projects.length ? <option value="">No ACA projects found</option> : null}
                {projects.map((project: any) => (
                  <option key={project.slug} value={project.slug}>
                    {project.slug}
                  </option>
                ))}
              </select>
              <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                <div className="text-sm font-semibold">
                  {selectedProject?.slug || "No project selected"}
                </div>
                <div className="tcp-subtle mt-1 text-xs">
                  {selectedProject?.repoUrl || "No repo URL stored"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Task source: {String(selectedProject?.taskSource?.type || "unknown")}
                </div>
              </div>
            </div>
          ) : (
            <EmptyState text="Register an ACA project to start using the coding dashboard." />
          )}
        </PanelCard>

        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
          <Metric
            label="Registered projects"
            value={projects.length}
            helper={
              selectedProjectSlug ? `Focused on ${selectedProjectSlug}` : "No project selected"
            }
            tone={projects.length ? "ok" : "warn"}
          />
          <Metric
            label="Visible runs"
            value={visibleRunsCount}
            helper={activeRunsCount ? `${activeRunsCount} active` : "Idle"}
            tone={activeRunsCount ? "warn" : "ok"}
          />
          <Metric
            label="Connected MCP servers"
            value={connectedMcpServersCount}
            helper={connectedMcpServersCount ? "GitHub available" : "MCP pending"}
            tone={connectedMcpServersCount ? "ok" : "warn"}
          />
          <Metric
            label="Registered tools"
            value={registeredToolsCount}
            helper="Engine tool surface"
            tone={registeredToolsCount ? "info" : "ghost"}
          />
        </div>

        <div className="grid gap-4">
          <PanelCard
            title="ACA snapshot"
            subtitle="Read-only runtime view the agent can use before intake"
            actions={
              <Badge tone={acaOverview.data ? "ok" : "warn"}>
                {acaOverview.data ? "Live" : "Loading"}
              </Badge>
            }
          >
            {acaOverview.isLoading ? (
              <div className="tcp-subtle text-sm">Loading ACA snapshot...</div>
            ) : acaOverview.isError ? (
              <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                {acaOverview.error instanceof Error
                  ? acaOverview.error.message
                  : "Could not load ACA snapshot."}
              </div>
            ) : acaOverview.data?.overview ? (
              <div className="grid gap-3">
                <div className="flex flex-wrap gap-2">
                  <Badge tone={acaOverview.data.overview.auth?.required ? "ok" : "warn"}>
                    {safeText(acaOverview.data.overview.auth?.mode, "bearer_api_key")}
                  </Badge>
                  <Badge tone={acaOverview.data.overview.validation?.ok ? "ok" : "warn"}>
                    {acaOverview.data.overview.validation?.ok ? "Config valid" : "Needs attention"}
                  </Badge>
                  <Badge tone={acaOverview.data.overview.engine?.healthy ? "ok" : "warn"}>
                    {acaOverview.data.overview.engine?.healthy ? "Engine healthy" : "Engine issue"}
                  </Badge>
                  <Badge tone={acaOverview.data.overview.github_mcp?.connected ? "ok" : "warn"}>
                    {acaOverview.data.overview.github_mcp?.connected
                      ? "GitHub MCP connected"
                      : "GitHub MCP pending"}
                  </Badge>
                </div>

                <div className="grid gap-3 md:grid-cols-2">
                  <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="text-xs uppercase tracking-[0.2em] text-slate-400">
                      Task source
                    </div>
                    <div className="mt-1 text-sm font-semibold">
                      {safeText(acaOverview.data.overview.task_source?.type, "unset")}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {acaOverview.data.overview.task_source?.owner
                        ? `${safeText(acaOverview.data.overview.task_source.owner)} / ${safeText(acaOverview.data.overview.task_source.repo)}`
                        : safeText(
                            acaOverview.data.overview.task_source?.source_name,
                            "No source details"
                          )}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      Project {safeText(acaOverview.data.overview.task_source?.project, "n/a")}
                      {acaOverview.data.overview.task_source?.item
                        ? ` · Item ${safeText(acaOverview.data.overview.task_source.item)}`
                        : ""}
                    </div>
                  </div>

                  <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="text-xs uppercase tracking-[0.2em] text-slate-400">
                      Repository
                    </div>
                    <div className="mt-1 text-sm font-semibold">
                      {safeText(
                        acaOverview.data.overview.repository?.slug ||
                          acaOverview.data.overview.repository?.path,
                        "Unbound"
                      )}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {acaOverview.data.overview.repository?.path
                        ? `Path ${safeText(acaOverview.data.overview.repository.path)}`
                        : "Repository path unavailable"}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      Branch{" "}
                      {safeText(acaOverview.data.overview.repository?.default_branch, "main")}
                      {acaOverview.data.overview.repository?.remote_name
                        ? ` · Remote ${safeText(acaOverview.data.overview.repository.remote_name, "origin")}`
                        : ""}
                    </div>
                  </div>

                  <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="text-xs uppercase tracking-[0.2em] text-slate-400">
                      Engine and GitHub
                    </div>
                    <div className="mt-1 text-sm font-semibold">
                      {safeText(acaOverview.data.overview.engine?.status, "unknown")} engine
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {acaOverview.data.overview.engine?.base_url
                        ? `Engine ${safeText(acaOverview.data.overview.engine.base_url)}`
                        : "Engine URL unavailable"}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      GitHub MCP {safeText(acaOverview.data.overview.github_mcp?.scope, "unset")}
                      {acaOverview.data.overview.github_mcp?.remote_sync
                        ? ` · Sync ${safeText(acaOverview.data.overview.github_mcp.remote_sync)}`
                        : ""}
                    </div>
                  </div>

                  <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="text-xs uppercase tracking-[0.2em] text-slate-400">
                      Latest run
                    </div>
                    <div className="mt-1 text-sm font-semibold">
                      {safeText(acaOverview.data.overview.latest_run?.run_id, "No run yet")}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {acaOverview.data.overview.latest_run?.status
                        ? `Status ${safeText(acaOverview.data.overview.latest_run.status)}`
                        : "No run status"}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {acaOverview.data.overview.latest_run?.phase
                        ? `Phase ${safeText(acaOverview.data.overview.latest_run.phase)}`
                        : "No phase recorded"}
                    </div>
                  </div>
                </div>

                <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <div className="text-sm font-semibold">Allowed next actions</div>
                      <div className="tcp-subtle mt-1 text-xs">
                        The agent should choose from these safe follow-ups.
                      </div>
                    </div>
                    <Badge tone="ghost">
                      Refreshed {formatOverviewTime(acaOverview.dataUpdatedAt)}
                    </Badge>
                  </div>
                  <div className="mt-3 flex flex-wrap gap-2">
                    {asStringList(acaOverview.data.overview.allowed_next_actions).length ? (
                      asStringList(acaOverview.data.overview.allowed_next_actions).map((action) => (
                        <Badge key={action} tone="info">
                          {formatStatus(action)}
                        </Badge>
                      ))
                    ) : (
                      <span className="tcp-subtle text-xs">No suggested actions.</span>
                    )}
                  </div>
                </div>

                <details className="rounded-2xl border border-white/10 bg-black/20 p-4">
                  <summary className="cursor-pointer text-sm font-semibold">Raw snapshot</summary>
                  <pre className="mt-3 max-h-64 overflow-auto whitespace-pre-wrap text-xs leading-6 text-slate-200">
                    {JSON.stringify(acaOverview.data.overview, null, 2)}
                  </pre>
                </details>
              </div>
            ) : (
              <EmptyState text="No ACA snapshot available yet." />
            )}
          </PanelCard>

          <PanelCard title="Task intake preview" subtitle="What ACA will try to pick up next">
            {selectedProjectSlug ? (
              projectTasksQuery.isLoading ? (
                <div className="tcp-subtle text-sm">Loading task preview...</div>
              ) : projectTasksQuery.isError ? (
                <div className="rounded-2xl border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-200">
                  {projectTasksQuery.error instanceof Error
                    ? projectTasksQuery.error.message
                    : "Could not load task preview."}
                </div>
              ) : projectTasksQuery.data?.task ? (
                <div className="grid gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-white/10 bg-black/20 p-3">
                    <div className="tcp-subtle text-xs">
                      GitHub Project intake is refreshed on demand through Tandem&apos;s GitHub MCP.
                      It does not auto-update here so we can keep GitHub calls down.
                      {taskPreviewRefreshAt
                        ? ` Last refreshed ${new Date(taskPreviewRefreshAt).toLocaleTimeString()}.`
                        : ""}
                    </div>
                    <button
                      type="button"
                      className="tcp-btn tcp-btn-secondary"
                      onClick={refreshTaskPreview}
                      disabled={projectTasksQuery.isFetching}
                    >
                      {projectTasksQuery.isFetching ? "Refreshing..." : "Refresh from GitHub"}
                    </button>
                  </div>
                  <div className="rounded-2xl border border-white/10 bg-black/20 p-4">
                    <div className="text-sm font-semibold">
                      {String(projectTasksQuery.data.task.title || "Untitled task")}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      {String(projectTasksQuery.data.source_type || "unknown")}
                      {projectTasksQuery.data.board_path
                        ? ` · ${String(projectTasksQuery.data.board_path)}`
                        : ""}
                    </div>
                    <div className="mt-3 flex flex-wrap gap-2">
                      <Badge tone={projectTasksQuery.data.eligible ? "ok" : "warn"}>
                        {projectTasksQuery.data.eligible ? "Eligible" : "Not eligible"}
                      </Badge>
                      <Badge tone="info">
                        ACA intake lane{" "}
                        {formatStatus(String(projectTasksQuery.data.task.lane || "ready"))}
                      </Badge>
                      {projectTasksQuery.data.task?.source ? (
                        projectTasksQuery.data.task.source.status ? (
                          <Badge tone="ghost">
                            GitHub status {String(projectTasksQuery.data.task.source.status)}
                          </Badge>
                        ) : (
                          <Badge tone="ghost">GitHub status unavailable from MCP</Badge>
                        )
                      ) : null}
                    </div>
                  </div>
                  {projectTasksQuery.data?.board_summary ? (
                    <div className="flex flex-wrap gap-2">
                      {Object.entries(projectTasksQuery.data.board_summary).map(([lane, count]) => (
                        <Badge key={lane} tone="ghost">
                          {lane}: {String(count)}
                        </Badge>
                      ))}
                    </div>
                  ) : null}
                  {String(projectTasksQuery.data?.warning || "").trim() ? (
                    <div className="rounded-2xl border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-100">
                      {String(projectTasksQuery.data.warning)}
                    </div>
                  ) : null}
                </div>
              ) : (
                <EmptyState text="No task preview available yet." />
              )
            ) : (
              <EmptyState text="Select a project to preview task intake." />
            )}
          </PanelCard>
        </div>
      </div>
    </>
  );
}
