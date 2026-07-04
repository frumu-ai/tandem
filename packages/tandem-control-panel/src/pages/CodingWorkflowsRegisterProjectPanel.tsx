import { Badge, PanelCard } from "../ui/index.tsx";
import type { GithubRepoRef, TaskSourceType } from "./CodingWorkflowsHelpers";
import { Icon } from "../ui/Icon";

type LinearCatalog = {
  ok?: boolean;
  auth_required?: boolean;
  auth_status?: string;
  authorization_url?: string;
  connected?: boolean;
  message?: string;
  teams?: Array<Record<string, any>>;
  projects?: Array<Record<string, any>>;
};

type LinearProject = Record<string, any>;

function textValue(value: unknown): string {
  return String(value || "").trim();
}

function linearProjectIcon(project: LinearProject): string {
  const icon = textValue(project?.icon).toLowerCase();
  const name = textValue(project?.name).toLowerCase();
  if (icon === "server" || name.includes("hosted")) return "server";
  if (icon.includes("shield") || name.includes("security") || name.includes("boundary")) return "shield";
  if (icon.includes("database") || name.includes("data")) return "database";
  if (icon.includes("workflow") || name.includes("workflow")) return "workflow";
  if (icon.includes("activity") || name.includes("monitor")) return "activity";
  if (icon.includes("rocket") || name.includes("launch")) return "rocket";
  if (icon.includes("bug") || name.includes("bug")) return "bug";
  if (icon.includes("code") || name.includes("runtime") || name.includes("engine")) return "code";
  return "folder-code";
}

function linearProjectColor(project: LinearProject): string {
  const color = textValue(project?.color);
  return /^#[0-9a-f]{3,8}$/i.test(color) ? color : "#38bdf8";
}

function linearProjectProgress(project: LinearProject): number | null {
  const raw =
    project?.completion_percent ??
    project?.completionPercent ??
    project?.progress_percent ??
    project?.progressPercent ??
    project?.completion ??
    project?.progress;
  const value = typeof raw === "number" ? raw : Number(raw);
  if (!Number.isFinite(value)) return null;
  return Math.max(0, Math.min(100, value <= 1 ? value * 100 : value));
}

function shortDate(value: unknown): string {
  const text = textValue(value);
  if (!text) return "";
  const dateOnly = text.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (dateOnly) {
    const [, year, month, day] = dateOnly;
    return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(
      new Date(Number(year), Number(month) - 1, Number(day))
    );
  }
  const date = new Date(text);
  if (Number.isNaN(date.getTime())) return text;
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(date);
}

function statusTone(statusType: unknown): string {
  const key = textValue(statusType).toLowerCase();
  if (key === "completed") return "border-emerald-400/30 bg-emerald-500/10 text-emerald-100";
  if (key === "started") return "border-sky-400/30 bg-sky-500/10 text-sky-100";
  if (key === "canceled" || key === "cancelled") return "border-slate-500/30 bg-slate-500/10 text-slate-300";
  if (key === "backlog" || key === "unstarted") return "border-amber-400/30 bg-amber-500/10 text-amber-100";
  return "border-white/10 bg-white/5 text-slate-200";
}

type Props = {
  hostedManaged: boolean;
  linearCatalog?: LinearCatalog | null;
  linearCatalogError?: string;
  linearCatalogLoading?: boolean;
  newCredentialFile: string;
  newDefaultBranch: string;
  newProjectName: string;
  newProjectSlug: string;
  newRemoteName: string;
  newRepoPath: string;
  newRepoRef: GithubRepoRef | null;
  newRepoUrl: string;
  newWorktreeRoot: string;
  registering: boolean;
  registerProject: () => void;
  refreshLinearCatalog?: () => void;
  setNewCredentialFile: (value: string) => void;
  setNewDefaultBranch: (value: string) => void;
  setNewProjectName: (value: string) => void;
  setNewProjectSlug: (value: string) => void;
  setNewRemoteName: (value: string) => void;
  setNewRepoPath: (value: string) => void;
  setNewRepoUrl: (value: string) => void;
  setNewWorktreeRoot: (value: string) => void;
  setTaskSourceLinearLabels: (value: string) => void;
  setTaskSourceLinearProject: (value: string) => void;
  setTaskSourceLinearQuery: (value: string) => void;
  setTaskSourceLinearStatuses: (value: string) => void;
  setTaskSourceLinearTeam: (value: string) => void;
  setTaskSourcePath: (value: string) => void;
  setTaskSourceProject: (value: string) => void;
  setTaskSourcePrompt: (value: string) => void;
  setTaskSourceType: (value: TaskSourceType) => void;
  taskSourceLinearLabels: string;
  taskSourceLinearProject: string;
  taskSourceLinearQuery: string;
  taskSourceLinearStatuses: string;
  taskSourceLinearTeam: string;
  taskSourcePath: string;
  taskSourceProject: string;
  taskSourcePrompt: string;
  taskSourceType: TaskSourceType;
};

export function CodingWorkflowsRegisterProjectPanel({
  hostedManaged,
  linearCatalog,
  linearCatalogError,
  linearCatalogLoading,
  newCredentialFile,
  newDefaultBranch,
  newProjectName,
  newProjectSlug,
  newRemoteName,
  newRepoPath,
  newRepoRef,
  newRepoUrl,
  newWorktreeRoot,
  registering,
  registerProject,
  refreshLinearCatalog,
  setNewCredentialFile,
  setNewDefaultBranch,
  setNewProjectName,
  setNewProjectSlug,
  setNewRemoteName,
  setNewRepoPath,
  setNewRepoUrl,
  setNewWorktreeRoot,
  setTaskSourceLinearLabels,
  setTaskSourceLinearProject,
  setTaskSourceLinearQuery,
  setTaskSourceLinearStatuses,
  setTaskSourceLinearTeam,
  setTaskSourcePath,
  setTaskSourceProject,
  setTaskSourcePrompt,
  setTaskSourceType,
  taskSourceLinearLabels,
  taskSourceLinearProject,
  taskSourceLinearQuery,
  taskSourceLinearStatuses,
  taskSourceLinearTeam,
  taskSourcePath,
  taskSourceProject,
  taskSourcePrompt,
  taskSourceType,
}: Props) {
  const linearTeams = Array.isArray(linearCatalog?.teams) ? linearCatalog.teams : [];
  const linearProjects = Array.isArray(linearCatalog?.projects) ? linearCatalog.projects : [];
  const linearAuthRequired = !!linearCatalog?.auth_required && linearCatalog?.connected !== true;
  const linearMessage = String(linearCatalog?.message || "").trim();
  const linearCatalogPartial =
    linearCatalog?.ok === false && (linearTeams.length > 0 || linearProjects.length > 0);
  const linearCatalogUnavailable =
    !!linearCatalogError || (linearCatalog?.ok === false && !linearCatalogPartial);
  const linearCatalogNotice = linearCatalogError || (!linearAuthRequired ? linearMessage : "");
  const filteredLinearProjects = linearProjects.filter((project) => {
    const selectedTeam = String(taskSourceLinearTeam || "").trim();
    const teamValues = [project?.team_key, project?.team_id, project?.team_name]
      .map((value) => String(value || "").trim())
      .filter(Boolean);
    return !selectedTeam || !teamValues.length || teamValues.includes(selectedTeam);
  });
  const selectLinearProject = (project: LinearProject) => {
    const value = String(project?.id || project?.name || "").trim();
    setTaskSourceLinearProject(value);
    if (project && !newProjectName.trim()) {
      setNewProjectName(String(project?.name || value));
    }
    if (project && !newProjectSlug.trim()) {
      const teamSeed = String(
        project?.team_key || project?.team_id || project?.team_name || taskSourceLinearTeam || "linear"
      ).toLowerCase();
      const projectSeed = String(project?.name || value)
        .toLowerCase()
        .replace(/[^a-z0-9._/-]+/g, "-")
        .replace(/^-+|-+$/g, "");
      setNewProjectSlug(`${teamSeed}-${projectSeed}`);
    }
  };
  return (
    <PanelCard
      title="Register project"
      subtitle="Bind a repository, managed checkout, and task source into ACA"
    >
      <div className="grid gap-3">
        {taskSourceType === "github_project" ? (
          <>
            <input
              className="tcp-input"
              placeholder="GitHub repo URL, e.g. https://github.com/frumu-ai/tandem"
              value={newRepoUrl}
              onInput={(event) => setNewRepoUrl((event.target as HTMLInputElement).value)}
            />
            <div className="rounded-2xl border border-cyan-500/20 bg-cyan-500/10 px-3 py-2 text-xs text-cyan-100">
              {newRepoRef
                ? `Detected ${newRepoRef.owner}/${newRepoRef.repo}. ACA will use this for the GitHub Project owner/repo binding.`
                : "Paste a GitHub repository URL and ACA will derive the owner, repo, and default project slug."}
            </div>
          </>
        ) : (
          <input
            className="tcp-input"
            placeholder="Repo URL (optional)"
            value={newRepoUrl}
            onInput={(event) => setNewRepoUrl((event.target as HTMLInputElement).value)}
          />
        )}
        {hostedManaged ? (
          <>
            <input
              className="tcp-input"
              placeholder="Managed checkout path, e.g. repos/team-alpha"
              value={newRepoPath}
              onInput={(event) => setNewRepoPath((event.target as HTMLInputElement).value)}
            />
            <input
              className="tcp-input"
              placeholder="Worktree root (optional)"
              value={newWorktreeRoot}
              onInput={(event) => setNewWorktreeRoot((event.target as HTMLInputElement).value)}
            />
            <div className="grid gap-3 md:grid-cols-2">
              <input
                className="tcp-input"
                placeholder="Default branch (optional)"
                value={newDefaultBranch}
                onInput={(event) => setNewDefaultBranch((event.target as HTMLInputElement).value)}
              />
              <input
                className="tcp-input"
                placeholder="Remote name (optional)"
                value={newRemoteName}
                onInput={(event) => setNewRemoteName((event.target as HTMLInputElement).value)}
              />
            </div>
            <input
              className="tcp-input"
              placeholder="Token file for private repos (optional)"
              value={newCredentialFile}
              onInput={(event) => setNewCredentialFile((event.target as HTMLInputElement).value)}
            />
            <div className="rounded-2xl border border-lime-500/20 bg-lime-500/10 px-3 py-2 text-xs text-lime-100">
              Hosted installs can use these fields to register named repos and managed checkout
              directories without exposing an interactive shell.
            </div>
          </>
        ) : null}
        <input
          className="tcp-input"
          placeholder={
            taskSourceType === "github_project"
              ? "Project slug (optional, defaults to owner/repo)"
              : taskSourceType === "linear"
                ? "Project slug (optional, defaults to linear-team-project)"
                : "Project slug"
          }
          value={newProjectSlug}
          onInput={(event) => setNewProjectSlug((event.target as HTMLInputElement).value)}
        />
        <input
          className="tcp-input"
          placeholder="Project display name (optional)"
          value={newProjectName}
          onInput={(event) => setNewProjectName((event.target as HTMLInputElement).value)}
        />
        <select
          className="tcp-input"
          value={taskSourceType}
          onChange={(event) =>
            setTaskSourceType((event.target as HTMLSelectElement).value as TaskSourceType)
          }
        >
          <option value="manual">Manual prompt</option>
          <option value="kanban_board">Kanban board</option>
          <option value="local_backlog">Local backlog</option>
          <option value="github_project">GitHub Project</option>
          <option value="linear">Linear team/project</option>
        </select>
        {taskSourceType === "manual" ? (
          <textarea
            className="tcp-input min-h-[120px]"
            placeholder="Manual task prompt"
            value={taskSourcePrompt}
            onInput={(event) => setTaskSourcePrompt((event.target as HTMLTextAreaElement).value)}
          />
        ) : null}
        {taskSourceType === "kanban_board" || taskSourceType === "local_backlog" ? (
          <input
            className="tcp-input"
            placeholder="Absolute file path"
            value={taskSourcePath}
            onInput={(event) => setTaskSourcePath((event.target as HTMLInputElement).value)}
          />
        ) : null}
        {taskSourceType === "github_project" ? (
          <>
            <input
              className="tcp-input"
              placeholder="GitHub Project number"
              value={taskSourceProject}
              onInput={(event) => setTaskSourceProject((event.target as HTMLInputElement).value)}
            />
            <div className="tcp-subtle text-xs">
              Only GitHub Project board tasks are imported. Public issues that are not on this
              project board remain outside ACA intake.
            </div>
          </>
        ) : null}
        {taskSourceType === "linear" ? (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-cyan-500/20 bg-cyan-500/10 px-3 py-2 text-xs text-cyan-100">
              <div className="flex flex-wrap items-center gap-2">
                <Badge
                  tone={
                    linearCatalogUnavailable || linearCatalogPartial
                      ? "warn"
                      : linearProjects.length
                        ? "ok"
                        : "info"
                  }
                >
                  {linearCatalogUnavailable
                    ? "Catalog unavailable"
                    : linearCatalogPartial
                      ? "Partial catalog"
                    : linearAuthRequired
                      ? "Connect Linear"
                    : linearCatalogLoading
                      ? "Loading Linear"
                      : `${linearProjects.length} projects`}
                </Badge>
                <span>
                  {linearAuthRequired
                    ? "Linear MCP needs browser authorization before ACA can list projects."
                    : "Use the connected Tandem Linear MCP catalog for exact team/project values."}
                </span>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {linearAuthRequired ? (
                  <a className="tcp-btn h-7 px-2.5 tcp-text-caption" href="#/settings?section=mcp">
                    <Icon name="plug-zap" />
                    Open MCP
                  </a>
                ) : null}
                <button
                  type="button"
                  className="tcp-btn h-7 px-2.5 tcp-text-caption"
                  onClick={() => refreshLinearCatalog?.()}
                  disabled={linearCatalogLoading}
                >
                  <Icon name="refresh-cw" />
                  Refresh Linear
                </button>
              </div>
            </div>
            {linearAuthRequired && linearMessage ? (
              <div className="rounded-xl border border-yellow-500/20 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-100">
                {linearMessage}
              </div>
            ) : null}
            {linearCatalogNotice ? (
              <div className="rounded-xl border border-yellow-500/20 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-100">
                {linearCatalogNotice}
              </div>
            ) : null}
            {linearTeams.length ? (
              <select
                className="tcp-input"
                value={taskSourceLinearTeam}
                onChange={(event) => {
                  const value = (event.target as HTMLSelectElement).value;
                  setTaskSourceLinearTeam(value);
                  setTaskSourceLinearProject("");
                }}
              >
                <option value="">Select Linear team</option>
                {linearTeams.map((team) => {
                  const value = String(team?.key || team?.id || team?.name || "").trim();
                  return (
                    <option key={String(team?.id || value)} value={value}>
                      {String(team?.display || team?.name || value)}
                    </option>
                  );
                })}
              </select>
            ) : (
              <input
                className="tcp-input"
                placeholder="Linear team key or id, e.g. TAN"
                value={taskSourceLinearTeam}
                onInput={(event) =>
                  setTaskSourceLinearTeam((event.target as HTMLInputElement).value)
                }
              />
            )}
            {linearProjects.length ? (
              <div
                className="max-h-[min(32rem,58dvh)] overflow-y-auto rounded-xl border border-white/10 bg-black/20"
                role="radiogroup"
                aria-label="Linear project"
              >
                {filteredLinearProjects.map((project) => {
                  const value = String(project?.id || project?.name || "").trim();
                  const selected = value === taskSourceLinearProject;
                  const color = linearProjectColor(project);
                  const progress = linearProjectProgress(project);
                  const issueCount =
                    project?.issue_count === null || project?.issue_count === undefined
                      ? ""
                      : `${project.issue_count} issue${Number(project.issue_count) === 1 ? "" : "s"}`;
                  const targetDate = shortDate(project?.target_date || project?.targetDate);
                  const summary = textValue(project?.summary);
                  const priority = textValue(project?.priority_name || project?.priority?.name);
                  const lead = textValue(project?.lead_name || project?.lead?.name);
                  const status = textValue(project?.status_name || project?.status?.name);
                  const statusType = textValue(project?.status_type || project?.status?.type);
                  return (
                    <button
                      key={String(project?.id || value)}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      className={`grid min-h-[72px] w-full grid-cols-[auto_minmax(0,1fr)_auto] gap-3 border-b border-white/10 px-3 py-3 text-left transition last:border-b-0 sm:px-4 ${
                        selected
                          ? "bg-sky-500/12 text-white"
                          : "bg-transparent text-slate-100 hover:bg-white/[0.04]"
                      }`}
                      onClick={() => selectLinearProject(project)}
                    >
                      <span
                        className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border"
                        style={{ borderColor: color, backgroundColor: `${color}1f`, color }}
                      >
                        <Icon name={linearProjectIcon(project)} className="h-4 w-4" />
                      </span>
                      <span className="min-w-0">
                        <span className="block break-words text-sm font-semibold leading-5">
                          {String(project?.name || value)}
                        </span>
                        {summary ? (
                          <span className="tcp-subtle mt-1 line-clamp-2 break-words text-xs leading-5">
                            {summary}
                          </span>
                        ) : null}
                        <span className="mt-2 flex flex-wrap gap-1.5">
                          {status ? (
                            <span
                              className={`rounded-full border px-2 py-0.5 tcp-text-caption ${statusTone(statusType)}`}
                            >
                              {status}
                            </span>
                          ) : null}
                          {priority ? (
                            <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 tcp-text-caption text-slate-200">
                              {priority}
                            </span>
                          ) : null}
                          {targetDate ? (
                            <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 tcp-text-caption text-slate-200">
                              {targetDate}
                            </span>
                          ) : null}
                          {lead ? (
                            <span className="max-w-full truncate rounded-full border border-white/10 bg-white/5 px-2 py-0.5 tcp-text-caption text-slate-200">
                              {lead}
                            </span>
                          ) : null}
                          {issueCount ? (
                            <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 tcp-text-caption text-slate-200">
                              {issueCount}
                            </span>
                          ) : null}
                        </span>
                        {progress !== null ? (
                          <span className="mt-2 grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2">
                            <span className="h-1.5 overflow-hidden rounded-full bg-white/10">
                              <span
                                className="block h-full rounded-full"
                                style={{ width: `${progress}%`, backgroundColor: color }}
                              />
                            </span>
                            <span className="tcp-text-caption text-slate-300">{Math.round(progress)}%</span>
                          </span>
                        ) : null}
                      </span>
                      <span
                        className={`mt-1 flex h-5 w-5 shrink-0 items-center justify-center rounded-full border ${
                          selected
                            ? "border-sky-300 bg-sky-400 text-slate-950"
                            : "border-slate-500 text-transparent"
                        }`}
                        aria-hidden="true"
                      >
                        <Icon name="check" className="h-3.5 w-3.5" />
                      </span>
                    </button>
                  );
                })}
              </div>
            ) : (
              <input
                className="tcp-input"
                placeholder="Linear project name, id, or slug (optional)"
                value={taskSourceLinearProject}
                onInput={(event) =>
                  setTaskSourceLinearProject((event.target as HTMLInputElement).value)
                }
              />
            )}
            <div className="grid gap-3 md:grid-cols-2">
              <input
                className="tcp-input"
                placeholder="Launch statuses, comma-separated"
                value={taskSourceLinearStatuses}
                onInput={(event) =>
                  setTaskSourceLinearStatuses((event.target as HTMLInputElement).value)
                }
              />
              <input
                className="tcp-input"
                placeholder="Required labels, comma-separated (optional)"
                value={taskSourceLinearLabels}
                onInput={(event) =>
                  setTaskSourceLinearLabels((event.target as HTMLInputElement).value)
                }
              />
            </div>
            <input
              className="tcp-input"
              placeholder="Linear search query (optional)"
              value={taskSourceLinearQuery}
              onInput={(event) =>
                setTaskSourceLinearQuery((event.target as HTMLInputElement).value)
              }
            />
            <div className="tcp-subtle text-xs">
              Connect Linear in the Integrations tab first. ACA will use the `linear` MCP server
              from Tandem and sync status, labels, and a run summary comment.
            </div>
          </>
        ) : null}
        <button
          type="button"
          className="tcp-btn"
          disabled={registering}
          onClick={registerProject}
        >
          {registering ? "Registering..." : "Register Project"}
        </button>
      </div>
    </PanelCard>
  );
}
