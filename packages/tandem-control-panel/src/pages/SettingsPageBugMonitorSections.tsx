import { AnimatePresence, motion } from "motion/react";
import { BugMonitorExternalProjectsPanel } from "../components/BugMonitorExternalProjectsPanel";
import { Badge, PanelCard, Toolbar } from "../ui/index.tsx";
import { useSettingsPageController, HOSTED_CODER_REPO_ROOT } from "./SettingsPageController";
import { EmptyState } from "./ui";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageBugMonitorSectionsProps = {
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

export function SettingsPageBugMonitorSections({
  controller,
}: SettingsPageBugMonitorSectionsProps) {
  const {
    activeSection,
    bugMonitorAutoComment,
    bugMonitorAutoCreateIssues,
    bugMonitorCreatedIntakeKey,
    bugMonitorDefaultDestinationsText,
    bugMonitorDestinationReadiness,
    bugMonitorDestinations,
    bugMonitorDestinationsJson,
    bugMonitorDisablingIntakeKeyId,
    bugMonitorDraftDecisionMutation,
    bugMonitorDrafts,
    bugMonitorDraftsQuery,
    bugMonitorEnabled,
    bugMonitorIncidents,
    bugMonitorIncidentsQuery,
    bugMonitorIntakeKeys,
    bugMonitorLogSourceActionResult,
    bugMonitorLogWatcher,
    bugMonitorMcpServer,
    bugMonitorModelId,
    bugMonitorMonitoredProjects,
    bugMonitorMonitoredProjectsError,
    bugMonitorMonitoredProjectsJson,
    bugMonitorPauseResumeMutation,
    bugMonitorPaused,
    bugMonitorPosts,
    bugMonitorPostsQuery,
    bugMonitorProviderId,
    bugMonitorProviderModels,
    bugMonitorProviderPreference,
    bugMonitorPublishDraftMutation,
    bugMonitorRecheckMatchMutation,
    bugMonitorReplayIncidentMutation,
    bugMonitorReplayingSourceKey,
    bugMonitorRepo,
    bugMonitorRequireApproval,
    bugMonitorResettingSourceKey,
    bugMonitorRoutePreviewError,
    bugMonitorRoutePreviewJson,
    bugMonitorRoutePreviewMutation,
    bugMonitorRoutePreviewResult,
    bugMonitorRoutes,
    bugMonitorRoutesJson,
    bugMonitorRoutingConfigError,
    bugMonitorSafetyDefaultsJson,
    bugMonitorStatus,
    bugMonitorStatusQuery,
    bugMonitorSuggestedWorkspaceRoot,
    bugMonitorTriageRunMutation,
    bugMonitorWorkspaceRoot,
    bugMonitorWorkspaceRootHint,
    bugMonitorWorkspaceSetupWarningText,
    copyBugMonitorDebugPayload,
    createBugMonitorIntakeKeyMutation,
    disableBugMonitorIntakeKeyMutation,
    mcpActionMutation,
    mcpServers,
    openMcpModal,
    providers,
    refreshBugMonitorBindingsMutation,
    replayBugMonitorLogSourceMutation,
    resetBugMonitorLogSourceMutation,
    saveBugMonitorMutation,
    selectedBugMonitorServer,
    setBugMonitorAutoComment,
    setBugMonitorAutoCreateIssues,
    setBugMonitorCreatedIntakeKey,
    setBugMonitorDefaultDestinationsText,
    setBugMonitorDestinationsJson,
    setBugMonitorEnabled,
    setBugMonitorMcpServer,
    setBugMonitorModelId,
    setBugMonitorMonitoredProjectsError,
    setBugMonitorMonitoredProjectsJson,
    setBugMonitorProviderId,
    setBugMonitorProviderPreference,
    setBugMonitorRepo,
    setBugMonitorRequireApproval,
    setBugMonitorRoutePreviewError,
    setBugMonitorRoutePreviewJson,
    setBugMonitorRoutesJson,
    setBugMonitorRoutingConfigError,
    setBugMonitorSafetyDefaultsJson,
    setBugMonitorWorkspaceBrowserDir,
    setBugMonitorWorkspaceBrowserOpen,
    setBugMonitorWorkspaceBrowserSearch,
    setBugMonitorWorkspaceRoot,
    setGithubMcpGuideOpen,
    toast,
  } = controller;
  const safeMcpServers = Array.isArray(mcpServers) ? mcpServers : [];
  const safeProviders = Array.isArray(providers) ? providers : [];
  const safeBugMonitorProviderModels = Array.isArray(bugMonitorProviderModels)
    ? bugMonitorProviderModels
    : [];
  const safeBugMonitorIncidents = Array.isArray(bugMonitorIncidents) ? bugMonitorIncidents : [];
  const safeBugMonitorDrafts = Array.isArray(bugMonitorDrafts) ? bugMonitorDrafts : [];
  const safeBugMonitorPosts = Array.isArray(bugMonitorPosts) ? bugMonitorPosts : [];
  const safeBugMonitorDestinations = Array.isArray(bugMonitorDestinations)
    ? bugMonitorDestinations
    : [];
  const safeBugMonitorRoutes = Array.isArray(bugMonitorRoutes) ? bugMonitorRoutes : [];
  const safeBugMonitorDestinationReadiness = Array.isArray(bugMonitorDestinationReadiness)
    ? bugMonitorDestinationReadiness
    : [];
  const readinessByDestination = new Map(
    safeBugMonitorDestinationReadiness.map((row) => [
      String(row.destination_id || row.destinationId || ""),
      row,
    ])
  );
  const routePreviewEffectiveDestinations = Array.isArray(
    bugMonitorRoutePreviewResult?.effective_destination_ids
  )
    ? bugMonitorRoutePreviewResult.effective_destination_ids
    : [];
  const routePreviewBlockedReasons = Array.isArray(bugMonitorRoutePreviewResult?.blocked_reasons)
    ? bugMonitorRoutePreviewResult.blocked_reasons
    : [];

  return (
    <>
      {activeSection === "bug_monitor" ? (
        <PanelCard
          title="Incident Monitor"
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge
                tone={
                  bugMonitorStatus.runtime?.monitoring_active
                    ? bugMonitorStatus.readiness?.publish_ready
                      ? "ok"
                      : "info"
                    : bugMonitorStatus.readiness?.ingest_ready
                      ? "info"
                      : "warn"
                }
              >
                {bugMonitorStatus.runtime?.monitoring_active
                  ? bugMonitorStatus.readiness?.publish_ready
                    ? "Monitoring"
                    : "Watching locally"
                  : bugMonitorStatus.readiness?.ingest_ready
                    ? "Ready"
                    : "Not ready"}
              </Badge>
              {bugMonitorPaused || bugMonitorStatus.runtime?.paused ? (
                <Badge tone="warn">Paused</Badge>
              ) : null}
              <Badge tone="info">
                {Number(bugMonitorStatus.runtime?.pending_incidents || 0)} incidents
              </Badge>
              <Badge tone="info">
                {Number(bugMonitorStatus.pending_drafts || 0)} pending drafts
              </Badge>
              <Badge tone="info">{Number(bugMonitorStatus.pending_posts || 0)} post attempts</Badge>
              <button
                className="tcp-icon-btn"
                title="Reload status"
                aria-label="Reload status"
                onClick={() =>
                  Promise.all([
                    bugMonitorStatusQuery.refetch(),
                    bugMonitorDraftsQuery.refetch(),
                    bugMonitorIncidentsQuery.refetch(),
                    bugMonitorPostsQuery.refetch(),
                  ]).then(() => toast("ok", "Incident Monitor status refreshed."))
                }
              >
                <i data-lucide="refresh-cw"></i>
              </button>
              <button
                className="tcp-icon-btn"
                title={
                  bugMonitorPaused || bugMonitorStatus.runtime?.paused
                    ? "Resume monitoring"
                    : "Pause monitoring"
                }
                aria-label={
                  bugMonitorPaused || bugMonitorStatus.runtime?.paused
                    ? "Resume monitoring"
                    : "Pause monitoring"
                }
                disabled={bugMonitorPauseResumeMutation.isPending}
                onClick={() =>
                  bugMonitorPauseResumeMutation.mutate({
                    action:
                      bugMonitorPaused || bugMonitorStatus.runtime?.paused ? "resume" : "pause",
                  })
                }
              >
                <i
                  data-lucide={
                    bugMonitorPaused || bugMonitorStatus.runtime?.paused ? "play" : "pause"
                  }
                ></i>
              </button>
              <button
                className="tcp-icon-btn"
                title="Refresh capability bindings"
                aria-label="Refresh capability bindings"
                disabled={refreshBugMonitorBindingsMutation.isPending}
                onClick={() => refreshBugMonitorBindingsMutation.mutate()}
              >
                <i data-lucide="rotate-cw"></i>
              </button>
              <button
                className="tcp-icon-btn"
                title="Copy debug payload"
                aria-label="Copy debug payload"
                onClick={() => void copyBugMonitorDebugPayload()}
              >
                <i data-lucide="copy"></i>
              </button>
              <button
                className="tcp-icon-btn"
                title="Open GitHub MCP guide"
                aria-label="Open GitHub MCP guide"
                onClick={() => setGithubMcpGuideOpen(true)}
              >
                <i data-lucide="book-open"></i>
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
                  className={`tcp-list-item text-left ${bugMonitorEnabled ? "ring-1 ring-emerald-400/40" : ""}`}
                  onClick={() => setBugMonitorEnabled((prev) => !prev)}
                >
                  <div className="font-medium">
                    {bugMonitorEnabled ? (bugMonitorPaused ? "Paused" : "Enabled") : "Disabled"}
                  </div>
                  <div className="tcp-subtle text-xs">
                    {bugMonitorEnabled
                      ? bugMonitorPaused
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
                    Coder syncs source checkouts into <code>{HOSTED_CODER_REPO_ROOT}</code>. For Bug
                    Monitor analysis, select the repo folder itself, usually{" "}
                    <code>{bugMonitorSuggestedWorkspaceRoot}</code>.
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-8 px-3 text-xs"
                      type="button"
                      onClick={() => setBugMonitorWorkspaceRoot(bugMonitorSuggestedWorkspaceRoot)}
                    >
                      <i data-lucide="badge-check"></i>
                      Use recommended path
                    </button>
                    <button
                      className="tcp-btn h-8 px-3 text-xs"
                      type="button"
                      onClick={() => {
                        setBugMonitorWorkspaceBrowserDir(HOSTED_CODER_REPO_ROOT);
                        setBugMonitorWorkspaceBrowserSearch("");
                        setBugMonitorWorkspaceBrowserOpen(true);
                      }}
                    >
                      <i data-lucide="folder-open"></i>
                      Browse synced repos
                    </button>
                  </div>
                </div>
                <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
                  <button
                    className="tcp-btn"
                    type="button"
                    onClick={() => {
                      const seed = String(bugMonitorWorkspaceRoot || "/").trim();
                      setBugMonitorWorkspaceBrowserDir(seed || "/");
                      setBugMonitorWorkspaceBrowserSearch("");
                      setBugMonitorWorkspaceBrowserOpen(true);
                    }}
                  >
                    <i data-lucide="folder-open"></i>
                    Browse
                  </button>
                  <input
                    className="tcp-input"
                    readOnly
                    value={bugMonitorWorkspaceRoot}
                    placeholder="No local directory selected. Use Browse."
                  />
                  <button
                    className="tcp-btn"
                    type="button"
                    onClick={() => setBugMonitorWorkspaceRoot("")}
                    disabled={!bugMonitorWorkspaceRoot}
                  >
                    <i data-lucide="x"></i>
                    Clear
                  </button>
                </div>
                <div className="tcp-subtle text-xs">
                  {bugMonitorWorkspaceRoot
                    ? `Reporter analysis root: ${bugMonitorWorkspaceRoot}${
                        bugMonitorWorkspaceRootHint ? ` (${bugMonitorWorkspaceRootHint})` : ""
                      }`
                    : "Defaults to the engine workspace root if not set."}
                </div>
                {bugMonitorWorkspaceSetupWarningText ? (
                  <div className="rounded-xl border border-amber-500/25 bg-amber-500/10 p-3 text-xs text-amber-100">
                    <div className="font-semibold">Setup check</div>
                    <div className="mt-1">{bugMonitorWorkspaceSetupWarningText}</div>
                  </div>
                ) : (
                  <div className="rounded-xl border border-emerald-500/20 bg-emerald-500/10 p-3 text-xs text-emerald-100">
                    <div className="font-semibold">Source checkout ready</div>
                    <div className="mt-1">
                      Bug Monitor triage will inspect this repo path and require concrete
                      source-file reads before it marks research complete.
                    </div>
                  </div>
                )}
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Target repo</span>
                <input
                  className="tcp-input"
                  value={bugMonitorRepo}
                  onChange={(event) => setBugMonitorRepo(event.target.value)}
                  placeholder="owner/repo"
                />
              </label>

              <label className="grid gap-2">
                <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">MCP server</span>
                <select
                  className="tcp-input"
                  value={bugMonitorMcpServer}
                  onChange={(event) => setBugMonitorMcpServer(event.target.value)}
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
                  value={bugMonitorProviderPreference}
                  onChange={(event) => setBugMonitorProviderPreference(event.target.value)}
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
                  value={bugMonitorProviderId}
                  onChange={(event) => {
                    const nextProvider = event.target.value;
                    setBugMonitorProviderId(nextProvider);
                    setBugMonitorModelId("");
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
                  value={bugMonitorModelId}
                  onChange={(event) => setBugMonitorModelId(event.target.value)}
                  list="bug-monitor-models"
                  disabled={!bugMonitorProviderId}
                  placeholder={
                    bugMonitorProviderId ? "Type or paste a model id" : "Choose a provider first"
                  }
                  spellCheck={false}
                />
                <datalist id="bug-monitor-models">
                  {safeBugMonitorProviderModels.map((modelId) => (
                    <option key={modelId} value={modelId} />
                  ))}
                </datalist>
                <div className="tcp-subtle text-xs">
                  {bugMonitorProviderId
                    ? safeBugMonitorProviderModels.length
                      ? `${safeBugMonitorProviderModels.length} suggested models from provider catalog`
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
                    className={`tcp-list-item text-left ${bugMonitorAutoCreateIssues && !bugMonitorRequireApproval ? "ring-1 ring-emerald-400/40" : ""}`}
                    onClick={() => {
                      setBugMonitorAutoCreateIssues((prev) => !prev);
                      if (bugMonitorRequireApproval && bugMonitorAutoCreateIssues) {
                        setBugMonitorRequireApproval(false);
                      }
                    }}
                  >
                    <div className="font-medium">Auto-create new issues</div>
                    <div className="tcp-subtle text-xs">
                      {bugMonitorAutoCreateIssues
                        ? "New drafts post to GitHub automatically."
                        : "New drafts stay internal until published manually."}
                    </div>
                  </button>
                  <button
                    type="button"
                    className={`tcp-list-item text-left ${bugMonitorRequireApproval ? "ring-1 ring-amber-400/40" : ""}`}
                    onClick={() => {
                      setBugMonitorRequireApproval((prev) => {
                        const next = !prev;
                        if (next) setBugMonitorAutoCreateIssues(false);
                        return next;
                      });
                    }}
                  >
                    <div className="font-medium">Require approval</div>
                    <div className="tcp-subtle text-xs">
                      {bugMonitorRequireApproval
                        ? "New drafts wait for a manual publish click."
                        : "Approval gate disabled."}
                    </div>
                  </button>
                  <button
                    type="button"
                    className={`tcp-list-item text-left ${bugMonitorAutoComment ? "ring-1 ring-sky-400/40" : ""}`}
                    onClick={() => setBugMonitorAutoComment((prev) => !prev)}
                  >
                    <div className="font-medium">Auto-comment matches</div>
                    <div className="tcp-subtle text-xs">
                      {bugMonitorAutoComment
                        ? "Open matching GitHub issues receive new evidence comments."
                        : "Matching issues are detected but not updated automatically."}
                    </div>
                  </button>
                </div>
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-4">
              {[
                ["Sources", `${bugMonitorMonitoredProjects.length} monitored project(s)`],
                ["Destinations", `${safeBugMonitorDestinations.length} configured`],
                ["Routing", `${safeBugMonitorRoutes.length} route(s)`],
                [
                  "Safety Defaults",
                  bugMonitorRequireApproval ? "Approval gate active" : "Policy inherits route",
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
                  <Badge tone={safeBugMonitorDestinations.length ? "info" : "ghost"}>
                    {safeBugMonitorDestinations.length}
                  </Badge>
                </div>
                <textarea
                  className="tcp-input mt-3 min-h-48 font-mono text-xs leading-5"
                  value={bugMonitorDestinationsJson}
                  onInput={(event) => {
                    setBugMonitorDestinationsJson((event.target as HTMLTextAreaElement).value);
                    setBugMonitorRoutingConfigError("");
                  }}
                  spellCheck={false}
                />
                <div className="mt-3 grid gap-2">
                  {safeBugMonitorDestinations.length ? (
                    safeBugMonitorDestinations.map((destination, index) => {
                      const destinationId = recordText(destination, ["destination_id", "id"]);
                      const readiness = readinessByDestination.get(destinationId);
                      const ready =
                        readiness?.ready === true || readiness?.publish_ready === true;
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
                  <Badge tone={safeBugMonitorRoutes.length ? "info" : "ghost"}>
                    {safeBugMonitorRoutes.length}
                  </Badge>
                </div>
                <label className="mt-3 grid gap-2">
                  <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">
                    Default destinations
                  </span>
                  <input
                    className="tcp-input"
                    value={bugMonitorDefaultDestinationsText}
                    onInput={(event) =>
                      setBugMonitorDefaultDestinationsText(
                        (event.target as HTMLInputElement).value
                      )
                    }
                    placeholder="legacy-github, linear-triage"
                  />
                </label>
                <textarea
                  className="tcp-input mt-3 min-h-48 font-mono text-xs leading-5"
                  value={bugMonitorRoutesJson}
                  onInput={(event) => {
                    setBugMonitorRoutesJson((event.target as HTMLTextAreaElement).value);
                    setBugMonitorRoutingConfigError("");
                  }}
                  spellCheck={false}
                />
                {safeBugMonitorRoutes.length ? (
                  <div className="mt-3 grid gap-2">
                    {safeBugMonitorRoutes.map((route, index) => (
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
                  value={bugMonitorRoutePreviewJson}
                  onInput={(event) => {
                    setBugMonitorRoutePreviewJson((event.target as HTMLTextAreaElement).value);
                    setBugMonitorRoutePreviewError("");
                  }}
                  spellCheck={false}
                />
                <div className="mt-3 flex flex-wrap items-center gap-2">
                  <button
                    type="button"
                    className="tcp-btn h-8 px-3 text-xs"
                    disabled={bugMonitorRoutePreviewMutation.isPending}
                    onClick={() => bugMonitorRoutePreviewMutation.mutate()}
                  >
                    <i data-lucide="route"></i>
                    Preview
                  </button>
                  {routePreviewEffectiveDestinations.map((destinationId: string) => (
                    <Badge key={destinationId} tone="info">
                      {destinationId}
                    </Badge>
                  ))}
                  {bugMonitorRoutePreviewResult ? (
                    <Badge tone={bugMonitorRoutePreviewResult.blocked ? "warn" : "ok"}>
                      {bugMonitorRoutePreviewResult.blocked ? "Blocked" : "Allowed"}
                    </Badge>
                  ) : null}
                </div>
                {routePreviewBlockedReasons.length ? (
                  <div className="tcp-subtle mt-2 text-xs">
                    Blocked: {routePreviewBlockedReasons.join(", ")}
                  </div>
                ) : null}
                {bugMonitorRoutePreviewError ? (
                  <div className="mt-2 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-100">
                    {bugMonitorRoutePreviewError}
                  </div>
                ) : null}
                {bugMonitorRoutePreviewResult ? (
                  <pre className="tcp-code mt-3 max-h-52 overflow-auto whitespace-pre-wrap break-words text-xs">
                    {formatJson(bugMonitorRoutePreviewResult)}
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
                  value={bugMonitorSafetyDefaultsJson}
                  onInput={(event) => {
                    setBugMonitorSafetyDefaultsJson((event.target as HTMLTextAreaElement).value);
                    setBugMonitorRoutingConfigError("");
                  }}
                  spellCheck={false}
                />
                <div className="tcp-subtle mt-2 text-xs">
                  Scoped intake keys can submit reports only for their configured project and never
                  mutate destinations, routes, or published issues.
                </div>
                {bugMonitorRoutingConfigError ? (
                  <div className="mt-2 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-100">
                    {bugMonitorRoutingConfigError}
                  </div>
                ) : null}
              </div>
            </div>

            <div className="flex flex-wrap gap-2">
              <button
                className="tcp-btn-primary"
                disabled={saveBugMonitorMutation.isPending}
                title="Save Incident Monitor settings"
                aria-label="Save Incident Monitor settings"
                onClick={() => saveBugMonitorMutation.mutate()}
              >
                <i data-lucide="save"></i>
                {saveBugMonitorMutation.isPending ? "Saving..." : null}
              </button>
              <button
                className="tcp-icon-btn"
                title="Add MCP server"
                aria-label="Add MCP server"
                onClick={() => openMcpModal()}
              >
                <i data-lucide="plus"></i>
              </button>
              <button
                className="tcp-icon-btn"
                title="Open setup guide"
                aria-label="Open setup guide"
                onClick={() => setGithubMcpGuideOpen(true)}
              >
                <i data-lucide="external-link"></i>
              </button>
              <button
                className="tcp-icon-btn"
                title="Refresh capability bindings"
                aria-label="Refresh capability bindings"
                disabled={refreshBugMonitorBindingsMutation.isPending}
                onClick={() => refreshBugMonitorBindingsMutation.mutate()}
              >
                <i data-lucide="rotate-cw"></i>
              </button>
              <button
                className="tcp-icon-btn"
                title="Copy debug payload"
                aria-label="Copy debug payload"
                onClick={() => void copyBugMonitorDebugPayload()}
              >
                <i data-lucide="copy"></i>
              </button>
              {selectedBugMonitorServer ? (
                <button
                  className="tcp-icon-btn"
                  title={
                    selectedBugMonitorServer.connected
                      ? "Refresh selected MCP"
                      : "Connect selected MCP"
                  }
                  aria-label={
                    selectedBugMonitorServer.connected
                      ? "Refresh selected MCP"
                      : "Connect selected MCP"
                  }
                  disabled={mcpActionMutation.isPending}
                  onClick={() =>
                    mcpActionMutation.mutate({
                      action: selectedBugMonitorServer.connected ? "refresh" : "connect",
                      server: selectedBugMonitorServer,
                    })
                  }
                >
                  <i
                    data-lucide={selectedBugMonitorServer.connected ? "refresh-cw" : "plug-zap"}
                  ></i>
                </button>
              ) : null}
            </div>

            <div className="grid gap-3 md:grid-cols-3">
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Readiness</div>
                <div className="mt-1 text-sm">
                  {bugMonitorStatus.runtime?.monitoring_active
                    ? bugMonitorStatus.readiness?.publish_ready
                      ? "Monitoring"
                      : "Watching locally"
                    : bugMonitorStatus.runtime?.paused || bugMonitorPaused
                      ? "Paused"
                      : bugMonitorStatus.readiness?.ingest_ready
                        ? "Ready"
                        : "Blocked"}
                </div>
                <div className="tcp-subtle text-xs">
                  {bugMonitorStatus.runtime?.last_runtime_error ||
                    bugMonitorStatus.last_error ||
                    "No blocking issue reported."}
                </div>
                {!bugMonitorStatus.readiness?.publish_ready &&
                Array.isArray(bugMonitorStatus.missing_required_capabilities) &&
                bugMonitorStatus.missing_required_capabilities.length ? (
                  <div className="tcp-subtle mt-2 text-xs">
                    Missing: {bugMonitorStatus.missing_required_capabilities.join(", ")}
                  </div>
                ) : null}
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Selected MCP</div>
                <div className="mt-1 text-sm">
                  {selectedBugMonitorServer?.name || "None selected"}
                </div>
                <div className="tcp-subtle text-xs">
                  {selectedBugMonitorServer
                    ? selectedBugMonitorServer.connected
                      ? "Connected"
                      : "Disconnected"
                    : "No server selected"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Bindings: {bugMonitorStatus.binding_source_version || "unknown version"}
                  {bugMonitorStatus.bindings_last_merged_at_ms
                    ? ` · merged ${new Date(bugMonitorStatus.bindings_last_merged_at_ms).toLocaleString()}`
                    : ""}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Local directory:{" "}
                  {bugMonitorWorkspaceRoot ||
                    String(bugMonitorStatus.config?.workspace_root || "").trim() ||
                    "engine workspace root"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Last event:{" "}
                  {String(bugMonitorStatus.runtime?.last_incident_event_type || "").trim() ||
                    "No incidents processed yet"}
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Model route</div>
                <div className="mt-1 break-all text-sm">
                  {bugMonitorStatus.selected_model?.provider_id &&
                  bugMonitorStatus.selected_model?.model_id
                    ? `${bugMonitorStatus.selected_model.provider_id} / ${bugMonitorStatus.selected_model.model_id}`
                    : "No dedicated model selected"}
                </div>
                <div className="tcp-subtle text-xs">
                  {bugMonitorStatus.readiness?.selected_model_ready
                    ? "Available"
                    : "Fail-closed when unavailable"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  Last processed:{" "}
                  {bugMonitorStatus.runtime?.last_processed_at_ms
                    ? new Date(
                        Number(bugMonitorStatus.runtime.last_processed_at_ms)
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
                    {bugMonitorStatus.required_capabilities?.github_list_issues
                      ? "ready"
                      : "missing"}
                  </div>
                  <div>
                    github.get_issue:{" "}
                    {bugMonitorStatus.required_capabilities?.github_get_issue ? "ready" : "missing"}
                  </div>
                  <div>
                    github.create_issue:{" "}
                    {bugMonitorStatus.required_capabilities?.github_create_issue
                      ? "ready"
                      : "missing"}
                  </div>
                  <div>
                    github.comment_on_issue:{" "}
                    {bugMonitorStatus.required_capabilities?.github_comment_on_issue
                      ? "ready"
                      : "missing"}
                  </div>
                </div>
                {Array.isArray(bugMonitorStatus.resolved_capabilities) &&
                bugMonitorStatus.resolved_capabilities.length ? (
                  <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                    {bugMonitorStatus.resolved_capabilities.map((row, index) => (
                      <div key={`${row.capability_id || "cap"}-${index}`}>
                        {String(row.capability_id || "unknown")}:{" "}
                        {String(row.tool_name || "unresolved")}
                      </div>
                    ))}
                  </div>
                ) : null}
                {Array.isArray(bugMonitorStatus.selected_server_binding_candidates) &&
                bugMonitorStatus.selected_server_binding_candidates.length ? (
                  <div className="tcp-subtle mt-3 grid gap-1 text-xs">
                    {bugMonitorStatus.selected_server_binding_candidates.map((row, index) => (
                      <div key={`${row.capability_id || "candidate"}-${index}`}>
                        {String(row.capability_id || "unknown")}:{" "}
                        {String(row.binding_tool_name || "unknown")}
                        {row.matched ? " · matched" : " · candidate"}
                      </div>
                    ))}
                  </div>
                ) : null}
                {Array.isArray(bugMonitorStatus.discovered_mcp_tools) &&
                bugMonitorStatus.discovered_mcp_tools.length ? (
                  <div className="mt-3">
                    <div className="tcp-subtle text-xs font-medium">Discovered MCP tools</div>
                    <pre className="tcp-code mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">
                      {bugMonitorStatus.discovered_mcp_tools.join("\n")}
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
                    {bugMonitorRequireApproval
                      ? "Manual publish"
                      : bugMonitorAutoCreateIssues
                        ? "Auto-create"
                        : "Internal draft only"}
                  </div>
                  <div>
                    Matched open issues: {bugMonitorAutoComment ? "Auto-comment" : "Detect only"}
                  </div>
                  <div>Dedupe: Fingerprint marker + label</div>
                  <div>Labels: bug-monitor</div>
                  <div>Workspace write tools: Disabled</div>
                  <div>Model fallback: Fail closed</div>
                </div>
              </div>
            </div>

            <BugMonitorExternalProjectsPanel
              projects={bugMonitorMonitoredProjects}
              watcher={bugMonitorLogWatcher}
              projectsJson={bugMonitorMonitoredProjectsJson}
              projectsJsonError={bugMonitorMonitoredProjectsError}
              intakeKeys={bugMonitorIntakeKeys}
              createdRawKey={bugMonitorCreatedIntakeKey}
              isCreatingKey={createBugMonitorIntakeKeyMutation.isPending}
              disablingKeyId={bugMonitorDisablingIntakeKeyId}
              resettingSourceKey={bugMonitorResettingSourceKey}
              replayingSourceKey={bugMonitorReplayingSourceKey}
              actionResult={bugMonitorLogSourceActionResult}
              onProjectsJsonChange={(value) => {
                setBugMonitorMonitoredProjectsJson(value);
                try {
                  const parsed = JSON.parse(value || "[]");
                  setBugMonitorMonitoredProjectsError(
                    Array.isArray(parsed) ? "" : "monitored_projects must be a JSON array"
                  );
                } catch (error) {
                  setBugMonitorMonitoredProjectsError(
                    error instanceof Error ? error.message : "Invalid JSON"
                  );
                }
              }}
              onCreateKey={(input) => createBugMonitorIntakeKeyMutation.mutate(input)}
              onDisableKey={(keyId) => disableBugMonitorIntakeKeyMutation.mutate(keyId)}
              onClearCreatedRawKey={() => setBugMonitorCreatedIntakeKey("")}
              onResetSourceOffset={(input) => resetBugMonitorLogSourceMutation.mutate(input)}
              onReplayLatestSourceCandidate={(input) =>
                replayBugMonitorLogSourceMutation.mutate(input)
              }
            />

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Recent incidents</div>
              {safeBugMonitorIncidents.length ? (
                <div className="grid gap-2">
                  {safeBugMonitorIncidents.map((incident) => (
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
                          disabled={bugMonitorReplayIncidentMutation.isPending}
                          onClick={() =>
                            bugMonitorReplayIncidentMutation.mutate({
                              incidentId: incident.incident_id,
                            })
                          }
                        >
                          <i data-lucide="rotate-cw"></i>
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
                <EmptyState text="No Bug Monitor incidents yet." />
              )}
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Recent reporter drafts</div>
              {safeBugMonitorDrafts.length ? (
                <div className="grid gap-2">
                  {safeBugMonitorDrafts.map((draft) => (
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
                            disabled={bugMonitorDraftDecisionMutation.isPending}
                            title="Approve draft"
                            aria-label="Approve draft"
                            onClick={() =>
                              bugMonitorDraftDecisionMutation.mutate({
                                draftId: draft.draft_id,
                                decision: "approve",
                              })
                            }
                          >
                            <i data-lucide="check"></i>
                            {bugMonitorDraftDecisionMutation.isPending ? "Updating..." : null}
                          </button>
                          <button
                            className="tcp-icon-btn"
                            title="Deny draft"
                            aria-label="Deny draft"
                            disabled={bugMonitorDraftDecisionMutation.isPending}
                            onClick={() =>
                              bugMonitorDraftDecisionMutation.mutate({
                                draftId: draft.draft_id,
                                decision: "deny",
                              })
                            }
                          >
                            <i data-lucide="x"></i>
                          </button>
                        </div>
                      ) : null}
                      {!draft.issue_number ? (
                        <div className="mt-3 flex flex-wrap gap-2">
                          <button
                            className="tcp-icon-btn"
                            title="Publish this draft to GitHub now"
                            aria-label="Publish this draft to GitHub now"
                            disabled={bugMonitorPublishDraftMutation.isPending}
                            onClick={() =>
                              bugMonitorPublishDraftMutation.mutate({
                                draftId: draft.draft_id,
                              })
                            }
                          >
                            <i data-lucide="bug-play"></i>
                          </button>
                          <button
                            className="tcp-icon-btn"
                            title="Recheck GitHub for an existing matching issue"
                            aria-label="Recheck GitHub for an existing matching issue"
                            disabled={bugMonitorRecheckMatchMutation.isPending}
                            onClick={() =>
                              bugMonitorRecheckMatchMutation.mutate({
                                draftId: draft.draft_id,
                              })
                            }
                          >
                            <i data-lucide="refresh-cw"></i>
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
                              <i data-lucide="external-link"></i>
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
                              <i data-lucide="message-square"></i>
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
                            disabled={bugMonitorTriageRunMutation.isPending}
                            onClick={() =>
                              bugMonitorTriageRunMutation.mutate({
                                draftId: draft.draft_id,
                              })
                            }
                          >
                            <i data-lucide="sparkles"></i>
                          </button>
                        </div>
                      ) : null}
                    </div>
                  ))}
                </div>
              ) : (
                <EmptyState text="No Bug Monitor drafts yet." />
              )}
            </div>

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Recent GitHub posts</div>
              {safeBugMonitorPosts.length ? (
                <div className="grid gap-2">
                  {safeBugMonitorPosts.map((post) => (
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
                            <i data-lucide="external-link"></i>
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
                            <i data-lucide="message-square"></i>
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
