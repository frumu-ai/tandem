import { Badge } from "../ui/index.tsx";
import { EmptyState } from "../pages/ui";
import { useMemo, useState } from "react";

export type BugMonitorLogSourceDraft = {
  source_id?: string;
  path?: string;
  source_kind?: string | null;
  format?: string;
  minimum_level?: string;
  watch_interval_seconds?: number;
  enabled?: boolean;
  paused?: boolean;
  start_position?: string;
  max_bytes_per_poll?: number;
  max_candidates_per_poll?: number;
  fingerprint_cooldown_ms?: number;
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  default_route_tags?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  approval_policy?: string;
  redaction_profile?: string | null;
  retention_profile?: string | null;
};

export type BugMonitorMonitoredProjectDraft = {
  project_id?: string;
  name?: string;
  enabled?: boolean;
  paused?: boolean;
  repo?: string;
  workspace_root?: string;
  source_kind?: string;
  mcp_server?: string | null;
  model_policy?: Record<string, unknown> | null;
  allowed_destination_ids?: string[];
  default_destination_ids?: string[];
  default_route_tags?: string[];
  tenant_id?: string | null;
  workspace_id?: string | null;
  event_schema_version?: string | null;
  approval_policy?: string;
  redaction_profile?: string | null;
  retention_profile?: string | null;
  auto_create_new_issues?: boolean;
  require_approval_for_new_issues?: boolean;
  auto_comment_on_matched_open_issues?: boolean;
  log_sources?: BugMonitorLogSourceDraft[];
};

export type BugMonitorLogSourceRuntimeStatusDraft = {
  project_id?: string;
  source_id?: string;
  path?: string;
  healthy?: boolean;
  offset?: number;
  inode?: string | null;
  file_size?: number | null;
  last_poll_at_ms?: number | null;
  last_candidate_at_ms?: number | null;
  last_submitted_at_ms?: number | null;
  last_error?: string | null;
  consecutive_errors?: number;
  total_bytes_read?: number;
  total_candidates?: number;
  total_submitted?: number;
};

export type BugMonitorLogWatcherStatusDraft = {
  running?: boolean;
  enabled_projects?: number;
  enabled_sources?: number;
  last_poll_at_ms?: number | null;
  last_error?: string | null;
  sources?: BugMonitorLogSourceRuntimeStatusDraft[];
};

export type BugMonitorProjectIntakeKeyDraft = {
  key_id?: string;
  project_id?: string;
  name?: string;
  key_hash?: string;
  enabled?: boolean;
  scopes?: string[];
  created_at_ms?: number | null;
  last_used_at_ms?: number | null;
};

function formatOptionalTime(value: unknown): string {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) return "never";
  return new Date(numeric).toLocaleString();
}

function formatOptionalBytes(value: unknown): string {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) return "0 B";
  if (numeric < 1024) return `${numeric} B`;
  if (numeric < 1024 * 1024) return `${(numeric / 1024).toFixed(1)} KB`;
  return `${(numeric / (1024 * 1024)).toFixed(1)} MB`;
}

function formatList(values: unknown, fallback = "none"): string {
  if (!Array.isArray(values)) return fallback;
  const cleaned = values.map((value) => String(value || "").trim()).filter(Boolean);
  return cleaned.length ? cleaned.join(", ") : fallback;
}

function sourceKey(projectId: string, sourceId: string): string {
  return `${projectId || "project"}::${sourceId || "source"}`;
}

export function BugMonitorExternalProjectsPanel({
  projects,
  watcher,
  projectsJson,
  projectsJsonError,
  intakeKeys,
  createdRawKey,
  isCreatingKey,
  disablingKeyId,
  resettingSourceKey,
  replayingSourceKey,
  actionResult,
  onProjectsJsonChange,
  onCreateKey,
  onDisableKey,
  onClearCreatedRawKey,
  onResetSourceOffset,
  onReplayLatestSourceCandidate,
}: {
  projects: BugMonitorMonitoredProjectDraft[];
  watcher: BugMonitorLogWatcherStatusDraft;
  projectsJson: string;
  projectsJsonError: string;
  intakeKeys: BugMonitorProjectIntakeKeyDraft[];
  createdRawKey: string;
  isCreatingKey: boolean;
  disablingKeyId: string;
  resettingSourceKey: string;
  replayingSourceKey: string;
  actionResult: Record<string, unknown> | null;
  onProjectsJsonChange: (value: string) => void;
  onCreateKey: (input: { project_id: string; name: string }) => void;
  onDisableKey: (keyId: string) => void;
  onClearCreatedRawKey: () => void;
  onResetSourceOffset: (input: { project_id: string; source_id: string }) => void;
  onReplayLatestSourceCandidate: (input: { project_id: string; source_id: string }) => void;
}) {
  const [newKeyProjectId, setNewKeyProjectId] = useState("");
  const [newKeyName, setNewKeyName] = useState("external reporter");
  const [starterProjectId, setStarterProjectId] = useState("external-demo");
  const [starterRepo, setStarterRepo] = useState("owner/repo");
  const [starterWorkspaceRoot, setStarterWorkspaceRoot] = useState("/path/to/repo");
  const [starterSourceId, setStarterSourceId] = useState("app-log");
  const [starterLogPath, setStarterLogPath] = useState("logs/app.log");
  const [projectFilter, setProjectFilter] = useState("all");
  const sources = Array.isArray(watcher.sources) ? watcher.sources : [];
  const statusBySource = useMemo(
    () =>
      new Map(
        sources.map((source) => [
          sourceKey(String(source.project_id || ""), String(source.source_id || "")),
          source,
        ])
      ),
    [sources]
  );
  const enabledProjectCount = Number(watcher.enabled_projects || 0);
  const enabledSourceCount = Number(watcher.enabled_sources || 0);
  const projectOptions = useMemo(
    () =>
      projects
        .map((project, index) => ({
          id: String(project.project_id || `project-${index + 1}`).trim(),
          label: String(project.name || project.project_id || `project-${index + 1}`).trim(),
        }))
        .filter((project) => project.id),
    [projects]
  );
  const selectedNewKeyProjectId = newKeyProjectId || projectOptions[0]?.id || "";
  const projectHealthRows = useMemo(
    () =>
      projects.map((project, index) => {
        const projectId = String(project.project_id || `project-${index + 1}`);
        const logSources = Array.isArray(project.log_sources) ? project.log_sources : [];
        const runtimeRows = logSources
          .map((source, sourceIndex) => {
            const sourceId = String(source.source_id || `source-${sourceIndex + 1}`);
            return statusBySource.get(sourceKey(projectId, sourceId));
          })
          .filter(Boolean) as BugMonitorLogSourceRuntimeStatusDraft[];
        const unhealthyCount = runtimeRows.filter((row) => row.healthy === false).length;
        const candidateCount = runtimeRows.reduce(
          (total, row) => total + Number(row.total_candidates || 0),
          0
        );
        const submittedCount = runtimeRows.reduce(
          (total, row) => total + Number(row.total_submitted || 0),
          0
        );
        return {
          project,
          projectId,
          logSourceCount: logSources.length,
          observedSourceCount: runtimeRows.length,
          unhealthyCount,
          candidateCount,
          submittedCount,
          lastPollAtMs: runtimeRows.reduce(
            (max, row) => Math.max(max, Number(row.last_poll_at_ms || 0)),
            0
          ),
        };
      }),
    [projects, statusBySource]
  );
  const filteredProjectRows = useMemo(
    () =>
      projectFilter === "all"
        ? projectHealthRows
        : projectHealthRows.filter((row) => row.projectId === projectFilter),
    [projectFilter, projectHealthRows]
  );

  return (
    <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="font-medium">External project log intake</div>
          <div className="tcp-subtle text-xs">
            Configure monitored projects as JSON, then watch source health here after saving.
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Badge tone={watcher.running ? "ok" : enabledSourceCount ? "warn" : "info"}>
            {watcher.running ? "Watcher running" : "Watcher idle"}
          </Badge>
          <Badge tone={enabledProjectCount ? "info" : "warn"}>{enabledProjectCount} projects</Badge>
          <Badge tone={enabledSourceCount ? "info" : "warn"}>{enabledSourceCount} sources</Badge>
        </div>
      </div>

      {projectHealthRows.length ? (
        <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_auto]">
          <div className="grid gap-2 md:grid-cols-3">
            {projectHealthRows.slice(0, 6).map((row) => (
              <button
                key={row.projectId}
                type="button"
                className={`tcp-list-item text-left ${
                  projectFilter === row.projectId ? "ring-1 ring-sky-400/50" : ""
                }`}
                onClick={() => {
                  setProjectFilter((current) =>
                    current === row.projectId ? "all" : row.projectId
                  );
                }}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="truncate font-medium">{row.project.name || row.projectId}</div>
                  <Badge
                    tone={row.unhealthyCount ? "warn" : row.observedSourceCount ? "ok" : "info"}
                  >
                    {row.unhealthyCount ? `${row.unhealthyCount} unhealthy` : "ok"}
                  </Badge>
                </div>
                <div className="tcp-subtle mt-1 text-xs">
                  {row.observedSourceCount}/{row.logSourceCount} observed · {row.candidateCount}{" "}
                  candidates · {row.submittedCount} submitted
                </div>
                <div className="tcp-subtle mt-1 text-xs">
                  Last poll: {formatOptionalTime(row.lastPollAtMs)}
                </div>
              </button>
            ))}
          </div>
          <label className="grid min-w-48 gap-1">
            <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Project filter</span>
            <select
              className="tcp-input"
              value={projectFilter}
              onChange={(event) => setProjectFilter(event.currentTarget.value)}
            >
              <option value="all">All projects</option>
              {projectHealthRows.map((row) => (
                <option key={row.projectId} value={row.projectId}>
                  {row.project.name || row.projectId}
                </option>
              ))}
            </select>
          </label>
        </div>
      ) : null}

      {watcher.last_error ? (
        <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-100">
          {watcher.last_error}
        </div>
      ) : null}

      {actionResult ? (
        <div className="rounded-lg border border-sky-500/30 bg-sky-500/10 p-3">
          <div className="text-sm font-medium text-sky-100">
            {String(actionResult.action || "Log source action")} completed
          </div>
          <div className="tcp-subtle mt-1 text-xs">
            {String(actionResult.project_id || "unknown project")} /{" "}
            {String(actionResult.source_id || "unknown source")}
          </div>
          <pre className="tcp-code mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">
            {JSON.stringify(actionResult, null, 2)}
          </pre>
        </div>
      ) : null}

      <label className="grid gap-2">
        <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Monitored projects</span>
        <div className="grid gap-2 rounded-lg border border-slate-700/60 bg-slate-950/30 p-3 md:grid-cols-5">
          <input
            className="tcp-input text-xs"
            value={starterProjectId}
            onChange={(event) => setStarterProjectId(event.currentTarget.value)}
            placeholder="project_id"
          />
          <input
            className="tcp-input text-xs"
            value={starterRepo}
            onChange={(event) => setStarterRepo(event.currentTarget.value)}
            placeholder="owner/repo"
          />
          <input
            className="tcp-input text-xs"
            value={starterWorkspaceRoot}
            onChange={(event) => setStarterWorkspaceRoot(event.currentTarget.value)}
            placeholder="/path/to/repo"
          />
          <input
            className="tcp-input text-xs"
            value={starterLogPath}
            onChange={(event) => setStarterLogPath(event.currentTarget.value)}
            placeholder="logs/app.log"
          />
          <button
            type="button"
            className="tcp-btn"
            onClick={() => {
              const projectId = starterProjectId.trim() || "external-demo";
              const sourceId = starterSourceId.trim() || "app-log";
              onProjectsJsonChange(
                JSON.stringify(
                  [
                    {
                      project_id: projectId,
                      name: projectId,
                      enabled: true,
                      repo: starterRepo.trim() || "owner/repo",
                      workspace_root: starterWorkspaceRoot.trim() || "/path/to/repo",
                      source_kind: "external_app",
                      allowed_destination_ids: ["legacy-github"],
                      default_destination_ids: ["legacy-github"],
                      default_route_tags: [projectId],
                      tenant_id: "tenant-a",
                      approval_policy: "inherit",
                      log_sources: [
                        {
                          source_id: sourceId,
                          path: starterLogPath.trim() || "logs/app.log",
                          source_kind: "ci",
                          format: "auto",
                          minimum_level: "error",
                          start_position: "end",
                          watch_interval_seconds: 5,
                          default_route_tags: [sourceId],
                          workspace_id: "workspace-a",
                          approval_policy: "inherit",
                        },
                      ],
                    },
                  ],
                  null,
                  2
                )
              );
            }}
          >
            Generate starter
          </button>
        </div>
        <div className="tcp-subtle text-xs">
          Starter source id:{" "}
          <input
            className="tcp-input ml-2 inline-block h-7 w-40 text-xs"
            value={starterSourceId}
            onChange={(event) => setStarterSourceId(event.currentTarget.value)}
            placeholder="source_id"
          />
        </div>
        <textarea
          className="tcp-input min-h-48 font-mono text-xs"
          value={projectsJson}
          onChange={(event) => onProjectsJsonChange(event.currentTarget.value)}
          spellCheck={false}
          placeholder='[{"project_id":"aca","repo":"owner/repo","workspace_root":"/path/to/repo","source_kind":"external_app","default_route_tags":["aca"],"allowed_destination_ids":["legacy-github"],"log_sources":[{"source_id":"ci","source_kind":"ci","path":"logs/ci.jsonl","default_route_tags":["ci"]}]}]'
        />
        <div className={projectsJsonError ? "text-xs text-amber-200" : "tcp-subtle text-xs"}>
          {projectsJsonError ||
            "This PATCH payload is validated by the engine. Paths must stay under each workspace root."}
        </div>
      </label>

      {filteredProjectRows.length ? (
        <div className="grid gap-2">
          {filteredProjectRows.map(({ project, projectId }) => {
            const logSources = Array.isArray(project.log_sources) ? project.log_sources : [];
            return (
              <div key={projectId} className="tcp-list-item">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="font-medium">{project.name || projectId}</div>
                    <div className="tcp-subtle text-xs">
                      {project.repo || "repo not set"} ·{" "}
                      {project.workspace_root || "workspace not set"}
                    </div>
                  </div>
                  <div className="flex flex-wrap gap-2">
                    <Badge tone={project.enabled === false || project.paused ? "warn" : "ok"}>
                      {project.enabled === false
                        ? "Disabled"
                        : project.paused
                          ? "Paused"
                          : "Enabled"}
                    </Badge>
                    <Badge tone="info">{project.source_kind || "customer_system"}</Badge>
                    <Badge tone="info">{logSources.length} log sources</Badge>
                  </div>
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  MCP: {project.mcp_server || "global default"} · Posting:{" "}
                  {project.require_approval_for_new_issues
                    ? "manual approval"
                    : project.auto_create_new_issues === false
                      ? "draft only"
                      : "auto-create enabled"}
                </div>
                <div className="tcp-subtle mt-2 grid gap-1 text-xs md:grid-cols-2">
                  <div>Route tags: {formatList(project.default_route_tags)}</div>
                  <div>
                    Allowed destinations: {formatList(project.allowed_destination_ids, "any")}
                  </div>
                  <div>
                    Default destinations: {formatList(project.default_destination_ids, "global")}
                  </div>
                  <div>
                    Scope: {project.tenant_id || "local"} /{" "}
                    {project.workspace_id || "any workspace"}
                  </div>
                </div>
                {logSources.length ? (
                  <div className="mt-3 grid gap-2">
                    {logSources.map((source, sourceIndex) => {
                      const sourceId = String(source.source_id || `source-${sourceIndex + 1}`);
                      const rowSourceKey = sourceKey(projectId, sourceId);
                      const status = statusBySource.get(sourceKey(projectId, sourceId));
                      return (
                        <div
                          key={`${projectId}-${sourceId}`}
                          className="rounded-lg border border-slate-700/60 bg-slate-950/30 p-3"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <div>
                              <div className="text-sm font-medium">{sourceId}</div>
                              <div className="tcp-subtle text-xs">
                                {source.path || "path not set"}
                              </div>
                            </div>
                            <div className="flex flex-wrap gap-2">
                              <Badge
                                tone={
                                  source.enabled === false || source.paused
                                    ? "warn"
                                    : status?.healthy === false
                                      ? "warn"
                                      : status
                                        ? "ok"
                                        : "info"
                                }
                              >
                                {source.enabled === false
                                  ? "Disabled"
                                  : source.paused
                                    ? "Paused"
                                    : status?.healthy === false
                                      ? "Unhealthy"
                                      : status
                                        ? "Healthy"
                                        : "Waiting"}
                              </Badge>
                              <Badge tone="info">{source.format || "auto"}</Badge>
                              <Badge tone="info">{source.minimum_level || "error"}+</Badge>
                              <Badge tone="info">
                                {source.source_kind || project.source_kind || "source"}
                              </Badge>
                            </div>
                          </div>
                          <div className="tcp-subtle mt-2 grid gap-1 text-xs md:grid-cols-2">
                            <div>Route tags: {formatList(source.default_route_tags)}</div>
                            <div>
                              Allowed destinations:{" "}
                              {formatList(source.allowed_destination_ids, "project")}
                            </div>
                            <div>
                              Default destinations:{" "}
                              {formatList(source.default_destination_ids, "project")}
                            </div>
                            <div>
                              Scope: {source.tenant_id || project.tenant_id || "local"} /{" "}
                              {source.workspace_id || project.workspace_id || "any workspace"}
                            </div>
                          </div>
                          <div className="tcp-subtle mt-2 grid gap-1 text-xs md:grid-cols-2">
                            <div>Offset: {Number(status?.offset || 0).toLocaleString()} bytes</div>
                            <div>File size: {formatOptionalBytes(status?.file_size)}</div>
                            <div>Last poll: {formatOptionalTime(status?.last_poll_at_ms)}</div>
                            <div>
                              Last candidate: {formatOptionalTime(status?.last_candidate_at_ms)}
                            </div>
                            <div>Total candidates: {Number(status?.total_candidates || 0)}</div>
                            <div>Total submitted: {Number(status?.total_submitted || 0)}</div>
                          </div>
                          {status?.last_error ? (
                            <div className="mt-2 rounded border border-amber-500/30 bg-amber-500/10 p-2 text-xs text-amber-100">
                              {status.last_error}
                            </div>
                          ) : null}
                          <div className="mt-3 flex flex-wrap gap-2">
                            <button
                              type="button"
                              className="tcp-btn"
                              disabled={resettingSourceKey === rowSourceKey}
                              onClick={() =>
                                onResetSourceOffset({
                                  project_id: projectId,
                                  source_id: sourceId,
                                })
                              }
                            >
                              Reset offset
                            </button>
                            <button
                              type="button"
                              className="tcp-btn"
                              disabled={replayingSourceKey === rowSourceKey}
                              onClick={() =>
                                onReplayLatestSourceCandidate({
                                  project_id: projectId,
                                  source_id: sourceId,
                                })
                              }
                            >
                              Replay latest
                            </button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="tcp-subtle mt-3 text-xs">No log sources configured.</div>
                )}
              </div>
            );
          })}
        </div>
      ) : projects.length ? (
        <EmptyState text="No projects match the current filter." />
      ) : (
        <EmptyState text="No external projects configured yet. Paste a monitored_projects JSON array above and save." />
      )}

      <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <div className="font-medium">Scoped intake keys</div>
            <div className="tcp-subtle text-xs">
              Create project-limited keys for CI or external reporters. Raw keys are shown once.
            </div>
          </div>
          <Badge tone={intakeKeys.length ? "info" : "warn"}>{intakeKeys.length} keys</Badge>
        </div>

        {createdRawKey ? (
          <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3">
            <div className="text-sm font-medium text-emerald-100">New raw key</div>
            <div className="tcp-subtle mt-1 text-xs">
              Store this now. Tandem only keeps the hash after creation.
            </div>
            <pre className="tcp-code mt-2 overflow-auto whitespace-pre-wrap break-all text-xs">
              {createdRawKey}
            </pre>
            <div className="mt-2 flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn"
                onClick={() => navigator.clipboard?.writeText(createdRawKey)}
              >
                Copy
              </button>
              <button type="button" className="tcp-btn" onClick={onClearCreatedRawKey}>
                Hide
              </button>
            </div>
          </div>
        ) : null}

        <div className="grid gap-2 md:grid-cols-[1fr_1fr_auto]">
          <label className="grid gap-1">
            <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Project</span>
            <select
              className="tcp-input"
              value={selectedNewKeyProjectId}
              onChange={(event) => setNewKeyProjectId(event.currentTarget.value)}
              disabled={!projectOptions.length}
            >
              {projectOptions.length ? (
                projectOptions.map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.label}
                  </option>
                ))
              ) : (
                <option value="">Configure a project first</option>
              )}
            </select>
          </label>
          <label className="grid gap-1">
            <span className="text-xs uppercase tracking-[0.24em] tcp-subtle">Key name</span>
            <input
              className="tcp-input"
              value={newKeyName}
              onChange={(event) => setNewKeyName(event.currentTarget.value)}
              placeholder="ci reporter"
            />
          </label>
          <div className="flex items-end">
            <button
              type="button"
              className="tcp-btn-primary w-full"
              disabled={!selectedNewKeyProjectId || !newKeyName.trim() || isCreatingKey}
              onClick={() =>
                onCreateKey({
                  project_id: selectedNewKeyProjectId,
                  name: newKeyName.trim(),
                })
              }
            >
              Create key
            </button>
          </div>
        </div>

        {intakeKeys.length ? (
          <div className="grid gap-2">
            {intakeKeys.map((key) => {
              const keyId = String(key.key_id || "");
              return (
                <div key={keyId || `${key.project_id}-${key.name}`} className="tcp-list-item">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="font-medium">{key.name || keyId || "intake key"}</div>
                      <div className="tcp-subtle text-xs">
                        {key.project_id || "unknown project"} ·{" "}
                        {(key.scopes || ["bug_monitor:report"]).join(", ")}
                      </div>
                    </div>
                    <div className="flex flex-wrap items-center gap-2">
                      <Badge tone={key.enabled === false ? "warn" : "ok"}>
                        {key.enabled === false ? "Disabled" : "Enabled"}
                      </Badge>
                      {key.enabled !== false && keyId ? (
                        <button
                          type="button"
                          className="tcp-btn-danger"
                          disabled={disablingKeyId === keyId}
                          onClick={() => onDisableKey(keyId)}
                        >
                          Disable
                        </button>
                      ) : null}
                    </div>
                  </div>
                  <div className="tcp-subtle mt-2 grid gap-1 text-xs md:grid-cols-2">
                    <div>Created: {formatOptionalTime(key.created_at_ms)}</div>
                    <div>Last used: {formatOptionalTime(key.last_used_at_ms)}</div>
                    <div className="md:col-span-2">Hash: {key.key_hash || "[redacted]"}</div>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <EmptyState text="No scoped intake keys yet." />
        )}
      </div>
    </div>
  );
}
