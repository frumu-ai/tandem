import { useEffect, useMemo, useState } from "react";
import { ChevronDown, RefreshCw } from "lucide-react";
import {
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Input,
} from "@/components/ui";
import { ProjectSwitcher } from "@/components/sidebar";
import { AdvancedMissionBuilder } from "@/components/agent-automation/AdvancedMissionBuilder";
import { DeveloperRunViewer } from "@/components/developer/DeveloperRunViewer";
import { CoderRunDetailCard } from "@/components/coder/shared/CoderRunDetailCard";
import { CoderRunList } from "@/components/coder/shared/CoderRunList";
import { CoderRunsSummary } from "@/components/coder/CoderRunsSummary";
import { CoderGithubProjectPanel } from "@/components/coder/CoderGithubProjectPanel";
import {
  coderMetadataFromAutomation,
  extractSessionIdsFromRun,
  matchesActiveProject,
  runIsActive,
  runSortTimestamp,
  runStatusTone,
  shortText,
  type DerivedCoderRun,
} from "@/components/coder/shared/coderRunUtils";
import {
  automationsV2List,
  automationsV2RunCancel,
  automationsV2RunGateDecide,
  automationsV2RunGet,
  automationsV2RunPause,
  automationsV2RunRecover,
  automationsV2RunResume,
  automationsV2Runs,
  getSessionMessages,
  getCoderProjectBinding,
  getCoderProjectGithubInbox,
  listCoderRuns,
  intakeCoderProjectItem,
  listProvidersFromSidecar,
  mcpListServers,
  onSidecarEventV2,
  orchestratorEngineLoadRun,
  orchestratorGetBlackboard,
  putCoderProjectBinding,
  orchestratorGetBlackboardPatches,
  resolveUserRepoContext,
  toolIds,
  type AutomationV2RunRecord,
  type AutomationV2Spec,
  type Blackboard,
  type BlackboardPatchRecord,
  type CoderGithubProjectInboxItem,
  type CoderAutomationMetadata,
  type CoderRunRecord,
  type CoderProjectBindingRecord,
  type McpServerRecord,
  type OrchestratorRunRecord,
  type ProviderInfo,
  type SessionMessage,
  type UserRepoContext,
  type UserProject,
} from "@/lib/tauri";

type CoderWorkspacePageProps = {
  userProjects: UserProject[];
  activeProject: UserProject | null;
  onSwitchProject: (projectId: string) => void;
  onAddProject: () => void;
  onManageProjects: () => void;
  projectSwitcherLoading?: boolean;
  onOpenAutomation: () => void;
  onOpenAutomationRun?: (runId: string) => void;
  onOpenContextRun?: (runId: string) => void;
  onOpenMcpExtensions?: () => void;
};

type SavedCoderTemplate = {
  id: string;
  name: string;
  notes?: string | null;
  presetId: (typeof CODER_PRESETS)[number]["id"];
  repoSlug?: string | null;
  branch?: string | null;
  defaultBranch?: string | null;
  createdAtMs: number;
  updatedAtMs: number;
};

const CODER_TEMPLATE_STORAGE_KEY = "tandem.coder.savedTemplates.v1";
const CODER_PRESET_STORAGE_KEY = "tandem.coder.selectedPreset.v1";

const CODER_PRESETS = [
  {
    id: "issue-fix",
    title: "Issue Fix",
    summary: "Plan a coding swarm around a concrete defect, patch path, and validation gate.",
  },
  {
    id: "pr-review",
    title: "PR Review",
    summary: "Split review, validation, and approval workstreams around a pull request.",
  },
  {
    id: "repo-task",
    title: "Repo Task",
    summary: "Coordinate implementation, testing, and review work against the current repo.",
  },
  {
    id: "custom-swarm",
    title: "Custom Swarm",
    summary: "Start from the existing advanced mission builder without a canned workflow shape.",
  },
] as const;

type TabKey = "create" | "runs";

type TabBadgeTone = "default" | "attention" | "danger";

function TabPill({
  active,
  label,
  count,
  tone = "default",
  onClick,
}: {
  active: boolean;
  label: string;
  count?: number;
  tone?: TabBadgeTone;
  onClick: () => void;
}) {
  const badgeClass =
    tone === "attention"
      ? "bg-amber-300/20 text-amber-100 border border-amber-300/40"
      : tone === "danger"
        ? "bg-red-500/20 text-red-100 border border-red-500/40"
        : "bg-surface-elevated text-text-muted border border-border";
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={`inline-flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium transition-all ${
        active
          ? "bg-primary text-background"
          : "text-text-muted hover:bg-surface-elevated hover:text-text"
      }`}
    >
      <span>{label}</span>
      {typeof count === "number" && count > 0 ? (
        <span
          className={`inline-flex min-w-[1.25rem] items-center justify-center rounded-full px-1.5 text-[10px] font-semibold ${active ? "bg-background/30 text-background" : badgeClass}`}
        >
          {count}
        </span>
      ) : null}
    </button>
  );
}

export function CoderWorkspacePage({
  userProjects,
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  projectSwitcherLoading = false,
  onOpenAutomation,
  onOpenAutomationRun,
  onOpenContextRun,
  onOpenMcpExtensions,
}: CoderWorkspacePageProps) {
  const [tab, setTab] = useState<TabKey>("create");
  const [tabAutoChosen, setTabAutoChosen] = useState(false);
  const [runsLastUpdatedMs, setRunsLastUpdatedMs] = useState<number | null>(null);
  const [legacyOpen, setLegacyOpen] = useState(false);
  const [selectedPreset, setSelectedPreset] =
    useState<(typeof CODER_PRESETS)[number]["id"]>("repo-task");
  const [savedTemplates, setSavedTemplates] = useState<SavedCoderTemplate[]>([]);
  const [templateEditorId, setTemplateEditorId] = useState<string | null>(null);
  const [templateNameInput, setTemplateNameInput] = useState("");
  const [templateNotesInput, setTemplateNotesInput] = useState("");
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerRecord[]>([]);
  const [availableToolIds, setAvailableToolIds] = useState<string[]>([]);
  const [loadingCatalog, setLoadingCatalog] = useState(true);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [coderRuns, setCoderRuns] = useState<DerivedCoderRun[]>([]);
  const [selectedRunId, setSelectedRunId] = useState("");
  const [selectedRunDetail, setSelectedRunDetail] = useState<AutomationV2RunRecord | null>(null);
  const [selectedCoderRunRecord, setSelectedCoderRunRecord] = useState<CoderRunRecord | null>(null);
  const [selectedContextRunId, setSelectedContextRunId] = useState<string | null>(null);
  const [selectedRunMessagesBySession, setSelectedRunMessagesBySession] = useState<
    Record<string, SessionMessage[]>
  >({});
  const [selectedContextRun, setSelectedContextRun] = useState<OrchestratorRunRecord | null>(null);
  const [selectedContextBlackboard, setSelectedContextBlackboard] = useState<Blackboard | null>(
    null
  );
  const [selectedContextPatches, setSelectedContextPatches] = useState<BlackboardPatchRecord[]>([]);
  const [selectedContextError, setSelectedContextError] = useState<string | null>(null);
  const [runsLoading, setRunsLoading] = useState(true);
  const [runsError, setRunsError] = useState<string | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [repoContext, setRepoContext] = useState<UserRepoContext | null>(null);
  const [repoContextLoading, setRepoContextLoading] = useState(false);
  const [repoContextError, setRepoContextError] = useState<string | null>(null);
  const [projectBinding, setProjectBinding] = useState<CoderProjectBindingRecord | null>(null);
  const [projectBindingLoading, setProjectBindingLoading] = useState(false);
  const [projectBindingError, setProjectBindingError] = useState<string | null>(null);
  const [githubProjectOwnerInput, setGithubProjectOwnerInput] = useState("");
  const [githubProjectNumberInput, setGithubProjectNumberInput] = useState("");
  const [githubProjectInbox, setGithubProjectInbox] = useState<CoderGithubProjectInboxItem[]>([]);
  const [githubProjectInboxLoading, setGithubProjectInboxLoading] = useState(false);
  const [githubProjectInboxError, setGithubProjectInboxError] = useState<string | null>(null);
  const [githubProjectSchemaDrift, setGithubProjectSchemaDrift] = useState(false);
  const [githubProjectLiveSchemaFingerprint, setGithubProjectLiveSchemaFingerprint] = useState("");
  const [githubProjectBusyKey, setGithubProjectBusyKey] = useState<string | null>(null);

  const metadataPatch: CoderAutomationMetadata = useMemo(() => {
    const workflowKind =
      selectedPreset === "issue-fix"
        ? "issue_fix"
        : selectedPreset === "pr-review"
          ? "pr_review"
          : selectedPreset === "repo-task"
            ? "repo_task"
            : "coding_swarm";

    const repoRoot = String(repoContext?.repo_root || activeProject?.path || "").trim();
    const repoSlug = String(repoContext?.repo_slug || "").trim();
    const defaultBranch = String(repoContext?.default_branch || "").trim();
    const currentBranch = String(repoContext?.current_branch || "").trim();

    return {
      surface: "coder",
      workflow_kind: workflowKind,
      preset_id: selectedPreset,
      launch_source: "desktop_coder",
      repo_binding:
        activeProject?.id && repoRoot && repoSlug
          ? {
              project_id: activeProject.id,
              workspace_id: `ws-${activeProject.id}`,
              workspace_root: repoRoot,
              repo_slug: repoSlug,
              default_branch: defaultBranch || null,
            }
          : null,
      branch_context:
        currentBranch || defaultBranch
          ? {
              current_branch: currentBranch || null,
              default_branch: defaultBranch || null,
            }
          : null,
    };
  }, [activeProject?.id, activeProject?.path, repoContext, selectedPreset]);

  useEffect(() => {
    let cancelled = false;
    const loadCatalog = async () => {
      setLoadingCatalog(true);
      try {
        const [providerRows, mcpRows, toolRows] = await Promise.all([
          listProvidersFromSidecar(),
          mcpListServers(),
          toolIds().catch(() => []),
        ]);
        if (cancelled) return;
        setProviders(providerRows);
        setMcpServers(mcpRows);
        setAvailableToolIds(Array.isArray(toolRows) ? toolRows : []);
        setCatalogError(null);
      } catch (error) {
        if (cancelled) return;
        setCatalogError(error instanceof Error ? error.message : String(error));
      } finally {
        if (!cancelled) {
          setLoadingCatalog(false);
        }
      }
    };
    void loadCatalog();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    try {
      const rawPreset = localStorage.getItem(CODER_PRESET_STORAGE_KEY);
      if (rawPreset && CODER_PRESETS.some((preset) => preset.id === rawPreset)) {
        setSelectedPreset(rawPreset as (typeof CODER_PRESETS)[number]["id"]);
      }
      const rawTemplates = localStorage.getItem(CODER_TEMPLATE_STORAGE_KEY);
      if (!rawTemplates) return;
      const parsed = JSON.parse(rawTemplates);
      if (!Array.isArray(parsed)) return;
      setSavedTemplates(
        parsed.filter(
          (row): row is SavedCoderTemplate =>
            row &&
            typeof row === "object" &&
            typeof row.id === "string" &&
            typeof row.name === "string" &&
            typeof row.presetId === "string"
        )
      );
    } catch {
      // ignore local persistence failures
    }
  }, []);

  useEffect(() => {
    try {
      localStorage.setItem(CODER_PRESET_STORAGE_KEY, selectedPreset);
    } catch {
      // ignore local persistence failures
    }
  }, [selectedPreset]);

  useEffect(() => {
    try {
      localStorage.setItem(CODER_TEMPLATE_STORAGE_KEY, JSON.stringify(savedTemplates));
    } catch {
      // ignore local persistence failures
    }
  }, [savedTemplates]);

  useEffect(() => {
    let cancelled = false;
    const loadRepoContext = async () => {
      if (!activeProject?.path) {
        setRepoContext(null);
        setRepoContextError(null);
        setRepoContextLoading(false);
        return;
      }
      setRepoContextLoading(true);
      try {
        const context = await resolveUserRepoContext(activeProject.path);
        if (cancelled) return;
        setRepoContext(context);
        setRepoContextError(null);
      } catch (error) {
        if (cancelled) return;
        setRepoContext(null);
        setRepoContextError(error instanceof Error ? error.message : String(error));
      } finally {
        if (!cancelled) {
          setRepoContextLoading(false);
        }
      }
    };
    void loadRepoContext();
    return () => {
      cancelled = true;
    };
  }, [activeProject?.id, activeProject?.path]);

  useEffect(() => {
    let cancelled = false;
    const loadProjectBinding = async () => {
      if (!activeProject?.id) {
        setProjectBinding(null);
        setProjectBindingLoading(false);
        setProjectBindingError(null);
        setGithubProjectOwnerInput("");
        setGithubProjectNumberInput("");
        setGithubProjectInbox([]);
        setGithubProjectInboxError(null);
        setGithubProjectInboxLoading(false);
        setGithubProjectSchemaDrift(false);
        setGithubProjectLiveSchemaFingerprint("");
        return;
      }
      setProjectBindingLoading(true);
      try {
        const response = await getCoderProjectBinding(activeProject.id);
        if (cancelled) return;
        const binding = response?.binding || null;
        setProjectBinding(binding);
        setProjectBindingError(null);
        setGithubProjectOwnerInput(binding?.github_project_binding?.owner || "");
        setGithubProjectNumberInput(
          binding?.github_project_binding?.project_number
            ? String(binding.github_project_binding.project_number)
            : ""
        );
      } catch (error) {
        if (cancelled) return;
        setProjectBinding(null);
        setProjectBindingError(error instanceof Error ? error.message : String(error));
      } finally {
        if (!cancelled) {
          setProjectBindingLoading(false);
        }
      }
    };
    void loadProjectBinding();
    return () => {
      cancelled = true;
    };
  }, [activeProject?.id]);

  const refreshGithubProjectInbox = async (projectId: string) => {
    setGithubProjectInboxLoading(true);
    try {
      const response = await getCoderProjectGithubInbox(projectId);
      setGithubProjectInbox(Array.isArray(response?.items) ? response.items : []);
      setGithubProjectSchemaDrift(Boolean(response?.schema_drift));
      setGithubProjectLiveSchemaFingerprint(String(response?.live_schema_fingerprint || ""));
      setGithubProjectInboxError(null);
    } catch (error) {
      setGithubProjectInbox([]);
      setGithubProjectSchemaDrift(false);
      setGithubProjectLiveSchemaFingerprint("");
      setGithubProjectInboxError(error instanceof Error ? error.message : String(error));
    } finally {
      setGithubProjectInboxLoading(false);
    }
  };

  useEffect(() => {
    const binding = projectBinding?.github_project_binding;
    if (!activeProject?.id || !binding) {
      setGithubProjectInbox([]);
      setGithubProjectInboxError(null);
      setGithubProjectInboxLoading(false);
      setGithubProjectSchemaDrift(false);
      setGithubProjectLiveSchemaFingerprint("");
      return;
    }
    void refreshGithubProjectInbox(activeProject.id);
  }, [
    activeProject?.id,
    projectBinding?.github_project_binding?.owner,
    projectBinding?.github_project_binding?.project_number,
  ]);

  const refreshCoderRuns = async () => {
    setRunsLoading(true);
    try {
      const response = await automationsV2List();
      const coderAutomations = (Array.isArray(response?.automations) ? response.automations : [])
        .map((automation) => ({
          automation,
          coderMetadata: coderMetadataFromAutomation(automation),
        }))
        .filter(
          (
            row
          ): row is {
            automation: AutomationV2Spec;
            coderMetadata: CoderAutomationMetadata;
          } => Boolean(row.coderMetadata)
        )
        .filter(({ automation }) => matchesActiveProject(automation, activeProject));
      const runRows = await Promise.all(
        coderAutomations.map(async ({ automation, coderMetadata }) => {
          const automationId = String(automation.automation_id || "").trim();
          if (!automationId) return [];
          try {
            const runsResponse = await automationsV2Runs(automationId, 12);
            const runs = Array.isArray(runsResponse?.runs) ? runsResponse.runs : [];
            return runs.map((run) => ({ automation, run, coderMetadata }));
          } catch {
            return [];
          }
        })
      );
      const nextRuns = runRows
        .flat()
        .sort((a, b) => runSortTimestamp(b.run) - runSortTimestamp(a.run));
      setCoderRuns(nextRuns);
      setRunsError(null);
      setRunsLastUpdatedMs(Date.now());
      setSelectedRunId((current) => {
        if (current && nextRuns.some((row) => row.run.run_id === current)) return current;
        return nextRuns[0]?.run.run_id || "";
      });
    } catch (error) {
      setRunsError(error instanceof Error ? error.message : String(error));
    } finally {
      setRunsLoading(false);
    }
  };

  const loadSelectedRunDetail = async (runId: string) => {
    const trimmed = String(runId || "").trim();
    if (!trimmed) {
      setSelectedRunDetail(null);
      setSelectedCoderRunRecord(null);
      setSelectedContextRunId(null);
      setSelectedContextRun(null);
      setSelectedContextBlackboard(null);
      setSelectedContextPatches([]);
      setSelectedContextError(null);
      setSelectedRunMessagesBySession({});
      return;
    }
    setBusyKey(`inspect:${trimmed}`);
    try {
      const response = await automationsV2RunGet(trimmed);
      const run = response?.run || null;
      setSelectedRunDetail(run);
      const linkedContextRunId = response?.linked_context_run_id || null;
      setSelectedContextRunId(linkedContextRunId);
      if (linkedContextRunId) {
        try {
          const coderRunsResponse = await listCoderRuns({
            limit: 80,
            repoSlug: repoContext?.repo_slug || undefined,
          });
          const matchedCoderRun =
            (Array.isArray(coderRunsResponse?.runs) ? coderRunsResponse.runs : []).find(
              (record) => record.linked_context_run_id === linkedContextRunId
            ) || null;
          setSelectedCoderRunRecord(matchedCoderRun);
        } catch {
          setSelectedCoderRunRecord(null);
        }
      } else {
        setSelectedCoderRunRecord(null);
      }
      if (linkedContextRunId) {
        try {
          const [contextRun, blackboard, patches] = await Promise.all([
            orchestratorEngineLoadRun(linkedContextRunId),
            orchestratorGetBlackboard(linkedContextRunId),
            orchestratorGetBlackboardPatches(linkedContextRunId, undefined, 50),
          ]);
          setSelectedContextRun(contextRun);
          setSelectedContextBlackboard(blackboard);
          setSelectedContextPatches(Array.isArray(patches) ? patches : []);
          setSelectedContextError(null);
        } catch (contextError) {
          setSelectedContextRun(null);
          setSelectedContextBlackboard(null);
          setSelectedContextPatches([]);
          setSelectedContextError(
            contextError instanceof Error ? contextError.message : String(contextError)
          );
        }
      } else {
        setSelectedContextRun(null);
        setSelectedContextBlackboard(null);
        setSelectedContextPatches([]);
        setSelectedContextError(null);
      }
      const sessionIds = extractSessionIdsFromRun(run);
      if (sessionIds.length === 0) {
        setSelectedRunMessagesBySession({});
        return;
      }
      const sessionRows = await Promise.all(
        sessionIds.map(async (sessionId) => ({
          sessionId,
          messages: await getSessionMessages(sessionId).catch(() => []),
        }))
      );
      setSelectedRunMessagesBySession(
        Object.fromEntries(sessionRows.map((row) => [row.sessionId, row.messages]))
      );
    } catch (error) {
      setRunsError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyKey((current) => (current === `inspect:${trimmed}` ? null : current));
    }
  };

  useEffect(() => {
    void refreshCoderRuns();
  }, [activeProject?.id]);

  useEffect(() => {
    if (tabAutoChosen) return;
    if (runsLoading) return;
    if (coderRuns.some((row) => runIsActive(row.run))) {
      setTab("runs");
    }
    setTabAutoChosen(true);
  }, [coderRuns, runsLoading, tabAutoChosen]);

  useEffect(() => {
    if (!selectedRunId) {
      setSelectedRunDetail(null);
      setSelectedCoderRunRecord(null);
      setSelectedContextRunId(null);
      setSelectedRunMessagesBySession({});
      return;
    }
    void loadSelectedRunDetail(selectedRunId);
  }, [selectedRunId, repoContext?.repo_slug]);

  const openContextRunForAutomationRun = async (runId: string) => {
    if (!onOpenContextRun) return;
    const trimmed = String(runId || "").trim();
    if (!trimmed) return;
    setBusyKey(`open-context:${trimmed}`);
    try {
      const response = await automationsV2RunGet(trimmed);
      const linkedContextRunId = String(response?.linked_context_run_id || "").trim();
      if (!linkedContextRunId) {
        setRunsError("The selected automation run does not expose a linked context run ID.");
        return;
      }
      onOpenContextRun(linkedContextRunId);
    } catch (error) {
      setRunsError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyKey((current) => (current === `open-context:${trimmed}` ? null : current));
    }
  };

  useEffect(() => {
    let refreshTimeout: ReturnType<typeof setTimeout> | null = null;
    let disposed = false;
    const start = async () => {
      const unlisten = await onSidecarEventV2((event) => {
        if (disposed) return;
        const payload = JSON.stringify(event || {}).toLowerCase();
        if (
          !payload.includes("automation") &&
          !payload.includes("workflow") &&
          !payload.includes("run")
        ) {
          return;
        }
        if (refreshTimeout) clearTimeout(refreshTimeout);
        refreshTimeout = setTimeout(() => {
          void refreshCoderRuns().catch(() => undefined);
          if (selectedRunId) {
            void loadSelectedRunDetail(selectedRunId).catch(() => undefined);
          }
        }, 500);
      });
      return unlisten;
    };
    let unlistenRef: (() => void) | null = null;
    void start().then((unlisten) => {
      unlistenRef = unlisten;
    });
    return () => {
      disposed = true;
      if (refreshTimeout) clearTimeout(refreshTimeout);
      if (unlistenRef) void unlistenRef();
    };
  }, [selectedRunId, activeProject?.id]);

  const selectedCoderRun = useMemo(
    () => coderRuns.find((row) => row.run.run_id === selectedRunId) || null,
    [coderRuns, selectedRunId]
  );

  const githubProjectBinding = projectBinding?.github_project_binding || null;
  const githubProjectServerConnected = useMemo(() => {
    const explicitServer = String(githubProjectBinding?.mcp_server || "").trim();
    if (explicitServer) {
      const exact = mcpServers.find((server) => server.name === explicitServer);
      if (exact) return exact.connected && exact.enabled;
    }
    const requiredTools = [
      "mcp.github.get_project",
      "mcp.github.list_project_items",
      "mcp.github.update_project_item_field",
    ];
    return requiredTools.every((toolName) => availableToolIds.includes(toolName));
  }, [availableToolIds, githubProjectBinding?.mcp_server, mcpServers]);

  const githubProjectReadReady = useMemo(() => {
    const requiredTools = ["mcp.github.get_project", "mcp.github.list_project_items"];
    return requiredTools.every((toolName) => availableToolIds.includes(toolName));
  }, [availableToolIds]);

  const githubProjectWriteReady = useMemo(
    () => availableToolIds.includes("mcp.github.update_project_item_field"),
    [availableToolIds]
  );

  const selectedSessionPreview = useMemo(() => {
    const firstSessionId = Object.keys(selectedRunMessagesBySession)[0];
    if (!firstSessionId) return null;
    const messages = selectedRunMessagesBySession[firstSessionId] || [];
    const latestMessage = messages[messages.length - 1];
    return {
      sessionId: firstSessionId,
      messageCount: messages.length,
      latestText: shortText(
        Array.isArray(latestMessage?.parts)
          ? latestMessage.parts
              .map((part) =>
                typeof part === "object" && part !== null
                  ? String((part as Record<string, unknown>).text || "")
                  : ""
              )
              .join(" ")
          : "",
        220
      ),
    };
  }, [selectedRunMessagesBySession]);

  const handleRunAction = async (
    runId: string,
    action: "pause" | "resume" | "cancel" | "recover"
  ) => {
    setBusyKey(`${action}:${runId}`);
    try {
      if (action === "pause") {
        await automationsV2RunPause(runId, "Paused from desktop coder workspace");
      } else if (action === "resume") {
        await automationsV2RunResume(runId, "Resumed from desktop coder workspace");
      } else if (action === "cancel") {
        await automationsV2RunCancel(runId, "Cancelled from desktop coder workspace");
      } else {
        await automationsV2RunRecover(runId, "Recovered from desktop coder workspace");
      }
      await refreshCoderRuns();
      await loadSelectedRunDetail(runId);
    } catch (error) {
      setRunsError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyKey(null);
    }
  };

  const handleGateDecision = async (runId: string, decision: "approve" | "rework" | "cancel") => {
    setBusyKey(`gate:${decision}:${runId}`);
    try {
      await automationsV2RunGateDecide(runId, { decision });
      await refreshCoderRuns();
      await loadSelectedRunDetail(runId);
    } catch (error) {
      setRunsError(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyKey(null);
    }
  };

  const saveCurrentTemplate = () => {
    const trimmed = templateNameInput.trim();
    if (!trimmed) return;
    const notes = templateNotesInput.trim();
    setSavedTemplates((current) => {
      const now = Date.now();
      if (templateEditorId) {
        return current.map((template) =>
          template.id === templateEditorId
            ? {
                ...template,
                name: trimmed,
                notes: notes || null,
                presetId: selectedPreset,
                repoSlug: repoContext?.repo_slug || null,
                branch: repoContext?.current_branch || null,
                defaultBranch: repoContext?.default_branch || null,
                updatedAtMs: now,
              }
            : template
        );
      }
      return [
        {
          id: crypto.randomUUID(),
          name: trimmed,
          notes: notes || null,
          presetId: selectedPreset,
          repoSlug: repoContext?.repo_slug || null,
          branch: repoContext?.current_branch || null,
          defaultBranch: repoContext?.default_branch || null,
          createdAtMs: now,
          updatedAtMs: now,
        },
        ...current.filter((template) => template.name !== trimmed).slice(0, 11),
      ];
    });
    setTemplateEditorId(null);
    setTemplateNameInput("");
    setTemplateNotesInput("");
  };

  const deleteTemplate = (templateId: string) => {
    setSavedTemplates((current) => current.filter((template) => template.id !== templateId));
    if (templateEditorId === templateId) {
      setTemplateEditorId(null);
      setTemplateNameInput("");
      setTemplateNotesInput("");
    }
  };

  const startEditingTemplate = (template: SavedCoderTemplate) => {
    setTemplateEditorId(template.id);
    setTemplateNameInput(template.name);
    setTemplateNotesInput(template.notes || "");
    setSelectedPreset(template.presetId);
  };

  const resetTemplateEditor = () => {
    setTemplateEditorId(null);
    setTemplateNameInput("");
    setTemplateNotesInput("");
  };

  const saveGithubProjectBinding = async () => {
    if (!activeProject?.id) {
      setProjectBindingError("Choose an active project before connecting a GitHub Project.");
      return;
    }
    const owner = githubProjectOwnerInput.trim();
    const projectNumber = Number(githubProjectNumberInput);
    if (!owner || !Number.isFinite(projectNumber) || projectNumber <= 0) {
      setProjectBindingError("Enter a GitHub owner and a valid project number.");
      return;
    }
    setGithubProjectBusyKey("save-binding");
    try {
      const response = await putCoderProjectBinding(activeProject.id, {
        github_project_binding: {
          owner,
          project_number: projectNumber,
          repo_slug: repoContext?.repo_slug || null,
        },
      });
      setProjectBinding(response.binding || null);
      setProjectBindingError(null);
      setGithubProjectOwnerInput(response.binding?.github_project_binding?.owner || owner);
      setGithubProjectNumberInput(
        response.binding?.github_project_binding?.project_number
          ? String(response.binding.github_project_binding.project_number)
          : String(projectNumber)
      );
      await refreshGithubProjectInbox(activeProject.id);
    } catch (error) {
      setProjectBindingError(error instanceof Error ? error.message : String(error));
    } finally {
      setGithubProjectBusyKey(null);
    }
  };

  const handleIntakeProjectItem = async (item: CoderGithubProjectInboxItem) => {
    if (!activeProject?.id) return;
    setGithubProjectBusyKey(`intake:${item.project_item_id}`);
    try {
      const response = await intakeCoderProjectItem(activeProject.id, {
        project_item_id: item.project_item_id,
        source_client: "desktop_coder",
      });
      const runId = String(
        (response as { coder_run?: { coder_run_id?: string } } | null)?.coder_run?.coder_run_id ||
          ""
      ).trim();
      await refreshCoderRuns();
      await refreshGithubProjectInbox(activeProject.id);
      if (runId) {
        setSelectedRunId(runId);
        setTab("runs");
      }
    } catch (error) {
      setGithubProjectInboxError(error instanceof Error ? error.message : String(error));
    } finally {
      setGithubProjectBusyKey(null);
    }
  };

  const runsTally = useMemo(() => {
    const tally = { total: coderRuns.length, active: 0, awaiting: 0, failed: 0 };
    for (const row of coderRuns) {
      const tone = runStatusTone(row.run);
      if (tone === "running" || tone === "queued" || tone === "paused" || tone === "awaiting") {
        tally.active += 1;
      }
      if (tone === "awaiting") tally.awaiting += 1;
      if (tone === "failed") tally.failed += 1;
    }
    return tally;
  }, [coderRuns]);

  const runsTabTone: TabBadgeTone =
    runsTally.awaiting > 0 ? "attention" : runsTally.failed > 0 ? "danger" : "default";
  const runsTabCount = runsTally.active + runsTally.failed;

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mx-auto max-w-[1480px] space-y-4">
        <Card className="p-4">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="min-w-0 flex-1">
              <CardTitle className="text-xl">Coder</CardTitle>
              <CardDescription className="mt-1">
                Launch coding swarms against your repo, pull in GitHub Project items, and steer runs
                as they go.
              </CardDescription>
              <div className="mt-3 max-w-md">
                <ProjectSwitcher
                  projects={userProjects}
                  activeProject={activeProject}
                  onSwitchProject={onSwitchProject}
                  onAddProject={onAddProject}
                  onManageProjects={onManageProjects}
                  isLoading={projectSwitcherLoading}
                />
                {repoContextLoading ? (
                  <div className="mt-1.5 text-[11px] text-text-subtle">Detecting git repo…</div>
                ) : repoContext?.is_repo ? (
                  <div className="mt-1.5 text-[11px] text-text-subtle">
                    {repoContext.repo_slug || "Local git repo"}
                    {repoContext.current_branch ? ` · ${repoContext.current_branch}` : ""}
                    {repoContext.default_branch &&
                    repoContext.default_branch !== repoContext.current_branch
                      ? ` (default ${repoContext.default_branch})`
                      : ""}
                  </div>
                ) : repoContextError ? (
                  <div className="mt-1.5 text-[11px] text-red-300">{repoContextError}</div>
                ) : activeProject ? (
                  <div className="mt-1.5 text-[11px] text-text-subtle">
                    Not a git repo — coder runs will use the folder path only.
                  </div>
                ) : null}
              </div>
            </div>
            <div className="flex flex-col items-end gap-2">
              <div className="flex flex-wrap items-center gap-1 rounded-lg border border-border bg-surface-elevated/40 p-1">
                <TabPill
                  active={tab === "create"}
                  label="Create"
                  onClick={() => {
                    setTab("create");
                    setTabAutoChosen(true);
                  }}
                />
                <TabPill
                  active={tab === "runs"}
                  label="Runs"
                  count={runsTabCount > 0 ? runsTabCount : undefined}
                  tone={runsTabTone}
                  onClick={() => {
                    setTab("runs");
                    setTabAutoChosen(true);
                  }}
                />
              </div>
              <Button size="sm" variant="ghost" onClick={onOpenAutomation}>
                Open Agent Automation
              </Button>
            </div>
          </div>
        </Card>

        {tab === "create" ? (
          <>
            <Card>
              <CardHeader>
                <CardTitle className="text-base">Coding Presets</CardTitle>
                <CardDescription>
                  Presets are now locally persisted so the Coder create flow can keep a lightweight
                  template shelf without forking the mission contract.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="grid gap-3 rounded-xl border border-border bg-surface-elevated/20 p-4 lg:grid-cols-[minmax(0,220px)_minmax(0,1fr)_auto]">
                  <div className="space-y-2">
                    <div className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                      Template Name
                    </div>
                    <Input
                      value={templateNameInput}
                      onChange={(event) => setTemplateNameInput(event.target.value)}
                      placeholder="Issue Fix Triage"
                    />
                  </div>
                  <div className="space-y-2">
                    <div className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                      Notes
                    </div>
                    <Input
                      value={templateNotesInput}
                      onChange={(event) => setTemplateNotesInput(event.target.value)}
                      placeholder="Save the current preset plus repo and branch context"
                    />
                  </div>
                  <div className="flex flex-wrap items-end gap-2">
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={saveCurrentTemplate}
                      disabled={!templateNameInput.trim()}
                    >
                      {templateEditorId ? "Update Template" : "Save Template"}
                    </Button>
                    {templateEditorId ? (
                      <Button size="sm" variant="ghost" onClick={resetTemplateEditor}>
                        New Template
                      </Button>
                    ) : null}
                  </div>
                </div>
                <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                  {CODER_PRESETS.map((preset) => {
                    const active = preset.id === selectedPreset;
                    return (
                      <button
                        key={preset.id}
                        type="button"
                        onClick={() => setSelectedPreset(preset.id)}
                        className={`rounded-xl border p-4 text-left transition-colors ${
                          active
                            ? "border-primary bg-primary/10"
                            : "border-border bg-surface-elevated/30 hover:bg-surface-elevated/50"
                        }`}
                      >
                        <div className="text-sm font-semibold text-text">{preset.title}</div>
                        <div className="mt-2 text-xs leading-5 text-text-muted">
                          {preset.summary}
                        </div>
                      </button>
                    );
                  })}
                </div>
                {savedTemplates.length > 0 ? (
                  <div className="space-y-3">
                    <div className="text-sm font-semibold text-text">Saved Templates</div>
                    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                      {savedTemplates.map((template) => (
                        <div
                          key={template.id}
                          className="rounded-xl border border-border bg-surface-elevated/20 p-4"
                        >
                          <div className="flex items-start justify-between gap-3">
                            <div>
                              <div className="text-sm font-semibold text-text">{template.name}</div>
                              <div className="mt-1 text-xs text-text-muted">
                                {template.presetId.replace(/-/g, " ")}
                              </div>
                            </div>
                            <div className="flex items-center gap-3">
                              <button
                                type="button"
                                onClick={() => startEditingTemplate(template)}
                                className="text-xs text-text-subtle transition-colors hover:text-text"
                              >
                                Edit
                              </button>
                              <button
                                type="button"
                                onClick={() => deleteTemplate(template.id)}
                                className="text-xs text-text-subtle transition-colors hover:text-text"
                              >
                                Delete
                              </button>
                            </div>
                          </div>
                          {template.notes ? (
                            <div className="mt-3 text-xs leading-5 text-text-muted">
                              {template.notes}
                            </div>
                          ) : null}
                          <div className="mt-3 text-xs text-text-muted">
                            {template.repoSlug || "No repo slug saved"}
                            {template.branch ? ` • ${template.branch}` : ""}
                            {template.defaultBranch ? ` • default ${template.defaultBranch}` : ""}
                          </div>
                          <div className="mt-1 text-[11px] text-text-subtle">
                            Updated{" "}
                            {new Date(
                              template.updatedAtMs || template.createdAtMs
                            ).toLocaleString()}
                          </div>
                          <div className="mt-3 flex flex-wrap gap-2">
                            <Button
                              size="sm"
                              variant="secondary"
                              onClick={() => setSelectedPreset(template.presetId)}
                            >
                              Apply
                            </Button>
                            <Button
                              size="sm"
                              variant="ghost"
                              onClick={() => startEditingTemplate(template)}
                            >
                              Load Into Editor
                            </Button>
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                ) : null}
              </CardContent>
            </Card>

            <CoderGithubProjectPanel
              activeProjectId={activeProject?.id}
              activeProjectName={activeProject?.name}
              projectBinding={projectBinding}
              projectBindingLoading={projectBindingLoading}
              projectBindingError={projectBindingError}
              githubProjectOwnerInput={githubProjectOwnerInput}
              setGithubProjectOwnerInput={setGithubProjectOwnerInput}
              githubProjectNumberInput={githubProjectNumberInput}
              setGithubProjectNumberInput={setGithubProjectNumberInput}
              githubProjectInbox={githubProjectInbox}
              githubProjectInboxLoading={githubProjectInboxLoading}
              githubProjectInboxError={githubProjectInboxError}
              githubProjectSchemaDrift={githubProjectSchemaDrift}
              githubProjectLiveSchemaFingerprint={githubProjectLiveSchemaFingerprint}
              githubProjectBusyKey={githubProjectBusyKey}
              githubProjectReadReady={githubProjectReadReady}
              githubProjectWriteReady={githubProjectWriteReady}
              githubProjectServerConnected={githubProjectServerConnected}
              onSaveBinding={() => void saveGithubProjectBinding()}
              onRefreshInbox={() =>
                activeProject?.id ? void refreshGithubProjectInbox(activeProject.id) : undefined
              }
              onIntakeItem={(item) => void handleIntakeProjectItem(item)}
              onOpenLinkedRun={(runId) => {
                setSelectedRunId(runId);
                setTab("runs");
                setTabAutoChosen(true);
              }}
              onOpenMcpExtensions={onOpenMcpExtensions}
            />

            <Card className="p-4">
              <div className="mb-3 flex items-center justify-between gap-3">
                <div>
                  <CardTitle className="text-base">Mission</CardTitle>
                  <CardDescription>
                    Using{" "}
                    <span className="font-medium text-text">
                      {CODER_PRESETS.find((preset) => preset.id === selectedPreset)?.title ||
                        selectedPreset}
                    </span>{" "}
                    preset. The builder below emits the same mission contract Agent Automation
                    consumes.
                  </CardDescription>
                </div>
              </div>
              <CardContent className="p-0">
                {catalogError ? (
                  <div className="mb-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                    {catalogError}
                  </div>
                ) : null}
                {loadingCatalog ? (
                  <div className="rounded-lg border border-border bg-surface px-4 py-8 text-center text-sm text-text-muted">
                    Loading builder catalog…
                  </div>
                ) : (
                  <AdvancedMissionBuilder
                    activeProject={activeProject}
                    providers={providers}
                    mcpServers={mcpServers}
                    toolIds={availableToolIds}
                    blueprintMetadataPatch={{ coder: metadataPatch }}
                    onRefreshAutomations={async () => undefined}
                    onShowAutomations={onOpenAutomation}
                    onShowRuns={() => {
                      setTab("runs");
                      setTabAutoChosen(true);
                    }}
                    onOpenMcpExtensions={onOpenMcpExtensions}
                  />
                )}
              </CardContent>
            </Card>
          </>
        ) : (
          <div className="space-y-3">
            <CoderRunsSummary
              runs={coderRuns}
              lastUpdatedMs={runsLastUpdatedMs}
              isLoading={runsLoading}
            />

            <div className="flex flex-wrap items-center gap-2">
              <Button
                size="sm"
                onClick={() => {
                  setTab("create");
                  setTabAutoChosen(true);
                }}
              >
                New coding swarm
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void refreshCoderRuns()}
                loading={runsLoading}
              >
                <RefreshCw className="mr-1 h-3.5 w-3.5" aria-hidden />
                Refresh
              </Button>
              <Button size="sm" variant="ghost" onClick={onOpenAutomation}>
                Agent Automation runtime
              </Button>
            </div>

            {runsError ? (
              <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                {runsError}
              </div>
            ) : null}

            {runsLoading && coderRuns.length === 0 ? (
              <div className="rounded-2xl border border-border bg-surface px-4 py-12 text-center text-sm text-text-muted">
                Loading coder runs…
              </div>
            ) : coderRuns.length === 0 ? (
              <div className="rounded-2xl border border-dashed border-border bg-surface-elevated/20 px-4 py-16 text-center">
                <div className="text-sm font-medium text-text">No coder runs yet</div>
                <div className="mt-1 text-xs text-text-muted">
                  Launch a coding swarm from the Create tab — runs will show up here live.
                </div>
                <Button
                  className="mt-4"
                  size="sm"
                  onClick={() => {
                    setTab("create");
                    setTabAutoChosen(true);
                  }}
                >
                  Go to Create
                </Button>
              </div>
            ) : (
              <div className="grid gap-3 xl:grid-cols-[380px_minmax(0,1fr)]">
                <CoderRunList
                  runs={coderRuns}
                  selectedRunId={selectedRunId}
                  onSelectRun={setSelectedRunId}
                  onOpenAutomationRun={onOpenAutomationRun}
                  onOpenContextRun={openContextRunForAutomationRun}
                />

                <CoderRunDetailCard
                  key={selectedRunId || "empty-run-detail"}
                  selectedCoderRun={selectedCoderRun}
                  selectedCoderRunRecord={selectedCoderRunRecord}
                  selectedRunDetail={selectedRunDetail}
                  selectedContextRunId={selectedContextRunId}
                  selectedSessionPreview={selectedSessionPreview}
                  sessionMessagesBySession={selectedRunMessagesBySession}
                  selectedContextRun={selectedContextRun}
                  selectedContextBlackboard={selectedContextBlackboard}
                  selectedContextPatches={selectedContextPatches}
                  selectedContextError={selectedContextError}
                  busyKey={busyKey}
                  onRefreshDetail={(runId) => void loadSelectedRunDetail(runId)}
                  onRunAction={(runId, action) => void handleRunAction(runId, action)}
                  onGateDecision={(runId, decision) => void handleGateDecision(runId, decision)}
                  onOpenAutomationRun={onOpenAutomationRun}
                  onOpenContextRun={onOpenContextRun}
                />
              </div>
            )}

            <div className="rounded-2xl border border-border bg-surface-elevated/20">
              <button
                type="button"
                onClick={() => setLegacyOpen((prev) => !prev)}
                className="flex w-full items-center justify-between gap-2 px-4 py-2.5 text-xs text-text-muted transition-colors hover:text-text"
              >
                <span>Legacy coder inspector</span>
                <ChevronDown
                  className={`h-3.5 w-3.5 transition-transform ${legacyOpen ? "rotate-180" : ""}`}
                  aria-hidden
                />
              </button>
              {legacyOpen ? (
                <div className="min-h-[600px] border-t border-border bg-surface">
                  <DeveloperRunViewer onOpenMcpSettings={onOpenMcpExtensions} />
                </div>
              ) : null}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
