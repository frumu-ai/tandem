import { Badge, PanelCard } from "../ui/index.tsx";
import type { GithubRepoRef, TaskSourceType } from "./CodingWorkflowsHelpers";

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
                  <a className="tcp-btn h-7 px-2.5 text-[11px]" href="#/settings?section=mcp">
                    <i data-lucide="plug-zap"></i>
                    Open MCP
                  </a>
                ) : null}
                <button
                  type="button"
                  className="tcp-btn h-7 px-2.5 text-[11px]"
                  onClick={() => refreshLinearCatalog?.()}
                  disabled={linearCatalogLoading}
                >
                  <i data-lucide="refresh-cw"></i>
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
              <select
                className="tcp-input"
                value={taskSourceLinearProject}
                onChange={(event) => {
                  const value = (event.target as HTMLSelectElement).value;
                  const selected = linearProjects.find(
                    (project) => String(project?.id || project?.name || "") === value
                  );
                  setTaskSourceLinearProject(value);
                  if (selected && !newProjectName.trim()) {
                    setNewProjectName(String(selected?.name || value));
                  }
                  if (selected && !newProjectSlug.trim()) {
                    const teamSeed = String(
                      selected?.team_key || selected?.team_id || selected?.team_name || taskSourceLinearTeam || "linear"
                    ).toLowerCase();
                    const projectSeed = String(selected?.name || value)
                      .toLowerCase()
                      .replace(/[^a-z0-9._/-]+/g, "-")
                      .replace(/^-+|-+$/g, "");
                    setNewProjectSlug(`${teamSeed}-${projectSeed}`);
                  }
                }}
              >
                <option value="">Select Linear project</option>
                {linearProjects
                  .filter((project) => {
                    const selectedTeam = String(taskSourceLinearTeam || "").trim();
                    const teamValues = [
                      project?.team_key,
                      project?.team_id,
                      project?.team_name,
                    ]
                      .map((value) => String(value || "").trim())
                      .filter(Boolean);
                    return !selectedTeam || !teamValues.length || teamValues.includes(selectedTeam);
                  })
                  .map((project) => {
                    const value = String(project?.id || project?.name || "").trim();
                    const count =
                      project?.issue_count === null || project?.issue_count === undefined
                        ? ""
                        : ` · ${project.issue_count} issue${Number(project.issue_count) === 1 ? "" : "s"}`;
                    return (
                      <option key={String(project?.id || value)} value={value}>
                        {String(project?.name || value)}
                        {count}
                      </option>
                    );
                  })}
              </select>
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
