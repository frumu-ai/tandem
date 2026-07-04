import { AnimatePresence, motion } from "motion/react";
import { IncidentMonitorExternalProjectsPanel } from "../components/IncidentMonitorExternalProjectsPanel";
import { Badge, PanelCard, Toolbar } from "../ui/index.tsx";
import { useSettingsPageController, HOSTED_CODER_REPO_ROOT } from "./SettingsPageController";
import { EmptyState } from "./ui";
import { Icon } from "../ui/Icon";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageIncidentMonitorSectionsProps = {
  controller: SettingsPageControllerState;
};

function recordText(record: Record<string, any> | undefined, keys: string[], fallback = "") {
  if (!record) return fallback;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value.trim();
    if (typeof value === "number" && Number.isFinite(value)) return String(value);
    if (typeof value === "boolean") return value ? "true" : "false";
  }
  return fallback;
}

function recordArrayText(record: Record<string, any> | undefined, key: string) {
  const value = record?.[key];
  return Array.isArray(value) ? value.map((item) => String(item)).join(", ") : "";
}

function formatJson(value: unknown) {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export function SettingsPageIncidentMonitorSections({
  controller,
}: SettingsPageIncidentMonitorSectionsProps) {
  const {
    activeSection,
    incidentMonitorAutoComment,
    incidentMonitorAutoCreateIssues,
    incidentMonitorCreatedIntakeKey,
    incidentMonitorDefaultDestinationsText,
    incidentMonitorDestinationReadiness,
    incidentMonitorDestinations,
    incidentMonitorDestinationsJson,
    incidentMonitorDisablingIntakeKeyId,
    incidentMonitorDraftDecisionMutation,
    incidentMonitorDrafts,
    incidentMonitorDraftsQuery,
    incidentMonitorEnabled,
    incidentMonitorIncidents,
    incidentMonitorIncidentsQuery,
    incidentMonitorIntakeKeys,
    incidentMonitorLogSourceActionResult,
    incidentMonitorLogWatcher,
    incidentMonitorMcpServer,
    incidentMonitorModelId,
    incidentMonitorMonitoredProjects,
    incidentMonitorMonitoredProjectsError,
    incidentMonitorMonitoredProjectsJson,
    incidentMonitorPauseResumeMutation,
    incidentMonitorPaused,
    incidentMonitorPosts,
    incidentMonitorPostsQuery,
    incidentMonitorProviderId,
    incidentMonitorProviderModels,
    incidentMonitorProviderPreference,
    incidentMonitorPublishDraftMutation,
    incidentMonitorRecheckMatchMutation,
    incidentMonitorReplayIncidentMutation,
    incidentMonitorReplayingSourceKey,
    incidentMonitorRepo,
    incidentMonitorRequireApproval,
    incidentMonitorResettingSourceKey,
    incidentMonitorRoutePreviewError,
    incidentMonitorRoutePreviewJson,
    incidentMonitorRoutePreviewMutation,
    incidentMonitorRoutePreviewResult,
    incidentMonitorRoutes,
    incidentMonitorRoutesJson,
    incidentMonitorRoutingConfigError,
    incidentMonitorSafetyDefaultsJson,
    incidentMonitorStatus,
    incidentMonitorStatusQuery,
    incidentMonitorSuggestedWorkspaceRoot,
    incidentMonitorTriageRunMutation,
    incidentMonitorWorkspaceRoot,
    incidentMonitorWorkspaceRootHint,
    incidentMonitorWorkspaceSetupWarningText,
    copyIncidentMonitorDebugPayload,
    createIncidentMonitorIntakeKeyMutation,
    disableIncidentMonitorIntakeKeyMutation,
    mcpActionMutation,
    mcpServers,
    openMcpModal,
    providers,
    refreshIncidentMonitorBindingsMutation,
    replayIncidentMonitorLogSourceMutation,
    resetIncidentMonitorLogSourceMutation,
    saveIncidentMonitorMutation,
    selectedIncidentMonitorServer,
    setIncidentMonitorAutoComment,
    setIncidentMonitorAutoCreateIssues,
    setIncidentMonitorCreatedIntakeKey,
    setIncidentMonitorDefaultDestinationsText,
    setIncidentMonitorDestinationsJson,
    setIncidentMonitorEnabled,
    setIncidentMonitorMcpServer,
    setIncidentMonitorModelId,
    setIncidentMonitorMonitoredProjectsError,
    setIncidentMonitorMonitoredProjectsJson,
    setIncidentMonitorProviderId,
    setIncidentMonitorProviderPreference,
    setIncidentMonitorRepo,
    setIncidentMonitorRequireApproval,
    setIncidentMonitorRoutePreviewError,
    setIncidentMonitorRoutePreviewJson,
    setIncidentMonitorRoutesJson,
    setIncidentMonitorRoutingConfigError,
    setIncidentMonitorSafetyDefaultsJson,
    setIncidentMonitorWorkspaceBrowserDir,
    setIncidentMonitorWorkspaceBrowserOpen,
    setIncidentMonitorWorkspaceBrowserSearch,
    setIncidentMonitorWorkspaceRoot,
    setGithubMcpGuideOpen,
    toast,
  } = controller;
  const safeMcpServers = Array.isArray(mcpServers) ? mcpServers : [];
  const safeProviders = Array.isArray(providers) ? providers : [];
  const safeIncidentMonitorProviderModels = Array.isArray(incidentMonitorProviderModels)
    ? incidentMonitorProviderModels
    : [];
  const safeIncidentMonitorIncidents = Array.isArray(incidentMonitorIncidents)
    ? incidentMonitorIncidents
    : [];
  const safeIncidentMonitorDrafts = Array.isArray(incidentMonitorDrafts)
    ? incidentMonitorDrafts
    : [];
  const safeIncidentMonitorPosts = Array.isArray(incidentMonitorPosts) ? incidentMonitorPosts : [];
  const safeIncidentMonitorDestinations = Array.isArray(incidentMonitorDestinations)
    ? incidentMonitorDestinations
    : [];
  const safeIncidentMonitorRoutes = Array.isArray(incidentMonitorRoutes)
    ? incidentMonitorRoutes
    : [];
  const safeIncidentMonitorDestinationReadiness = Array.isArray(incidentMonitorDestinationReadiness)
    ? incidentMonitorDestinationReadiness
    : [];
  const readinessByDestination = new Map(
    safeIncidentMonitorDestinationReadiness.map((row) => [
      String(row.destination_id || row.destinationId || ""),
      row,
    ])
  );
  const routePreviewEffectiveDestinations = Array.isArray(
    incidentMonitorRoutePreviewResult?.effective_destination_ids
  )
    ? incidentMonitorRoutePreviewResult.effective_destination_ids
    : [];
  const routePreviewBlockedReasons = Array.isArray(
    incidentMonitorRoutePreviewResult?.blocked_reasons
  )
    ? incidentMonitorRoutePreviewResult.blocked_reasons
    : [];

  return (
    <>
      {activeSection === "incident_monitor" ? (
        <PanelCard
          title="Incident Monitor"
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge
                tone={
                  incidentMonitorStatus.runtime?.monitoring_active
                    ? incidentMonitorStatus.readiness?.publish_ready
                      ? "ok"
                      : "info"
                    : incidentMonitorStatus.readiness?.ingest_ready
                      ? "info"
                      : "warn"
                }
              >
                {incidentMonitorStatus.runtime?.monitoring_active
                  ? incidentMonitorStatus.readiness?.publish_ready
                    ? "Monitoring"
                    : "Watching locally"
                  : incidentMonitorStatus.readiness?.ingest_ready
                    ? "Ready"
                    : "Not ready"}
              </Badge>
              {incidentMonitorPaused || incidentMonitorStatus.runtime?.paused ? (
                <Badge tone="warn">Paused</Badge>
              ) : null}
              <Badge tone="info">
                {Number(incidentMonitorStatus.runtime?.pending_incidents || 0)} incidents
              </Badge>
              <Badge tone="info">
                {Number(incidentMonitorStatus.pending_drafts || 0)} pending drafts
              </Badge>
              <Badge tone="info">
                {Number(incidentMonitorStatus.pending_posts || 0)} post attempts
              </Badge>
              <button
                className="tcp-icon-btn"
                title="Reload status"
                aria-label="Reload status"
                onClick={() =>
                  Promise.all([
                    incidentMonitorStatusQuery.refetch(),
                    incidentMonitorDraftsQuery.refetch(),
                    incidentMonitorIncidentsQuery.refetch(),
                    incidentMonitorPostsQuery.refetch(),
                  ]).then(() => toast("ok", "Incident Monitor status refreshed."))
                }
              >
                <Icon name="refresh-cw" />
              </button>
              <button
                className="tcp-icon-btn"
                title={
                  incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                    ? "Resume monitoring"
                    : "Pause monitoring"
                }
                aria-label={
                  incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                    ? "Resume monitoring"
                    : "Pause monitoring"
                }
                disabled={incidentMonitorPauseResumeMutation.isPending}
                onClick={() =>
                  incidentMonitorPauseResumeMutation.mutate({
                    action:
                      incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                        ? "resume"
                        : "pause",
                  })
                }
              >
                <Icon
                  name={
                    incidentMonitorPaused || incidentMonitorStatus.runtime?.paused
                      ? "play"
                      : "pause"
                  }
                 />
              </button>
              <button
                className="tcp-icon-btn"
                title="Refresh capability bindings"
                aria-label="Refresh capability bindings"
                disabled={refreshIncidentMonitorBindingsMutation.isPending}
                onClick={() => refreshIncidentMonitorBindingsMutation.mutate()}
              >
                <Icon name="rotate-cw" />
              </button>
              <button
                className="tcp-icon-btn"
                title="Copy debug payload"
                aria-label="Copy debug payload"
                onClick={() => void copyIncidentMonitorDebugPayload()}
              >
                <Icon name="copy" />
              </button>
              <button
                className="tcp-icon-btn"
                title="Open GitHub MCP guide"
                aria-label="Open GitHub MCP guide"
                onClick={() => setGithubMcpGuideOpen(true)}
              >
                <Icon name="book-open" />
              </button>
            </div>
          }
        >
          <div className="grid gap-4">
            <div className="grid gap-3 md:grid-cols-2">
              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                  Reporter state
                </span>
                <button
                  type="button"
                  className={`tcp-list-item text-left ${incidentMonitorEnabled ? "ring-1 ring-emerald-400/40" : ""}`}
                  onClick={() => setIncidentMonitorEnabled((prev) => !prev)}
                >
                  <div className="font-medium">
                    {incidentMonitorEnabled
                      ? incidentMonitorPaused
                        ? "Paused"
                        : "Enabled"
                      : "Disabled"}
                  </div>
                  <div className="tcp-subtle text-xs">
                    {incidentMonitorEnabled
                      ? incidentMonitorPaused
                        ? "Monitoring is paused. Resume to process new failures."
                        : "Failure events can be analyzed once readiness is green."
                      : "No reporter work will execute."}
                  </div>
                </button>
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                  Local directory
                </span>
                <div className="rounded-xl border border-sky-500/20 bg-sky-500/10 p-3 text-xs text-sky-100">
                  <div className="font-semibold">Hosted path map</div>
                  <div className="mt-1 tcp-subtle">
                    Coder syncs source checkouts into <code>{HOSTED_CODER_REPO_ROOT}</code>. For
                    Incident Monitor analysis, select the repo folder itself, usually{" "}
                    <code>{incidentMonitorSuggestedWorkspaceRoot}</code>.
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-8 px-3 text-xs"
                      type="button"
                      onClick={() =>
                        setIncidentMonitorWorkspaceRoot(incidentMonitorSuggestedWorkspaceRoot)
                      }
                    >
                      <Icon name="badge-check" />
                      Use recommended path
                    </button>
                    <button
                      className="tcp-btn h-8 px-3 text-xs"
                      type="button"
                      onClick={() => {
                        setIncidentMonitorWorkspaceBrowserDir(HOSTED_CODER_REPO_ROOT);
                        setIncidentMonitorWorkspaceBrowserSearch("");
                        setIncidentMonitorWorkspaceBrowserOpen(true);
                      }}
                    >
                      <Icon name="folder-open" />
                      Browse synced repos
                    </button>
                  </div>
                </div>
                <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
                  <button
                    className="tcp-btn"
                    type="button"
                    onClick={() => {
                      const seed = String(incidentMonitorWorkspaceRoot || "/").trim();
                      setIncidentMonitorWorkspaceBrowserDir(seed || "/");
                      setIncidentMonitorWorkspaceBrowserSearch("");
                      setIncidentMonitorWorkspaceBrowserOpen(true);
                    }}
                  >
                    <Icon name="folder-open" />
                    Browse
                  </button>
                  <input
                    className="tcp-input"
                    readOnly
                    value={incidentMonitorWorkspaceRoot}
                    placeholder="No local directory selected. Use Browse."
                  />
                  <button
                    className="tcp-btn"
                    type="button"
                    onClick={() => setIncidentMonitorWorkspaceRoot("")}
                    disabled={!incidentMonitorWorkspaceRoot}
                  >
                    <Icon name="x" />
                    Clear
                  </button>
                </div>
                <div className="tcp-subtle text-xs">
                  {incidentMonitorWorkspaceRoot
                    ? `Reporter analysis root: ${incidentMonitorWorkspaceRoot}${
                        incidentMonitorWorkspaceRootHint
                          ? ` (${incidentMonitorWorkspaceRootHint})`
                          : ""
                      }`
                    : "Defaults to the engine workspace root if not set."}
                </div>
                {incidentMonitorWorkspaceSetupWarningText ? (
                  <div className="rounded-xl border border-amber-500/25 bg-amber-500/10 p-3 text-xs text-amber-100">
                    <div className="font-semibold">Setup check</div>
                    <div className="mt-1">{incidentMonitorWorkspaceSetupWarningText}</div>
                  </div>
                ) : (
                  <div className="rounded-xl border border-emerald-500/20 bg-emerald-500/10 p-3 text-xs text-emerald-100">
                    <div className="font-semibold">Source checkout ready</div>
                    <div className="mt-1">
                      Incident Monitor triage will inspect this repo path and require concrete
                      source-file reads before it marks research complete.
                    </div>
                  </div>
                )}
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Target repo</span>
                <input
                  className="tcp-input"
                  value={incidentMonitorRepo}
                  onChange={(event) => setIncidentMonitorRepo(event.target.value)}
                  placeholder="owner/repo"
                />
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">MCP server</span>
                <select
                  className="tcp-input"
                  value={incidentMonitorMcpServer}
                  onChange={(event) => setIncidentMonitorMcpServer(event.target.value)}
                >
                  <option value="">Select an MCP server</option>
                  {safeMcpServers.map((server) => (
                    <option key={server.name} value={server.name}>
                      {server.name}
                    </option>
                  ))}
                </select>
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                  Provider preference
                </span>
                <select
                  className="tcp-input"
                  value={incidentMonitorProviderPreference}
                  onChange={(event) => setIncidentMonitorProviderPreference(event.target.value)}
                >
                  <option value="auto">Auto</option>
                  <option value="official_github">Official GitHub</option>
                  <option value="composio">Composio</option>
                  <option value="arcade">Arcade</option>
                </select>
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Provider</span>
                <select
                  className="tcp-input"
                  value={incidentMonitorProviderId}
                  onChange={(event) => {
                    const nextProvider = event.target.value;
                    setIncidentMonitorProviderId(nextProvider);
                    setIncidentMonitorModelId("");
                  }}
                >
                  <option value="">Select a provider</option>
                  {safeProviders.map((provider: any) => (
                    <option key={String(provider?.id || "")} value={String(provider?.id || "")}>
                      {String(provider?.id || "")}
                    </option>
                  ))}
                </select>
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Model</span>
                <input
                  className="tcp-input"
                  value={incidentMonitorModelId}
                  onChange={(event) => setIncidentMonitorModelId(event.target.value)}
                  list="incident-monitor-models"
                  disabled={!incidentMonitorProviderId}
                  placeholder={
                    incidentMonitorProviderId
                      ? "Type or paste a model id"
                      : "Choose a provider first"
                  }
                  spellCheck={false}
                />
                <datalist id="incident-monitor-models">
                  {safeIncidentMonitorProviderModels.map((modelId) => (
                    <option key={modelId} value={modelId} />
                  ))}
                </datalist>
                <div className="tcp-subtle text-xs">
                  {incidentMonitorProviderId
                    ? safeIncidentMonitorProviderModels.length
                      ? `${safeIncidentMonitorProviderModels.length} suggested models from provider catalog`
                      : "No provider catalog models available. Manual model ids are allowed."
                    : "Select a provider to load model suggestions."}
                </div>
              </label>

              <div className="grid gap-2 md:col-span-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                  GitHub posting
                </span>
                <div className="grid gap-2 md:grid-cols-3">
                  <button
                    type="button"
                    className={`tcp-list-item text-left ${incidentMonitorAutoCreateIssues && !incidentMonitorRequireApproval ? "ring-1 ring-emerald-400/40" : ""}`}
                    onClick={() => {
                      setIncidentMonitorAutoCreateIssues((prev) => !prev);
                      if (incidentMonitorRequireApproval && incidentMonitorAutoCreateIssues) {
                        setIncidentMonitorRequireApproval(false);
                      }
                    }}
                  >
                    <div className="font-medium">Auto-create new issues</div>
                    <div className="tcp-subtle text-xs">
                      {incidentMonitorAutoCreateIssues
                        ? "New drafts post to GitHub automatically."
                        : "New drafts stay internal until published manually."}
                    </div>
                  </button>
                  <button
                    type="button"
                    className={`tcp-list-item text-left ${incidentMonitorRequireApproval ? "ring-1 ring-amber-400/40" : ""}`}
                    onClick={() => {
                      setIncidentMonitorRequireApproval((prev) => {
                        const next = !prev;
                        if (next) setIncidentMonitorAutoCreateIssues(false);
                        return next;
                      });
                    }}
                  >
                    <div className="font-medium">Require approval</div>
                    <div className="tcp-subtle text-xs">
                      {incidentMonitorRequireApproval
                        ? "New drafts wait for a manual publish click."
                        : "Approval gate disabled."}
                    </div>
                  </button>
                  <button
                    type="button"
                    className={`tcp-list-item text-left ${incidentMonitorAutoComment ? "ring-1 ring-sky-400/40" : ""}`}
                    onClick={() => setIncidentMonitorAutoComment((prev) => !prev)}
                  >
                    <div className="font-medium">Auto-comment matches</div>
                    <div className="tcp-subtle text-xs">
                      {incidentMonitorAutoComment
                        ? "Open matching GitHub issues receive new evidence comments."
                        : "Matching issues are detected but not updated automatically."}
                    </div>
                  </button>
                </div>
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-4">
              {[
                ["Sources", `${incidentMonitorMonitoredProjects.length} monitored project(s)`],
                ["Destinations", `${safeIncidentMonitorDestinations.length} configured`],
                ["Routing", `${safeIncidentMonitorRoutes.length} route(s)`],
                [
                  "Safety Defaults",
                  incidentMonitorRequireApproval ? "Approval gate active" : "Policy inherits route",
                ],
              ].map(([label, value]) => (
                <div key={label} className="tcp-list-item">
                  <div className="text-xs uppercase tracking-[0.24em] tcp-subtle">{label}</div>
                  <div className="mt-1 text-sm font-medium">{value}</div>
                </div>
              ))}
            </div>

            <div className="grid gap-3 xl:grid-cols-2">
              <div className="tcp-list-item">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="text-sm font-medium">Destinations</div>
                    <div className="tcp-subtle text-xs">
                      GitHub remains the legacy destination when no router override is configured.
                    </div>
                  </div>
                  <Badge tone={safeIncidentMonitorDestinations.length ? "info" : "ghost"}>
                    {safeIncidentMonitorDestinations.length}
                  </Badge>
                </div>
                <textarea
                  className="tcp-input mt-3 min-h-48 font-mono text-xs leading-5"
                  value={incidentMonitorDestinationsJson}
                  onInput={(event) => {
                    setIncidentMonitorDestinationsJson((event.target as HTMLTextAreaElement).value);
                    setIncidentMonitorRoutingConfigError("");
                  }}
                  spellCheck={false}
                />
                <div className="mt-3 grid gap-2">
                  {safeIncidentMonitorDestinations.length ? (
                    safeIncidentMonitorDestinations.map((destination, index) => {
                      const destinationId = recordText(destination, ["destination_id", "id"]);
                      const readiness = readinessByDestination.get(destinationId);
                      const ready = readiness?.ready === true || readiness?.publish_ready === true;
                      return (
                        <div
                          key={destinationId || `destination-${index}`}
                          className="rounded-lg border border-slate-700/70 p-2"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <span className="break-all text-sm font-medium">
                              {recordText(destination, ["name"], destinationId || "Destination")}
                            </span>
                            <Badge tone={ready ? "ok" : "warn"}>
                              {ready ? "Ready" : "Needs setup"}
                            </Badge>
                          </div>
                          <div className="tcp-subtle mt-1 text-xs">
                            {[
                              destinationId || "missing id",
                              recordText(destination, ["kind"], "unknown kind"),
                              readiness?.detail || "",
                            ]
                              .filter(Boolean)
                              .join(" | ")}
                          </div>
                          {Array.isArray(readiness?.missing) && readiness.missing.length ? (
                            <div className="tcp-subtle mt-1 text-xs">
                              Missing: {readiness.missing.join(", ")}
                            </div>
                          ) : null}
                        </div>
                      );
                    })
                  ) : (
                    <div className="tcp-subtle text-xs">
                      No explicit destinations saved; legacy GitHub settings are still honored.
                    </div>
                  )}
                </div>
              </div>

              <div className="tcp-list-item">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="text-sm font-medium">Routing</div>
                    <div className="tcp-subtle text-xs">
                      Routes match source, risk, tags, tenant/workspace, and expected destination.
                    </div>
                  </div>
                  <Badge tone={safeIncidentMonitorRoutes.length ? "info" : "ghost"}>
                    {safeIncidentMonitorRoutes.length}
                  </Badge>
                </div>
                <label className="mt-3 grid gap-2">
                  <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                    Default destinations
                  </span>
                  <input
                    className="tcp-input"
                    value={incidentMonitorDefaultDestinationsText}
                    onInput={(event) =>
                      setIncidentMonitorDefaultDestinationsText(
                        (event.target as HTMLInputElement).value
                      )
                    }
                    placeholder="legacy-github, linear-triage"
                  />
                </label>
                <textarea
                  className="tcp-input mt-3 min-h-48 font-mono text-xs leading-5"
                  value={incidentMonitorRoutesJson}
                  onInput={(event) => {
                    setIncidentMonitorRoutesJson((event.target as HTMLTextAreaElement).value);
                    setIncidentMonitorRoutingConfigError("");
                  }}
                  spellCheck={false}
                />
                {safeIncidentMonitorRoutes.length ? (
                  <div className="mt-3 grid gap-2">
                    {safeIncidentMonitorRoutes.map((route, index) => (
                      <div
                        key={recordText(route, ["route_id", "id"], `route-${index}`)}
                        className="rounded-lg border border-slate-700/70 p-2"
                      >
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <span className="break-all text-sm font-medium">
                            {recordText(route, ["name"], recordText(route, ["route_id"], "Route"))}
                          </span>
                          <Badge tone={route.enabled === false ? "ghost" : "ok"}>
                            {route.enabled === false ? "Disabled" : "Enabled"}
                          </Badge>
                        </div>
                        <div className="tcp-subtle mt-1 text-xs">
                          destinations: {recordArrayText(route, "destination_ids") || "default"}
                        </div>
                        <div className="tcp-subtle mt-1 text-xs">
                          tags: {recordArrayText(route, "match_route_tags") || "any"}
                        </div>
                      </div>
                    ))}
                  </div>
                ) : null}
              </div>

              <div className="tcp-list-item">
                <div className="text-sm font-medium">Route Preview</div>
                <textarea
                  className="tcp-input mt-3 min-h-36 font-mono text-xs leading-5"
                  value={incidentMonitorRoutePreviewJson}
                  onInput={(event) => {
                    setIncidentMonitorRoutePreviewJson((event.target as HTMLTextAreaElement).value);
                    setIncidentMonitorRoutePreviewError("");
                  }}
                  spellCheck={false}
                />
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  <button
                    type="button"
                    className="tcp-btn h-8 px-3 text-xs"
                    disabled={incidentMonitorRoutePreviewMutation.isPending}
                    onClick={() => incidentMonitorRoutePreviewMutation.mutate()}
                  >
                    <Icon name="route" />
                    Preview
                  </button>
                  {routePreviewEffectiveDestinations.map((destinationId: string) => (
                    <Badge key={destinationId} tone="info">
                      {destinationId}
                    </Badge>
                  ))}
                  {incidentMonitorRoutePreviewResult ? (
                    <Badge tone={incidentMonitorRoutePreviewResult.blocked ? "warn" : "ok"}>
                      {incidentMonitorRoutePreviewResult.blocked ? "Blocked" : "Allowed"}
                    </Badge>
                  ) : null}
                </div>
                {routePreviewBlockedReasons.length ? (
                  <div className="tcp-subtle mt-2 text-xs">
                    Blocked: {routePreviewBlockedReasons.join(", ")}
                  </div>
                ) : null}
                {incidentMonitorRoutePreviewError ? (
                  <div className="mt-2 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-100">
                    {incidentMonitorRoutePreviewError}
                  </div>
                ) : null}
                {incidentMonitorRoutePreviewResult ? (
                  <pre className="tcp-code mt-3 max-h-52 overflow-auto whitespace-pre-wrap break-words text-xs">
                    {formatJson(incidentMonitorRoutePreviewResult)}
                  </pre>
                ) : null}
              </div>

              <div className="tcp-list-item">
                <div className="text-sm font-medium">Safety Defaults</div>
                <div className="tcp-subtle text-xs">
                  High-risk approval, redaction, unready destination blocking, and retention.
                </div>
                <textarea
                  className="tcp-input mt-3 min-h-36 font-mono text-xs leading-5"
                  value={incidentMonitorSafetyDefaultsJson}
                  onInput={(event) => {
                    setIncidentMonitorSafetyDefaultsJson(
                      (event.target as HTMLTextAreaElement).value
                    );
                    setIncidentMonitorRoutingConfigError("");
                  }}
                  spellCheck={false}
                />
                <div className="tcp-subtle mt-2 text-xs">
                  Scoped intake keys can submit reports only for their configured project and never
                  mutate destinations, routes, or published issues.
                </div>
                {incidentMonitorRoutingConfigError ? (
                  <div className="mt-2 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-100">
                    {incidentMonitorRoutingConfigError}
                  </div>
                ) : null}
              </div>
            </div>

            <div className="flex flex-wrap gap-2">
              <button
                className="tcp-btn-primary"
                disabled={saveIncidentMonitorMutation.isPending}
                title="Save Incident Monitor settings"
                aria-label="Save Incident Monitor settings"
                onClick={() => saveIncidentMonitorMutation.mutate()}
              >
                <Icon name="save" />
                {saveIncidentMonitorMutation.isPending ? "Saving..." : null}
              </button>
              <button
                className="tcp-icon-btn"
                title="Add MCP server"
                aria-label="Add MCP server"
                onClick={() => openMcpModal()}
              >
                <Icon name="plus" />
              </button>
              <button
                className="tcp-icon-btn"
                title="Open setup guide"
                aria-label="Open setup guide"
                onClick={() => setGithubMcpGuideOpen(true)}
              >
                <Icon name="external-link" />
              </button>
              <button
                className="tcp-icon-btn"
                title="Refresh capability bindings"
                aria-label="Refresh capability bindings"
                disabled={refreshIncidentMonitorBindingsMutation.isPending}
                onClick={() => refreshIncidentMonitorBindingsMutation.mutate()}
              >
                <Icon name="rotate-cw" />
              </button>
              <button
                className="tcp-icon-btn"
                title="Copy debug payload"
                aria-label="Copy debug payload"
                onClick={() => void copyIncidentMonitorDebugPayload()}
              >
                <Icon name="copy" />
              </button>
              {selectedIncidentMonitorServer ? (
                <button
                  className="tcp-icon-btn"
                  title={
                    selectedIncidentMonitorServer.connected
                      ? "Refresh selected MCP"
                      : "Connect selected MCP"
                  }
                  aria-label={
                    selectedIncidentMonitorServer.connected
                      ? "Refresh selected MCP"
                      : "Connect selected MCP"
                  }
                  disabled={mcpActionMutation.isPending}
                  onClick={() =>
                    mcpActionMutation.mutate({
                      action: selectedIncidentMonitorServer.connected ? "refresh" : "connect",
                      server: selectedIncidentMonitorServer,
                    })
                  }
                >
                  <Icon
                    name={
                      selectedIncidentMonitorServer.connected ? "refresh-cw" : "plug-zap"
                    }
                   />
                </button>
              ) : null}
            </div>

            <div className="grid gap-3 md:grid-cols-3">
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Readiness</div>
                <div className="mt-1 text-sm">
                  {incidentMonitorStatus.runtime?.monitoring_active
                    ? incidentMonitorStatus.readiness?.publish_ready
                      ? "Monitoring"
                      : "Watching locally"
                    : incidentMonitorStatus.runtime?.paused || incidentMonitorPaused
                      ? "Paused"
                      : incidentMonitorStatus.readiness?.ingest_ready
                        ? "Ready"
                        : "Blocked"}
                </div>
                <div className="tcp-subtle text-xs">
                  {incidentMonitorStatus.runtime?.last_runtime_error ||
                    incidentMonitorStatus.last_error ||
                    "No blocking issue reported."}
                </div>
                {!incidentMonitorStatus.readiness?.publish_ready &&
                Array.isArray(incidentMonitorStatus.missing_required_capabilities) &&
                incidentMonitorStatus.missing_required_capabilities.length ? (
                  <div className="tcp-subtle mt-2 text-xs">
                    Missing: {incidentMonitorStatus.missing_required_capabilities.join(", ")}
                  </div>
                ) : null}
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Selected MCP</div>
                <div className="mt-1 text-sm">
                  {selectedIncidentMonitorServer?.name || "None selected"}
                </div>
                <div className="tcp-subtle text-xs">
                  {selectedIncidentMonitorServer
                    ? selectedIncidentMonitorServer.connected
                      ? "Connected"
                      : "Disconnected"
                    : "No server selected"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Bindings: {incidentMonitorStatus.binding_source_version || "unknown version"}
                  {incidentMonitorStatus.bindings_last_merged_at_ms
                    ? ` · merged ${new Date(incidentMonitorStatus.bindings_last_merged_at_ms).toLocaleString()}`
                    : ""}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Local directory:{" "}
                  {incidentMonitorWorkspaceRoot ||
                    String(incidentMonitorStatus.config?.workspace_root || "").trim() ||
                    "engine workspace root"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Last event:{" "}
                  {String(incidentMonitorStatus.runtime?.last_incident_event_type || "").trim() ||
                    "No incidents processed yet"}
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Model route</div>
                <div className="mt-1 break-all text-sm">
                  {incidentMonitorStatus.selected_model?.provider_id &&
                  incidentMonitorStatus.selected_model?.model_id
                    ? `${incidentMonitorStatus.selected_model.provider_id} / ${incidentMonitorStatus.selected_model.model_id}`
                    : "No dedicated model selected"}
                </div>
                <div className="tcp-subtle text-xs">
                  {incidentMonitorStatus.readiness?.selected_model_ready
                    ? "Available"
                    : "Fail-closed when unavailable"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Last processed:{" "}
                  {incidentMonitorStatus.runtime?.last_processed_at_ms
                    ? new Date(
                        Number(incidentMonitorStatus.runtime.last_processed_at_ms)
                      ).toLocaleString()
                    : "Not processed yet"}
                </div>
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-2">
              <div className="tcp-list-item">
                <div className="font-medium">Capability readiness</div>
                <div className="tcp-subtle mt-2 grid gap-1 text-xs">
                  <div>
                    github.list_issues:{" "}
                    {incidentMonitorStatus.required_capabilities?.github_list_issues
                      ? "ready"
                      : "missing"}
                  </div>
                  <div>
                    github.get_issue:{" "}
                    {incidentMonitorStatus.required_capabilities?.github_get_issue
                      ? "ready"
                      : "missing"}
                  </div>
                  <div>
                    github.create_issue:{" "}
                    {incidentMonitorStatus.required_capabilities?.github_create_issue
                      ? "ready"
                      : "missing"}
                  </div>
                  <div>
                    github.comment_on_issue:{" "}
                    {incidentMonitorStatus.required_capabilities?.github_comment_on_issue
                      ? "ready"
                      : "missing"}
                  </div>
                </div>
                {Array.isArray(incidentMonitorStatus.resolved_capabilities) &&
                incidentMonitorStatus.resolved_capabilities.length ? (
                  <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                    {incidentMonitorStatus.resolved_capabilities.map((row, index) => (
                      <div key={`${row.capability_id || "cap"}-${index}`}>
                        {String(row.capability_id || "unknown")}:{" "}
                        {String(row.tool_name || "unresolved")}
                      </div>
                    ))}
                  </div>
                ) : null}
                {Array.isArray(incidentMonitorStatus.selected_server_binding_candidates) &&
                incidentMonitorStatus.selected_server_binding_candidates.length ? (
                  <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                    {incidentMonitorStatus.selected_server_binding_candidates.map((row, index) => (
                      <div key={`${row.capability_id || "candidate"}-${index}`}>
                        {String(row.capability_id || "unknown")}:{" "}
                        {String(row.binding_tool_name || "unknown")}
                        {row.matched ? " · matched" : " · candidate"}
                      </div>
                    ))}
                  </div>
                ) : null}
                {Array.isArray(incidentMonitorStatus.discovered_mcp_tools) &&
                incidentMonitorStatus.discovered_mcp_tools.length ? (
                  <div className="mt-3">
                    <div className="tcp-subtle text-xs font-medium">Discovered MCP tools</div>
                    <pre className="tcp-code mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">
                      {incidentMonitorStatus.discovered_mcp_tools.join("\n")}
                    </pre>
                  </div>
                ) : (
                  <div className="tcp-subtle mt-3 text-xs">
                    No MCP tools were discovered for the selected server.
                  </div>
                )}
              </div>

              <div className="tcp-list-item">
                <div className="font-medium">Posting policy</div>
                <div className="tcp-subtle mt-2 grid gap-1 text-xs">
                  <div>
                    New issues:{" "}
                    {incidentMonitorRequireApproval
                      ? "Manual publish"
                      : incidentMonitorAutoCreateIssues
                        ? "Auto-create"
                        : "Internal draft only"}
                  </div>
                  <div>
                    Matched open issues:{" "}
                    {incidentMonitorAutoComment ? "Auto-comment" : "Detect only"}
                  </div>
                  <div>Dedupe: Fingerprint marker + label</div>
                  <div>Labels: incident-monitor</div>
                  <div>Workspace write tools: Disabled</div>
                  <div>Model fallback: Fail closed</div>
                </div>
              </div>
            </div>

            <IncidentMonitorExternalProjectsPanel
              projects={incidentMonitorMonitoredProjects}
              watcher={incidentMonitorLogWatcher}
              projectsJson={incidentMonitorMonitoredProjectsJson}
              projectsJsonError={incidentMonitorMonitoredProjectsError}
              intakeKeys={incidentMonitorIntakeKeys}
              createdRawKey={incidentMonitorCreatedIntakeKey}
              isCreatingKey={createIncidentMonitorIntakeKeyMutation.isPending}
              disablingKeyId={incidentMonitorDisablingIntakeKeyId}
              resettingSourceKey={incidentMonitorResettingSourceKey}
              replayingSourceKey={incidentMonitorReplayingSourceKey}
              actionResult={incidentMonitorLogSourceActionResult}
              onProjectsJsonChange={(value) => {
                setIncidentMonitorMonitoredProjectsJson(value);
                try {
                  const parsed = JSON.parse(value || "[]");
                  setIncidentMonitorMonitoredProjectsError(
                    Array.isArray(parsed) ? "" : "monitored_projects must be a JSON array"
                  );
                } catch (error) {
                  setIncidentMonitorMonitoredProjectsError(
                    error instanceof Error ? error.message : "Invalid JSON"
                  );
                }
              }}
              onCreateKey={(input) => createIncidentMonitorIntakeKeyMutation.mutate(input)}
              onDisableKey={(keyId) => disableIncidentMonitorIntakeKeyMutation.mutate(keyId)}
              onClearCreatedRawKey={() => setIncidentMonitorCreatedIntakeKey("")}
              onResetSourceOffset={(input) => resetIncidentMonitorLogSourceMutation.mutate(input)}
              onReplayLatestSourceCandidate={(input) =>
                replayIncidentMonitorLogSourceMutation.mutate(input)
              }
            />

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Recent incidents</div>
              {safeIncidentMonitorIncidents.length ? (
                <div className="grid gap-2">
                  {safeIncidentMonitorIncidents.map((incident) => (
                    <div key={incident.incident_id} className="tcp-list-item">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium">{incident.title || incident.event_type}</div>
                        <Badge tone={incident.last_error ? "warn" : "info"}>
                          {incident.status}
                        </Badge>
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {incident.event_type} · seen {Number(incident.occurrence_count || 0)}x
                        {" · "}
                        {incident.updated_at_ms
                          ? new Date(incident.updated_at_ms).toLocaleString()
                          : "time unavailable"}
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {incident.workspace_root || "engine workspace root"}
                      </div>
                      {incident.last_error ? (
                        <div className="tcp-subtle mt-1 text-xs">{incident.last_error}</div>
                      ) : null}
                      {incident.detail ? (
                        <div className="tcp-subtle mt-1 text-xs">{incident.detail}</div>
                      ) : null}
                      <div className="mt-3 flex flex-wrap gap-2">
                        <button
                          className="tcp-icon-btn"
                          title="Replay triage for this incident"
                          aria-label="Replay triage for this incident"
                          disabled={incidentMonitorReplayIncidentMutation.isPending}
                          onClick={() =>
                            incidentMonitorReplayIncidentMutation.mutate({
                              incidentId: incident.incident_id,
                            })
                          }
                        >
                          <Icon name="rotate-cw" />
                        </button>
                        {incident.triage_run_id ? (
                          <span className="tcp-subtle text-xs">
                            triage run: {incident.triage_run_id}
                          </span>
                        ) : null}
                        {incident.draft_id ? (
                          <span className="tcp-subtle text-xs">draft: {incident.draft_id}</span>
                        ) : null}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <EmptyState text="No Incident Monitor incidents yet." />
              )}
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Recent reporter drafts</div>
              {safeIncidentMonitorDrafts.length ? (
                <div className="grid gap-2">
                  {safeIncidentMonitorDrafts.map((draft) => (
                    <div key={draft.draft_id} className="tcp-list-item">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium">{draft.title || draft.fingerprint}</div>
                        <Badge tone={draft.status === "approval_required" ? "warn" : "info"}>
                          {draft.status}
                        </Badge>
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {draft.repo} ·{" "}
                        {draft.issue_number ? `issue #${draft.issue_number}` : "draft only"} ·{" "}
                        {draft.created_at_ms
                          ? new Date(draft.created_at_ms).toLocaleString()
                          : "time unavailable"}
                      </div>
                      {draft.github_status ? (
                        <div className="tcp-subtle mt-1 text-xs">
                          GitHub: {draft.github_status}
                          {draft.matched_issue_number
                            ? ` · matched #${draft.matched_issue_number}${draft.matched_issue_state ? ` (${draft.matched_issue_state})` : ""}`
                            : ""}
                        </div>
                      ) : null}
                      {draft.detail ? (
                        <div className="tcp-subtle mt-1 text-xs">{draft.detail}</div>
                      ) : null}
                      {draft.last_post_error ? (
                        <div className="tcp-subtle mt-1 text-xs">{draft.last_post_error}</div>
                      ) : null}
                      {draft.triage_run_id ? (
                        <div className="tcp-subtle mt-2 text-xs">
                          triage run: {draft.triage_run_id}
                        </div>
                      ) : null}
                      {draft.status === "approval_required" ? (
                        <div className="mt-3 flex flex-wrap gap-2">
                          <button
                            className="tcp-btn-primary"
                            disabled={incidentMonitorDraftDecisionMutation.isPending}
                            title="Approve draft"
                            aria-label="Approve draft"
                            onClick={() =>
                              incidentMonitorDraftDecisionMutation.mutate({
                                draftId: draft.draft_id,
                                decision: "approve",
                              })
                            }
                          >
                            <Icon name="check" />
                            {incidentMonitorDraftDecisionMutation.isPending ? "Updating..." : null}
                          </button>
                          <button
                            className="tcp-icon-btn"
                            title="Deny draft"
                            aria-label="Deny draft"
                            disabled={incidentMonitorDraftDecisionMutation.isPending}
                            onClick={() =>
                              incidentMonitorDraftDecisionMutation.mutate({
                                draftId: draft.draft_id,
                                decision: "deny",
                              })
                            }
                          >
                            <Icon name="x" />
                          </button>
                        </div>
                      ) : null}
                      {!draft.issue_number ? (
                        <div className="mt-3 flex flex-wrap gap-2">
                          <button
                            className="tcp-icon-btn"
                            title="Publish this draft to GitHub now"
                            aria-label="Publish this draft to GitHub now"
                            disabled={incidentMonitorPublishDraftMutation.isPending}
                            onClick={() =>
                              incidentMonitorPublishDraftMutation.mutate({
                                draftId: draft.draft_id,
                              })
                            }
                          >
                            <Icon name="shield-alert" />
                          </button>
                          <button
                            className="tcp-icon-btn"
                            title="Recheck GitHub for an existing matching issue"
                            aria-label="Recheck GitHub for an existing matching issue"
                            disabled={incidentMonitorRecheckMatchMutation.isPending}
                            onClick={() =>
                              incidentMonitorRecheckMatchMutation.mutate({
                                draftId: draft.draft_id,
                              })
                            }
                          >
                            <Icon name="refresh-cw" />
                          </button>
                        </div>
                      ) : null}
                      {(draft.github_issue_url || draft.github_comment_url) && (
                        <div className="mt-3 flex flex-wrap gap-2 text-xs">
                          {draft.github_issue_url ? (
                            <a
                              className="tcp-btn"
                              href={draft.github_issue_url}
                              target="_blank"
                              rel="noreferrer"
                            >
                              <Icon name="external-link" />
                              Open issue
                            </a>
                          ) : null}
                          {draft.github_comment_url ? (
                            <a
                              className="tcp-btn"
                              href={draft.github_comment_url}
                              target="_blank"
                              rel="noreferrer"
                            >
                              <Icon name="message-square" />
                              Open comment
                            </a>
                          ) : null}
                        </div>
                      )}
                      {(draft.status === "draft_ready" || draft.status === "triage_queued") &&
                      !draft.triage_run_id ? (
                        <div className="mt-3 flex flex-wrap gap-2">
                          <button
                            className="tcp-icon-btn"
                            title="Create triage run"
                            aria-label="Create triage run"
                            disabled={incidentMonitorTriageRunMutation.isPending}
                            onClick={() =>
                              incidentMonitorTriageRunMutation.mutate({
                                draftId: draft.draft_id,
                              })
                            }
                          >
                            <Icon name="sparkles" />
                          </button>
                        </div>
                      ) : null}
                    </div>
                  ))}
                </div>
              ) : (
                <EmptyState text="No Incident Monitor drafts yet." />
              )}
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Recent GitHub posts</div>
              {safeIncidentMonitorPosts.length ? (
                <div className="grid gap-2">
                  {safeIncidentMonitorPosts.map((post) => (
                    <div key={post.post_id} className="tcp-list-item">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium">{post.operation}</div>
                        <Badge tone={post.status === "posted" ? "ok" : "warn"}>{post.status}</Badge>
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {post.repo}
                        {post.issue_number ? ` · issue #${post.issue_number}` : ""}
                        {post.updated_at_ms
                          ? ` · ${new Date(post.updated_at_ms).toLocaleString()}`
                          : ""}
                      </div>
                      {post.error ? (
                        <div className="tcp-subtle mt-1 text-xs">{post.error}</div>
                      ) : null}
                      <div className="mt-3 flex flex-wrap gap-2">
                        {post.issue_url ? (
                          <a
                            className="tcp-btn"
                            href={post.issue_url}
                            target="_blank"
                            rel="noreferrer"
                          >
                            <Icon name="external-link" />
                            Open issue
                          </a>
                        ) : null}
                        {post.comment_url ? (
                          <a
                            className="tcp-btn"
                            href={post.comment_url}
                            target="_blank"
                            rel="noreferrer"
                          >
                            <Icon name="message-square" />
                            Open comment
                          </a>
                        ) : null}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <EmptyState text="No GitHub post attempts yet." />
              )}
            </div>
          </div>
        </PanelCard>
      ) : null}
    </>
  );
}
