import { useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ExternalLink,
  Github,
  PlugZap,
  RefreshCw,
  Settings,
} from "lucide-react";
import {
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Input,
} from "@/components/ui";
import type { CoderGithubProjectInboxItem, CoderProjectBindingRecord } from "@/lib/tauri";

type CoderGithubProjectPanelProps = {
  activeProjectId: string | undefined;
  activeProjectName: string | undefined;
  projectBinding: CoderProjectBindingRecord | null;
  projectBindingLoading: boolean;
  projectBindingError: string | null;
  githubProjectOwnerInput: string;
  setGithubProjectOwnerInput: (value: string) => void;
  githubProjectNumberInput: string;
  setGithubProjectNumberInput: (value: string) => void;
  githubProjectInbox: CoderGithubProjectInboxItem[];
  githubProjectInboxLoading: boolean;
  githubProjectInboxError: string | null;
  githubProjectSchemaDrift: boolean;
  githubProjectLiveSchemaFingerprint: string;
  githubProjectBusyKey: string | null;
  githubProjectReadReady: boolean;
  githubProjectWriteReady: boolean;
  githubProjectServerConnected: boolean;
  onSaveBinding: () => void;
  onRefreshInbox: () => void;
  onIntakeItem: (item: CoderGithubProjectInboxItem) => void;
  onOpenLinkedRun: (runId: string) => void;
  onOpenMcpExtensions?: () => void;
};

function syncStateLabel(value: string | null | undefined): string {
  if (!value) return "Unknown";
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export function CoderGithubProjectPanel(props: CoderGithubProjectPanelProps) {
  const {
    activeProjectId,
    activeProjectName,
    projectBinding,
    projectBindingLoading,
    projectBindingError,
    githubProjectOwnerInput,
    setGithubProjectOwnerInput,
    githubProjectNumberInput,
    setGithubProjectNumberInput,
    githubProjectInbox,
    githubProjectInboxLoading,
    githubProjectInboxError,
    githubProjectSchemaDrift,
    githubProjectLiveSchemaFingerprint,
    githubProjectBusyKey,
    githubProjectReadReady,
    githubProjectWriteReady,
    githubProjectServerConnected,
    onSaveBinding,
    onRefreshInbox,
    onIntakeItem,
    onOpenLinkedRun,
    onOpenMcpExtensions,
  } = props;

  const githubProjectBinding = projectBinding?.github_project_binding || null;
  const githubProjectStatusMapping = githubProjectBinding?.status_mapping || null;
  const [configExpanded, setConfigExpanded] = useState(!githubProjectBinding);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const actionableCount = githubProjectInbox.filter((item) => item.actionable).length;

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <CardTitle className="flex items-center gap-2 text-base">
              <Github className="h-4 w-4" aria-hidden /> GitHub Project Intake
            </CardTitle>
            <CardDescription>
              Pull issue-backed TODO items from a GitHub Project into Coder and project status
              changes back out.
            </CardDescription>
          </div>
          {githubProjectBinding ? (
            <span className="inline-flex items-center gap-1.5 rounded-full border border-emerald-500/40 bg-emerald-500/10 px-2.5 py-1 text-xs text-emerald-200">
              <CheckCircle2 className="h-3 w-3" aria-hidden />
              Connected · {githubProjectBinding.owner} #{githubProjectBinding.project_number}
            </span>
          ) : (
            <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-surface px-2.5 py-1 text-xs text-text-muted">
              <PlugZap className="h-3 w-3" aria-hidden />
              Not connected
            </span>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {projectBindingError ? (
          <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
            {projectBindingError}
          </div>
        ) : null}
        {githubProjectInboxError ? (
          <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
            {githubProjectInboxError}
          </div>
        ) : null}

        {!githubProjectReadReady ? (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-100">
            <span>
              GitHub Project tools aren't ready yet — connect the GitHub MCP server to read or list
              project items.
            </span>
            {onOpenMcpExtensions ? (
              <Button size="sm" variant="secondary" onClick={onOpenMcpExtensions}>
                Open MCP Extensions
              </Button>
            ) : null}
          </div>
        ) : null}

        {/* Connected: compact header + refresh; expandable to change */}
        {githubProjectBinding && !configExpanded ? (
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-border bg-surface-elevated/30 px-3 py-2 text-xs text-text-muted">
            <div className="flex items-center gap-2">
              <span className="text-text-subtle">Bound to</span>
              <span className="font-medium text-text">
                {githubProjectBinding.owner} #{githubProjectBinding.project_number}
              </span>
              {githubProjectSchemaDrift ? (
                <span className="rounded-full border border-amber-400/40 bg-amber-400/10 px-2 py-0.5 text-[10px] text-amber-200">
                  Schema drift
                </span>
              ) : null}
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={onRefreshInbox}
                loading={githubProjectInboxLoading}
              >
                <RefreshCw className="mr-1 h-3 w-3" aria-hidden />
                Refresh
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setConfigExpanded(true)}>
                <Settings className="mr-1 h-3 w-3" aria-hidden />
                Change
              </Button>
            </div>
          </div>
        ) : (
          <div className="space-y-3 rounded-xl border border-border bg-surface-elevated/20 p-3">
            <div className="grid gap-3 lg:grid-cols-[minmax(0,1fr)_160px_auto]">
              <div className="space-y-1.5">
                <label className="text-[11px] uppercase tracking-wide text-text-subtle">
                  Owner or org
                </label>
                <Input
                  value={githubProjectOwnerInput}
                  onChange={(event) => setGithubProjectOwnerInput(event.target.value)}
                  placeholder="acme-inc"
                  disabled={!activeProjectId || projectBindingLoading}
                />
              </div>
              <div className="space-y-1.5">
                <label className="text-[11px] uppercase tracking-wide text-text-subtle">
                  Project #
                </label>
                <Input
                  value={githubProjectNumberInput}
                  onChange={(event) => setGithubProjectNumberInput(event.target.value)}
                  placeholder="12"
                  inputMode="numeric"
                  disabled={!activeProjectId || projectBindingLoading}
                />
              </div>
              <div className="flex flex-wrap items-end gap-2">
                <Button
                  size="sm"
                  onClick={onSaveBinding}
                  loading={githubProjectBusyKey === "save-binding"}
                  disabled={!activeProjectId || !githubProjectReadReady}
                >
                  {githubProjectBinding ? "Update binding" : "Connect project"}
                </Button>
                {githubProjectBinding ? (
                  <Button size="sm" variant="ghost" onClick={() => setConfigExpanded(false)}>
                    Cancel
                  </Button>
                ) : null}
              </div>
            </div>
            {!activeProjectId ? (
              <div className="text-[11px] text-text-subtle">
                Pick an active project above before connecting a GitHub Project.
              </div>
            ) : !githubProjectWriteReady && githubProjectReadReady ? (
              <div className="text-[11px] text-text-subtle">
                Read-only access detected — outgoing status projection requires the project-update
                tool.
              </div>
            ) : (
              <div className="text-[11px] text-text-subtle">
                For <span className="text-text">{activeProjectName || "this project"}</span>.
                {githubProjectServerConnected ? " MCP transport ready." : ""}
              </div>
            )}
          </div>
        )}

        {/* Advanced disclosure for status mapping + fingerprints */}
        {githubProjectBinding ? (
          <div className="rounded-xl border border-border bg-surface-elevated/10">
            <button
              type="button"
              onClick={() => setAdvancedOpen((prev) => !prev)}
              className="flex w-full items-center justify-between gap-2 px-3 py-2 text-xs text-text-muted transition-colors hover:text-text"
            >
              <span>Advanced · status mapping & schema</span>
              <ChevronDown
                className={`h-3.5 w-3.5 transition-transform ${advancedOpen ? "rotate-180" : ""}`}
                aria-hidden
              />
            </button>
            {advancedOpen ? (
              <div className="space-y-3 border-t border-border px-3 py-3 text-xs text-text-muted">
                {githubProjectStatusMapping ? (
                  <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-5">
                    {[
                      ["TODO", githubProjectStatusMapping.todo.name],
                      ["In Progress", githubProjectStatusMapping.in_progress.name],
                      ["In Review", githubProjectStatusMapping.in_review.name],
                      ["Blocked", githubProjectStatusMapping.blocked.name],
                      ["Done", githubProjectStatusMapping.done.name],
                    ].map(([label, value]) => (
                      <div
                        key={label}
                        className="rounded-md border border-border bg-surface px-2 py-1.5"
                      >
                        <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                          {label}
                        </div>
                        <div className="mt-0.5 truncate text-xs text-text">{value || "—"}</div>
                      </div>
                    ))}
                  </div>
                ) : null}
                <div className="grid gap-2 md:grid-cols-2">
                  <div>
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Saved fingerprint
                    </div>
                    <div className="mt-0.5 truncate font-mono text-[11px] text-text">
                      {githubProjectBinding.schema_fingerprint || "—"}
                    </div>
                  </div>
                  <div>
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Live fingerprint
                    </div>
                    <div className="mt-0.5 truncate font-mono text-[11px] text-text">
                      {githubProjectLiveSchemaFingerprint || "—"}
                    </div>
                  </div>
                </div>
                {githubProjectSchemaDrift ? (
                  <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-100">
                    Live schema drifted from saved binding — update the binding to refresh.
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        ) : null}

        {/* Inbox */}
        {!githubProjectBinding ? null : githubProjectInboxLoading ? (
          <div className="rounded-lg border border-border bg-surface px-4 py-6 text-center text-sm text-text-muted">
            Loading GitHub Project inbox…
          </div>
        ) : githubProjectInbox.length === 0 ? (
          <div className="rounded-lg border border-dashed border-border bg-surface-elevated/10 px-4 py-6 text-center text-sm text-text-muted">
            Nothing in the inbox right now.
          </div>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center justify-between text-xs text-text-muted">
              <span>
                Inbox · <span className="font-medium text-text">{githubProjectInbox.length}</span>{" "}
                items
                {actionableCount > 0 ? (
                  <span className="text-text-subtle"> · {actionableCount} actionable</span>
                ) : null}
              </span>
            </div>
            <div className="space-y-2">
              {githubProjectInbox.map((item) => {
                const linkedRunId = item.linked_run?.coder_run?.coder_run_id || "";
                const canIntake = item.actionable && (!item.linked_run || !item.linked_run.active);
                return (
                  <div
                    key={item.project_item_id}
                    className="rounded-xl border border-border bg-surface-elevated/20 p-3"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-medium text-text">{item.title}</div>
                        <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-text-subtle">
                          {item.issue ? (
                            <a
                              href={item.issue.html_url || "#"}
                              target="_blank"
                              rel="noreferrer"
                              className="inline-flex items-center gap-1 text-text-muted hover:text-text"
                            >
                              <ExternalLink className="h-3 w-3" aria-hidden />#{item.issue.number}
                            </a>
                          ) : (
                            <span>Unsupported item</span>
                          )}
                          <span aria-hidden>·</span>
                          <span>{item.status_name}</span>
                          <span aria-hidden>·</span>
                          <span className="text-text-subtle">
                            {syncStateLabel(item.remote_sync_state)}
                          </span>
                        </div>
                      </div>
                      <div className="flex flex-wrap items-center gap-2">
                        {canIntake ? (
                          <Button
                            size="sm"
                            onClick={() => onIntakeItem(item)}
                            loading={githubProjectBusyKey === `intake:${item.project_item_id}`}
                          >
                            Pull into Coder
                          </Button>
                        ) : null}
                        {linkedRunId ? (
                          <Button
                            size="sm"
                            variant="secondary"
                            onClick={() => onOpenLinkedRun(linkedRunId)}
                          >
                            Open run
                          </Button>
                        ) : null}
                      </div>
                    </div>
                    {!item.actionable && item.unsupported_reason ? (
                      <div className="mt-2 rounded-md border border-border bg-surface px-2 py-1.5 text-[11px] text-text-muted">
                        {item.unsupported_reason}
                      </div>
                    ) : null}
                    {item.linked_run ? (
                      <div className="mt-2 text-[11px] text-text-subtle">
                        Linked run {item.linked_run.coder_run.coder_run_id.slice(0, 8)}
                        {item.linked_run.active
                          ? " is active and owns this item."
                          : " is terminal — a new intake starts a fresh run."}
                      </div>
                    ) : null}
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
