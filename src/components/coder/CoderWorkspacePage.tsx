import { useEffect, useMemo, useState } from "react";
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
import {
  coderMetadataFromAutomation,
  extractSessionIdsFromRun,
  matchesActiveProject,
  runSortTimestamp,
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

type CoderTab = "create" | "runs";
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

function TabButton({
  active,
  children,
  onClick,
}: {
  active: boolean;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <Button size="sm" variant={active ? "primary" : "secondary"} onClick={onClick}>
      {children}
    </Button>
  );
}

function syncStateLabel(value: string | null | undefined): string {
  if (!value) return "Unknown";
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
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
  const [tab, setTab] = useState<CoderTab>("create");
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
  const githubProjectStatusMapping = githubProjectBinding?.status_mapping || null;
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

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mx-auto max-w-[1480px] space-y-4">
        <Card>
          <CardHeader className="flex flex-row items-start justify-between gap-4 space-y-0">
            <div>
              <CardTitle>Coder</CardTitle>
              <CardDescription>
                Home for coding swarm creation and operation, reusing the existing mission and
                automation machinery.
              </CardDescription>
            </div>
            <div className="flex flex-wrap gap-2">
              <TabButton active={tab === "create"} onClick={() => setTab("create")}>
                Create
              </TabButton>
              <TabButton active={tab === "runs"} onClick={() => setTab("runs")}>
                Runs
              </TabButton>
              <Button size="sm" variant="secondary" onClick={onOpenAutomation}>
                Open Agent Automation
              </Button>
            </div>
          </CardHeader>
          <CardContent className="grid gap-3 md:grid-cols-3">
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                Active Project
              </div>
              <div className="mt-1 text-sm font-medium text-text">
                {activeProject?.name || "No folder selected"}
              </div>
              <div className="mt-1 text-xs text-text-muted">
                {activeProject?.path || "Select a user repo before launching a coding swarm."}
              </div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                First Slice
              </div>
              <div className="mt-1 text-sm font-medium text-text">UI-only Coder shell</div>
              <div className="mt-1 text-xs text-text-muted">
                This slice consolidates navigation and creation UX. Automation-backed coder runs are
                wired in the follow-on backend slices.
              </div>
            </div>
            <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
              <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                Compatibility
              </div>
              <div className="mt-1 text-sm font-medium text-text">Legacy coder runs remain</div>
              <div className="mt-1 text-xs text-text-muted">
                The existing coder inspector stays available below until the unified run model is in
                place.
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Project Context</CardTitle>
            <CardDescription>
              The Coder workspace operates against the active user project and its git repo.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <ProjectSwitcher
              projects={userProjects}
              activeProject={activeProject}
              onSwitchProject={onSwitchProject}
              onAddProject={onAddProject}
              onManageProjects={onManageProjects}
              isLoading={projectSwitcherLoading}
            />
          </CardContent>
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

            <Card>
              <CardHeader>
                <CardTitle className="text-base">User Repo Context</CardTitle>
                <CardDescription>
                  Detected from the active user project path and merged into coder-originated
                  mission metadata when available.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                {repoContextError ? (
                  <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                    {repoContextError}
                  </div>
                ) : null}
                {repoContextLoading ? (
                  <div className="rounded-lg border border-border bg-surface px-4 py-6 text-sm text-text-muted">
                    Detecting git repo context...
                  </div>
                ) : (
                  <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                    <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Repo Root
                      </div>
                      <div className="mt-1 break-all text-xs text-text">
                        {repoContext?.repo_root || activeProject?.path || "Not detected"}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Remote Slug
                      </div>
                      <div className="mt-1 break-all text-xs text-text">
                        {repoContext?.repo_slug || "Not detected"}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Current Branch
                      </div>
                      <div className="mt-1 break-all text-xs text-text">
                        {repoContext?.current_branch || "Not detected"}
                      </div>
                    </div>
                    <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                      <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                        Default Branch
                      </div>
                      <div className="mt-1 break-all text-xs text-text">
                        {repoContext?.default_branch || "Not detected"}
                      </div>
                    </div>
                  </div>
                )}
                <div className="rounded-lg border border-border bg-surface-elevated/20 px-4 py-3 text-xs text-text-muted">
                  {repoContext?.is_repo
                    ? "Detected git metadata is now used to prefill coder mission metadata for this user repo."
                    : "The active project path is not currently resolving to a git repo with a discoverable origin remote."}
                </div>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-base">GitHub Project Intake</CardTitle>
                <CardDescription>
                  Connect one GitHub Project to this coder workspace, pull issue-backed TODO items
                  into Tandem, and project key status changes back out.
                </CardDescription>
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
                  <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-3 text-sm text-amber-100">
                    <div>
                      GitHub Project tools are not ready yet. We need MCP read access before this
                      intake flow can validate or list project items.
                    </div>
                    {onOpenMcpExtensions ? (
                      <Button size="sm" variant="secondary" onClick={onOpenMcpExtensions}>
                        Open MCP Extensions
                      </Button>
                    ) : null}
                  </div>
                ) : null}
                {githubProjectReadReady && !githubProjectWriteReady ? (
                  <div className="rounded-lg border border-border bg-surface-elevated/20 px-4 py-3 text-xs text-text-muted">
                    GitHub Project access is currently read-only. Inbox listing and intake can still
                    work, but outward status projection is unavailable until update access is
                    connected.
                  </div>
                ) : null}
                <div className="grid gap-3 rounded-xl border border-border bg-surface-elevated/20 p-4 lg:grid-cols-[minmax(0,1fr)_180px_auto]">
                  <div className="space-y-2">
                    <div className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                      Owner
                    </div>
                    <Input
                      value={githubProjectOwnerInput}
                      onChange={(event) => setGithubProjectOwnerInput(event.target.value)}
                      placeholder="acme-inc"
                      disabled={!activeProject || projectBindingLoading}
                    />
                  </div>
                  <div className="space-y-2">
                    <div className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                      Project Number
                    </div>
                    <Input
                      value={githubProjectNumberInput}
                      onChange={(event) => setGithubProjectNumberInput(event.target.value)}
                      placeholder="12"
                      inputMode="numeric"
                      disabled={!activeProject || projectBindingLoading}
                    />
                  </div>
                  <div className="flex flex-wrap items-end gap-2">
                    <Button
                      size="sm"
                      onClick={() => void saveGithubProjectBinding()}
                      loading={githubProjectBusyKey === "save-binding"}
                      disabled={!activeProject || !githubProjectReadReady}
                    >
                      {githubProjectBinding ? "Refresh Binding" : "Connect Project"}
                    </Button>
                    {githubProjectBinding && activeProject?.id ? (
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => void refreshGithubProjectInbox(activeProject.id)}
                        loading={githubProjectInboxLoading}
                      >
                        Refresh Inbox
                      </Button>
                    ) : null}
                  </div>
                </div>
                <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Repo Slug
                    </div>
                    <div className="mt-1 break-all text-xs text-text">
                      {repoContext?.repo_slug || githubProjectBinding?.repo_slug || "Advisory only"}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      MCP Transport
                    </div>
                    <div className="mt-1 text-xs text-text">
                      {githubProjectBinding?.mcp_server ||
                        (githubProjectServerConnected ? "Detected" : "Not connected")}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Saved Fingerprint
                    </div>
                    <div className="mt-1 break-all text-xs text-text">
                      {githubProjectBinding?.schema_fingerprint || "Not bound"}
                    </div>
                  </div>
                  <div className="rounded-lg border border-border bg-surface-elevated/40 p-3">
                    <div className="text-[10px] uppercase tracking-wide text-text-subtle">
                      Live Fingerprint
                    </div>
                    <div className="mt-1 break-all text-xs text-text">
                      {githubProjectLiveSchemaFingerprint || "Not checked"}
                    </div>
                  </div>
                </div>
                {githubProjectStatusMapping ? (
                  <div className="rounded-xl border border-border bg-surface-elevated/20 p-4">
                    <div className="text-sm font-semibold text-text">Resolved Status Mapping</div>
                    <div className="mt-3 grid gap-2 text-xs text-text-muted md:grid-cols-2 xl:grid-cols-5">
                      <div>
                        TODO:{" "}
                        <span className="font-medium text-text">
                          {githubProjectStatusMapping.todo.name}
                        </span>
                      </div>
                      <div>
                        In Progress:{" "}
                        <span className="font-medium text-text">
                          {githubProjectStatusMapping.in_progress.name}
                        </span>
                      </div>
                      <div>
                        In Review:{" "}
                        <span className="font-medium text-text">
                          {githubProjectStatusMapping.in_review.name}
                        </span>
                      </div>
                      <div>
                        Blocked:{" "}
                        <span className="font-medium text-text">
                          {githubProjectStatusMapping.blocked.name}
                        </span>
                      </div>
                      <div>
                        Done:{" "}
                        <span className="font-medium text-text">
                          {githubProjectStatusMapping.done.name}
                        </span>
                      </div>
                    </div>
                    {githubProjectSchemaDrift ? (
                      <div className="mt-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-100">
                        The live project schema no longer matches the saved binding fingerprint.
                        Re-saving the binding will refresh the stored mapping.
                      </div>
                    ) : null}
                  </div>
                ) : null}
                {!githubProjectBinding ? (
                  <div className="rounded-lg border border-dashed border-border bg-surface-elevated/10 px-4 py-8 text-center text-sm text-text-muted">
                    Connect a GitHub Project to show a live inbox of issue-backed TODO items for
                    this coder workspace.
                  </div>
                ) : githubProjectInboxLoading ? (
                  <div className="rounded-lg border border-border bg-surface px-4 py-8 text-center text-sm text-text-muted">
                    Loading GitHub Project inbox...
                  </div>
                ) : githubProjectInbox.length === 0 ? (
                  <div className="rounded-lg border border-dashed border-border bg-surface-elevated/10 px-4 py-8 text-center text-sm text-text-muted">
                    No project items are currently ready for intake.
                  </div>
                ) : (
                  <div className="space-y-3">
                    <div className="text-sm font-semibold text-text">Inbox</div>
                    <div className="space-y-3">
                      {githubProjectInbox.map((item) => {
                        const linkedRunId = item.linked_run?.coder_run?.coder_run_id || "";
                        const canIntake =
                          item.actionable && (!item.linked_run || !item.linked_run.active);
                        return (
                          <div
                            key={item.project_item_id}
                            className="rounded-xl border border-border bg-surface-elevated/20 p-4"
                          >
                            <div className="flex flex-wrap items-start justify-between gap-3">
                              <div className="min-w-0 flex-1">
                                <div className="text-sm font-semibold text-text">{item.title}</div>
                                <div className="mt-1 text-xs text-text-muted">
                                  {item.issue ? `Issue #${item.issue.number}` : "Unsupported item"}
                                  {item.issue?.html_url ? ` • ${item.issue.html_url}` : ""}
                                </div>
                              </div>
                              <div className="flex flex-wrap items-center gap-2 text-xs">
                                <span className="rounded-full border border-border bg-surface px-2 py-1 text-text-muted">
                                  {item.status_name}
                                </span>
                                <span className="rounded-full border border-border bg-surface px-2 py-1 text-text-muted">
                                  {syncStateLabel(item.remote_sync_state)}
                                </span>
                              </div>
                            </div>
                            {!item.actionable && item.unsupported_reason ? (
                              <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2 text-xs text-text-muted">
                                {item.unsupported_reason}
                              </div>
                            ) : null}
                            {item.linked_run ? (
                              <div className="mt-3 rounded-lg border border-border bg-surface px-3 py-2 text-xs text-text-muted">
                                Linked run {item.linked_run.coder_run.coder_run_id}
                                {item.linked_run.active
                                  ? " is active and already owns this item."
                                  : " is terminal, so a new intake can create a fresh run generation."}
                              </div>
                            ) : null}
                            <div className="mt-3 flex flex-wrap gap-2">
                              {canIntake ? (
                                <Button
                                  size="sm"
                                  onClick={() => void handleIntakeProjectItem(item)}
                                  loading={
                                    githubProjectBusyKey === `intake:${item.project_item_id}`
                                  }
                                >
                                  Intake Into Coder
                                </Button>
                              ) : null}
                              {linkedRunId ? (
                                <Button
                                  size="sm"
                                  variant="secondary"
                                  onClick={() => {
                                    setSelectedRunId(linkedRunId);
                                    setTab("runs");
                                  }}
                                >
                                  Open Linked Run
                                </Button>
                              ) : null}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardTitle className="text-base">Mission Builder</CardTitle>
                <CardDescription>
                  The existing advanced mission builder is the authoring engine behind Coder in this
                  first slice.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="rounded-lg border border-border bg-surface-elevated/40 px-4 py-3 text-sm text-text-muted">
                  Selected preset:{" "}
                  <span className="font-medium text-text">
                    {CODER_PRESETS.find((preset) => preset.id === selectedPreset)?.title}
                  </span>
                  . The preset cards are UI scaffolding in this slice; the builder below still emits
                  the existing mission contract unchanged.
                </div>
                {catalogError ? (
                  <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                    {catalogError}
                  </div>
                ) : null}
                {loadingCatalog ? (
                  <div className="rounded-lg border border-border bg-surface px-4 py-8 text-center text-sm text-text-muted">
                    Loading builder catalog...
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
                    onShowRuns={() => setTab("runs")}
                    onOpenMcpExtensions={onOpenMcpExtensions}
                  />
                )}
              </CardContent>
            </Card>
          </>
        ) : (
          <div className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle className="text-base">Automation-backed Coder Runs</CardTitle>
                <CardDescription>
                  Coder now projects runs from coder-tagged Automation V2 records instead of relying
                  only on the legacy coder store.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" onClick={() => setTab("create")}>
                    New Coding Swarm
                  </Button>
                  <Button size="sm" variant="secondary" onClick={() => void refreshCoderRuns()}>
                    Refresh Runs
                  </Button>
                  <Button size="sm" variant="secondary" onClick={onOpenAutomation}>
                    Open Agent Automation Runtime
                  </Button>
                </div>
                {runsError ? (
                  <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-200">
                    {runsError}
                  </div>
                ) : null}
                {runsLoading ? (
                  <div className="rounded-lg border border-border bg-surface px-4 py-8 text-center text-sm text-text-muted">
                    Loading coder-tagged automation runs...
                  </div>
                ) : coderRuns.length === 0 ? (
                  <div className="rounded-lg border border-dashed border-border bg-surface-elevated/20 px-4 py-8 text-center text-sm text-text-muted">
                    No coder-tagged automation runs yet. Launch a coding swarm from the Create tab
                    to populate this view.
                  </div>
                ) : (
                  <div className="grid gap-4 xl:grid-cols-[420px_minmax(0,1fr)]">
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
              </CardContent>
            </Card>

            <div>
              <div className="mb-3 px-1">
                <div className="text-sm font-semibold text-text">Legacy Compatibility</div>
                <div className="text-xs text-text-muted">
                  Existing coder runs remain available here until the hybrid run model is wired.
                </div>
              </div>
              <div className="min-h-[960px] rounded-2xl border border-border bg-surface">
                <DeveloperRunViewer onOpenMcpSettings={onOpenMcpExtensions} />
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
